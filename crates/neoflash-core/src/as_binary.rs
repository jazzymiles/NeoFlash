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
        "compress" | "uncompress" => {
            let zlib = match parameter {
                "zlib" => true,
                "deflate" => false,
                _ => {
                    return Err(error(
                        "unsupported compression algorithm; use zlib or deflate (LZMA is not implemented)",
                    ));
                }
            };
            if operation == "compress" {
                compress(bytes, zlib, limit)
            } else {
                decompress(bytes, zlib, limit)
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytearray_codec_family() {
        let source = "NeoFlash € 🦀 / Rust binary packet".repeat(24);
        for algorithm in ["zlib", "deflate"] {
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
                    &packed[..packed.len() - 2],
                    "",
                    algorithm,
                    4096
                )
                .is_err()
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
        assert!(binary_transform("compress", b"abc", "", "lzma", 100).is_err());
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
