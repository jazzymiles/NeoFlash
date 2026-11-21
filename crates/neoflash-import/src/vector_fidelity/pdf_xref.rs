//! Bounded PDF 1.4–1.7 document, cross-reference stream (`/Type /XRef`, `/XRefStm`),
//! object stream (`/Type /ObjStm`), and Flate/ASCII85/ASCIIHex/Predictor decoder (`T-415`).

use crate::ImportError;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::io::Read;

pub(super) const MAX_PDF_STREAM_DECODE_BYTES: usize = 64 * 1024 * 1024;
pub(super) const MAX_PDF_DECODED_STREAM_BYTES: usize = MAX_PDF_STREAM_DECODE_BYTES;
const MAX_PDF_OBJECTS: usize = 200_000;
const MAX_PDF_NESTING: usize = 64;

#[derive(Clone, Copy, Debug)]
enum XrefSlot {
    Free,
    Normal { offset: usize, generation: u16 },
    Compressed { container: u32 },
}

fn paeth_predict(left: u8, above: u8, upper_left: u8) -> u8 {
    let l = i16::from(left);
    let a = i16::from(above);
    let ul = i16::from(upper_left);
    let p = l + a - ul;
    let dl = (p - l).abs();
    let da = (p - a).abs();
    let dul = (p - ul).abs();
    if dl <= da && dl <= dul {
        left
    } else if da <= dul {
        above
    } else {
        upper_left
    }
}

pub(super) fn apply_pdf_predictor(
    data: &[u8],
    params: Option<&Dictionary>,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let Some(params) = params else {
        return Ok(data.to_vec());
    };
    let predictor = params
        .get(b"Predictor")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(1);
    if predictor <= 1 {
        return Ok(data.to_vec());
    }
    let columns = params
        .get(b"Columns")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(1)
        .max(1) as usize;
    let colors = params
        .get(b"Colors")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(1)
        .max(1) as usize;
    let bpc = params
        .get(b"BitsPerComponent")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(8)
        .clamp(1, 32) as usize;

    let row_bits = columns
        .checked_mul(colors)
        .and_then(|v| v.checked_mul(bpc))
        .ok_or_else(|| "PDF predictor row width overflowed".to_owned())?;
    let row_bytes = row_bits.div_ceil(8);
    if row_bytes == 0 || row_bytes > max_bytes {
        return Err("PDF predictor row exceeds decode limit".into());
    }
    let bpp = (colors * bpc).div_ceil(8).max(1);

    if predictor == 2 {
        // TIFF Predictor 2: horizontal differencing without a per-row filter tag byte.
        if data.len() > max_bytes {
            return Err("PDF TIFF predictor stream exceeds 64 MiB limit".into());
        }
        let mut out = data.to_vec();
        if bpc == 16 {
            let stride = colors * 2;
            for row in out.chunks_mut(row_bytes) {
                for i in (stride..row.len().saturating_sub(1)).step_by(2) {
                    let prev = u16::from_be_bytes([row[i - stride], row[i - stride + 1]]);
                    let cur = u16::from_be_bytes([row[i], row[i + 1]]).wrapping_add(prev);
                    let [hi, lo] = cur.to_be_bytes();
                    row[i] = hi;
                    row[i + 1] = lo;
                }
            }
        } else {
            for row in out.chunks_mut(row_bytes) {
                for i in bpp..row.len() {
                    row[i] = row[i].wrapping_add(row[i - bpp]);
                }
            }
        }
        return Ok(out);
    }

    if (10..=15).contains(&predictor) {
        let est_rows = data.len() / (row_bytes + 1) + 1;
        let est_out = est_rows.saturating_mul(row_bytes);
        if est_out > max_bytes.saturating_add(row_bytes) {
            return Err("PDF PNG predictor output exceeds 64 MiB limit".into());
        }
        let mut out = Vec::with_capacity(est_out.min(max_bytes).min(4 * 1024 * 1024));
        let mut prev = vec![0u8; row_bytes];
        let mut cur = vec![0u8; row_bytes];
        let mut pos = 0usize;
        while pos < data.len() {
            let filter_tag = data[pos];
            pos += 1;
            let avail = (data.len() - pos).min(row_bytes);
            cur[..avail].copy_from_slice(&data[pos..pos + avail]);
            if avail < row_bytes {
                cur[avail..].fill(0);
            }
            pos += avail;
            match filter_tag {
                0 => {}
                1 => {
                    for i in bpp..row_bytes {
                        cur[i] = cur[i].wrapping_add(cur[i - bpp]);
                    }
                }
                2 => {
                    for i in 0..row_bytes {
                        cur[i] = cur[i].wrapping_add(prev[i]);
                    }
                }
                3 => {
                    for i in 0..row_bytes {
                        let left = if i >= bpp { u16::from(cur[i - bpp]) } else { 0 };
                        let avg = ((left + u16::from(prev[i])) / 2) as u8;
                        cur[i] = cur[i].wrapping_add(avg);
                    }
                }
                4 => {
                    for i in 0..row_bytes {
                        let left = if i >= bpp { cur[i - bpp] } else { 0 };
                        let ul = if i >= bpp { prev[i - bpp] } else { 0 };
                        cur[i] = cur[i].wrapping_add(paeth_predict(left, prev[i], ul));
                    }
                }
                other => {
                    return Err(format!("invalid PNG predictor row filter {other}"));
                }
            }
            if out.len().saturating_add(avail) > max_bytes {
                return Err("PDF PNG predictor output exceeds 64 MiB limit".into());
            }
            out.extend_from_slice(&cur[..avail]);
            std::mem::swap(&mut prev, &mut cur);
        }
        return Ok(out);
    }

    Err(format!("unsupported PDF Predictor {predictor}"))
}

fn decode_ascii_hex(input: &[u8], max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity((input.len() / 2).min(max_bytes).min(256 * 1024));
    let mut high: Option<u8> = None;
    for &b in input {
        if b == b'>' {
            break;
        }
        if b.is_ascii_whitespace() {
            continue;
        }
        let nib = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err("invalid hex digit in ASCIIHexDecode stream".into()),
        };
        if let Some(hi) = high.take() {
            if out.len() >= max_bytes {
                return Err("ASCIIHexDecode stream exceeds 64 MiB limit".into());
            }
            out.push((hi << 4) | nib);
        } else {
            high = Some(nib);
        }
    }
    if let Some(hi) = high {
        if out.len() >= max_bytes {
            return Err("ASCIIHexDecode stream exceeds 64 MiB limit".into());
        }
        out.push(hi << 4);
    }
    Ok(out)
}

fn decode_ascii85(input: &[u8], max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity((input.len() * 4 / 5).min(max_bytes).min(256 * 1024));
    let mut tuple = 0u32;
    let mut count = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        let b = input[i];
        i += 1;
        if b == b'~' {
            break;
        }
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'z' {
            if count != 0 {
                return Err("invalid 'z' inside ASCII85 group".into());
            }
            if out.len().saturating_add(4) > max_bytes {
                return Err("ASCII85Decode stream exceeds 64 MiB limit".into());
            }
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&b) {
            break;
        }
        tuple = tuple
            .checked_mul(85)
            .and_then(|v| v.checked_add(u32::from(b - b'!')))
            .ok_or_else(|| "ASCII85Decode group overflow".to_owned())?;
        count += 1;
        if count == 5 {
            if out.len().saturating_add(4) > max_bytes {
                return Err("ASCII85Decode stream exceeds 64 MiB limit".into());
            }
            out.extend_from_slice(&tuple.to_be_bytes());
            tuple = 0;
            count = 0;
        }
    }
    if count >= 2 {
        for _ in count..5 {
            tuple = tuple
                .checked_mul(85)
                .and_then(|v| v.checked_add(84))
                .ok_or_else(|| "ASCII85Decode tail overflow".to_owned())?;
        }
        let bytes = tuple.to_be_bytes();
        let n = count - 1;
        if out.len().saturating_add(n) > max_bytes {
            return Err("ASCII85Decode stream exceeds 64 MiB limit".into());
        }
        out.extend_from_slice(&bytes[..n]);
    }
    Ok(out)
}

fn decode_run_length(input: &[u8], max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(input.len().min(max_bytes).min(256 * 1024));
    let mut i = 0usize;
    while i < input.len() {
        let len_byte = input[i];
        i += 1;
        if len_byte == 128 {
            break;
        } else if len_byte < 128 {
            let copy_len = usize::from(len_byte) + 1;
            let end = (i + copy_len).min(input.len());
            let slice = &input[i..end];
            if out.len().saturating_add(slice.len()) > max_bytes {
                return Err("RunLengthDecode stream exceeds 64 MiB limit".into());
            }
            out.extend_from_slice(slice);
            i = end;
        } else {
            let repeat_len = 257usize - usize::from(len_byte);
            let Some(&val) = input.get(i) else {
                break;
            };
            i += 1;
            if out.len().saturating_add(repeat_len) > max_bytes {
                return Err("RunLengthDecode stream exceeds 64 MiB limit".into());
            }
            out.resize(out.len() + repeat_len, val);
        }
    }
    Ok(out)
}

fn inflate_flate_bounded(input: &[u8], max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let zlib_ok = flate2::read::ZlibDecoder::new(input)
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut out)
        .is_ok();
    if zlib_ok && (!out.is_empty() || input.is_empty()) {
        if out.len() > max_bytes {
            return Err("FlateDecode stream exceeds 64 MiB limit".into());
        }
        return Ok(out);
    }
    out.clear();
    flate2::read::DeflateDecoder::new(input)
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("FlateDecode failed: {e}"))?;
    if out.len() > max_bytes {
        return Err("FlateDecode stream exceeds 64 MiB limit".into());
    }
    Ok(out)
}

fn stream_filters_and_parms<'a>(
    dict: &'a Dictionary,
    doc: Option<&'a Document>,
) -> (Vec<&'a [u8]>, Vec<Option<&'a Dictionary>>) {
    let deref = |obj: &'a Object| -> &'a Object {
        doc.and_then(|d| d.dereference(obj).ok().map(|(_, o)| o))
            .unwrap_or(obj)
    };
    let mut filters = Vec::new();
    if let Ok(f_obj) = dict.get(b"Filter").or_else(|_| dict.get(b"F")) {
        let resolved = deref(f_obj);
        if let Ok(name) = resolved.as_name() {
            filters.push(name);
        } else if let Ok(arr) = resolved.as_array() {
            for item in arr {
                if let Ok(name) = deref(item).as_name() {
                    filters.push(name);
                }
            }
        }
    }
    let mut parms = Vec::new();
    if let Ok(p_obj) = dict.get(b"DecodeParms").or_else(|_| dict.get(b"DP")) {
        let resolved = deref(p_obj);
        if let Ok(d) = resolved.as_dict() {
            parms.push(Some(d));
        } else if let Ok(arr) = resolved.as_array() {
            for item in arr {
                parms.push(deref(item).as_dict().ok());
            }
        }
    }
    (filters, parms)
}

fn decode_pdf_lzw(
    input: &[u8],
    params: Option<&Dictionary>,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let early_change = params
        .and_then(|d| d.get(b"EarlyChange").ok())
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(1)
        != 0;
    let threshold_delta: usize = if early_change { 1 } else { 0 };

    let mut table: Vec<Vec<u8>> = (0..=255).map(|b| vec![b as u8]).collect();
    table.push(Vec::new()); // 256 = Clear
    table.push(Vec::new()); // 257 = EOD

    let mut code_bits = 9usize;
    let mut bit_pos = 0usize;
    let total_bits = input.len().saturating_mul(8);
    let mut out = Vec::new();
    let mut prev_seq: Option<Vec<u8>> = None;

    let read_code = |bit_pos: &mut usize, bits: usize| -> Option<usize> {
        if *bit_pos + bits > total_bits {
            return None;
        }
        let mut val = 0usize;
        for _ in 0..bits {
            let byte = input[*bit_pos / 8];
            let shift = 7 - (*bit_pos % 8);
            val = (val << 1) | usize::from((byte >> shift) & 1);
            *bit_pos += 1;
        }
        Some(val)
    };

    while let Some(code) = read_code(&mut bit_pos, code_bits) {
        if code == 257 {
            break;
        }
        if code == 256 {
            table.truncate(258);
            code_bits = 9;
            prev_seq = None;
            continue;
        }
        let entry = if code < table.len() {
            table[code].clone()
        } else if code == table.len()
            && let Some(ref prev) = prev_seq
        {
            let mut seq = prev.clone();
            seq.push(prev[0]);
            seq
        } else {
            return Err("corrupt PDF LZWDecode code".into());
        };

        if out.len().saturating_add(entry.len()) > max_bytes {
            return Err("LZW decoded stream exceeds 64 MiB limit".into());
        }
        out.extend_from_slice(&entry);

        if let Some(mut prev) = prev_seq
            && table.len() < 4096
        {
            prev.push(entry[0]);
            table.push(prev);
            let limit = (1usize << code_bits).saturating_sub(threshold_delta);
            if table.len() >= limit && code_bits < 12 {
                code_bits += 1;
            }
        }
        prev_seq = Some(entry);
    }

    apply_pdf_predictor(&out, params, max_bytes)
}

/// Decode a PDF stream up to `max_bytes`. When `stop_before_dct` is `true` and the final filter is
/// `DCTDecode` / `DCT`, returns `(jpeg_bytes, true)` so the caller can decode the JPEG image with
/// `image::Limits`.
pub(super) fn decode_pdf_stream_pipeline(
    stream: &Stream,
    doc: Option<&Document>,
    max_bytes: usize,
    stop_before_dct: bool,
) -> Result<(Vec<u8>, bool), String> {
    let (filters, parms) = stream_filters_and_parms(&stream.dict, doc);
    if filters.is_empty() {
        if stream.content.len() > max_bytes {
            return Err("PDF stream exceeds 64 MiB limit".into());
        }
        return Ok((stream.content.clone(), false));
    }
    let mut current = stream.content.clone();
    for (idx, filter) in filters.iter().copied().enumerate() {
        let param = parms.get(idx).copied().flatten();
        let is_last = idx + 1 == filters.len();
        match filter {
            b"FlateDecode" | b"Fl" => {
                let inflated = inflate_flate_bounded(&current, max_bytes)?;
                current = apply_pdf_predictor(&inflated, param, max_bytes)?;
            }
            b"ASCIIHexDecode" | b"AHx" => {
                current = decode_ascii_hex(&current, max_bytes)?;
            }
            b"ASCII85Decode" | b"A85" => {
                current = decode_ascii85(&current, max_bytes)?;
            }
            b"RunLengthDecode" | b"RL" => {
                current = decode_run_length(&current, max_bytes)?;
            }
            b"LZWDecode" | b"LZW" => {
                if current.len() > max_bytes {
                    return Err("LZW stream exceeds 64 MiB limit".into());
                }
                current = decode_pdf_lzw(&current, param, max_bytes)?;
            }
            b"DCTDecode" | b"DCT" if stop_before_dct && is_last => {
                if current.len() > max_bytes {
                    return Err("DCTDecode JPEG stream exceeds 64 MiB limit".into());
                }
                return Ok((current, true));
            }
            other => {
                return Err(format!(
                    "unsupported PDF stream filter '{}'",
                    String::from_utf8_lossy(other)
                ));
            }
        }
    }
    Ok((current, false))
}

pub(super) fn decode_pdf_stream_bounded(
    stream: &Stream,
    max_bytes: usize,
) -> Result<Vec<u8>, ImportError> {
    decode_pdf_stream_pipeline(stream, None, max_bytes, false)
        .map(|(b, _)| b)
        .map_err(|e| ImportError::Container(format!("PDF stream decode: {e}")))
}

pub(super) fn decode_pdf_stream_with_doc(
    stream: &Stream,
    doc: Option<&Document>,
    max_bytes: usize,
    stop_before_dct: bool,
) -> Result<(Vec<u8>, bool), ImportError> {
    decode_pdf_stream_pipeline(stream, doc, max_bytes, stop_before_dct)
        .map_err(|e| ImportError::Container(format!("PDF stream decode: {e}")))
}

// ============================================================================
// Bounded PDF 1.4–1.7 Lexer & Parser (`/XRef`, `/XRefStm`, `/ObjStm`)
// ============================================================================

#[inline]
fn is_pdf_ws(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}

#[inline]
fn is_pdf_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

struct PdfCursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> PdfCursor<'a> {
    fn new(buf: &'a [u8], pos: usize) -> Self {
        Self {
            buf,
            pos: pos.min(buf.len()),
        }
    }

    fn skip_ws_and_comments(&mut self) {
        while self.pos < self.buf.len() {
            let b = self.buf[self.pos];
            if is_pdf_ws(b) {
                self.pos += 1;
            } else if b == b'%' {
                self.pos += 1;
                while self.pos < self.buf.len() && !matches!(self.buf[self.pos], b'\r' | b'\n') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws_and_comments();
        self.buf.get(self.pos).copied()
    }

    fn read_word(&mut self) -> Option<&'a [u8]> {
        self.skip_ws_and_comments();
        let start = self.pos;
        while self.pos < self.buf.len() {
            let b = self.buf[self.pos];
            if is_pdf_ws(b) || is_pdf_delim(b) {
                break;
            }
            self.pos += 1;
        }
        (self.pos > start).then(|| &self.buf[start..self.pos])
    }

    fn read_name(&mut self) -> Option<Vec<u8>> {
        self.skip_ws_and_comments();
        if self.buf.get(self.pos) != Some(&b'/') {
            return None;
        }
        self.pos += 1;
        let mut name = Vec::new();
        while self.pos < self.buf.len() && name.len() < 4096 {
            let b = self.buf[self.pos];
            if is_pdf_ws(b) || is_pdf_delim(b) {
                break;
            }
            if b == b'#'
                && let (Some(h1), Some(h2)) = (
                    self.buf
                        .get(self.pos + 1)
                        .and_then(|c| char::from(*c).to_digit(16)),
                    self.buf
                        .get(self.pos + 2)
                        .and_then(|c| char::from(*c).to_digit(16)),
                )
            {
                name.push(((h1 << 4) | h2) as u8);
                self.pos += 3;
                continue;
            }
            name.push(b);
            self.pos += 1;
        }
        Some(name)
    }

    fn read_literal_string(&mut self) -> Option<Vec<u8>> {
        if self.buf.get(self.pos) != Some(&b'(') {
            return None;
        }
        self.pos += 1;
        let mut out = Vec::new();
        let mut depth = 1usize;
        while self.pos < self.buf.len() && out.len() <= MAX_PDF_STREAM_DECODE_BYTES {
            let b = self.buf[self.pos];
            self.pos += 1;
            if b == b'\\' {
                let Some(&esc) = self.buf.get(self.pos) else {
                    break;
                };
                self.pos += 1;
                match esc {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0C),
                    b'(' | b')' | b'\\' => out.push(esc),
                    b'\r' => {
                        if self.buf.get(self.pos) == Some(&b'\n') {
                            self.pos += 1;
                        }
                    }
                    b'\n' => {}
                    b'0'..=b'7' => {
                        let mut val = u32::from(esc - b'0');
                        for _ in 0..2 {
                            if let Some(&d @ b'0'..=b'7') = self.buf.get(self.pos) {
                                self.pos += 1;
                                val = val * 8 + u32::from(d - b'0');
                            } else {
                                break;
                            }
                        }
                        out.push((val & 0xFF) as u8);
                    }
                    other => out.push(other),
                }
            } else if b == b'(' {
                depth += 1;
                out.push(b'(');
            } else if b == b')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                out.push(b')');
            } else {
                out.push(b);
            }
        }
        Some(out)
    }

    fn read_hex_string(&mut self) -> Option<Vec<u8>> {
        if self.buf.get(self.pos) != Some(&b'<') {
            return None;
        }
        self.pos += 1;
        let mut out = Vec::new();
        let mut high: Option<u8> = None;
        while self.pos < self.buf.len() && out.len() <= MAX_PDF_STREAM_DECODE_BYTES {
            let b = self.buf[self.pos];
            self.pos += 1;
            if b == b'>' {
                break;
            }
            if is_pdf_ws(b) {
                continue;
            }
            let nib = char::from(b).to_digit(16).map(|d| d as u8)?;
            if let Some(hi) = high.take() {
                out.push((hi << 4) | nib);
            } else {
                high = Some(nib);
            }
        }
        if let Some(hi) = high {
            out.push(hi << 4);
        }
        Some(out)
    }

    fn parse_direct_object(&mut self, depth: usize) -> Option<Object> {
        if depth > MAX_PDF_NESTING {
            return None;
        }
        let b = self.peek()?;
        match b {
            b'/' => Some(Object::Name(self.read_name()?)),
            b'(' => Some(Object::String(
                self.read_literal_string()?,
                StringFormat::Literal,
            )),
            b'<' => {
                if self.buf.get(self.pos + 1) == Some(&b'<') {
                    self.pos += 2;
                    let mut dict = Dictionary::new();
                    loop {
                        let next = self.peek()?;
                        if next == b'>' && self.buf.get(self.pos + 1) == Some(&b'>') {
                            self.pos += 2;
                            break;
                        }
                        let key = self.read_name()?;
                        let val = self.parse_direct_object(depth + 1)?;
                        dict.set(key, val);
                    }
                    Some(Object::Dictionary(dict))
                } else {
                    Some(Object::String(
                        self.read_hex_string()?,
                        StringFormat::Hexadecimal,
                    ))
                }
            }
            b'[' => {
                self.pos += 1;
                let mut arr = Vec::new();
                while self.peek()? != b']' {
                    if arr.len() >= MAX_PDF_OBJECTS {
                        return None;
                    }
                    arr.push(self.parse_direct_object(depth + 1)?);
                }
                self.pos += 1;
                Some(Object::Array(arr))
            }
            _ => {
                let word = self.read_word()?;
                match word {
                    b"null" => Some(Object::Null),
                    b"true" => Some(Object::Boolean(true)),
                    b"false" => Some(Object::Boolean(false)),
                    _ => {
                        let s = std::str::from_utf8(word).ok()?;
                        if let Ok(int_val) = s.parse::<i64>() {
                            // Look ahead for `N G R` indirect reference!
                            let saved = self.pos;
                            if int_val >= 0
                                && let Some(gen_word) = self.read_word()
                                && let Ok(gen_val) =
                                    std::str::from_utf8(gen_word).ok()?.parse::<u16>()
                                && self.read_word() == Some(b"R")
                            {
                                return Some(Object::Reference((int_val as u32, gen_val)));
                            }
                            self.pos = saved;
                            Some(Object::Integer(int_val))
                        } else if let Ok(float_val) = s.parse::<f32>() {
                            Some(Object::Real(float_val))
                        } else {
                            None
                        }
                    }
                }
            }
        }
    }

    fn parse_indirect_object(
        &mut self,
        resolve_int: &dyn Fn(ObjectId) -> Option<i64>,
    ) -> Option<(ObjectId, Object)> {
        let id_num: u32 = std::str::from_utf8(self.read_word()?).ok()?.parse().ok()?;
        let gen_num: u16 = std::str::from_utf8(self.read_word()?).ok()?.parse().ok()?;
        if self.read_word()? != b"obj" {
            return None;
        }
        let obj = self.parse_direct_object(0)?;
        let saved = self.pos;
        if let Object::Dictionary(dict) = obj {
            if self.read_word() == Some(b"stream") {
                // Exactly one EOL (`\r\n` or `\n`, or `\r`) follows `stream`.
                if self.buf.get(self.pos) == Some(&b'\r') {
                    self.pos += 1;
                }
                if self.buf.get(self.pos) == Some(&b'\n') {
                    self.pos += 1;
                }
                let data_start = self.pos;
                let declared_len = dict
                    .get(b"Length")
                    .ok()
                    .and_then(|l| match l {
                        Object::Integer(n) if *n >= 0 => Some(*n as usize),
                        Object::Reference(rid) => {
                            resolve_int(*rid).filter(|&n| n >= 0).map(|n| n as usize)
                        }
                        _ => None,
                    })
                    .filter(|&len| data_start.saturating_add(len) <= self.buf.len());

                let content = if let Some(len) = declared_len
                    && has_endstream_near(self.buf, data_start + len)
                {
                    self.pos = data_start + len;
                    self.buf[data_start..data_start + len].to_vec()
                } else {
                    let rel_end = self.buf[data_start..]
                        .windows(9)
                        .position(|w| w == b"endstream")?;
                    let mut trim_end = data_start + rel_end;
                    if trim_end > data_start && self.buf[trim_end - 1] == b'\n' {
                        trim_end -= 1;
                    }
                    if trim_end > data_start && self.buf[trim_end - 1] == b'\r' {
                        trim_end -= 1;
                    }
                    self.pos = data_start + rel_end;
                    self.buf[data_start..trim_end].to_vec()
                };
                let _ = self.read_word(); // endstream
                let _ = self.read_word(); // endobj
                return Some((
                    (id_num, gen_num),
                    Object::Stream(Stream::new(dict, content)),
                ));
            }
            self.pos = saved;
            let _ = self.read_word(); // endobj
            return Some(((id_num, gen_num), Object::Dictionary(dict)));
        }
        let _ = self.read_word(); // endobj
        Some(((id_num, gen_num), obj))
    }
}

fn has_endstream_near(buf: &[u8], mut pos: usize) -> bool {
    while pos < buf.len() && is_pdf_ws(buf[pos]) {
        pos += 1;
    }
    buf.get(pos..pos + 9) == Some(b"endstream")
}

fn find_startxref_offset(pdf_bytes: &[u8]) -> Option<usize> {
    let tail_start = pdf_bytes.len().saturating_sub(8192);
    let tail = &pdf_bytes[tail_start..];
    let rel = tail.windows(9).rposition(|w| w == b"startxref")?;
    let mut cur = PdfCursor::new(pdf_bytes, tail_start + rel + 9);
    let word = cur.read_word()?;
    std::str::from_utf8(word)
        .ok()?
        .parse::<usize>()
        .ok()
        .filter(|&off| off < pdf_bytes.len())
}

fn read_be_u64(slice: &[u8]) -> u64 {
    let mut v = 0u64;
    for &b in slice {
        v = (v << 8) | u64::from(b);
    }
    v
}

fn decode_xref_stream_entries(
    stream: &Stream,
    slots: &mut BTreeMap<u32, XrefSlot>,
    decoded_used: &mut usize,
) -> Result<(), String> {
    let dict = &stream.dict;
    let size = dict
        .get(b"Size")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .filter(|&s| (0..=2_000_000).contains(&s))
        .ok_or_else(|| "invalid /Size in /XRef stream".to_owned())? as u32;

    let widths_arr = dict
        .get(b"W")
        .and_then(Object::as_array)
        .map_err(|_| "missing /W in /XRef stream".to_owned())?;
    if widths_arr.len() < 3 {
        return Err("invalid /W length in /XRef stream".into());
    }
    let w0 = widths_arr[0].as_i64().map_err(|_| "invalid W[0]")?;
    let w1 = widths_arr[1].as_i64().map_err(|_| "invalid W[1]")?;
    let w2 = widths_arr[2].as_i64().map_err(|_| "invalid W[2]")?;
    if !(0..=8).contains(&w0) || !(0..=8).contains(&w1) || !(0..=8).contains(&w2) {
        return Err("unsupported /W field width in /XRef stream".into());
    }
    let (w0, w1, w2) = (w0 as usize, w1 as usize, w2 as usize);
    let stride = w0 + w1 + w2;
    if stride == 0 {
        return Err("zero-width /W in /XRef stream".into());
    }

    let indices: Vec<u32> = if let Ok(idx_arr) = dict.get(b"Index").and_then(Object::as_array) {
        idx_arr
            .iter()
            .map(|o| o.as_i64().ok().and_then(|v| u32::try_from(v).ok()))
            .collect::<Option<Vec<u32>>>()
            .ok_or_else(|| "invalid /Index in /XRef stream".to_owned())?
    } else {
        vec![0, size]
    };

    let remaining = MAX_PDF_STREAM_DECODE_BYTES.saturating_sub(*decoded_used);
    if remaining == 0 {
        return Err("PDF /XRef stream exceeds cumulative 64 MiB decoding budget".into());
    }
    let (raw, _) = decode_pdf_stream_pipeline(stream, None, remaining, false)?;
    *decoded_used = decoded_used.saturating_add(raw.len());
    let mut pos = 0usize;
    for pair in indices.as_chunks::<2>().0 {
        let (start_id, count) = (pair[0], pair[1]);
        if count > MAX_PDF_OBJECTS as u32 {
            return Err("/XRef subsection exceeds object budget".into());
        }
        for i in 0..count {
            if pos + stride > raw.len() {
                break;
            }
            let f0 = if w0 == 0 {
                1
            } else {
                read_be_u64(&raw[pos..pos + w0])
            };
            let f1 = read_be_u64(&raw[pos + w0..pos + w0 + w1]);
            let f2 = read_be_u64(&raw[pos + w0 + w1..pos + stride]);
            pos += stride;
            let obj_id = start_id.saturating_add(i);
            let slot = match f0 {
                0 => XrefSlot::Free,
                1 => XrefSlot::Normal {
                    offset: usize::try_from(f1).map_err(|_| "xref offset overflow")?,
                    generation: u16::try_from(f2).unwrap_or(0),
                },
                2 => {
                    let _ = usize::try_from(f2).map_err(|_| "ObjStm index overflow")?;
                    XrefSlot::Compressed {
                        container: u32::try_from(f1).map_err(|_| "ObjStm id overflow")?,
                    }
                }
                _ => continue,
            };
            slots.entry(obj_id).or_insert(slot);
        }
    }
    Ok(())
}

fn parse_classic_xref_and_trailer(
    pdf_bytes: &[u8],
    offset: usize,
    slots: &mut BTreeMap<u32, XrefSlot>,
) -> Option<Dictionary> {
    let mut cur = PdfCursor::new(pdf_bytes, offset);
    if cur.read_word()? != b"xref" {
        return None;
    }
    loop {
        let word = cur.read_word()?;
        if word == b"trailer" {
            let Object::Dictionary(dict) = cur.parse_direct_object(0)? else {
                return None;
            };
            return Some(dict);
        }
        let start_id: u32 = std::str::from_utf8(word).ok()?.parse().ok()?;
        let count: u32 = std::str::from_utf8(cur.read_word()?).ok()?.parse().ok()?;
        if count > MAX_PDF_OBJECTS as u32 {
            return None;
        }
        for i in 0..count {
            let off_w = cur.read_word()?;
            let gen_w = cur.read_word()?;
            let kind_w = cur.read_word()?;
            let entry_offset: usize = std::str::from_utf8(off_w).ok()?.parse().ok()?;
            let generation: u16 = std::str::from_utf8(gen_w).ok()?.parse().ok()?;
            let obj_id = start_id.saturating_add(i);
            let slot = if kind_w == b"n" {
                XrefSlot::Normal {
                    offset: entry_offset,
                    generation,
                }
            } else {
                XrefSlot::Free
            };
            slots.entry(obj_id).or_insert(slot);
        }
    }
}

fn unpack_object_stream(
    doc: &Document,
    container_id: u32,
    out_objects: &mut BTreeMap<ObjectId, Object>,
    visited_streams: &mut HashSet<u32>,
    decoded_used: &mut usize,
) -> Result<(), String> {
    if !visited_streams.insert(container_id) || visited_streams.len() > 4096 {
        return Ok(());
    }
    let Some(Object::Stream(stream)) = doc.objects.get(&(container_id, 0)) else {
        return Ok(());
    };
    if stream.dict.get(b"Type").and_then(Object::as_name).ok() != Some(b"ObjStm") {
        return Ok(());
    }
    let n = stream
        .dict
        .get(b"N")
        .and_then(Object::as_i64)
        .map_err(|_| "missing /N in /ObjStm")?;
    let first = stream
        .dict
        .get(b"First")
        .and_then(Object::as_i64)
        .map_err(|_| "missing /First in /ObjStm")?;
    if n < 0 || n > MAX_PDF_OBJECTS as i64 || first < 0 {
        return Err("invalid /N or /First in /ObjStm".into());
    }
    let (n, first) = (n as usize, first as usize);
    let remaining = MAX_PDF_STREAM_DECODE_BYTES.saturating_sub(*decoded_used);
    if remaining == 0 {
        return Err("PDF /ObjStm streams exceed cumulative 64 MiB decoding budget".into());
    }
    let (raw, _) = decode_pdf_stream_pipeline(stream, Some(doc), remaining, false)?;
    *decoded_used = decoded_used.saturating_add(raw.len());
    if first > raw.len() {
        return Err("/First exceeds /ObjStm length".into());
    }

    let mut hdr_cur = PdfCursor::new(&raw[..first], 0);
    let mut pairs = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        let (Some(id_w), Some(off_w)) = (hdr_cur.read_word(), hdr_cur.read_word()) else {
            break;
        };
        let (Ok(id_num), Ok(rel_off)) = (
            std::str::from_utf8(id_w).unwrap_or("").parse::<u32>(),
            std::str::from_utf8(off_w).unwrap_or("").parse::<usize>(),
        ) else {
            break;
        };
        pairs.push((id_num, rel_off));
    }

    for (id_num, rel_off) in pairs {
        if doc.objects.len().saturating_add(out_objects.len()) >= MAX_PDF_OBJECTS {
            return Err("PDF object table exceeds safety budget".into());
        }
        let abs_off = first.saturating_add(rel_off);
        if abs_off >= raw.len() {
            continue;
        }
        let mut obj_cur = PdfCursor::new(&raw, abs_off);
        if let Some(obj) = obj_cur.parse_direct_object(0) {
            out_objects.entry((id_num, 0)).or_insert(obj);
        }
    }

    if let Ok(ext_ref) = stream.dict.get(b"Extends").and_then(Object::as_reference) {
        unpack_object_stream(doc, ext_ref.0, out_objects, visited_streams, decoded_used)?;
    }
    Ok(())
}

fn parse_pdf_17_bounded(pdf_bytes: &[u8]) -> Result<(Document, usize), String> {
    let header_end = pdf_bytes
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .unwrap_or(pdf_bytes.len().min(32));
    let header_line = std::str::from_utf8(&pdf_bytes[..header_end]).unwrap_or("%PDF-1.4");
    let version = header_line
        .strip_prefix("%PDF-")
        .unwrap_or("1.4")
        .trim()
        .to_owned();

    let start_xref = find_startxref_offset(pdf_bytes)
        .ok_or_else(|| "missing or invalid startxref".to_owned())?;

    let mut doc = Document::with_version(version);
    let mut slots: BTreeMap<u32, XrefSlot> = BTreeMap::new();
    let mut queue = VecDeque::from([start_xref]);
    let mut visited_xrefs = HashSet::new();
    let mut decoded_used = 0usize;

    while let Some(xref_off) = queue.pop_front() {
        if xref_off >= pdf_bytes.len()
            || !visited_xrefs.insert(xref_off)
            || visited_xrefs.len() > 64
        {
            continue;
        }
        let mut peek_cur = PdfCursor::new(pdf_bytes, xref_off);
        let is_classic = peek_cur.read_word() == Some(b"xref");
        let section_trailer = if is_classic {
            parse_classic_xref_and_trailer(pdf_bytes, xref_off, &mut slots)
                .ok_or_else(|| "invalid classic xref table".to_owned())?
        } else {
            let mut cur = PdfCursor::new(pdf_bytes, xref_off);
            let (_, obj) = cur
                .parse_indirect_object(&|_| None)
                .ok_or_else(|| "invalid /XRef indirect object".to_owned())?;
            let Object::Stream(xref_stream) = obj else {
                return Err("expected /XRef stream object at startxref".into());
            };
            if xref_stream.dict.get(b"Type").and_then(Object::as_name).ok() != Some(b"XRef") {
                return Err("stream at startxref is not /Type /XRef".into());
            }
            decode_xref_stream_entries(&xref_stream, &mut slots, &mut decoded_used)?;
            xref_stream.dict
        };

        for key in [b"Root".as_slice(), b"Encrypt", b"Info", b"ID", b"Size"] {
            if !doc.trailer.has(key)
                && let Ok(val) = section_trailer.get(key)
            {
                doc.trailer.set(key.to_vec(), val.clone());
            }
        }
        // Handle hybrid-reference `/XRefStm` even when `/Prev` is absent!
        if let Ok(stm_off) = section_trailer.get(b"XRefStm").and_then(Object::as_i64)
            && let Ok(off) = usize::try_from(stm_off)
        {
            queue.push_back(off);
        }
        if let Ok(prev_off) = section_trailer.get(b"Prev").and_then(Object::as_i64)
            && let Ok(off) = usize::try_from(prev_off)
        {
            queue.push_back(off);
        }
    }

    if doc.trailer.get(b"Encrypt").is_ok() {
        return Err("Unlock the document and save an unencrypted PDF.".into());
    }
    if !doc.trailer.has(b"Root") {
        return Err("PDF trailer is missing /Root catalog".into());
    }
    if slots.len() > MAX_PDF_OBJECTS {
        return Err("PDF object table exceeds safety budget".into());
    }

    // Pass 1: parse uncompressed scalar/dictionary objects and collect indirect integer values (for `/Length N 0 R`).
    let normal_offsets: BTreeMap<ObjectId, usize> = slots
        .iter()
        .filter_map(|(&id, slot)| match *slot {
            XrefSlot::Normal { offset, generation } if offset < pdf_bytes.len() => {
                Some(((id, generation), offset))
            }
            _ => None,
        })
        .collect();

    let resolve_int = |rid: ObjectId| -> Option<i64> {
        let &off = normal_offsets.get(&rid)?;
        let mut cur = PdfCursor::new(pdf_bytes, off);
        let _ = cur.read_word()?;
        let _ = cur.read_word()?;
        if cur.read_word()? != b"obj" {
            return None;
        }
        let w = cur.read_word()?;
        std::str::from_utf8(w).ok()?.parse::<i64>().ok()
    };

    let mut max_id = 0u32;
    for (&expected_id, &offset) in &normal_offsets {
        let mut cur = PdfCursor::new(pdf_bytes, offset);
        if let Some((parsed_id, obj)) = cur.parse_indirect_object(&resolve_int) {
            max_id = max_id.max(parsed_id.0).max(expected_id.0);
            doc.objects.insert(parsed_id, obj);
        }
    }

    // Pass 2: unpack compressed objects from `/Type /ObjStm` containers against the shared 64 MiB budget.
    let mut containers: Vec<u32> = slots
        .values()
        .filter_map(|slot| match *slot {
            XrefSlot::Compressed { container, .. } => Some(container),
            _ => None,
        })
        .collect();
    for (&(id, _), obj) in &doc.objects {
        if let Object::Stream(s) = obj
            && s.dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"ObjStm")
        {
            containers.push(id);
        }
    }
    containers.sort_unstable();
    containers.dedup();

    let mut unpacked = BTreeMap::new();
    let mut visited_streams = HashSet::new();
    for cid in containers {
        unpack_object_stream(
            &doc,
            cid,
            &mut unpacked,
            &mut visited_streams,
            &mut decoded_used,
        )?;
    }
    for (oid, obj) in unpacked {
        max_id = max_id.max(oid.0);
        doc.objects.entry(oid).or_insert(obj);
    }
    doc.max_id = max_id;
    Ok((doc, decoded_used))
}

pub(super) fn load_pdf_document_bounded(
    raw_bytes: &[u8],
) -> Result<(Document, usize), ImportError> {
    let error = |msg: &str| ImportError::Container(format!("PDF page import: {msg}"));
    let head_window = &raw_bytes[..raw_bytes.len().min(1024)];
    let Some(hdr_pos) = head_window.windows(5).position(|w| w == b"%PDF-") else {
        return Err(error("missing %PDF- header within first 1024 bytes"));
    };
    let pdf_bytes = &raw_bytes[hdr_pos..];
    parse_pdf_17_bounded(pdf_bytes).map_err(|e| error(&e))
}
