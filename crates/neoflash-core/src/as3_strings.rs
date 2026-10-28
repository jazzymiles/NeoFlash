//! Bounded AVM2 string and URI algorithms, adapted into NeoFlash-owned values.
//! Portions adapted from Ruffle 89a7049965f3090e706265670df309a5128233ac.
//! Copyright (c) 2018-2022 Ruffle LLC <ruffle@ruffle.rs> and Ruffle contributors.
//! MIT selected; pinned sources and modifications: docs/RUNTIME-STRINGS-PROVENANCE.md.

use crate::as2_strings::{MAX_AS2_STRING_UNITS, find, rfind, swf_to_lowercase, swf_to_uppercase};
use serde::{Deserialize, Serialize};

pub const MAX_AS3_STRING_JSON_BYTES: usize = 32 * 1024 * 1024;
const LIMIT: usize = MAX_AS2_STRING_UNITS;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct As3StringRequest {
    pub operation: String,
    #[serde(default)]
    pub receiver: Vec<u16>,
    #[serde(default)]
    pub strings: Vec<Vec<u16>>,
    #[serde(default)]
    pub numbers: Vec<String>,
    #[serde(default)]
    pub arg_count: usize,
    #[serde(default)]
    pub null_pattern: bool,
}
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum As3StringResult {
    String(Vec<u16>),
    Strings(Vec<Vec<u16>>),
    Number(String),
    Boolean(bool),
}
#[derive(Debug, thiserror::Error)]
#[error("AS3 String: {0}")]
pub struct As3StringError(pub String);
fn error(message: &str) -> As3StringError {
    As3StringError(message.into())
}
fn text(value: &[u16]) -> As3StringResult {
    As3StringResult::String(value.to_vec())
}
fn number(value: f64) -> As3StringResult {
    As3StringResult::Number(value.to_string())
}
fn numeric(request: &As3StringRequest, index: usize, default: f64) -> Result<f64, As3StringError> {
    match request.numbers.get(index).map(String::as_str) {
        None => Ok(default),
        Some("NaN") => Ok(f64::NAN),
        Some("Infinity") => Ok(f64::INFINITY),
        Some("-Infinity") => Ok(f64::NEG_INFINITY),
        Some(value) => value
            .parse()
            .map_err(|_| error("invalid numeric transport")),
    }
}
fn int(value: f64) -> f64 {
    if value.is_nan() { 0.0 } else { value.trunc() }
}
fn uint(value: f64) -> u32 {
    if !value.is_finite() || value == 0.0 {
        0
    } else {
        value.trunc().rem_euclid(4294967296.0) as u32
    }
}
// Ruffle's non-wrapping and wrapping slice indices, on bounded UTF-16 slices.
fn index(value: f64, length: usize) -> usize {
    if value < 0.0 {
        0
    } else {
        (value as usize).min(length)
    }
}
fn wrapping(value: f64, length: usize) -> usize {
    if value <= -1.0 {
        if value.is_infinite() {
            0
        } else {
            length.saturating_sub(value.abs().trunc() as usize)
        }
    } else {
        index(value, length)
    }
}
pub fn as3_string_execute(request: As3StringRequest) -> Result<As3StringResult, As3StringError> {
    let total = request
        .strings
        .iter()
        .try_fold(request.receiver.len(), |sum, s| sum.checked_add(s.len()))
        .ok_or_else(|| error("input size overflow"))?;
    if total > LIMIT
        || request.arg_count > LIMIT
        || request.strings.len() > LIMIT
        || request.numbers.len() > LIMIT
    {
        return Err(error(
            "input exceeds the supported string limit of 1000000 UTF-16 units/items",
        ));
    }
    let source = &request.receiver;
    let len = source.len();
    let empty = Vec::new();
    let pattern = request.strings.first().unwrap_or(&empty);
    let output = match request.operation.as_str() {
        "toString" | "valueOf" => text(source),
        "toLowerCase" | "toLocaleLowerCase" => {
            As3StringResult::String(source.iter().copied().map(swf_to_lowercase).collect())
        }
        "toUpperCase" | "toLocaleUpperCase" => {
            As3StringResult::String(source.iter().copied().map(swf_to_uppercase).collect())
        }
        "charAt" | "charCodeAt" => {
            let position = numeric(&request, 0, 0.0)?;
            let unit = if position < 0.0 {
                None
            } else {
                source.get(if position.is_nan() {
                    0
                } else {
                    position as usize
                })
            };
            if request.operation == "charAt" {
                text(unit.map(std::slice::from_ref).unwrap_or(&[]))
            } else {
                number(unit.map_or(f64::NAN, |u| f64::from(*u)))
            }
        }
        "fromCharCode" => As3StringResult::String(
            (0..request.numbers.len())
                .map(|i| numeric(&request, i, 0.0).map(|n| uint(n) as u16))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        "concat" => {
            let mut result = source.clone();
            for value in &request.strings {
                result.extend(value);
            }
            As3StringResult::String(result)
        }
        "indexOf" | "lastIndexOf" | "replaceIndex" => {
            if request.null_pattern {
                number(-1.0)
            } else {
                let position = numeric(
                    &request,
                    0,
                    if request.operation == "lastIndexOf" {
                        len as f64
                    } else {
                        0.0
                    },
                )?;
                let found = if request.operation == "lastIndexOf" {
                    if position < 0.0 {
                        None
                    } else {
                        let start = if position.is_nan() {
                            len
                        } else {
                            index(position, len)
                        };
                        rfind(
                            &source[..start.saturating_add(pattern.len()).min(len)],
                            pattern,
                        )
                    }
                } else {
                    let start = index(position, len);
                    find(&source[start..], pattern).map(|i| i + start)
                };
                number(found.map_or(-1.0, |i| i as f64))
            }
        }
        "localeCompare" => {
            // SWF11+ profile: lexicographic UTF-16 difference, not OS collation.
            let difference = source.iter().zip(pattern).find_map(|(left, right)| {
                if left == right {
                    None
                } else {
                    Some(i32::from(*left) - i32::from(*right))
                }
            });
            number(
                difference.unwrap_or_else(|| match source.len().cmp(&pattern.len()) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                }) as f64,
            )
        }
        "slice" | "substring" | "substr" => {
            let start = numeric(&request, 0, 0.0)?;
            let end = numeric(&request, 1, len as f64)?;
            let (mut from, mut to) = if request.operation == "substring" {
                (index(start, len), index(end, len))
            } else {
                let from = wrapping(start, len);
                // Keep the prior documented AVM2 negative-count profile, already
                // used by authored projects, rather than JS String.substr.
                let to = if request.operation == "substr" {
                    wrapping(from as f64 + int(end), len).max(from)
                } else {
                    wrapping(end, len)
                };
                (from, to)
            };
            if request.operation == "substring" && from > to {
                std::mem::swap(&mut from, &mut to);
            }
            text(if from < to { &source[from..to] } else { &[] })
        }
        "splitEarly" => As3StringResult::Boolean(
            uint(numeric(&request, 0, u32::MAX as f64)?) == 0 || source.is_empty(),
        ),
        "split" => {
            let limit = uint(numeric(&request, 0, u32::MAX as f64)?) as usize;
            let mut parts = Vec::new();
            if limit > 0 {
                if source.is_empty() {
                    parts.push(Vec::new());
                } else if pattern.is_empty() {
                    parts.extend(source.iter().take(limit).map(|u| vec![*u]));
                } else {
                    let mut start = 0;
                    while start <= len && parts.len() < limit {
                        if let Some(end) = find(&source[start..], pattern) {
                            parts.push(source[start..start + end].to_vec());
                            start += end + pattern.len();
                        } else {
                            parts.push(source[start..].to_vec());
                            break;
                        }
                    }
                }
            }
            As3StringResult::Strings(parts)
        }
        "replace" => {
            if let Some(at) = find(source, pattern) {
                let replacement = request
                    .strings
                    .get(1)
                    .ok_or_else(|| error("missing replacement transport"))?;
                let output_len = len - pattern.len() + replacement.len();
                if output_len > LIMIT {
                    return Err(error("replace result exceeds the supported string limit"));
                }
                let mut value = Vec::with_capacity(output_len);
                value.extend_from_slice(&source[..at]);
                value.extend(replacement);
                value.extend_from_slice(&source[at + pattern.len()..]);
                As3StringResult::String(value)
            } else {
                text(source)
            }
        }
        "escape" => As3StringResult::String(escape(source)),
        "unescape" => As3StringResult::String(unescape(source)),
        "encodeURI" | "encodeURIComponent" => {
            As3StringResult::String(encode_uri(source, request.operation == "encodeURI")?)
        }
        "decodeURI" | "decodeURIComponent" => {
            As3StringResult::String(decode_uri(source, request.operation == "decodeURI")?)
        }
        "escapeMultiByte" => As3StringResult::String(escape_multi_byte(source)),
        "unescapeMultiByte" => As3StringResult::String(unescape_multi_byte(source)),
        "isXMLName" => {
            As3StringResult::Boolean(!request.null_pattern && xml_name::is_xml_name(source))
        }
        _ => {
            return Err(error(&format!(
                "{} is outside the supported String profile",
                request.operation
            )));
        }
    };
    let size = match &output {
        As3StringResult::String(s) => s.len(),
        As3StringResult::Strings(a) => a.iter().map(Vec::len).sum::<usize>().max(a.len()),
        _ => 0,
    };
    if size > LIMIT {
        return Err(error(
            "output exceeds the supported string limit of 1000000 UTF-16 units/items",
        ));
    }
    Ok(output)
}
const HEX: &[u8; 16] = b"0123456789ABCDEF";
fn percent(output: &mut Vec<u16>, value: u16, wide: bool) {
    output.push(b'%' as u16);
    if wide {
        output.push(b'u' as u16);
        output.push(HEX[(value >> 12) as usize] as u16);
        output.push(HEX[((value >> 8) & 15) as usize] as u16);
    }
    output.push(HEX[((value >> 4) & 15) as usize] as u16);
    output.push(HEX[(value & 15) as usize] as u16);
}
fn hex(value: u16) -> Option<u16> {
    match value {
        48..=57 => Some(value - 48),
        65..=70 => Some(value - 55),
        97..=102 => Some(value - 87),
        _ => None,
    }
}
fn parse_hex(value: &[u16]) -> Option<u16> {
    value
        .iter()
        .try_fold(0, |sum, u| hex(*u).map(|n| sum * 16 + n))
}
// Adapted from AVM2 Toplevel escape/unescape: %uXXXX preserves lone surrogates.
fn escape(source: &[u16]) -> Vec<u16> {
    let mut output = Vec::new();
    for unit in source {
        if *unit < 128
            && (char::from_u32(*unit as u32).is_some_and(|c| c.is_ascii_alphanumeric())
                || b"@-_.*+/".contains(&(*unit as u8)))
        {
            output.push(*unit);
        } else {
            percent(&mut output, *unit, *unit > 255);
        }
    }
    output
}
fn unescape(source: &[u16]) -> Vec<u16> {
    let mut output = Vec::with_capacity(source.len());
    let mut at = 0;
    while at < source.len() {
        let unit = source[at];
        at += 1;
        if unit != b'%' as u16 {
            output.push(unit);
            continue;
        }
        let start = at;
        let wide = source.get(at) == Some(&(b'u' as u16));
        if wide {
            at += 1;
        }
        let size = if wide { 4 } else { 2 };
        if let Some(value) = source.get(at..at + size).and_then(parse_hex) {
            output.push(value);
            at += size;
        } else {
            output.push(b'%' as u16);
            at = start;
        }
    }
    output
}
fn encode_uri(source: &[u16], reserved: bool) -> Result<Vec<u16>, As3StringError> {
    let input = String::from_utf16(source)
        .map_err(|_| error("URIError #1052: unpaired UTF-16 surrogate"))?;
    let mut output = Vec::new();
    for byte in input.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || b"-_.!~*'()".contains(&byte)
            || (reserved && b";/?:@&=+$,#".contains(&byte));
        if keep {
            output.push(byte as u16);
        } else {
            percent(&mut output, byte as u16, false);
        }
        if output.len() > LIMIT {
            return Err(error("URI output exceeds the supported string limit"));
        }
    }
    Ok(output)
}
fn decode_uri(source: &[u16], reserved: bool) -> Result<Vec<u16>, As3StringError> {
    // Decode one percent-encoded UTF-8 scalar at a time; never silently replace
    // malformed UTF-8. Reserved escapes preserve their original hex spelling.
    let mut output = Vec::new();
    let mut at = 0;
    while at < source.len() {
        if source[at] != b'%' as u16 {
            output.push(source[at]);
            at += 1;
            continue;
        }
        let start = at;
        let first = source
            .get(at + 1..at + 3)
            .and_then(parse_hex)
            .ok_or_else(|| error("URIError #1052: malformed percent escape"))?
            as u8;
        at += 3;
        if first < 128 {
            if reserved && b"#$&+,/:;=?@".contains(&first) {
                output.extend_from_slice(&source[start..at]);
            } else {
                output.push(first as u16);
            }
            continue;
        }
        let count = first.leading_ones() as usize;
        if !(2..=4).contains(&count) {
            return Err(error("URIError #1052: invalid UTF-8 lead byte"));
        }
        let mut bytes = vec![first];
        for _ in 1..count {
            if source.get(at) != Some(&(b'%' as u16)) {
                return Err(error("URIError #1052: incomplete UTF-8 escape"));
            }
            let byte = source
                .get(at + 1..at + 3)
                .and_then(parse_hex)
                .ok_or_else(|| error("URIError #1052: malformed UTF-8 escape"))?
                as u8;
            bytes.push(byte);
            at += 3;
        }
        let decoded = std::str::from_utf8(&bytes)
            .map_err(|_| error("URIError #1052: invalid UTF-8 escape"))?;
        output.extend(decoded.encode_utf16());
    }
    String::from_utf16(&output).map_err(|_| error("URIError #1052: unpaired UTF-16 surrogate"))?;
    Ok(output)
}

// Adapted from flash/utils.rs: NUL termination and lossy UTF-8 differ from URI codecs.
fn escape_multi_byte(source: &[u16]) -> Vec<u16> {
    let input = String::from_utf16_lossy(source);
    let mut output = Vec::new();
    for byte in input.bytes() {
        if byte == 0 {
            break;
        }
        if byte.is_ascii_alphanumeric() {
            output.push(byte as u16);
        } else {
            percent(&mut output, byte as u16, false);
        }
    }
    output
}
fn unescape_multi_byte(source: &[u16]) -> Vec<u16> {
    let input = String::from_utf16_lossy(source);
    let mut chars = input.chars().peekable();
    let mut output = String::new();
    let mut bytes = Vec::new();
    while let Some(c) = chars.next() {
        if c == '\0' {
            break;
        }
        if c != '%' {
            output.push(c);
            continue;
        }
        loop {
            let high = chars.next().and_then(|c| c.to_digit(16));
            let low = if high.is_some() {
                chars.next().and_then(|c| c.to_digit(16))
            } else {
                None
            };
            let (Some(high), Some(low)) = (high, low) else {
                break;
            };
            bytes.push(((high << 4) | low) as u8);
            if chars.peek() != Some(&'%') {
                break;
            }
            chars.next();
        }
        output.push_str(&String::from_utf8_lossy(&bytes));
        bytes.clear();
    }
    output.encode_utf16().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(operation: &str, source: &str, strings: &[&str], numbers: &[&str]) -> As3StringResult {
        as3_string_execute(As3StringRequest {
            operation: operation.into(),
            receiver: source.encode_utf16().collect(),
            strings: strings.iter().map(|s| s.encode_utf16().collect()).collect(),
            numbers: numbers.iter().map(|s| s.to_string()).collect(),
            arg_count: numbers.len().max(strings.len()),
            null_pattern: false,
        })
        .unwrap()
    }
    #[test]
    fn adapted_as3_string_uri_family() {
        assert_eq!(
            run("toUpperCase", "straße", &[], &[]),
            text(&"STRAßE".encode_utf16().collect::<Vec<_>>())
        );
        assert_eq!(run("localeCompare", "abc", &["abd"], &[]), number(-1.0));
        assert_eq!(
            run("substr", "abcdef", &[], &["1", "-2"]),
            text(&"bcde".encode_utf16().collect::<Vec<_>>())
        );
        assert_eq!(
            run("split", "", &[""], &[]),
            As3StringResult::Strings(vec![vec![]])
        );
        assert_eq!(
            run("encodeURIComponent", "a/b α", &[], &[]),
            text(&"a%2Fb%20%CE%B1".encode_utf16().collect::<Vec<_>>())
        );
        assert_eq!(
            run("decodeURI", "%2f%23", &[], &[]),
            text(&"%2f%23".encode_utf16().collect::<Vec<_>>())
        );
        assert_eq!(
            unescape(&escape(&[0xD800, 0x20, 0x20AC])),
            vec![0xD800, 0x20, 0x20AC]
        );
        assert!(decode_uri(&"%E0%A4".encode_utf16().collect::<Vec<_>>(), false).is_err());
        assert_eq!(
            run("escapeMultiByte", "é +/", &[], &[]),
            text(&"%C3%A9%20%2B%2F".encode_utf16().collect::<Vec<_>>())
        );
        assert_eq!(
            run("unescapeMultiByte", "%C3%A9%20%2B%2F", &[], &[]),
            text(&"é +/".encode_utf16().collect::<Vec<_>>())
        );
        assert_eq!(
            run("isXMLName", "élève_2", &[], &[]),
            As3StringResult::Boolean(true)
        );
        assert_eq!(
            run("isXMLName", "a:b", &[], &[]),
            As3StringResult::Boolean(false)
        );
    }
}

// Historical E4X name ranges, adapted from pinned Ruffle is_xml_name.rs.
mod xml_name {
    // https://www.w3.org/TR/2004/REC-xml-20040204/#NT-Letter
    static LETTER_TABLE: &[(u32, u32)] = &[
        // Base char.
        (0x0041, 0x005A),
        (0x0061, 0x007A),
        (0x00C0, 0x00D6),
        (0x00D8, 0x00F6),
        (0x00F8, 0x00FF),
        (0x0100, 0x0131),
        (0x0134, 0x013E),
        (0x0141, 0x0148),
        (0x014A, 0x017E),
        (0x0180, 0x01C3),
        (0x01CD, 0x01F0),
        (0x01F4, 0x01F5),
        (0x01FA, 0x0217),
        (0x0250, 0x02A8),
        (0x02BB, 0x02C1),
        (0x0386, 0x0386), // single
        (0x0388, 0x038A),
        (0x038C, 0x038C), // single
        (0x038E, 0x03A1),
        (0x03A3, 0x03CE),
        (0x03D0, 0x03D6),
        (0x03DA, 0x03DA), // single
        (0x03DC, 0x03DC), // single
        (0x03DE, 0x03DE), // single
        (0x03E0, 0x03E0), // single
        (0x03E2, 0x03F3),
        (0x0401, 0x040C),
        (0x040E, 0x044F),
        (0x0451, 0x045C),
        (0x045E, 0x0481),
        (0x0490, 0x04C4),
        (0x04C7, 0x04C8),
        (0x04CB, 0x04CC),
        (0x04D0, 0x04EB),
        (0x04EE, 0x04F5),
        (0x04F8, 0x04F9),
        (0x0531, 0x0556),
        (0x0559, 0x0559), // single
        (0x0561, 0x0586),
        (0x05D0, 0x05EA),
        (0x05F0, 0x05F2),
        (0x0621, 0x063A),
        (0x0641, 0x064A),
        (0x0671, 0x06B7),
        (0x06BA, 0x06BE),
        (0x06C0, 0x06CE),
        (0x06D0, 0x06D3),
        (0x06D5, 0x06D5), // single
        (0x06E5, 0x06E6),
        (0x0905, 0x0939),
        (0x093D, 0x093D), // single
        (0x0958, 0x0961),
        (0x0985, 0x098C),
        (0x098F, 0x0990),
        (0x0993, 0x09A8),
        (0x09AA, 0x09B0),
        (0x09B2, 0x09B2), // single
        (0x09B6, 0x09B9),
        (0x09DC, 0x09DD),
        (0x09DF, 0x09E1),
        (0x09F0, 0x09F1),
        (0x0A05, 0x0A0A),
        (0x0A0F, 0x0A10),
        (0x0A13, 0x0A28),
        (0x0A2A, 0x0A30),
        (0x0A32, 0x0A33),
        (0x0A35, 0x0A36),
        (0x0A38, 0x0A39),
        (0x0A59, 0x0A5C),
        (0x0A5E, 0x0A5E), // single
        (0x0A72, 0x0A74),
        (0x0A85, 0x0A8B),
        (0x0A8D, 0x0A8D), // single
        (0x0A8F, 0x0A91),
        (0x0A93, 0x0AA8),
        (0x0AAA, 0x0AB0),
        (0x0AB2, 0x0AB3),
        (0x0AB5, 0x0AB9),
        (0x0ABD, 0x0ABD), // single
        (0x0AE0, 0x0AE0), // single
        (0x0B05, 0x0B0C),
        (0x0B0F, 0x0B10),
        (0x0B13, 0x0B28),
        (0x0B2A, 0x0B30),
        (0x0B32, 0x0B33),
        (0x0B36, 0x0B39),
        (0x0B3D, 0x0B3D), // single
        (0x0B5C, 0x0B5D),
        (0x0B5F, 0x0B61),
        (0x0B85, 0x0B8A),
        (0x0B8E, 0x0B90),
        (0x0B92, 0x0B95),
        (0x0B99, 0x0B9A),
        (0x0B9C, 0x0B9C), // single
        (0x0B9E, 0x0B9F),
        (0x0BA3, 0x0BA4),
        (0x0BA8, 0x0BAA),
        (0x0BAE, 0x0BB5),
        (0x0BB7, 0x0BB9),
        (0x0C05, 0x0C0C),
        (0x0C0E, 0x0C10),
        (0x0C12, 0x0C28),
        (0x0C2A, 0x0C33),
        (0x0C35, 0x0C39),
        (0x0C60, 0x0C61),
        (0x0C85, 0x0C8C),
        (0x0C8E, 0x0C90),
        (0x0C92, 0x0CA8),
        (0x0CAA, 0x0CB3),
        (0x0CB5, 0x0CB9),
        (0x0CDE, 0x0CDE), // single
        (0x0CE0, 0x0CE1),
        (0x0D05, 0x0D0C),
        (0x0D0E, 0x0D10),
        (0x0D12, 0x0D28),
        (0x0D2A, 0x0D39),
        (0x0D60, 0x0D61),
        (0x0E01, 0x0E2E),
        (0x0E30, 0x0E30), // single
        (0x0E32, 0x0E33),
        (0x0E40, 0x0E45),
        (0x0E81, 0x0E82),
        (0x0E84, 0x0E84), // single
        (0x0E87, 0x0E88),
        (0x0E8A, 0x0E8A), // single
        (0x0E8D, 0x0E8D), // single
        (0x0E94, 0x0E97),
        (0x0E99, 0x0E9F),
        (0x0EA1, 0x0EA3),
        (0x0EA5, 0x0EA5), // single
        (0x0EA7, 0x0EA7), // single
        (0x0EAA, 0x0EAB),
        (0x0EAD, 0x0EAE),
        (0x0EB0, 0x0EB0), // single
        (0x0EB2, 0x0EB3),
        (0x0EBD, 0x0EBD), // single
        (0x0EC0, 0x0EC4),
        (0x0F40, 0x0F47),
        (0x0F49, 0x0F69),
        (0x10A0, 0x10C5),
        (0x10D0, 0x10F6),
        (0x1100, 0x1100), // single
        (0x1102, 0x1103),
        (0x1105, 0x1107),
        (0x1109, 0x1109), // single
        (0x110B, 0x110C),
        (0x110E, 0x1112),
        (0x113C, 0x113C), // single
        (0x113E, 0x113E), // single
        (0x1140, 0x1140), // single
        (0x114C, 0x114C), // single
        (0x114E, 0x114E), // single
        (0x1150, 0x1150), // single
        (0x1154, 0x1155),
        (0x1159, 0x1159), // single
        (0x115F, 0x1161),
        (0x1163, 0x1163), // single
        (0x1165, 0x1165), // single
        (0x1167, 0x1167), // single
        (0x1169, 0x1169), // single
        (0x116D, 0x116E),
        (0x1172, 0x1173),
        (0x1175, 0x1175), // single
        (0x119E, 0x119E), // single
        (0x11A8, 0x11A8), // single
        (0x11AB, 0x11AB), // single
        (0x11AE, 0x11AF),
        (0x11B7, 0x11B8),
        (0x11BA, 0x11BA), // single
        (0x11BC, 0x11C2),
        (0x11EB, 0x11EB), // single
        (0x11F0, 0x11F0), // single
        (0x11F9, 0x11F9), // single
        (0x1E00, 0x1E9B),
        (0x1EA0, 0x1EF9),
        (0x1F00, 0x1F15),
        (0x1F18, 0x1F1D),
        (0x1F20, 0x1F45),
        (0x1F48, 0x1F4D),
        (0x1F50, 0x1F57),
        (0x1F59, 0x1F59), // single
        (0x1F5B, 0x1F5B), // single
        (0x1F5D, 0x1F5D), // single
        (0x1F5F, 0x1F7D),
        (0x1F80, 0x1FB4),
        (0x1FB6, 0x1FBC),
        (0x1FBE, 0x1FBE), // single
        (0x1FC2, 0x1FC4),
        (0x1FC6, 0x1FCC),
        (0x1FD0, 0x1FD3),
        (0x1FD6, 0x1FDB),
        (0x1FE0, 0x1FEC),
        (0x1FF2, 0x1FF4),
        (0x1FF6, 0x1FFC),
        (0x2126, 0x2126), // single
        (0x212A, 0x212B),
        (0x212E, 0x212E), // single
        (0x2180, 0x2182),
        (0x3007, 0x3007), // Ideographic, single
        (0x3021, 0x3029), // Ideographic
        (0x3041, 0x3094),
        (0x30A1, 0x30FA),
        (0x3105, 0x312C),
        (0x4E00, 0x9FA5), // Ideographic
        (0xAC00, 0xD7A3),
    ];

    // https://www.w3.org/TR/2004/REC-xml-20040204/#NT-Digit
    static DIGIT_TABLE: &[(u32, u32)] = &[
        (0x0030, 0x0039),
        (0x0660, 0x0669),
        (0x06F0, 0x06F9),
        (0x0966, 0x096F),
        (0x09E6, 0x09EF),
        (0x0A66, 0x0A6F),
        (0x0AE6, 0x0AEF),
        (0x0B66, 0x0B6F),
        (0x0BE7, 0x0BEF),
        (0x0C66, 0x0C6F),
        (0x0CE6, 0x0CEF),
        (0x0D66, 0x0D6F),
        (0x0E50, 0x0E59),
        (0x0ED0, 0x0ED9),
        (0x0F20, 0x0F29),
    ];

    // https://www.w3.org/TR/2004/REC-xml-20040204/#NT-CombiningChar
    static COMBINING_TABLE: &[(u32, u32)] = &[
        (0x0300, 0x0345),
        (0x0360, 0x0361),
        (0x0483, 0x0486),
        (0x0591, 0x05A1),
        (0x05A3, 0x05B9),
        (0x05BB, 0x05BD),
        (0x05BF, 0x05BF), //single
        (0x05C1, 0x05C2),
        (0x05C4, 0x05C4), //single
        (0x064B, 0x0652),
        (0x0670, 0x0670), //single
        (0x06D6, 0x06DC),
        (0x06DD, 0x06DF),
        (0x06E0, 0x06E4),
        (0x06E7, 0x06E8),
        (0x06EA, 0x06ED),
        (0x0901, 0x0903),
        (0x093C, 0x093C), //single
        (0x093E, 0x094C),
        (0x094D, 0x094D), //single
        (0x0951, 0x0954),
        (0x0962, 0x0963),
        (0x0981, 0x0983),
        (0x09BC, 0x09BC), //single
        (0x09BE, 0x09BE), //single
        (0x09BF, 0x09BF), //single
        (0x09C0, 0x09C4),
        (0x09C7, 0x09C8),
        (0x09CB, 0x09CD),
        (0x09D7, 0x09D7), //single
        (0x09E2, 0x09E3),
        (0x0A02, 0x0A02), //single
        (0x0A3C, 0x0A3C), //single
        (0x0A3E, 0x0A3E), //single
        (0x0A3F, 0x0A3F), //single
        (0x0A40, 0x0A42),
        (0x0A47, 0x0A48),
        (0x0A4B, 0x0A4D),
        (0x0A70, 0x0A71),
        (0x0A81, 0x0A83),
        (0x0ABC, 0x0ABC), //single
        (0x0ABE, 0x0AC5),
        (0x0AC7, 0x0AC9),
        (0x0ACB, 0x0ACD),
        (0x0B01, 0x0B03),
        (0x0B3C, 0x0B3C), //single
        (0x0B3E, 0x0B43),
        (0x0B47, 0x0B48),
        (0x0B4B, 0x0B4D),
        (0x0B56, 0x0B57),
        (0x0B82, 0x0B83),
        (0x0BBE, 0x0BC2),
        (0x0BC6, 0x0BC8),
        (0x0BCA, 0x0BCD),
        (0x0BD7, 0x0BD7), //single
        (0x0C01, 0x0C03),
        (0x0C3E, 0x0C44),
        (0x0C46, 0x0C48),
        (0x0C4A, 0x0C4D),
        (0x0C55, 0x0C56),
        (0x0C82, 0x0C83),
        (0x0CBE, 0x0CC4),
        (0x0CC6, 0x0CC8),
        (0x0CCA, 0x0CCD),
        (0x0CD5, 0x0CD6),
        (0x0D02, 0x0D03),
        (0x0D3E, 0x0D43),
        (0x0D46, 0x0D48),
        (0x0D4A, 0x0D4D),
        (0x0D57, 0x0D57), //single
        (0x0E31, 0x0E31), //single
        (0x0E34, 0x0E3A),
        (0x0E47, 0x0E4E),
        (0x0EB1, 0x0EB1), //single
        (0x0EB4, 0x0EB9),
        (0x0EBB, 0x0EBC),
        (0x0EC8, 0x0ECD),
        (0x0F18, 0x0F19),
        (0x0F35, 0x0F35), //single
        (0x0F37, 0x0F37), //single
        (0x0F39, 0x0F39), //single
        (0x0F3E, 0x0F3E), //single
        (0x0F3F, 0x0F3F), //single
        (0x0F71, 0x0F84),
        (0x0F86, 0x0F8B),
        (0x0F90, 0x0F95),
        (0x0F97, 0x0F97), //single
        (0x0F99, 0x0FAD),
        (0x0FB1, 0x0FB7),
        (0x0FB9, 0x0FB9), //single
        (0x20D0, 0x20DC),
        (0x20E1, 0x20E1), //single
        (0x302A, 0x302F),
        (0x3099, 0x3099), //single
        (0x309A, 0x309A), //single
    ];

    // https://www.w3.org/TR/2004/REC-xml-20040204/#NT-Extender
    static EXTENDER_TABLE: &[(u32, u32)] = &[
        (0x00B7, 0x00B7), // single
        (0x02D0, 0x02D0), // single
        (0x02D1, 0x02D1), // single
        (0x0387, 0x0387), // single
        (0x0640, 0x0640), // single
        (0x0E46, 0x0E46), // single
        (0x0EC6, 0x0EC6), // single
        (0x3005, 0x3005), // single
        (0x3031, 0x3035),
        (0x309D, 0x309E),
        (0x30FC, 0x30FE),
    ];

    fn is_letter(c: char) -> bool {
        let c: u32 = c.into();

        for (start, end) in LETTER_TABLE {
            if c < *start {
                return false;
            }

            if c >= *start && c <= *end {
                return true;
            }
        }

        false
    }

    fn is_digit(c: char) -> bool {
        let c: u32 = c.into();

        for (start, end) in DIGIT_TABLE {
            if c < *start {
                return false;
            }

            if c >= *start && c <= *end {
                return true;
            }
        }

        false
    }

    fn is_combining(c: char) -> bool {
        let c: u32 = c.into();

        for (start, end) in COMBINING_TABLE {
            if c < *start {
                return false;
            }

            if c >= *start && c <= *end {
                return true;
            }
        }

        false
    }

    fn is_extender(c: char) -> bool {
        let c: u32 = c.into();

        for (start, end) in EXTENDER_TABLE {
            if c < *start {
                return false;
            }

            if c >= *start && c <= *end {
                return true;
            }
        }

        false
    }

    // Based on https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/AvmCore.cpp#L3478
    pub(super) fn is_xml_name(name: &[u16]) -> bool {
        if name.is_empty() {
            return false;
        }

        let mut name = char::decode_utf16(name.iter().copied());
        let first_char = match name.next().expect("At least one character should exist") {
            Ok(c) => c,
            Err(_) => return false,
        };

        if !is_letter(first_char) && first_char != '_' {
            return false;
        }

        name.map(|x| x.ok()).all(|x| {
            if let Some(x) = x {
                is_digit(x)
                    || is_letter(x)
                    || x == '.'
                    || x == '-'
                    || x == '_'
                    || is_combining(x)
                    || is_extender(x)
            } else {
                false
            }
        })
    }
}
