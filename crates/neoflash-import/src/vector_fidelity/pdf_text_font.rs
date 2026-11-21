//! PDF 1.4–1.7 Font resources (`/WinAnsiEncoding`, `/MacRomanEncoding`, `/Differences`,
//! `/ToUnicode` UTF-16BE `CMap`s with Cyrillic/CJK/ligatures/surrogates, glyph widths)
//! and `BT .. ET` text state machine (`Tf`, `Tm`, `Td`, `TD`, `T*`, `TL`, `Tc`, `Tw`, `Tz`,
//! `Ts`, `Tj`, `TJ`, `'`, `"`) producing editable `ElementKind::TextBox` runs.

use super::pdf_color_fn::{function_dict, scalar};
use super::pdf_xref::decode_pdf_stream_bounded;
use super::*;
use lopdf::{Document, Object};
use neoflash_core::{
    AuthoredTextFieldOptions, BlendMode, TextFieldState, TextFormat, TextFormatRun, normalize_text,
};
use std::collections::HashMap;

const MAX_CMAP_STREAM_BYTES: usize = 4 * 1024 * 1024;
const MAX_CMAP_ENTRIES: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BaseByteEncoding {
    Standard,
    WinAnsi,
    MacRoman,
}

#[derive(Clone, Debug)]
pub(super) struct PdfFontResource {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub is_two_byte_cid: bool,
    base_encoding: BaseByteEncoding,
    differences: HashMap<u16, String>,
    to_unicode_1b: HashMap<u8, String>,
    to_unicode_2b: HashMap<u16, String>,
    code_lengths: Vec<u8>,
    widths: HashMap<u16, f64>,
    default_width: f64,
}

impl Default for PdfFontResource {
    fn default() -> Self {
        Self {
            family: "sans-serif".to_owned(),
            bold: false,
            italic: false,
            is_two_byte_cid: false,
            base_encoding: BaseByteEncoding::WinAnsi,
            differences: HashMap::new(),
            to_unicode_1b: HashMap::new(),
            to_unicode_2b: HashMap::new(),
            code_lengths: Vec::new(),
            widths: HashMap::new(),
            default_width: 550.0,
        }
    }
}

impl PdfFontResource {
    /// Decode a raw PDF byte string into Unicode text and return `(decoded_text, total_1000ths_em_width, char_count, space_count)`.
    pub fn decode_bytes(&self, bytes: &[u8]) -> (String, f64, usize, usize) {
        // Explicit UTF-16BE BOM string
        if bytes.starts_with(&[0xFE, 0xFF]) && bytes.len() >= 4 {
            let units: Vec<u16> = bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_be_bytes)
                .collect();
            let text: String = char::decode_utf16(units)
                .map(|r| r.unwrap_or('\u{FFFD}'))
                .filter(|&c| c != '\0')
                .collect();
            let chars = text.chars().count();
            let spaces = text.chars().filter(|&c| c == ' ').count();
            let width = (chars as f64) * self.default_width;
            return (text, width, chars, spaces);
        }

        let mut out = String::new();
        let mut width_1000 = 0.0;
        let mut char_codes = 0usize;
        let mut space_codes = 0usize;

        let has_cmap = !self.to_unicode_1b.is_empty() || !self.to_unicode_2b.is_empty();
        let prefer_2b = self.is_two_byte_cid
            || (!self.to_unicode_2b.is_empty()
                && self.to_unicode_1b.is_empty()
                && bytes.len().is_multiple_of(2));

        let mut i = 0usize;
        while i < bytes.len() {
            if has_cmap {
                // Check 2-byte CMap first when 2-byte codespace or CID font is active
                if (prefer_2b || self.code_lengths.contains(&2)) && i + 1 < bytes.len() {
                    let code2 = u16::from_be_bytes([bytes[i], bytes[i + 1]]);
                    if let Some(mapped) = self.to_unicode_2b.get(&code2) {
                        out.push_str(mapped);
                        width_1000 += self
                            .widths
                            .get(&code2)
                            .copied()
                            .unwrap_or(self.default_width);
                        char_codes += 1;
                        if mapped == " " {
                            space_codes += 1;
                        }
                        i += 2;
                        continue;
                    }
                    if prefer_2b && !self.to_unicode_1b.contains_key(&bytes[i]) {
                        if let Some(diff) = self.differences.get(&code2) {
                            out.push_str(diff);
                        } else if let Some(ch) = char::from_u32(u32::from(code2))
                            && ch != '\0'
                        {
                            out.push(ch);
                        }
                        width_1000 += self
                            .widths
                            .get(&code2)
                            .copied()
                            .unwrap_or(self.default_width);
                        char_codes += 1;
                        if code2 == 0x0020 {
                            space_codes += 1;
                        }
                        i += 2;
                        continue;
                    }
                }
                if let Some(mapped) = self.to_unicode_1b.get(&bytes[i]) {
                    let code1 = u16::from(bytes[i]);
                    out.push_str(mapped);
                    width_1000 += self
                        .widths
                        .get(&code1)
                        .copied()
                        .unwrap_or(self.default_width);
                    char_codes += 1;
                    if mapped == " " || bytes[i] == b' ' {
                        space_codes += 1;
                    }
                    i += 1;
                    continue;
                }
                if i + 1 < bytes.len() {
                    let code2 = u16::from_be_bytes([bytes[i], bytes[i + 1]]);
                    if let Some(mapped) = self.to_unicode_2b.get(&code2) {
                        out.push_str(mapped);
                        width_1000 += self
                            .widths
                            .get(&code2)
                            .copied()
                            .unwrap_or(self.default_width);
                        char_codes += 1;
                        if mapped == " " {
                            space_codes += 1;
                        }
                        i += 2;
                        continue;
                    }
                }
            }

            if self.is_two_byte_cid && i + 1 < bytes.len() {
                let code2 = u16::from_be_bytes([bytes[i], bytes[i + 1]]);
                if let Some(diff) = self.differences.get(&code2) {
                    out.push_str(diff);
                } else if let Some(ch) = char::from_u32(u32::from(code2))
                    && ch != '\0'
                {
                    out.push(ch);
                }
                width_1000 += self
                    .widths
                    .get(&code2)
                    .copied()
                    .unwrap_or(self.default_width);
                char_codes += 1;
                if code2 == 0x0020 {
                    space_codes += 1;
                }
                i += 2;
                continue;
            }

            let b = bytes[i];
            let code1 = u16::from(b);
            if let Some(diff) = self.differences.get(&code1) {
                out.push_str(diff);
                if diff == " " {
                    space_codes += 1;
                }
            } else {
                let ch = match self.base_encoding {
                    BaseByteEncoding::WinAnsi => win_ansi_to_char(b),
                    BaseByteEncoding::MacRoman => mac_roman_to_char(b),
                    BaseByteEncoding::Standard => standard_encoding_to_char(b),
                };
                if ch != '\0' {
                    out.push(ch);
                }
                if b == b' ' {
                    space_codes += 1;
                }
            }
            width_1000 += self
                .widths
                .get(&code1)
                .copied()
                .unwrap_or_else(|| approximate_ascii_width_1000(b, self.default_width));
            char_codes += 1;
            i += 1;
        }

        (out, width_1000, char_codes, space_codes)
    }
}

fn approximate_ascii_width_1000(b: u8, fallback: f64) -> f64 {
    match b {
        b' ' => 278.0,
        b'i' | b'l' | b'I' | b'!' | b'|' | b'.' | b',' | b':' | b';' | b'\'' => 278.0,
        b'f' | b'j' | b'r' | b't' | b'(' | b')' | b'[' | b']' => 333.0,
        b'm' | b'w' => 833.0,
        b'M' | b'W' => 889.0,
        b'A'..=b'Z' => 667.0,
        b'a'..=b'z' | b'0'..=b'9' => 556.0,
        _ => fallback,
    }
}

fn win_ansi_to_char(b: u8) -> char {
    match b {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        other => char::from(other),
    }
}

const MAC_ROMAN_HIGH: [char; 128] = [
    '\u{00C4}', '\u{00C5}', '\u{00C7}', '\u{00C9}', '\u{00D1}', '\u{00D6}', '\u{00DC}', '\u{00E1}',
    '\u{00E0}', '\u{00E2}', '\u{00E4}', '\u{00E3}', '\u{00E5}', '\u{00E7}', '\u{00E9}', '\u{00E8}',
    '\u{00EA}', '\u{00EB}', '\u{00ED}', '\u{00EC}', '\u{00EE}', '\u{00EF}', '\u{00F1}', '\u{00F3}',
    '\u{00F2}', '\u{00F4}', '\u{00F6}', '\u{00F5}', '\u{00FA}', '\u{00F9}', '\u{00FB}', '\u{00FC}',
    '\u{2020}', '\u{00B0}', '\u{00A2}', '\u{00A3}', '\u{00A7}', '\u{2022}', '\u{00B6}', '\u{00DF}',
    '\u{00AE}', '\u{00A9}', '\u{2122}', '\u{00B4}', '\u{00A8}', '\u{2260}', '\u{00C6}', '\u{00D8}',
    '\u{221E}', '\u{00B1}', '\u{2264}', '\u{2265}', '\u{00A5}', '\u{00B5}', '\u{2202}', '\u{2211}',
    '\u{220F}', '\u{03C0}', '\u{222B}', '\u{00AA}', '\u{00BA}', '\u{03A9}', '\u{00E6}', '\u{00F8}',
    '\u{00BF}', '\u{00A1}', '\u{00AC}', '\u{221A}', '\u{0192}', '\u{2248}', '\u{2206}', '\u{00AB}',
    '\u{00BB}', '\u{2026}', '\u{00A0}', '\u{00C0}', '\u{00C3}', '\u{00D5}', '\u{0152}', '\u{0153}',
    '\u{2013}', '\u{2014}', '\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}', '\u{00F7}', '\u{25CA}',
    '\u{00FF}', '\u{0178}', '\u{2044}', '\u{20AC}', '\u{2039}', '\u{203A}', '\u{FB01}', '\u{FB02}',
    '\u{2021}', '\u{00B7}', '\u{201A}', '\u{201E}', '\u{2030}', '\u{00C2}', '\u{00CA}', '\u{00C1}',
    '\u{00CB}', '\u{00C8}', '\u{00CD}', '\u{00CE}', '\u{00CF}', '\u{00CC}', '\u{00D3}', '\u{00D4}',
    '\u{F8FF}', '\u{00D2}', '\u{00DA}', '\u{00DB}', '\u{00D9}', '\u{0131}', '\u{02C6}', '\u{02DC}',
    '\u{00AF}', '\u{02D8}', '\u{02D9}', '\u{02DA}', '\u{00B8}', '\u{02DD}', '\u{02DB}', '\u{02C7}',
];

fn mac_roman_to_char(b: u8) -> char {
    if b < 0x80 {
        char::from(b)
    } else {
        MAC_ROMAN_HIGH[usize::from(b - 0x80)]
    }
}

fn standard_encoding_to_char(b: u8) -> char {
    match b {
        0x27 => '\u{2019}',
        0x60 => '\u{2018}',
        0xA1 => '\u{00A1}',
        0xA2 => '\u{00A2}',
        0xA3 => '\u{00A3}',
        0xA4 => '\u{2044}',
        0xA5 => '\u{00A5}',
        0xA6 => '\u{0192}',
        0xA7 => '\u{00A7}',
        0xA8 => '\u{00A4}',
        0xA9 => '\'',
        0xAA => '\u{201C}',
        0xAB => '\u{00AB}',
        0xAC => '\u{2039}',
        0xAD => '\u{203A}',
        0xAE => '\u{FB01}',
        0xAF => '\u{FB02}',
        0xB1 => '\u{2013}',
        0xB2 => '\u{2020}',
        0xB3 => '\u{2021}',
        0xB4 => '\u{00B7}',
        0xB6 => '\u{00B6}',
        0xB7 => '\u{2022}',
        0xB8 => '\u{201A}',
        0xB9 => '\u{201E}',
        0xBA => '\u{201D}',
        0xBB => '\u{00BB}',
        0xBC => '\u{2026}',
        0xBD => '\u{2030}',
        0xBF => '\u{00BF}',
        0xD0 => '\u{2014}',
        0xE1 => '\u{00C6}',
        0xE3 => '\u{00AA}',
        0xE8 => '\u{0141}',
        0xE9 => '\u{00D8}',
        0xEA => '\u{0152}',
        0xEB => '\u{00BA}',
        0xF1 => '\u{00E6}',
        0xF5 => '\u{0131}',
        0xF8 => '\u{0142}',
        0xF9 => '\u{00F8}',
        0xFA => '\u{0153}',
        0xFB => '\u{00DF}',
        other => win_ansi_to_char(other),
    }
}

fn glyph_name_to_unicode(raw_name: &str) -> Option<String> {
    let base = raw_name.split('.').next().unwrap_or(raw_name);
    if base.is_empty() || base == ".notdef" {
        return None;
    }
    if base.len() == 1 {
        return Some(base.to_owned());
    }
    // `uniXXXX` or `uniXXXXYYYY...` (4-hex-digit UTF-16BE chunks)
    if let Some(hex) = base.strip_prefix("uni")
        && hex.len() >= 4
        && hex.len().is_multiple_of(4)
        && hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        let units: Vec<u16> = (0..hex.len())
            .step_by(4)
            .filter_map(|i| u16::from_str_radix(&hex[i..i + 4], 16).ok())
            .collect();
        let s: String = char::decode_utf16(units).filter_map(Result::ok).collect();
        if !s.is_empty() {
            return Some(s);
        }
    }
    // `uXXXX`..`uXXXXXX`
    if let Some(hex) = base.strip_prefix('u')
        && (4..=6).contains(&hex.len())
        && hex.chars().all(|c| c.is_ascii_hexdigit())
        && let Ok(cp) = u32::from_str_radix(hex, 16)
        && let Some(ch) = char::from_u32(cp)
    {
        return Some(ch.to_string());
    }

    let mapped = match base {
        "space" => " ",
        "nbspace" | "uni00A0" => "\u{00A0}",
        "exclam" => "!",
        "quotedbl" => "\"",
        "numbersign" => "#",
        "dollar" => "$",
        "percent" => "%",
        "ampersand" => "&",
        "quotesingle" => "'",
        "parenleft" => "(",
        "parenright" => ")",
        "asterisk" => "*",
        "plus" => "+",
        "comma" => ",",
        "hyphen" | "minus" => "-",
        "endash" => "\u{2013}",
        "emdash" => "\u{2014}",
        "period" => ".",
        "ellipsis" => "\u{2026}",
        "slash" => "/",
        "zero" => "0",
        "one" => "1",
        "two" => "2",
        "three" => "3",
        "four" => "4",
        "five" => "5",
        "six" => "6",
        "seven" => "7",
        "eight" => "8",
        "nine" => "9",
        "colon" => ":",
        "semicolon" => ";",
        "less" => "<",
        "equal" => "=",
        "greater" => ">",
        "question" => "?",
        "at" => "@",
        "bracketleft" => "[",
        "backslash" => "\\",
        "bracketright" => "]",
        "asciicircum" => "^",
        "underscore" => "_",
        "grave" => "`",
        "braceleft" => "{",
        "bar" => "|",
        "braceright" => "}",
        "asciitilde" => "~",
        "quoteleft" => "\u{2018}",
        "quoteright" => "\u{2019}",
        "quotedblleft" => "\u{201C}",
        "quotedblright" => "\u{201D}",
        "quotesinglbase" => "\u{201A}",
        "quotedblbase" => "\u{201E}",
        "guilsinglleft" => "\u{2039}",
        "guilsinglright" => "\u{203A}",
        "guillemotleft" => "\u{00AB}",
        "guillemotright" => "\u{00BB}",
        "bullet" => "\u{2022}",
        "dagger" => "\u{2020}",
        "daggerdbl" => "\u{2021}",
        "perthousand" => "\u{2030}",
        "Euro" | "euro" => "\u{20AC}",
        "sterling" => "\u{00A3}",
        "yen" => "\u{00A5}",
        "cent" => "\u{00A2}",
        "copyright" => "\u{00A9}",
        "registered" => "\u{00AE}",
        "trademark" => "\u{2122}",
        "degree" => "\u{00B0}",
        "plusminus" => "\u{00B1}",
        "multiply" => "\u{00D7}",
        "divide" => "\u{00F7}",
        "fi" => "fi",
        "fl" => "fl",
        "ff" => "ff",
        "ffi" => "ffi",
        "ffl" => "ffl",
        "AE" => "\u{00C6}",
        "ae" => "\u{00E6}",
        "OE" => "\u{0152}",
        "oe" => "\u{0153}",
        "Oslash" => "\u{00D8}",
        "oslash" => "\u{00F8}",
        "germandbls" => "\u{00DF}",
        "Agrave" => "\u{00C0}",
        "Aacute" => "\u{00C1}",
        "Acircumflex" => "\u{00C2}",
        "Atilde" => "\u{00C3}",
        "Adieresis" => "\u{00C4}",
        "Aring" => "\u{00C5}",
        "Ccedilla" => "\u{00C7}",
        "Egrave" => "\u{00C8}",
        "Eacute" => "\u{00C9}",
        "Ecircumflex" => "\u{00CA}",
        "Edieresis" => "\u{00CB}",
        "Igrave" => "\u{00CC}",
        "Iacute" => "\u{00CD}",
        "Icircumflex" => "\u{00CE}",
        "Idieresis" => "\u{00CF}",
        "Ntilde" => "\u{00D1}",
        "Ograve" => "\u{00D2}",
        "Oacute" => "\u{00D3}",
        "Ocircumflex" => "\u{00D4}",
        "Otilde" => "\u{00D5}",
        "Odieresis" => "\u{00D6}",
        "Ugrave" => "\u{00D9}",
        "Uacute" => "\u{00DA}",
        "Ucircumflex" => "\u{00DB}",
        "Udieresis" => "\u{00DC}",
        "Yacute" => "\u{00DD}",
        "Ydieresis" => "\u{0178}",
        "agrave" => "\u{00E0}",
        "aacute" => "\u{00E1}",
        "acircumflex" => "\u{00E2}",
        "atilde" => "\u{00E3}",
        "adieresis" => "\u{00E4}",
        "aring" => "\u{00E5}",
        "ccedilla" => "\u{00E7}",
        "egrave" => "\u{00E8}",
        "eacute" => "\u{00E9}",
        "ecircumflex" => "\u{00EA}",
        "edieresis" => "\u{00EB}",
        "igrave" => "\u{00EC}",
        "iacute" => "\u{00ED}",
        "icircumflex" => "\u{00EE}",
        "idieresis" => "\u{00EF}",
        "ntilde" => "\u{00F1}",
        "ograve" => "\u{00F2}",
        "oacute" => "\u{00F3}",
        "ocircumflex" => "\u{00F4}",
        "otilde" => "\u{00F5}",
        "odieresis" => "\u{00F6}",
        "ugrave" => "\u{00F9}",
        "uacute" => "\u{00FA}",
        "ucircumflex" => "\u{00FB}",
        "udieresis" => "\u{00FC}",
        "yacute" => "\u{00FD}",
        "ydieresis" => "\u{00FF}",
        "Scaron" => "\u{0160}",
        "scaron" => "\u{0161}",
        "Zcaron" => "\u{017D}",
        "zcaron" => "\u{017E}",
        // Standard Adobe Cyrillic glyph names (`afii10017`..`afii10109`)
        s if s.starts_with("afii") => {
            if let Ok(num) = s[4..].parse::<u32>() {
                let cp = match num {
                    10023 => Some(0x0401),
                    10071 => Some(0x0451),
                    10017..=10048 => Some(0x0410 + (num - 10017)),
                    10065..=10096 => Some(0x0430 + (num - 10065)),
                    _ => None,
                };
                if let Some(c) = cp.and_then(char::from_u32) {
                    return Some(c.to_string());
                }
            }
            return None;
        }
        _ => return None,
    };
    Some(mapped.to_owned())
}

fn parse_hex_bytes(hex_tok: &str) -> Option<Vec<u8>> {
    let inner = hex_tok.trim().strip_prefix('<')?.strip_suffix('>')?;
    let mut nibbles: Vec<u8> = inner
        .chars()
        .filter_map(|c| c.to_digit(16).map(|d| d as u8))
        .collect();
    if !nibbles.len().is_multiple_of(2) {
        nibbles.push(0);
    }
    Some(
        nibbles
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| (p[0] << 4) | p[1])
            .collect(),
    )
}

fn decode_utf16be_hex_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if bytes.len() == 1 {
        return char::from(bytes[0]).to_string();
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(u16::from_be_bytes)
        .collect();
    char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .filter(|&c| c != '\0')
        .collect()
}

fn increment_be_bytes(buf: &mut [u8], delta: u32) {
    let mut carry = u64::from(delta);
    for b in buf.iter_mut().rev() {
        if carry == 0 {
            break;
        }
        let sum = u64::from(*b) + (carry & 0xFF);
        *b = (sum & 0xFF) as u8;
        carry = (carry >> 8) + (sum >> 8);
    }
}

fn tokenize_cmap(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if tokens.len() >= 250_000 {
            break;
        }
        if ch.is_ascii_whitespace() {
            chars.next();
            continue;
        }
        if ch == '%' {
            for c in chars.by_ref() {
                if c == '\n' || c == '\r' {
                    break;
                }
            }
            continue;
        }
        if ch == '<' {
            chars.next();
            if chars.peek() == Some(&'<') {
                chars.next();
                tokens.push("<<".to_owned());
                continue;
            }
            let mut s = String::from("<");
            for c in chars.by_ref() {
                if c == '>' {
                    s.push('>');
                    break;
                }
                if !c.is_ascii_whitespace() {
                    s.push(c);
                }
            }
            if !s.ends_with('>') {
                s.push('>');
            }
            tokens.push(s);
            continue;
        }
        if ch == '>' {
            chars.next();
            if chars.peek() == Some(&'>') {
                chars.next();
                tokens.push(">>".to_owned());
            } else {
                tokens.push(">".to_owned());
            }
            continue;
        }
        if matches!(ch, '[' | ']') {
            tokens.push(chars.next().unwrap().to_string());
            continue;
        }
        let mut word = String::new();
        if ch == '/' {
            word.push(chars.next().unwrap());
        }
        while let Some(&c) = chars.peek() {
            if c.is_ascii_whitespace() || matches!(c, '<' | '>' | '[' | ']' | '%' | '/') {
                break;
            }
            word.push(chars.next().unwrap());
        }
        if !word.is_empty() {
            tokens.push(word);
        }
    }
    tokens
}

fn parse_to_unicode_cmap(text: &str, font: &mut PdfFontResource) {
    let tokens = tokenize_cmap(text);
    let mut idx = 0usize;
    let mut entries_added = 0usize;

    let insert_code = |font: &mut PdfFontResource, src_bytes: &[u8], dst: String| {
        if dst.is_empty() {
            return;
        }
        match src_bytes {
            [b] => {
                font.to_unicode_1b.insert(*b, dst);
            }
            [hi, lo] => {
                font.to_unicode_2b
                    .insert(u16::from_be_bytes([*hi, *lo]), dst);
            }
            other if other.len() > 2 => {
                let n = other.len();
                font.to_unicode_2b
                    .insert(u16::from_be_bytes([other[n - 2], other[n - 1]]), dst);
            }
            _ => {}
        }
    };

    while idx < tokens.len() && entries_added < MAX_CMAP_ENTRIES {
        match tokens[idx].as_str() {
            "begincodespacerange" => {
                idx += 1;
                while idx < tokens.len() && tokens[idx] != "endcodespacerange" {
                    if let (Some(lo), Some(_hi)) = (
                        tokens.get(idx).and_then(|t| parse_hex_bytes(t)),
                        tokens.get(idx + 1).and_then(|t| parse_hex_bytes(t)),
                    ) {
                        let len = lo.len() as u8;
                        if (1..=4).contains(&len) && !font.code_lengths.contains(&len) {
                            font.code_lengths.push(len);
                        }
                        idx += 2;
                    } else {
                        idx += 1;
                    }
                }
                idx += 1;
            }
            "beginbfchar" => {
                idx += 1;
                while idx < tokens.len()
                    && tokens[idx] != "endbfchar"
                    && entries_added < MAX_CMAP_ENTRIES
                {
                    let Some(src) = tokens.get(idx).and_then(|t| parse_hex_bytes(t)) else {
                        idx += 1;
                        continue;
                    };
                    let Some(dst_tok) = tokens.get(idx + 1) else {
                        break;
                    };
                    let dst_str = if dst_tok.starts_with('<') {
                        parse_hex_bytes(dst_tok)
                            .map(|b| decode_utf16be_hex_bytes(&b))
                            .unwrap_or_default()
                    } else if let Some(name) = dst_tok.strip_prefix('/') {
                        glyph_name_to_unicode(name).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    insert_code(font, &src, dst_str);
                    entries_added += 1;
                    idx += 2;
                }
                idx += 1;
            }
            "beginbfrange" => {
                idx += 1;
                while idx < tokens.len()
                    && tokens[idx] != "endbfrange"
                    && entries_added < MAX_CMAP_ENTRIES
                {
                    let (Some(lo_bytes), Some(hi_bytes)) = (
                        tokens.get(idx).and_then(|t| parse_hex_bytes(t)),
                        tokens.get(idx + 1).and_then(|t| parse_hex_bytes(t)),
                    ) else {
                        idx += 1;
                        continue;
                    };
                    if lo_bytes.is_empty() || lo_bytes.len() != hi_bytes.len() || lo_bytes.len() > 2
                    {
                        idx += 3;
                        continue;
                    }
                    let lo_code = if lo_bytes.len() == 1 {
                        u32::from(lo_bytes[0])
                    } else {
                        u32::from(u16::from_be_bytes([lo_bytes[0], lo_bytes[1]]))
                    };
                    let hi_code = if hi_bytes.len() == 1 {
                        u32::from(hi_bytes[0])
                    } else {
                        u32::from(u16::from_be_bytes([hi_bytes[0], hi_bytes[1]]))
                    };
                    let Some(third) = tokens.get(idx + 2) else {
                        break;
                    };
                    if third == "[" {
                        idx += 3;
                        let mut code = lo_code;
                        while idx < tokens.len() && tokens[idx] != "]" {
                            if code <= hi_code && entries_added < MAX_CMAP_ENTRIES {
                                let dst_str = if tokens[idx].starts_with('<') {
                                    parse_hex_bytes(&tokens[idx])
                                        .map(|b| decode_utf16be_hex_bytes(&b))
                                        .unwrap_or_default()
                                } else if let Some(name) = tokens[idx].strip_prefix('/') {
                                    glyph_name_to_unicode(name).unwrap_or_default()
                                } else {
                                    String::new()
                                };
                                let src = if lo_bytes.len() == 1 {
                                    vec![(code & 0xFF) as u8]
                                } else {
                                    (code as u16).to_be_bytes().to_vec()
                                };
                                insert_code(font, &src, dst_str);
                                entries_added += 1;
                            }
                            code = code.saturating_add(1);
                            idx += 1;
                        }
                        if idx < tokens.len() && tokens[idx] == "]" {
                            idx += 1;
                        }
                    } else {
                        if let Some(base_dst) = parse_hex_bytes(third)
                            && hi_code >= lo_code
                        {
                            let count = (hi_code - lo_code + 1).min(4096);
                            for step in 0..count {
                                if entries_added >= MAX_CMAP_ENTRIES {
                                    break;
                                }
                                let code = lo_code + step;
                                let mut cur_dst = base_dst.clone();
                                increment_be_bytes(&mut cur_dst, step);
                                let dst_str = decode_utf16be_hex_bytes(&cur_dst);
                                let src = if lo_bytes.len() == 1 {
                                    vec![(code & 0xFF) as u8]
                                } else {
                                    (code as u16).to_be_bytes().to_vec()
                                };
                                insert_code(font, &src, dst_str);
                                entries_added += 1;
                            }
                        }
                        idx += 3;
                    }
                }
                idx += 1;
            }
            _ => {
                idx += 1;
            }
        }
    }
}

fn clean_pdf_font_name(raw: &str) -> (String, bool, bool) {
    let trimmed = raw.trim().trim_start_matches('/');
    // Strip 6-uppercase-letter subset tag `ABCDEF+MyriadPro-BoldIt`
    let unsubset = if let Some((prefix, rest)) = trimmed.split_once('+')
        && prefix.len() == 6
        && prefix.chars().all(|c| c.is_ascii_uppercase())
        && !rest.is_empty()
    {
        rest
    } else {
        trimmed
    };

    let lower = unsubset.to_ascii_lowercase();
    let bold = lower.contains("bold")
        || lower.contains("semibold")
        || lower.contains("demibold")
        || lower.contains("black")
        || lower.contains("heavy");
    let italic = lower.contains("italic")
        || lower.contains("oblique")
        || lower.ends_with("it")
        || lower.contains("-it");

    // Extract clean family before style hyphen/comma (`MyriadPro-BoldIt` -> `MyriadPro`, `Arial,Bold` -> `Arial`)
    let base_part = unsubset
        .split([',', '-'])
        .next()
        .unwrap_or(unsubset)
        .replace('_', " ")
        .trim()
        .to_owned();

    let family = if base_part.is_empty() {
        "sans-serif".to_owned()
    } else {
        base_part
    };
    (family, bold, italic)
}

fn parse_cid_widths_array(doc: &Document, w_arr: &[Object], out: &mut HashMap<u16, f64>) {
    let mut i = 0usize;
    while i + 1 < w_arr.len() {
        let Some(c_first) = w_arr[i].as_i64().ok().filter(|&v| (0..=65535).contains(&v)) else {
            i += 1;
            continue;
        };
        let second = doc
            .dereference(&w_arr[i + 1])
            .map_or(&w_arr[i + 1], |(_, o)| o);
        if let Ok(sub_arr) = second.as_array() {
            for (offset, val) in sub_arr.iter().enumerate() {
                if let Some(w) = scalar(val)
                    && let Ok(cid) = u16::try_from(c_first as usize + offset)
                {
                    out.insert(cid, w.clamp(0.0, 4000.0));
                }
            }
            i += 2;
        } else if i + 2 < w_arr.len()
            && let (Some(c_last), Some(w)) = (
                w_arr[i + 1]
                    .as_i64()
                    .ok()
                    .filter(|&v| v >= c_first && v <= 65535),
                scalar(
                    doc.dereference(&w_arr[i + 2])
                        .map_or(&w_arr[i + 2], |(_, o)| o),
                ),
            )
        {
            for cid in (c_first as u16)..=(c_last as u16).min((c_first as u16).saturating_add(4096))
            {
                out.insert(cid, w.clamp(0.0, 4000.0));
            }
            i += 3;
        } else {
            i += 1;
        }
    }
}

pub(super) fn parse_pdf_font_resource(doc: &Document, obj: &Object) -> Option<PdfFontResource> {
    let dict = function_dict(doc, obj)?;
    let mut font = PdfFontResource::default();

    let subtype = dict
        .get(b"Subtype")
        .ok()
        .and_then(|o| o.as_name().ok())
        .unwrap_or(b"");
    if subtype == b"Type0" {
        font.is_two_byte_cid = true;
    }

    if let Ok(base_obj) = dict.get(b"BaseFont")
        && let Ok(name) = doc
            .dereference(base_obj)
            .map_or(base_obj, |(_, o)| o)
            .as_name()
    {
        let raw_str = String::from_utf8_lossy(name);
        let (fam, bold, italic) = clean_pdf_font_name(&raw_str);
        font.family = fam;
        font.bold = bold;
        font.italic = italic;
    }

    // Encoding (`/WinAnsiEncoding`, `/MacRomanEncoding`, or dictionary with `/BaseEncoding` + `/Differences`)
    if let Ok(enc_obj) = dict.get(b"Encoding") {
        let resolved_enc = doc.dereference(enc_obj).map_or(enc_obj, |(_, o)| o);
        if let Ok(enc_name) = resolved_enc.as_name() {
            match enc_name {
                b"MacRomanEncoding" => font.base_encoding = BaseByteEncoding::MacRoman,
                b"StandardEncoding" => font.base_encoding = BaseByteEncoding::Standard,
                b"WinAnsiEncoding" => font.base_encoding = BaseByteEncoding::WinAnsi,
                b"Identity-H" | b"Identity-V" => font.is_two_byte_cid = true,
                _ => {}
            }
        } else if let Some(enc_dict) = function_dict(doc, resolved_enc) {
            if let Ok(base_enc) = enc_dict.get(b"BaseEncoding")
                && let Ok(base_name) = base_enc.as_name()
            {
                match base_name {
                    b"MacRomanEncoding" => font.base_encoding = BaseByteEncoding::MacRoman,
                    b"StandardEncoding" => font.base_encoding = BaseByteEncoding::Standard,
                    _ => font.base_encoding = BaseByteEncoding::WinAnsi,
                }
            }
            if let Ok(diff_obj) = enc_dict.get(b"Differences")
                && let Ok(diff_arr) = doc
                    .dereference(diff_obj)
                    .map_or(diff_obj, |(_, o)| o)
                    .as_array()
            {
                let mut cur_code: Option<u16> = None;
                for item in diff_arr {
                    if let Ok(code) = item.as_i64()
                        && (0..=65535).contains(&code)
                    {
                        cur_code = Some(code as u16);
                    } else if let (Some(code), Ok(gname)) = (cur_code, item.as_name()) {
                        let gstr = String::from_utf8_lossy(gname);
                        if let Some(uni) = glyph_name_to_unicode(&gstr) {
                            font.differences.insert(code, uni);
                        }
                        cur_code = code.checked_add(1);
                    }
                }
            }
        }
    }

    // Simple font `/FirstChar` + `/Widths`
    if let (Some(first_char), Some(widths_arr)) = (
        dict.get(b"FirstChar")
            .ok()
            .and_then(|o| o.as_i64().ok())
            .filter(|&v| (0..=255).contains(&v)),
        dict.get(b"Widths")
            .ok()
            .and_then(|o| doc.dereference(o).map_or(o, |(_, r)| r).as_array().ok()),
    ) {
        for (idx, w_obj) in widths_arr.iter().enumerate() {
            if let Some(w) = scalar(doc.dereference(w_obj).map_or(w_obj, |(_, r)| r))
                && let Ok(code) = u16::try_from(first_char as usize + idx)
            {
                font.widths.insert(code, w.clamp(0.0, 4000.0));
            }
        }
    }

    // Type0 `/DescendantFonts` (`/DW` and `/W` CID widths + `/FontDescriptor`)
    if let Ok(desc_obj) = dict.get(b"DescendantFonts")
        && let Ok(desc_arr) = doc
            .dereference(desc_obj)
            .map_or(desc_obj, |(_, r)| r)
            .as_array()
        && let Some(first_desc) = desc_arr.first()
        && let Some(cid_dict) = function_dict(doc, first_desc)
    {
        if let Some(dw) = cid_dict.get(b"DW").ok().and_then(scalar) {
            font.default_width = dw.clamp(100.0, 4000.0);
        }
        if let Ok(w_obj) = cid_dict.get(b"W")
            && let Ok(w_arr) = doc.dereference(w_obj).map_or(w_obj, |(_, r)| r).as_array()
        {
            parse_cid_widths_array(doc, w_arr, &mut font.widths);
        }
        if font.family == "sans-serif"
            && let Ok(base_obj) = cid_dict.get(b"BaseFont")
            && let Ok(name) = doc
                .dereference(base_obj)
                .map_or(base_obj, |(_, o)| o)
                .as_name()
        {
            let (fam, bold, italic) = clean_pdf_font_name(&String::from_utf8_lossy(name));
            font.family = fam;
            font.bold |= bold;
            font.italic |= italic;
        }
    }

    // `/ToUnicode` stream
    if let Ok(tu_obj) = dict.get(b"ToUnicode")
        && let Object::Stream(tu_stream) = doc.dereference(tu_obj).map_or(tu_obj, |(_, r)| r)
        && let Ok(raw_cmap) = decode_pdf_stream_bounded(tu_stream, MAX_CMAP_STREAM_BYTES)
    {
        let cmap_text = String::from_utf8_lossy(&raw_cmap);
        parse_to_unicode_cmap(&cmap_text, &mut font);
    }

    Some(font)
}

/// Extract raw unmapped bytes from a tokenized PDF string `(...)` or hex string `<...>`.
pub(super) fn decode_pdf_token_raw_bytes(tok: &str) -> Option<Vec<u8>> {
    if tok.starts_with('(') && tok.ends_with(')') && tok.len() >= 2 {
        let inner = &tok[1..tok.len() - 1];
        let mut bytes = Vec::with_capacity(inner.len());
        let mut chars = inner.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                let Some(esc) = chars.next() else {
                    break;
                };
                match esc {
                    'n' => bytes.push(b'\n'),
                    'r' => bytes.push(b'\r'),
                    't' => bytes.push(b'\t'),
                    'b' => bytes.push(0x08),
                    'f' => bytes.push(0x0C),
                    '(' => bytes.push(b'('),
                    ')' => bytes.push(b')'),
                    '\\' => bytes.push(b'\\'),
                    '\r' => {
                        if chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                    }
                    '\n' => {}
                    '0'..='7' => {
                        let mut val = (esc as u32) - ('0' as u32);
                        for _ in 0..2 {
                            if let Some(&d @ '0'..='7') = chars.peek() {
                                chars.next();
                                val = val * 8 + ((d as u32) - ('0' as u32));
                            } else {
                                break;
                            }
                        }
                        bytes.push((val & 0xFF) as u8);
                    }
                    other => {
                        if other.is_ascii() {
                            bytes.push(other as u8);
                        } else {
                            let mut buf = [0u8; 4];
                            bytes.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                        }
                    }
                }
            } else if c.is_ascii() {
                bytes.push(c as u8);
            } else {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
        return Some(bytes);
    }

    if tok.starts_with('<') && tok.ends_with('>') && tok != "<<" && tok.len() >= 2 {
        return parse_hex_bytes(tok);
    }

    None
}

#[derive(Clone, Debug)]
pub(super) struct PdfTextStyleState {
    pub font_name: String,
    pub font_size: f64,
    pub char_space: f64,
    pub word_space: f64,
    pub horiz_scale: f64,
    pub leading: f64,
    pub text_rise: f64,
    pub render_mode: i64,
}

impl Default for PdfTextStyleState {
    fn default() -> Self {
        Self {
            font_name: String::new(),
            font_size: 16.0,
            char_space: 0.0,
            word_space: 0.0,
            horiz_scale: 100.0,
            leading: 0.0,
            text_rise: 0.0,
            render_mode: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct CapturedTextSpan {
    text: String,
    font_family: String,
    font_size_local: f64,
    bold: bool,
    italic: bool,
    color: Color,
    letter_spacing_local: f64,
    text_matrix: [f64; 6],
    advance_local: f64,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PdfTextBlockBuilder {
    pub in_text: bool,
    pub text_matrix: [f64; 6],
    pub line_matrix: [f64; 6],
    spans: Vec<CapturedTextSpan>,
}

impl PdfTextBlockBuilder {
    pub fn begin_text(&mut self) {
        self.in_text = true;
        self.text_matrix = identity_matrix();
        self.line_matrix = identity_matrix();
        self.spans.clear();
    }

    pub fn set_matrix(&mut self, m: [f64; 6]) {
        self.text_matrix = m;
        self.line_matrix = m;
    }

    pub fn move_line(&mut self, tx: f64, ty: f64) {
        let step = [1.0, 0.0, 0.0, 1.0, tx, ty];
        self.line_matrix = affine_multiply(self.line_matrix, step);
        self.text_matrix = self.line_matrix;
    }

    pub fn next_line(&mut self, leading: f64) {
        self.move_line(0.0, -leading);
    }

    pub fn show_tokens(
        &mut self,
        tokens: &[String],
        style: &PdfTextStyleState,
        fonts: &BTreeMap<String, PdfFontResource>,
        fill_color: Color,
        stroke_color: Color,
    ) {
        if style.render_mode == 3 {
            // Invisible OCR text rendering mode (`3 Tr`) — advance matrix without emitting visible text
            return;
        }
        let explicit_font = fonts.get(&style.font_name);
        let default_font = PdfFontResource::default();
        let font = explicit_font.unwrap_or(&default_font);
        let fs = style.font_size;
        let tz = (style.horiz_scale / 100.0).clamp(0.05, 20.0);
        let paint_color = if style.render_mode == 1 {
            stroke_color
        } else {
            fill_color
        };

        let mut start_tm = self.text_matrix;
        if style.text_rise.abs() > 1e-6 {
            start_tm = affine_multiply(start_tm, [1.0, 0.0, 0.0, 1.0, 0.0, style.text_rise]);
        }

        let mut combined = String::new();
        let mut total_advance = 0.0;

        for tok in tokens {
            if tok == "[" || tok == "]" {
                continue;
            }
            if let Ok(kern_num) = tok.parse::<f64>() {
                // PDF TJ number: positive shifts left, negative shifts right by -kern/1000 * fs * tz
                let delta = -(kern_num / 1000.0) * fs * tz;
                if kern_num <= -220.0 && !combined.is_empty() && !combined.ends_with(' ') {
                    combined.push(' ');
                }
                total_advance += delta;
                self.text_matrix =
                    affine_multiply(self.text_matrix, [1.0, 0.0, 0.0, 1.0, delta, 0.0]);
                continue;
            }
            if let Some(raw_bytes) = decode_pdf_token_raw_bytes(tok) {
                let (decoded, w1000, n_chars, n_spaces) = if let Some(f) = explicit_font {
                    f.decode_bytes(&raw_bytes)
                } else if tok.starts_with('<')
                    && raw_bytes.len() >= 2
                    && raw_bytes.len().is_multiple_of(2)
                    && raw_bytes.contains(&0)
                {
                    // Unreferenced font hex token with NUL bytes -> UTF-16BE (`<0048>` -> `"H"`)
                    let s = decode_utf16be_hex_bytes(
                        raw_bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(&raw_bytes),
                    );
                    let nc = s.chars().count();
                    let ns = s.chars().filter(|&c| c == ' ').count();
                    (s, (nc as f64) * 550.0, nc, ns)
                } else if !raw_bytes.starts_with(&[0xFE, 0xFF])
                    && !raw_bytes.contains(&0)
                    && let Ok(utf8) = std::str::from_utf8(&raw_bytes)
                {
                    let s = utf8.to_owned();
                    let nc = s.chars().count();
                    let ns = s.chars().filter(|&c| c == ' ').count();
                    (s, (nc as f64) * 550.0, nc, ns)
                } else {
                    font.decode_bytes(&raw_bytes)
                };
                let adv = ((w1000 / 1000.0) * fs
                    + (n_chars as f64) * style.char_space
                    + (n_spaces as f64) * style.word_space)
                    * tz;
                combined.push_str(&decoded);
                total_advance += adv;
                self.text_matrix =
                    affine_multiply(self.text_matrix, [1.0, 0.0, 0.0, 1.0, adv, 0.0]);
            }
        }

        if !combined.is_empty() {
            self.spans.push(CapturedTextSpan {
                text: combined,
                font_family: font.family.clone(),
                font_size_local: fs.abs().max(0.01),
                bold: font.bold || matches!(style.render_mode, 2 | 6),
                italic: font.italic,
                color: paint_color,
                letter_spacing_local: style.char_space * tz,
                text_matrix: start_tm,
                advance_local: total_advance.max(0.0),
            });
        }
    }

    pub fn flush_elements(
        &mut self,
        ctm: [f64; 6],
        blend_mode: BlendMode,
        options: &VectorImportOptions,
        loc: &str,
        report: &mut ImportReport,
    ) -> Vec<Element> {
        self.in_text = false;
        let spans = std::mem::take(&mut self.spans);
        if spans.is_empty() {
            return Vec::new();
        }

        // Group spans that share compatible orientation and sit on the same baseline or sequential paragraph lines
        struct SpanGroup {
            spans: Vec<(CapturedTextSpan, bool)>, // (span, starts_new_line)
        }

        let mut groups: Vec<SpanGroup> = Vec::new();
        for span in spans {
            let mut merged = false;
            if let Some(last_group) = groups.last_mut()
                && let Some((prev, _)) = last_group.spans.last()
            {
                // Compare linear 2x2 part of text_matrix
                let same_linear = (prev.text_matrix[0] - span.text_matrix[0]).abs() <= 1e-3
                    && (prev.text_matrix[1] - span.text_matrix[1]).abs() <= 1e-3
                    && (prev.text_matrix[2] - span.text_matrix[2]).abs() <= 1e-3
                    && (prev.text_matrix[3] - span.text_matrix[3]).abs() <= 1e-3;
                if same_linear
                    && let Some(inv_lin) = super::invert_affine_2x2_with_origin(prev.text_matrix)
                {
                    let (rel_x, rel_y) =
                        transform_point(inv_lin, span.text_matrix[4], span.text_matrix[5]);
                    let fs = prev.font_size_local.max(1.0);
                    let expected_end_x = prev.advance_local;
                    let gap_x = rel_x - expected_end_x;
                    // Same baseline continuation
                    if rel_y.abs() <= fs * 0.18 && gap_x >= -fs * 0.45 && gap_x <= fs * 1.8 {
                        let mut adjusted = span.clone();
                        if gap_x >= fs * 0.22
                            && !prev.text.ends_with(char::is_whitespace)
                            && !adjusted.text.starts_with(char::is_whitespace)
                        {
                            adjusted.text.insert(0, ' ');
                        }
                        last_group.spans.push((adjusted, false));
                        merged = true;
                    } else if (-fs * 2.4..=-fs * 0.65).contains(&rel_y)
                        && rel_x >= -prev.advance_local - fs * 2.0
                        && rel_x <= fs * 2.0
                    {
                        // Sequential paragraph line inside the same `BT..ET` text frame
                        last_group.spans.push((span.clone(), true));
                        merged = true;
                    }
                }
            }
            if !merged {
                groups.push(SpanGroup {
                    spans: vec![(span, true)],
                });
            }
        }

        let mut elements = Vec::new();
        for group in groups {
            let first = &group.spans[0].0;
            let full_stage_tm = affine_multiply(ctm, first.text_matrix);
            let y_scale = full_stage_tm[2].hypot(full_stage_tm[3]);
            let x_scale = full_stage_tm[0].hypot(full_stage_tm[1]);
            let scale_mag = if y_scale > 1e-6 {
                y_scale
            } else if x_scale > 1e-6 {
                x_scale
            } else {
                1.0
            };

            let base_fs = (first.font_size_local * scale_mag).clamp(1.0, 1024.0);

            // Build combined text and per-run UTF-16 spans
            let mut full_text = String::new();
            struct RawRun {
                start_u16: u32,
                end_u16: u32,
                fmt: TextFormat,
                alpha: u8,
            }
            let mut raw_runs: Vec<RawRun> = Vec::new();
            let mut cursor_u16 = 0u32;
            let mut max_line_width_px = 0.0f64;
            let mut cur_line_width_px = 0.0f64;
            let mut line_count = 1usize;

            for (idx, (sp, new_line)) in group.spans.iter().enumerate() {
                if idx > 0 && *new_line {
                    full_text.push('\n');
                    cursor_u16 += 1;
                    max_line_width_px = max_line_width_px.max(cur_line_width_px);
                    cur_line_width_px = 0.0;
                    line_count += 1;
                }
                let norm = normalize_text(&sp.text);
                if norm.is_empty() {
                    continue;
                }
                let u16_len = norm.encode_utf16().count() as u32;
                let run_fs = (sp.font_size_local * scale_mag).clamp(1.0, 1024.0);
                let rgb = (u32::from(sp.color[0]) << 16)
                    | (u32::from(sp.color[1]) << 8)
                    | u32::from(sp.color[2]);
                let fmt = TextFormat {
                    font: sp.font_family.clone(),
                    size: run_fs,
                    color: rgb,
                    bold: sp.bold,
                    italic: sp.italic,
                    letter_spacing: (sp.letter_spacing_local * scale_mag).clamp(-60.0, 60.0),
                    leading: 2.0,
                    ..TextFormat::default()
                };
                let est_w = (sp.advance_local * scale_mag)
                    .max((norm.chars().count() as f64) * run_fs * 0.58);
                cur_line_width_px += est_w;
                raw_runs.push(RawRun {
                    start_u16: cursor_u16,
                    end_u16: cursor_u16 + u16_len,
                    fmt,
                    alpha: sp.color[3],
                });
                cursor_u16 += u16_len;
                full_text.push_str(&norm);
            }
            max_line_width_px = max_line_width_px.max(cur_line_width_px);

            if full_text.trim().is_empty() || raw_runs.is_empty() {
                continue;
            }

            match options.text_handling {
                VectorTextHandling::ConvertToPaths => {
                    report.warning(
                        "pdf-text-converted-to-paths",
                        loc.to_owned(),
                        format!("Converted PDF text '{full_text}' to vector outline paths."),
                    );
                    let unit_stage = [
                        full_stage_tm[0] / scale_mag,
                        full_stage_tm[1] / scale_mag,
                        full_stage_tm[2] / scale_mag,
                        full_stage_tm[3] / scale_mag,
                        full_stage_tm[4],
                        full_stage_tm[5],
                    ];
                    let box_stage =
                        affine_multiply(unit_stage, [1.0, 0.0, 0.0, -1.0, 0.0, base_fs * 0.8]);
                    let outline = TextFieldState::static_text_to_outline_commands(
                        &full_text,
                        &raw_runs[0].fmt.font,
                        base_fs,
                        None,
                    );
                    let cmds = if outline.is_empty() {
                        text_to_outline_commands(&full_text, 0.0, base_fs * 0.8, base_fs)
                    } else {
                        outline
                    };
                    let baked = bake_matrix_into_commands(&cmds, box_stage);
                    if validate_path_commands(&baked).is_ok() {
                        let c0 = first.color;
                        elements.push(Element {
                            id: new_id("element"),
                            name: format!("PDF Outlined Text ({full_text})"),
                            transform: Transform::default(),
                            kind: ElementKind::Path {
                                commands: baked,
                                fill_rule: FillRule::NonZero,
                                fill: c0,
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            blend_mode,
                            ..Element::default()
                        });
                        report.statistics.elements += 1;
                    }
                }
                VectorTextHandling::EditableText => {
                    // Map PDF glyph space `(0, 0)` = baseline-left, `(+x, +y)` = right & ascender-up
                    // to NeoFlash `TextBox` local space `(0, 0)` = top-left, `(0, +0.8 * base_fs)` = baseline!
                    let unit_stage = [
                        full_stage_tm[0] / scale_mag,
                        full_stage_tm[1] / scale_mag,
                        full_stage_tm[2] / scale_mag,
                        full_stage_tm[3] / scale_mag,
                        full_stage_tm[4],
                        full_stage_tm[5],
                    ];
                    let box_stage =
                        affine_multiply(unit_stage, [1.0, 0.0, 0.0, -1.0, 0.0, base_fs * 0.8]);
                    let mut tf = Transform::from_matrix(box_stage).unwrap_or(Transform {
                        x: box_stage[4],
                        y: box_stage[5],
                        ..Transform::default()
                    });
                    tf.alpha = (f64::from(raw_runs[0].alpha) / 255.0).clamp(0.0, 1.0);

                    let mut base_fmt = raw_runs[0].fmt.clone();
                    let mut opts = AuthoredTextFieldOptions {
                        padding_left: 0.0,
                        padding_right: 0.0,
                        padding_top: 0.0,
                        padding_bottom: 0.0,
                        auto_size: "left".to_owned(),
                        ..AuthoredTextFieldOptions::default()
                    };
                    let has_distinct_runs =
                        raw_runs.len() > 1 && raw_runs.iter().skip(1).any(|r| r.fmt != base_fmt);
                    if has_distinct_runs {
                        let mut format_runs = Vec::with_capacity(raw_runs.len());
                        let mut prev_end = 0u32;
                        for r in &raw_runs {
                            if r.start_u16 > prev_end {
                                format_runs.push(TextFormatRun {
                                    start: prev_end,
                                    end: r.start_u16,
                                    format: base_fmt.clone(),
                                });
                            }
                            format_runs.push(TextFormatRun {
                                start: r.start_u16,
                                end: r.end_u16,
                                format: r.fmt.clone(),
                            });
                            prev_end = r.end_u16;
                        }
                        if prev_end < cursor_u16 {
                            format_runs.push(TextFormatRun {
                                start: prev_end,
                                end: cursor_u16,
                                format: base_fmt.clone(),
                            });
                        }
                        opts.format_runs = format_runs;
                    }
                    base_fmt.set_field_options(Some(opts));

                    let box_w = (max_line_width_px + base_fs * 0.5).clamp(16.0, 16_384.0);
                    let box_h = ((line_count as f64) * base_fs * 1.25 + 4.0)
                        .max(base_fs * 1.25)
                        .clamp(16.0, 16_384.0);
                    let multiline = line_count > 1 || full_text.contains('\n');

                    let kind = ElementKind::TextBox {
                        text: full_text.clone(),
                        width: box_w,
                        height: box_h,
                        format: base_fmt,
                        word_wrap: false,
                        multiline,
                    };
                    let final_kind = if TextFieldState::from_kind(&kind)
                        .and_then(|f| f.validate().ok())
                        .is_some()
                    {
                        kind
                    } else {
                        ElementKind::Text {
                            text: full_text.clone(),
                            font_family: first.font_family.clone(),
                            font_size: base_fs,
                            color: first.color,
                        }
                    };

                    let label_preview: String = full_text.chars().take(32).collect();
                    elements.push(Element {
                        id: new_id("element"),
                        name: format!("PDF Text ({label_preview})"),
                        transform: tf,
                        kind: final_kind,
                        blend_mode,
                        ..Element::default()
                    });
                    report.statistics.elements += 1;
                }
            }
        }
        elements
    }
}
