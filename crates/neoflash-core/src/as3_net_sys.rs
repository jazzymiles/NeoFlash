//! Shared Rust core for ActionScript 3 Area 8: Globalization, Networking,
//! Streaming/FLV/NetGroup, LocalConnection, XMLSocket framing, X.509 DER/PEM
//! Certificate parsing, DRM voucher lifecycle, System/Capabilities, Workers,
//! Concurrency (Mutex/Condition), Crypto CSPRNG, Sampler and Telemetry.
//!
//! Independently implemented from Adobe Flash Platform ActionScript 3.0
//! Reference specifications, RFC 5280 (X.509), RFC 6234 (SHA-256) and BCP-47.

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

// ---------------------------------------------------------------------------
// Pure-Rust RFC 6234 SHA-256 + CSPRNG whitening
// ---------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub fn hex_encode_upper(bytes: &[u8]) -> String {
    hex_encode(bytes).to_ascii_uppercase()
}

pub fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    let clean = text.trim();
    if !clean.len().is_multiple_of(2) {
        return Err("Hex string must contain an even number of digits".into());
    }
    let mut out = Vec::with_capacity(clean.len() / 2);
    let bytes = clean.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = (bytes[i] as char)
            .to_digit(16)
            .ok_or_else(|| "Invalid hex character".to_owned())?;
        let lo = (bytes[i + 1] as char)
            .to_digit(16)
            .ok_or_else(|| "Invalid hex character".to_owned())?;
        out.push(((hi << 4) | lo) as u8);
    }
    Ok(out)
}

/// Generates `1..=1024` cryptographically whitened random bytes per
/// `flash.crypto.generateRandomBytes(numberRandomBytes:uint)`.
pub fn generate_random_bytes(
    count: u32,
    entropy: &[u8],
    counter: &mut u64,
) -> Result<Vec<u8>, String> {
    if !(1..=1024).contains(&count) {
        return Err(
            "Error #2004: One of the parameters is invalid. generateRandomBytes requires 1..1024 bytes."
                .into(),
        );
    }
    let len = count as usize;
    let mut out = Vec::with_capacity(len);
    let mut block_idx = 0u32;
    while out.len() < len {
        *counter = counter.wrapping_add(1);
        let mut seed_material = Vec::with_capacity(entropy.len() + 20);
        seed_material.extend_from_slice(b"NeoFlash-CSPRNG-v1:");
        seed_material.extend_from_slice(&counter.to_le_bytes());
        seed_material.extend_from_slice(&block_idx.to_le_bytes());
        if entropy.len() >= (block_idx as usize) * 32 {
            let start = ((block_idx as usize) * 32).min(entropy.len());
            let end = (start + 32).min(entropy.len());
            seed_material.extend_from_slice(&entropy[start..end]);
            seed_material.extend_from_slice(entropy);
        } else {
            seed_material.extend_from_slice(entropy);
        }
        let digest = sha256(&seed_material);
        let take = (len - out.len()).min(32);
        out.extend_from_slice(&digest[..take]);
        block_idx += 1;
    }
    // Mix raw host entropy directly via XOR when exact-length entropy is supplied.
    if entropy.len() == len {
        for (dst, &src) in out.iter_mut().zip(entropy.iter()) {
            *dst ^= src;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Globalization: NationalDigitsType, LocaleID, CurrencyFormatter, Collator, StringTools
// ---------------------------------------------------------------------------

/// All 32 Unicode decimal zero codepoints in `flash.globalization.NationalDigitsType`
/// (plus Tai Tham Hora/Tham accepted by `NumberFormatter`).
pub const NATIONAL_DIGIT_BASES: &[u32] = &[
    0x0030,  // EUROPEAN
    0x0660,  // ARABIC_INDIC
    0x06F0,  // EXTENDED_ARABIC_INDIC
    0x07C0,  // NKO
    0x0966,  // DEVANAGARI
    0x09E6,  // BENGALI
    0x0A66,  // GURMUKHI
    0x0AE6,  // GUJARATI
    0x0B66,  // ORIYA
    0x0BE6,  // TAMIL
    0x0C66,  // TELUGU
    0x0CE6,  // KANNADA
    0x0D66,  // MALAYALAM
    0x0E50,  // THAI
    0x0ED0,  // LAO
    0x0F20,  // TIBETAN
    0x1040,  // MYANMAR
    0x1090,  // MYANMAR_SHAN
    0x17E0,  // KHMER
    0x1810,  // MONGOLIAN
    0x1946,  // LIMBU
    0x19D0,  // NEW_TAI_LUE
    0x1A80,  // TAI_THAM_HORA (NumberFormatter superset)
    0x1A90,  // TAI_THAM_THAM (NumberFormatter superset)
    0x1B50,  // BALINESE
    0x1BB0,  // SUNDANESE
    0x1C40,  // LEPCHA
    0x1C50,  // OL_CHIKI
    0xA620,  // VAI
    0xA8D0,  // SAURASHTRA
    0xA900,  // KAYAH_LI
    0xAA50,  // CHAM
    0xFF10,  // FULL_WIDTH
    0x104A0, // OSMANYA
];

pub const SUPPORTED_LOCALES: &[&str] = &[
    "en-US",
    "en-GB",
    "en-CA",
    "en-AU",
    "de-DE",
    "fr-FR",
    "es-ES",
    "es-MX",
    "it-IT",
    "pt-BR",
    "pt-PT",
    "nl-NL",
    "pl-PL",
    "ru-RU",
    "uk-UA",
    "cs-CZ",
    "sk-SK",
    "sv-SE",
    "da-DK",
    "fi-FI",
    "nb-NO",
    "el-GR",
    "tr-TR",
    "az-Latn-AZ",
    "lt-LT",
    "he-IL",
    "ar-SA",
    "ar-EG",
    "fa-IR",
    "ur-PK",
    "hi-IN",
    "th-TH",
    "vi-VN",
    "id-ID",
    "ja-JP",
    "ko-KR",
    "zh-CN",
    "zh-TW",
    "zh-HK",
];

pub fn national_digit_value(ch: char) -> Option<u8> {
    let cp = ch as u32;
    for &base in NATIONAL_DIGIT_BASES {
        if (base..base + 10).contains(&cp) {
            return Some((cp - base) as u8);
        }
    }
    None
}

pub fn render_national_digits(ascii: &str, base: u32) -> String {
    if base == 0x30 {
        return ascii.to_owned();
    }
    ascii
        .chars()
        .map(|c| {
            if c.is_ascii_digit() {
                char::from_u32(base + (c as u32 - 0x30)).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedLocale {
    pub requested: String,
    pub canonical: String,
    pub language: String,
    pub script: String,
    pub region: String,
    pub variant: String,
    pub keys: BTreeMap<String, String>,
    pub rtl: bool,
    pub status: String,
}

fn infer_script(lang: &str, region: &str) -> &'static str {
    match lang {
        "zh" => match region {
            "TW" | "HK" | "MO" => "Hant",
            _ => "Hans",
        },
        "ja" => "Jpan",
        "ko" => "Kore",
        "ar" | "fa" | "ur" | "ps" | "ug" | "sd" => "Arab",
        "he" | "iw" | "yi" | "ji" => "Hebr",
        "dv" => "Thaa",
        "ru" | "uk" | "bg" | "be" | "mk" | "kk" | "ky" | "mn" | "sr" => "Cyrl",
        "el" => "Grek",
        "hi" | "mr" | "ne" | "sa" => "Deva",
        "bn" => "Beng",
        "pa" => "Guru",
        "gu" => "Gujr",
        "or" => "Orya",
        "ta" => "Taml",
        "te" => "Telu",
        "kn" => "Knda",
        "ml" => "Mlym",
        "th" => "Thai",
        "lo" => "Laoo",
        "bo" => "Tibt",
        "my" => "Mymr",
        "km" => "Khmr",
        "hy" => "Armn",
        "ka" => "Geor",
        "am" | "ti" => "Ethi",
        _ => "Latn",
    }
}

fn infer_region(lang: &str, script: &str) -> &'static str {
    match lang {
        "en" => "US",
        "de" => "DE",
        "fr" => "FR",
        "es" => "ES",
        "it" => "IT",
        "pt" => "BR",
        "nl" => "NL",
        "pl" => "PL",
        "ru" => "RU",
        "uk" => "UA",
        "cs" => "CZ",
        "sk" => "SK",
        "sv" => "SE",
        "da" => "DK",
        "fi" => "FI",
        "nb" | "no" | "nn" => "NO",
        "el" => "GR",
        "tr" => "TR",
        "az" => "AZ",
        "lt" => "LT",
        "he" | "iw" => "IL",
        "ar" => "SA",
        "fa" => "IR",
        "ur" => "PK",
        "hi" => "IN",
        "th" => "TH",
        "vi" => "VN",
        "id" | "in" => "ID",
        "ja" => "JP",
        "ko" => "KR",
        "zh" => {
            if script == "Hant" {
                "TW"
            } else {
                "CN"
            }
        }
        _ => "",
    }
}

pub fn parse_locale_id(raw: &str, default_locale: &str) -> ParsedLocale {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 256 {
        return ParsedLocale {
            requested: raw.to_owned(),
            canonical: String::new(),
            language: String::new(),
            script: String::new(),
            region: String::new(),
            variant: String::new(),
            keys: BTreeMap::new(),
            rtl: false,
            status: "illegalArgumentError".into(),
        };
    }
    if trimmed.eq_ignore_ascii_case("i-default") || trimmed == "default" {
        let fallback = if default_locale.trim().is_empty()
            || default_locale.eq_ignore_ascii_case("i-default")
        {
            "en-US"
        } else {
            default_locale.trim()
        };
        let mut resolved = parse_locale_id(fallback, "en-US");
        resolved.requested = raw.to_owned();
        if resolved.status == "illegalArgumentError" {
            resolved = parse_locale_id("en-US", "en-US");
            resolved.requested = raw.to_owned();
            resolved.status = "usingDefaultWarning".into();
        } else {
            resolved.status = "noError".into();
        }
        return resolved;
    }

    let mut keys = BTreeMap::new();
    let (base_part, at_part) = match trimmed.split_once('@') {
        Some((b, k)) => (b, Some(k)),
        None => (trimmed, None),
    };
    if let Some(kv_str) = at_part {
        for pair in kv_str.split([';', '&']) {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            if let Some((k, v)) = pair.split_once('=') {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim().to_owned();
                if !k.is_empty() && !v.is_empty() {
                    keys.insert(k, v);
                }
            }
        }
    }

    let normalized = base_part.replace('_', "-");
    if normalized.starts_with('-')
        || normalized.ends_with('-')
        || normalized.contains("--")
        || !normalized
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return ParsedLocale {
            requested: raw.to_owned(),
            canonical: String::new(),
            language: String::new(),
            script: String::new(),
            region: String::new(),
            variant: String::new(),
            keys: BTreeMap::new(),
            rtl: false,
            status: "illegalArgumentError".into(),
        };
    }

    let tokens: Vec<&str> = normalized.split('-').collect();
    let first = tokens[0];
    if !(2..=8).contains(&first.len()) || !first.chars().all(|c| c.is_ascii_alphabetic()) {
        return ParsedLocale {
            requested: raw.to_owned(),
            canonical: String::new(),
            language: String::new(),
            script: String::new(),
            region: String::new(),
            variant: String::new(),
            keys: BTreeMap::new(),
            rtl: false,
            status: "illegalArgumentError".into(),
        };
    }

    let language = match first.to_ascii_lowercase().as_str() {
        "iw" => "he".to_owned(),
        "in" => "id".to_owned(),
        "ji" => "yi".to_owned(),
        other => other.to_owned(),
    };

    let mut idx = 1usize;
    let mut explicit_script = String::new();
    let mut explicit_region = String::new();
    let mut variants = Vec::new();

    if idx < tokens.len()
        && tokens[idx].len() == 4
        && tokens[idx].chars().all(|c| c.is_ascii_alphabetic())
    {
        let mut chars = tokens[idx].chars();
        if let Some(first_c) = chars.next() {
            explicit_script.push(first_c.to_ascii_uppercase());
            for c in chars {
                explicit_script.push(c.to_ascii_lowercase());
            }
        }
        idx += 1;
    }

    if idx < tokens.len()
        && ((tokens[idx].len() == 2 && tokens[idx].chars().all(|c| c.is_ascii_alphabetic()))
            || (tokens[idx].len() == 3 && tokens[idx].chars().all(|c| c.is_ascii_digit())))
    {
        explicit_region = tokens[idx].to_ascii_uppercase();
        idx += 1;
    }

    while idx < tokens.len() {
        let tok = tokens[idx];
        if tok.len() == 1 {
            let singleton = tok.to_ascii_lowercase();
            idx += 1;
            if singleton == "u" {
                while idx < tokens.len() && tokens[idx].len() > 1 {
                    let key = tokens[idx].to_ascii_lowercase();
                    idx += 1;
                    let mut val_parts = Vec::new();
                    while idx < tokens.len() && tokens[idx].len() >= 3 {
                        val_parts.push(tokens[idx]);
                        idx += 1;
                    }
                    let val = if val_parts.is_empty() {
                        "true".to_owned()
                    } else {
                        val_parts.join("-")
                    };
                    keys.entry(key).or_insert(val);
                }
            } else {
                while idx < tokens.len() && tokens[idx].len() > 1 {
                    idx += 1;
                }
            }
            continue;
        }
        variants.push(tok.to_owned());
        idx += 1;
    }

    let inferred_script = if explicit_script.is_empty() {
        infer_script(&language, &explicit_region).to_owned()
    } else {
        explicit_script.clone()
    };
    let region = if explicit_region.is_empty() {
        infer_region(&language, &inferred_script).to_owned()
    } else {
        explicit_region.clone()
    };

    let mut canonical = language.clone();
    if !explicit_script.is_empty() {
        canonical.push('-');
        canonical.push_str(&explicit_script);
    }
    if !explicit_region.is_empty() {
        canonical.push('-');
        canonical.push_str(&explicit_region);
    }
    for v in &variants {
        canonical.push('-');
        canonical.push_str(v);
    }

    let rtl = matches!(
        inferred_script.as_str(),
        "Arab" | "Hebr" | "Thaa" | "Syrc" | "Nkoo" | "Adlm" | "Mand" | "Samr"
    );

    let known_lang = SUPPORTED_LOCALES
        .iter()
        .any(|loc| loc.split('-').next() == Some(language.as_str()))
        || matches!(
            language.as_str(),
            "af" | "am"
                | "ar"
                | "az"
                | "be"
                | "bg"
                | "bn"
                | "bo"
                | "ca"
                | "cs"
                | "cy"
                | "da"
                | "de"
                | "dv"
                | "el"
                | "en"
                | "eo"
                | "es"
                | "et"
                | "eu"
                | "fa"
                | "fi"
                | "fil"
                | "fr"
                | "ga"
                | "gl"
                | "gu"
                | "he"
                | "hi"
                | "hr"
                | "hu"
                | "hy"
                | "id"
                | "is"
                | "it"
                | "ja"
                | "ka"
                | "kk"
                | "km"
                | "kn"
                | "ko"
                | "ku"
                | "ky"
                | "lo"
                | "lt"
                | "lv"
                | "mk"
                | "ml"
                | "mn"
                | "mr"
                | "ms"
                | "mt"
                | "my"
                | "nb"
                | "ne"
                | "nl"
                | "nn"
                | "no"
                | "or"
                | "pa"
                | "pl"
                | "ps"
                | "pt"
                | "ro"
                | "ru"
                | "sa"
                | "sd"
                | "si"
                | "sk"
                | "sl"
                | "sq"
                | "sr"
                | "sv"
                | "sw"
                | "ta"
                | "te"
                | "th"
                | "ti"
                | "tr"
                | "ug"
                | "uk"
                | "ur"
                | "uz"
                | "vi"
                | "yi"
                | "zh"
                | "zu"
        );
    let status = if known_lang {
        "noError".into()
    } else {
        "usingFallbackWarning".into()
    };

    ParsedLocale {
        requested: raw.to_owned(),
        canonical,
        language,
        script: inferred_script,
        region,
        variant: variants.join("-"),
        keys,
        rtl,
        status,
    }
}

pub fn determine_preferred_locales(
    want: &[String],
    have: &[String],
    _keyword: &str,
) -> Vec<String> {
    if want.is_empty() || have.is_empty() {
        return Vec::new();
    }
    let parsed_have: Vec<(String, ParsedLocale)> = have
        .iter()
        .map(|h| (h.clone(), parse_locale_id(h, "en-US")))
        .filter(|(_, p)| p.status != "illegalArgumentError")
        .collect();

    let mut result = Vec::new();
    let mut seen = HashSet::new();

    for w in want {
        let pw = parse_locale_id(w, "en-US");
        if pw.status == "illegalArgumentError" {
            continue;
        }
        // 1. Exact canonical match
        if let Some((orig, _)) = parsed_have
            .iter()
            .find(|(_, ph)| ph.canonical.eq_ignore_ascii_case(&pw.canonical))
        {
            if seen.insert(orig.clone()) {
                result.push(orig.clone());
            }
            continue;
        }
        // 2. Language + Script + Region match
        if let Some((orig, _)) = parsed_have.iter().find(|(_, ph)| {
            ph.language == pw.language && ph.script == pw.script && ph.region == pw.region
        }) {
            if seen.insert(orig.clone()) {
                result.push(orig.clone());
            }
            continue;
        }
        // 3. Language + Script match
        if let Some((orig, _)) = parsed_have
            .iter()
            .find(|(_, ph)| ph.language == pw.language && ph.script == pw.script)
        {
            if seen.insert(orig.clone()) {
                result.push(orig.clone());
            }
            continue;
        }
        // 4. Language match
        if let Some((orig, _)) = parsed_have
            .iter()
            .find(|(_, ph)| ph.language == pw.language)
            && seen.insert(orig.clone())
        {
            result.push(orig.clone());
        }
    }
    result
}

// ---------------------------------------------------------------------------
// CurrencyFormatterState
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyFormatterState {
    #[serde(rename = "requestedLocaleIDName")]
    pub requested_locale_id_name: String,
    #[serde(rename = "actualLocaleIDName")]
    pub actual_locale_id_name: String,
    #[serde(rename = "currencyISOCode")]
    pub currency_iso_code: String,
    pub currency_symbol: String,
    pub decimal_separator: String,
    pub grouping_separator: String,
    pub grouping_pattern: String,
    pub digits_type: u32,
    pub fractional_digits: usize,
    pub leading_zero: bool,
    pub trailing_zeros: bool,
    pub use_grouping: bool,
    pub positive_currency_format: u32,
    pub negative_currency_format: u32,
    pub negative_symbol: String,
    pub last_operation_status: String,
}

fn default_currency_for_locale(
    lang: &str,
    region: &str,
) -> (
    &'static str,
    &'static str,
    usize,
    u32,
    u32,
    &'static str,
    &'static str,
    &'static str,
    u32,
) {
    // Returns (iso, symbol, frac, pos_fmt, neg_fmt, dec, grp, grp_pat, digits_base)
    match (lang, region) {
        ("en", "GB") => ("GBP", "£", 2, 0, 1, ".", ",", "3;*", 0x30),
        ("en", "CA") => ("CAD", "$", 2, 0, 0, ".", ",", "3;*", 0x30),
        ("en", "AU") => ("AUD", "$", 2, 0, 1, ".", ",", "3;*", 0x30),
        ("de", "CH") | ("fr", "CH") | ("it", "CH") => {
            ("CHF", "CHF", 2, 2, 9, ".", "'", "3;*", 0x30)
        }
        ("de", _) => ("EUR", "€", 2, 3, 8, ",", ".", "3;*", 0x30),
        ("fr", _) => ("EUR", "€", 2, 3, 8, ",", "\u{00a0}", "3;*", 0x30),
        ("es", "MX") => ("MXN", "$", 2, 0, 1, ".", ",", "3;*", 0x30),
        ("es", _) | ("it", _) | ("nl", _) | ("fi", _) | ("el", _) | ("sk", _) => {
            ("EUR", "€", 2, 3, 8, ",", ".", "3;*", 0x30)
        }
        ("pt", "BR") => ("BRL", "R$", 2, 2, 9, ",", ".", "3;*", 0x30),
        ("pt", _) => ("EUR", "€", 2, 3, 8, ",", "\u{00a0}", "3;*", 0x30),
        ("ja", _) => ("JPY", "￥", 0, 0, 1, ".", ",", "3;*", 0x30),
        ("zh", "TW") => ("TWD", "NT$", 2, 0, 1, ".", ",", "3;*", 0x30),
        ("zh", "HK") => ("HKD", "HK$", 2, 0, 0, ".", ",", "3;*", 0x30),
        ("zh", _) => ("CNY", "¥", 2, 0, 2, ".", ",", "3;*", 0x30),
        ("ko", _) => ("KRW", "₩", 0, 0, 1, ".", ",", "3;*", 0x30),
        ("ru", _) => ("RUB", "₽", 2, 3, 8, ",", "\u{00a0}", "3;*", 0x30),
        ("uk", _) => ("UAH", "₴", 2, 3, 8, ",", "\u{00a0}", "3;*", 0x30),
        ("pl", _) => ("PLN", "zł", 2, 3, 8, ",", "\u{00a0}", "3;*", 0x30),
        ("cs", _) => ("CZK", "Kč", 2, 3, 8, ",", "\u{00a0}", "3;*", 0x30),
        ("sv", _) => ("SEK", "kr", 2, 3, 8, ",", "\u{00a0}", "3;*", 0x30),
        ("da", _) => ("DKK", "kr.", 2, 3, 8, ",", ".", "3;*", 0x30),
        ("nb", _) | ("no", _) | ("nn", _) => ("NOK", "kr", 2, 2, 9, ",", "\u{00a0}", "3;*", 0x30),
        ("tr", _) => ("TRY", "₺", 2, 0, 1, ",", ".", "3;*", 0x30),
        ("he", _) => ("ILS", "₪", 2, 2, 9, ".", ",", "3;*", 0x30),
        ("ar", "EG") => ("EGP", "ج.م.", 2, 2, 9, "٫", "٬", "3;*", 0x0660),
        ("ar", _) => ("SAR", "ر.س.", 2, 2, 9, "٫", "٬", "3;*", 0x0660),
        ("fa", _) => ("IRR", "ریال", 0, 2, 9, "٫", "٬", "3;*", 0x06F0),
        ("hi", _) => ("INR", "₹", 2, 0, 1, ".", ",", "3;2;*", 0x30),
        ("th", _) => ("THB", "฿", 2, 0, 1, ".", ",", "3;*", 0x30),
        ("vi", _) => ("VND", "₫", 0, 3, 8, ",", ".", "3;*", 0x30),
        ("id", _) => ("IDR", "Rp", 0, 0, 1, ",", ".", "3;*", 0x30),
        _ => ("USD", "$", 2, 0, 0, ".", ",", "3;*", 0x30),
    }
}

fn validate_grouping_pattern(pattern: &str) -> Result<(Vec<usize>, bool), String> {
    if pattern.is_empty() {
        return Ok((Vec::new(), false));
    }
    let mut parts: Vec<&str> = pattern.split(';').collect();
    let repeat = parts.last() == Some(&"*");
    if repeat {
        parts.pop();
    }
    if parts.is_empty() || parts.len() > 4 {
        return Err("Grouping pattern requires 1..4 sizes, optionally ending with ';*'".into());
    }
    let mut sizes = Vec::with_capacity(parts.len());
    for part in parts {
        let size = part
            .parse::<usize>()
            .map_err(|_| "Invalid grouping pattern size".to_owned())?;
        if !(1..=9).contains(&size) {
            return Err("Grouping sizes must be in 1..=9".into());
        }
        sizes.push(size);
    }
    Ok((sizes, repeat))
}

impl CurrencyFormatterState {
    pub fn new(requested: &str, default_locale: &str) -> Self {
        let parsed = parse_locale_id(requested, default_locale);
        if parsed.status == "illegalArgumentError" {
            let (iso, sym, frac, pos, neg, dec, grp, pat, digits) =
                default_currency_for_locale("en", "US");
            return Self {
                requested_locale_id_name: requested.to_owned(),
                actual_locale_id_name: "en-US".into(),
                currency_iso_code: iso.into(),
                currency_symbol: sym.into(),
                decimal_separator: dec.into(),
                grouping_separator: grp.into(),
                grouping_pattern: pat.into(),
                digits_type: digits,
                fractional_digits: frac,
                leading_zero: true,
                trailing_zeros: true,
                use_grouping: true,
                positive_currency_format: pos,
                negative_currency_format: neg,
                negative_symbol: "-".into(),
                last_operation_status: "usingDefaultWarning".into(),
            };
        }
        let actual = if parsed.region.is_empty() {
            parsed.language.clone()
        } else {
            format!("{}-{}", parsed.language, parsed.region)
        };
        let (mut iso, mut sym, frac, pos, neg, dec, grp, pat, digits) =
            default_currency_for_locale(&parsed.language, &parsed.region);
        if let Some(custom_cur) = parsed.keys.get("currency") {
            let up = custom_cur.to_ascii_uppercase();
            if up.len() == 3 && up.chars().all(|c| c.is_ascii_uppercase()) {
                iso = Box::leak(up.clone().into_boxed_str());
                sym = match up.as_str() {
                    "USD" => "$",
                    "EUR" => "€",
                    "GBP" => "£",
                    "JPY" | "CNY" => "¥",
                    "RUB" => "₽",
                    "INR" => "₹",
                    "KRW" => "₩",
                    _ => iso,
                };
            }
        }
        Self {
            requested_locale_id_name: requested.to_owned(),
            actual_locale_id_name: actual,
            currency_iso_code: iso.into(),
            currency_symbol: sym.into(),
            decimal_separator: dec.into(),
            grouping_separator: grp.into(),
            grouping_pattern: pat.into(),
            digits_type: digits,
            fractional_digits: frac,
            leading_zero: true,
            trailing_zeros: true,
            use_grouping: true,
            positive_currency_format: pos,
            negative_currency_format: neg,
            negative_symbol: "-".into(),
            last_operation_status: parsed.status,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.fractional_digits > 20
            || self.positive_currency_format > 3
            || self.negative_currency_format > 15
            || !NATIONAL_DIGIT_BASES.contains(&self.digits_type)
        {
            return Err(
                "CurrencyFormatter requires fractionalDigits 0..20, positiveCurrencyFormat 0..3, negativeCurrencyFormat 0..15 and a valid NationalDigitsType"
                    .into(),
            );
        }
        if self.decimal_separator.is_empty()
            || self.decimal_separator.len() > 32
            || self.grouping_separator.len() > 32
            || self.negative_symbol.is_empty()
            || self.negative_symbol.len() > 32
            || self.decimal_separator == self.grouping_separator
        {
            return Err(
                "CurrencyFormatter separators and negativeSymbol must be non-empty distinct bounded strings"
                    .into(),
            );
        }
        validate_grouping_pattern(&self.grouping_pattern)?;
        Ok(())
    }

    pub fn set_property(&mut self, name: &str, value: Value) -> Result<(), String> {
        if !matches!(
            name,
            "decimalSeparator"
                | "groupingSeparator"
                | "groupingPattern"
                | "digitsType"
                | "fractionalDigits"
                | "leadingZero"
                | "trailingZeros"
                | "useGrouping"
                | "positiveCurrencyFormat"
                | "negativeCurrencyFormat"
                | "negativeSymbol"
        ) {
            self.last_operation_status = "illegalArgumentError".into();
            return Err(format!("CurrencyFormatter.{name} is read-only or unknown"));
        }
        let mut next = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
        next[name] = value;
        match serde_json::from_value::<Self>(next)
            .map_err(|e| e.to_string())
            .and_then(|cand| {
                cand.validate()?;
                Ok(cand)
            }) {
            Ok(mut cand) => {
                cand.last_operation_status = "noError".into();
                *self = cand;
                Ok(())
            }
            Err(err) => {
                self.last_operation_status = "illegalArgumentError".into();
                Err(err)
            }
        }
    }

    pub fn set_currency(&mut self, iso_code: &str, symbol: &str) -> Result<(), String> {
        if iso_code.len() != 3
            || !iso_code.chars().all(|c| c.is_ascii_alphabetic())
            || symbol.len() > 64
        {
            self.last_operation_status = "illegalArgumentError".into();
            return Err("Error #2004: CurrencyFormatter.setCurrency requires a 3-letter ISO-4217 currency code.".into());
        }
        self.currency_iso_code = iso_code.to_ascii_uppercase();
        self.currency_symbol = symbol.to_owned();
        self.last_operation_status = "noError".into();
        Ok(())
    }

    pub fn is_symbol_safe(&mut self, requested_iso: &str) -> Result<bool, String> {
        if requested_iso.len() != 3 || !requested_iso.chars().all(|c| c.is_ascii_alphabetic()) {
            self.last_operation_status = "illegalArgumentError".into();
            return Err("Error #2004: formattingWithCurrencySymbolIsSafe requires a 3-letter ISO-4217 code.".into());
        }
        self.last_operation_status = "noError".into();
        Ok(self.currency_iso_code.eq_ignore_ascii_case(requested_iso))
    }

    fn format_unsigned_magnitude(&self, abs_val: f64) -> Result<String, String> {
        let raw = format!("{:.prec$}", abs_val, prec = self.fractional_digits);
        let (int_raw, frac_raw) = match raw.split_once('.') {
            Some((i, f)) => (i, f),
            None => (raw.as_str(), ""),
        };
        let int_grouped = if self.use_grouping && !self.grouping_separator.is_empty() {
            let (sizes, repeat) = validate_grouping_pattern(&self.grouping_pattern)?;
            if sizes.is_empty() || int_raw.len() <= sizes[0] {
                int_raw.to_owned()
            } else {
                let mut groups = Vec::new();
                let mut end = int_raw.len();
                let mut idx = 0usize;
                while end > 0 {
                    let sz = if idx < sizes.len() {
                        sizes[idx]
                    } else if repeat {
                        *sizes.last().unwrap()
                    } else {
                        end
                    };
                    let start = end.saturating_sub(sz);
                    groups.push(&int_raw[start..end]);
                    end = start;
                    idx += 1;
                }
                groups.reverse();
                groups.join(&self.grouping_separator)
            }
        } else {
            int_raw.to_owned()
        };

        let mut frac = frac_raw.to_owned();
        if !self.trailing_zeros {
            while frac.ends_with('0') {
                frac.pop();
            }
        }
        let int_display = if !self.leading_zero && int_grouped == "0" && !frac.is_empty() {
            String::new()
        } else {
            int_grouped
        };

        let combined = if frac.is_empty() {
            int_display
        } else {
            format!("{int_display}{}{frac}", self.decimal_separator)
        };
        Ok(render_national_digits(&combined, self.digits_type))
    }

    pub fn format(&mut self, value: f64, with_currency_symbol: bool) -> Result<String, String> {
        if value.is_nan() {
            self.last_operation_status = "noError".into();
            return Ok("NaN".into());
        }
        if !value.is_finite() {
            self.last_operation_status = "numberOverflowError".into();
            return Ok(if value.is_sign_negative() {
                format!("{}Infinity", self.negative_symbol)
            } else {
                "Infinity".into()
            });
        }
        let negative =
            value < 0.0 && value.abs() >= 0.5 * 10_f64.powi(-(self.fractional_digits as i32));
        let num = self.format_unsigned_magnitude(value.abs())?;
        let cur = if with_currency_symbol {
            &self.currency_symbol
        } else {
            &self.currency_iso_code
        };
        let neg = &self.negative_symbol;
        self.last_operation_status = "noError".into();
        if !negative {
            Ok(match self.positive_currency_format {
                0 => format!("{cur}{num}"),
                1 => format!("{num}{cur}"),
                2 => format!("{cur} {num}"),
                _ => format!("{num} {cur}"),
            })
        } else {
            Ok(match self.negative_currency_format {
                0 => format!("({cur}{num})"),
                1 => format!("{neg}{cur}{num}"),
                2 => format!("{cur}{neg}{num}"),
                3 => format!("{cur}{num}{neg}"),
                4 => format!("({num}{cur})"),
                5 => format!("{neg}{num}{cur}"),
                6 => format!("{num}{neg}{cur}"),
                7 => format!("{num}{cur}{neg}"),
                8 => format!("{neg}{num} {cur}"),
                9 => format!("{neg}{cur} {num}"),
                10 => format!("{num} {cur}{neg}"),
                11 => format!("{cur} {num}{neg}"),
                12 => format!("{cur} {neg}{num}"),
                13 => format!("{num}{neg} {cur}"),
                14 => format!("({cur} {num})"),
                _ => format!("({num} {cur})"),
            })
        }
    }

    pub fn parse(&mut self, input: &str) -> Result<Value, String> {
        if input.len() > 1_048_576 {
            self.last_operation_status = "bufferOverflowError".into();
            return Err("CurrencyFormatter.parse input exceeds 1 MiB".into());
        }
        let mut s = input.trim();
        if s.is_empty() {
            self.last_operation_status = "parseError".into();
            return Ok(json!({ "value": null, "currencyString": "" }));
        }
        let mut negative = false;
        let mut paren = false;
        if s.starts_with('(') && s.ends_with(')') && s.len() >= 2 {
            negative = true;
            paren = true;
            s = s[1..s.len() - 1].trim();
        }
        if !self.negative_symbol.is_empty() {
            if let Some(rest) = s.strip_prefix(&self.negative_symbol) {
                negative = true;
                s = rest.trim();
            } else if let Some(rest) = s.strip_suffix(&self.negative_symbol) {
                negative = true;
                s = rest.trim();
            }
        }

        // Locate the first and last digit (or leading decimal separator before a digit).
        let mut num_start: Option<usize> = None;
        let mut num_end: usize = 0;
        let mut idx = 0usize;
        while idx < s.len() {
            if num_start.is_none() && s[idx..].starts_with(&self.decimal_separator) {
                let after = idx + self.decimal_separator.len();
                if s[after..]
                    .chars()
                    .next()
                    .and_then(national_digit_value)
                    .is_some()
                {
                    num_start = Some(idx);
                    idx = after;
                    continue;
                }
            }
            let ch = s[idx..].chars().next().unwrap();
            if national_digit_value(ch).is_some() {
                if num_start.is_none() {
                    num_start = Some(idx);
                }
                idx += ch.len_utf8();
                num_end = idx;
                continue;
            }
            if num_start.is_some() {
                if s[idx..].starts_with(&self.decimal_separator) {
                    idx += self.decimal_separator.len();
                    num_end = idx;
                    continue;
                }
                if !self.grouping_separator.is_empty()
                    && s[idx..].starts_with(&self.grouping_separator)
                {
                    let after = idx + self.grouping_separator.len();
                    if s[after..]
                        .chars()
                        .next()
                        .and_then(national_digit_value)
                        .is_some()
                    {
                        idx = after;
                        continue;
                    }
                }
                break;
            }
            idx += ch.len_utf8();
        }

        let Some(start_pos) = num_start else {
            self.last_operation_status = "parseError".into();
            return Ok(json!({ "value": null, "currencyString": "" }));
        };

        let mut prefix = s[..start_pos].trim();
        let mut suffix = s[num_end..].trim();
        if !self.negative_symbol.is_empty() {
            if let Some(rest) = prefix.strip_suffix(&self.negative_symbol) {
                negative = true;
                prefix = rest.trim();
            } else if let Some(rest) = prefix.strip_prefix(&self.negative_symbol) {
                negative = true;
                prefix = rest.trim();
            }
            if let Some(rest) = suffix.strip_prefix(&self.negative_symbol) {
                negative = true;
                suffix = rest.trim();
            } else if let Some(rest) = suffix.strip_suffix(&self.negative_symbol) {
                negative = true;
                suffix = rest.trim();
            }
        }

        if !prefix.is_empty() && !suffix.is_empty() {
            self.last_operation_status = "parseError".into();
            return Ok(json!({ "value": null, "currencyString": "" }));
        }
        let currency_str = if !prefix.is_empty() { prefix } else { suffix };
        if currency_str
            .chars()
            .any(|c| national_digit_value(c).is_some())
            || (paren && currency_str.contains(['(', ')']))
        {
            self.last_operation_status = "parseError".into();
            return Ok(json!({ "value": null, "currencyString": "" }));
        }

        // Parse the numeric substring strictly.
        let num_slice = &s[start_pos..num_end];
        let mut ascii_num = String::new();
        let mut seen_dec = false;
        let mut pos = 0usize;
        let mut digit_count = 0usize;
        while pos < num_slice.len() {
            if !seen_dec && num_slice[pos..].starts_with(&self.decimal_separator) {
                ascii_num.push('.');
                seen_dec = true;
                pos += self.decimal_separator.len();
                continue;
            }
            if !seen_dec
                && !self.grouping_separator.is_empty()
                && num_slice[pos..].starts_with(&self.grouping_separator)
            {
                pos += self.grouping_separator.len();
                continue;
            }
            let ch = num_slice[pos..].chars().next().unwrap();
            if let Some(d) = national_digit_value(ch) {
                ascii_num.push(char::from(b'0' + d));
                digit_count += 1;
                pos += ch.len_utf8();
            } else {
                self.last_operation_status = "parseError".into();
                return Ok(json!({ "value": null, "currencyString": "" }));
            }
        }
        if digit_count == 0 {
            self.last_operation_status = "parseError".into();
            return Ok(json!({ "value": null, "currencyString": "" }));
        }
        let Ok(mut parsed_val) = ascii_num.parse::<f64>() else {
            self.last_operation_status = "parseError".into();
            return Ok(json!({ "value": null, "currencyString": "" }));
        };
        if negative && parsed_val != 0.0 {
            parsed_val = -parsed_val;
        }
        self.last_operation_status = "noError".into();
        Ok(json!({
            "value": if parsed_val.is_finite() { Some(parsed_val) } else { None },
            "currencyString": currency_str,
        }))
    }
}

// ---------------------------------------------------------------------------
// CollatorState & StringToolsState
// ---------------------------------------------------------------------------

fn fold_halfwidth_fullwidth(input: &str) -> String {
    // Halfwidth Katakana table U+FF66..=U+FF9D + dakuten (FF9E) / handakuten (FF9F).
    const HW_KANA: [char; 56] = [
        'ヲ', 'ァ', 'ィ', 'ゥ', 'ェ', 'ォ', 'ャ', 'ュ', 'ョ', 'ッ', 'ー', 'ア', 'イ', 'ウ', 'エ',
        'オ', 'カ', 'キ', 'ク', 'ケ', 'コ', 'サ', 'シ', 'ス', 'セ', 'ソ', 'タ', 'チ', 'ツ', 'テ',
        'ト', 'ナ', 'ニ', 'ヌ', 'ネ', 'ノ', 'ハ', 'ヒ', 'フ', 'ヘ', 'ホ', 'マ', 'ミ', 'ム', 'メ',
        'モ', 'ヤ', 'ユ', 'ヨ', 'ラ', 'リ', 'ル', 'レ', 'ロ', 'ワ', 'ン',
    ];
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let cp = chars[i] as u32;
        if cp == 0x3000 {
            out.push(' ');
            i += 1;
            continue;
        }
        if (0xFF01..=0xFF5E).contains(&cp) {
            out.push(char::from_u32(cp - 0xFEE0).unwrap_or(chars[i]));
            i += 1;
            continue;
        }
        if cp == 0xFF65 {
            out.push('・');
            i += 1;
            continue;
        }
        if (0xFF66..=0xFF9D).contains(&cp) {
            let mut base = HW_KANA[(cp - 0xFF66) as usize];
            if i + 1 < chars.len() {
                let next_cp = chars[i + 1] as u32;
                if next_cp == 0xFF9E {
                    let voiced = match base {
                        'ウ' => Some('ヴ'),
                        'カ' | 'キ' | 'ク' | 'ケ' | 'コ' | 'サ' | 'シ' | 'ス' | 'セ' | 'ソ'
                        | 'タ' | 'チ' | 'ツ' | 'テ' | 'ト' | 'ハ' | 'ヒ' | 'フ' | 'ヘ' | 'ホ' => {
                            char::from_u32(base as u32 + 1)
                        }
                        _ => None,
                    };
                    if let Some(v) = voiced {
                        base = v;
                        i += 1;
                    }
                } else if next_cp == 0xFF9F
                    && matches!(base, 'ハ' | 'ヒ' | 'フ' | 'ヘ' | 'ホ')
                    && let Some(semi) = char::from_u32(base as u32 + 2)
                {
                    base = semi;
                    i += 1;
                }
            }
            out.push(base);
            i += 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn fold_katakana_to_hiragana(ch: char) -> char {
    let cp = ch as u32;
    if (0x30A1..=0x30F6).contains(&cp) || matches!(cp, 0x30FD | 0x30FE) {
        char::from_u32(cp - 0x60).unwrap_or(ch)
    } else {
        ch
    }
}

fn is_combining_diacritic(ch: char) -> bool {
    let cp = ch as u32;
    (0x0300..=0x036F).contains(&cp)
        || (0x1AB0..=0x1AFF).contains(&cp)
        || (0x1DC0..=0x1DFF).contains(&cp)
        || (0x20D0..=0x20FF).contains(&cp)
        || (0xFE20..=0xFE2F).contains(&cp)
        || matches!(cp, 0x3099 | 0x309A)
}

fn strip_precomposed_diacritic(ch: char) -> char {
    match ch {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' | 'Ǎ' => 'A',
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'ǎ' => 'a',
        'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => 'C',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'Ď' | 'Đ' => 'D',
        'ď' | 'đ' => 'd',
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => 'E',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' => 'G',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'Ĥ' | 'Ħ' => 'H',
        'ĥ' | 'ħ' => 'h',
        'Ì' | 'Í' | 'Î' | 'Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' | 'Ǐ' => 'I',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ǐ' => 'i',
        'Ĵ' => 'J',
        'ĵ' => 'j',
        'Ķ' => 'K',
        'ķ' => 'k',
        'Ĺ' | 'Ļ' | 'Ľ' | 'Ŀ' | 'Ł' => 'L',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' => 'N',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ŏ' | 'Ő' | 'Ǒ' => 'O',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'ǒ' => 'o',
        'Ŕ' | 'Ŗ' | 'Ř' => 'R',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'Ś' | 'Ŝ' | 'Ş' | 'Š' | 'Ș' => 'S',
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => 's',
        'Ţ' | 'Ť' | 'Ŧ' | 'Ț' => 'T',
        'ţ' | 'ť' | 'ŧ' | 'ț' => 't',
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' | 'Ǔ' | 'Ǖ' | 'Ǘ' | 'Ǚ' | 'Ǜ' => {
            'U'
        }
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'ǔ' | 'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' => {
            'u'
        }
        'Ŵ' => 'W',
        'ŵ' => 'w',
        'Ý' | 'Ÿ' | 'Ŷ' => 'Y',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'Ź' | 'Ż' | 'Ž' => 'Z',
        'ź' | 'ż' | 'ž' => 'z',
        // Greek & Cyrillic accented forms
        'Ά' => 'Α',
        'Έ' => 'Ε',
        'Ή' => 'Η',
        'Ί' | 'Ϊ' => 'Ι',
        'Ό' => 'Ο',
        'Ύ' | 'Ϋ' => 'Υ',
        'Ώ' => 'Ω',
        'ά' => 'α',
        'έ' => 'ε',
        'ή' => 'η',
        'ί' | 'ϊ' | 'ΐ' => 'ι',
        'ό' => 'ο',
        'ύ' | 'ϋ' | 'ΰ' => 'υ',
        'ώ' => 'ω',
        'Ё' => 'Е',
        'ё' => 'е',
        'Й' => 'И',
        'й' => 'и',
        // Voiced Kana -> base Kana when ignoring diacritics
        'ガ' => 'カ',
        'ギ' => 'キ',
        'グ' => 'ク',
        'ゲ' => 'ケ',
        'ゴ' => 'コ',
        'ザ' => 'サ',
        'ジ' => 'シ',
        'ズ' => 'ス',
        'ゼ' => 'セ',
        'ゾ' => 'ソ',
        'ダ' => 'タ',
        'ヂ' => 'チ',
        'ヅ' => 'ツ',
        'デ' => 'テ',
        'ド' => 'ト',
        'バ' | 'パ' => 'ハ',
        'ビ' | 'ピ' => 'ヒ',
        'ブ' | 'プ' => 'フ',
        'ベ' | 'ペ' => 'ヘ',
        'ボ' | 'ポ' => 'ホ',
        'ヴ' => 'ウ',
        'が' => 'か',
        'ぎ' => 'き',
        'ぐ' => 'く',
        'げ' => 'け',
        'ご' => 'こ',
        'ざ' => 'さ',
        'じ' => 'し',
        'ず' => 'す',
        'ぜ' => 'せ',
        'ぞ' => 'そ',
        'だ' => 'た',
        'ぢ' => 'ち',
        'づ' => 'つ',
        'で' => 'て',
        'ど' => 'と',
        'ば' | 'ぱ' => 'は',
        'び' | 'ぴ' => 'ひ',
        'ぶ' | 'ぷ' => 'ふ',
        'べ' | 'ぺ' => 'へ',
        'ぼ' | 'ぽ' => 'ほ',
        'ゔ' => 'う',
        other => other,
    }
}

pub fn locale_to_lower_case(input: &str, lang: &str) -> String {
    let turkic = matches!(lang, "tr" | "az");
    let lithuanian = lang == "lt";
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let is_cased = |c: char| c.is_lowercase() || c.is_uppercase();

    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if turkic {
            if ch == 'I' {
                if i + 1 < chars.len() && chars[i + 1] == '\u{0307}' {
                    out.push('i');
                    i += 2;
                    continue;
                }
                out.push('ı');
                i += 1;
                continue;
            }
            if ch == 'İ' {
                out.push('i');
                i += 1;
                continue;
            }
        }
        if lithuanian
            && matches!(ch, 'I' | 'J' | 'Į')
            && i + 1 < chars.len()
            && is_combining_diacritic(chars[i + 1])
        {
            for low in ch.to_lowercase() {
                out.push(low);
            }
            out.push('\u{0307}');
            i += 1;
            continue;
        }
        if ch == 'Σ' {
            let mut prev_cased = false;
            let mut back = i;
            while back > 0 {
                back -= 1;
                if is_combining_diacritic(chars[back]) {
                    continue;
                }
                prev_cased = is_cased(chars[back]);
                break;
            }
            let mut next_cased = false;
            let mut fwd = i + 1;
            while fwd < chars.len() {
                if is_combining_diacritic(chars[fwd]) {
                    fwd += 1;
                    continue;
                }
                next_cased = is_cased(chars[fwd]);
                break;
            }
            out.push(if prev_cased && !next_cased {
                'ς'
            } else {
                'σ'
            });
            i += 1;
            continue;
        }
        for low in ch.to_lowercase() {
            out.push(low);
        }
        i += 1;
    }
    out
}

pub fn locale_to_upper_case(input: &str, lang: &str) -> String {
    let turkic = matches!(lang, "tr" | "az");
    let lithuanian = lang == "lt";
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());

    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if turkic {
            if ch == 'i' {
                out.push('İ');
                i += 1;
                continue;
            }
            if ch == 'ı' {
                out.push('I');
                i += 1;
                continue;
            }
        }
        if lithuanian
            && i > 0
            && ch == '\u{0307}'
            && matches!(chars[i - 1], 'i' | 'j' | 'į' | 'I' | 'J' | 'Į')
        {
            i += 1;
            continue;
        }
        for up in ch.to_uppercase() {
            out.push(up);
        }
        i += 1;
    }
    out
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollatorState {
    #[serde(rename = "requestedLocaleIDName")]
    pub requested_locale_id_name: String,
    #[serde(rename = "actualLocaleIDName")]
    pub actual_locale_id_name: String,
    pub ignore_case: bool,
    pub ignore_character_width: bool,
    pub ignore_diacritics: bool,
    pub ignore_kana_type: bool,
    pub ignore_symbols: bool,
    pub numeric_comparison: bool,
    pub last_operation_status: String,
    #[serde(skip)]
    lang: String,
}

impl CollatorState {
    pub fn new(requested: &str, initial_mode: &str, default_locale: &str) -> Result<Self, String> {
        let matching = match initial_mode {
            "sorting" => false,
            "matching" => true,
            _ => {
                return Err(
                    "Error #2004: Collator initialMode must be CollatorMode.SORTING or CollatorMode.MATCHING."
                        .into(),
                );
            }
        };
        let parsed = parse_locale_id(requested, default_locale);
        let (actual, lang, status) = if parsed.status == "illegalArgumentError" {
            (
                "en-US".to_owned(),
                "en".to_owned(),
                "usingDefaultWarning".to_owned(),
            )
        } else {
            let act = if parsed.region.is_empty() {
                parsed.language.clone()
            } else {
                format!("{}-{}", parsed.language, parsed.region)
            };
            (act, parsed.language, parsed.status)
        };
        Ok(Self {
            requested_locale_id_name: requested.to_owned(),
            actual_locale_id_name: actual,
            ignore_case: matching,
            ignore_character_width: matching,
            ignore_diacritics: matching,
            ignore_kana_type: matching,
            ignore_symbols: false,
            numeric_comparison: false,
            last_operation_status: status,
            lang,
        })
    }

    pub fn set_bool(&mut self, name: &str, val: bool) -> Result<(), String> {
        match name {
            "ignoreCase" => self.ignore_case = val,
            "ignoreCharacterWidth" => self.ignore_character_width = val,
            "ignoreDiacritics" => self.ignore_diacritics = val,
            "ignoreKanaType" => self.ignore_kana_type = val,
            "ignoreSymbols" => self.ignore_symbols = val,
            "numericComparison" => self.numeric_comparison = val,
            _ => {
                self.last_operation_status = "illegalArgumentError".into();
                return Err(format!("Collator.{name} is read-only or unknown"));
            }
        }
        self.last_operation_status = "noError".into();
        Ok(())
    }

    fn normalize(&self, s: &str, fold_case_always: bool, strip_diacritics_always: bool) -> String {
        let step1 = if self.ignore_character_width {
            fold_halfwidth_fullwidth(s)
        } else {
            s.to_owned()
        };
        let mut step2 = String::with_capacity(step1.len());
        for mut ch in step1.chars() {
            if (self.ignore_diacritics || strip_diacritics_always) && is_combining_diacritic(ch) {
                continue;
            }
            if self.ignore_kana_type {
                ch = fold_katakana_to_hiragana(ch);
            }
            if self.ignore_diacritics || strip_diacritics_always {
                ch = strip_precomposed_diacritic(ch);
            }
            if self.ignore_symbols && !ch.is_alphanumeric() && !is_combining_diacritic(ch) {
                continue;
            }
            if (self.ignore_case || fold_case_always) && ch == 'ß' {
                step2.push_str("ss");
                continue;
            }
            step2.push(ch);
        }
        if self.ignore_case || fold_case_always {
            let lowered = locale_to_lower_case(&step2, &self.lang);
            lowered.replace('ς', "σ")
        } else {
            step2
        }
    }

    fn compare_tokens(&self, a: &str, b: &str) -> Ordering {
        if !self.numeric_comparison {
            return a.cmp(b);
        }
        let ac: Vec<char> = a.chars().collect();
        let bc: Vec<char> = b.chars().collect();
        let (mut ia, mut ib) = (0usize, 0usize);
        while ia < ac.len() && ib < bc.len() {
            let da = national_digit_value(ac[ia]);
            let db = national_digit_value(bc[ib]);
            if da.is_some() && db.is_some() {
                let mut digits_a = Vec::new();
                while ia < ac.len() {
                    if let Some(d) = national_digit_value(ac[ia]) {
                        digits_a.push(d);
                        ia += 1;
                    } else {
                        break;
                    }
                }
                let mut digits_b = Vec::new();
                while ib < bc.len() {
                    if let Some(d) = national_digit_value(bc[ib]) {
                        digits_b.push(d);
                        ib += 1;
                    } else {
                        break;
                    }
                }
                let trim_a = digits_a
                    .iter()
                    .position(|&d| d != 0)
                    .map_or(&[][..], |p| &digits_a[p..]);
                let trim_b = digits_b
                    .iter()
                    .position(|&d| d != 0)
                    .map_or(&[][..], |p| &digits_b[p..]);
                match trim_a.len().cmp(&trim_b.len()) {
                    Ordering::Equal => match trim_a.cmp(trim_b) {
                        Ordering::Equal => {}
                        non_eq => return non_eq,
                    },
                    non_eq => return non_eq,
                }
                continue;
            }
            match ac[ia].cmp(&bc[ib]) {
                Ordering::Equal => {
                    ia += 1;
                    ib += 1;
                }
                non_eq => return non_eq,
            }
        }
        (ac.len() - ia).cmp(&(bc.len() - ib))
    }

    pub fn compare(&mut self, a: &str, b: &str) -> i32 {
        self.last_operation_status = "noError".into();
        // Level 1: Primary letters (base letters, case-folded & diacritic-folded)
        let prim_a = self.normalize(a, true, true);
        let prim_b = self.normalize(b, true, true);
        match self.compare_tokens(&prim_a, &prim_b) {
            Ordering::Less => return -1,
            Ordering::Greater => return 1,
            Ordering::Equal => {}
        }
        // Level 2: Secondary diacritics (if !ignoreDiacritics)
        if !self.ignore_diacritics {
            let sec_a = self.normalize(a, true, false);
            let sec_b = self.normalize(b, true, false);
            match self.compare_tokens(&sec_a, &sec_b) {
                Ordering::Less => return -1,
                Ordering::Greater => return 1,
                Ordering::Equal => {}
            }
        }
        // Level 3: Tertiary case / width / kana (if not ignored)
        let tert_a = self.normalize(a, false, false);
        let tert_b = self.normalize(b, false, false);
        if !self.ignore_case {
            // UCA lowercase-before-uppercase tie-breaking on identical base/diacritics.
            for (ca, cb) in tert_a.chars().zip(tert_b.chars()) {
                if ca != cb {
                    let la = ca.is_lowercase();
                    let lb = cb.is_lowercase();
                    if la != lb && ca.to_lowercase().eq(cb.to_lowercase()) {
                        return if la { -1 } else { 1 };
                    }
                    return if ca < cb { -1 } else { 1 };
                }
            }
        }
        match self.compare_tokens(&tert_a, &tert_b) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }

    pub fn equals(&mut self, a: &str, b: &str) -> bool {
        self.compare(a, b) == 0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StringToolsState {
    #[serde(rename = "requestedLocaleIDName")]
    pub requested_locale_id_name: String,
    #[serde(rename = "actualLocaleIDName")]
    pub actual_locale_id_name: String,
    pub last_operation_status: String,
    #[serde(skip)]
    lang: String,
}

impl StringToolsState {
    pub fn new(requested: &str, default_locale: &str) -> Self {
        let parsed = parse_locale_id(requested, default_locale);
        let (actual, lang, status) = if parsed.status == "illegalArgumentError" {
            (
                "en-US".to_owned(),
                "en".to_owned(),
                "usingDefaultWarning".to_owned(),
            )
        } else {
            let act = if parsed.region.is_empty() {
                parsed.language.clone()
            } else {
                format!("{}-{}", parsed.language, parsed.region)
            };
            (act, parsed.language, parsed.status)
        };
        Self {
            requested_locale_id_name: requested.to_owned(),
            actual_locale_id_name: actual,
            last_operation_status: status,
            lang,
        }
    }

    pub fn to_lower(&mut self, value: &str) -> String {
        self.last_operation_status = "noError".into();
        locale_to_lower_case(value, &self.lang)
    }

    pub fn to_upper(&mut self, value: &str) -> String {
        self.last_operation_status = "noError".into();
        locale_to_upper_case(value, &self.lang)
    }
}

// ---------------------------------------------------------------------------
// GroupSpecifier & NetGroup P2P Mesh
// ---------------------------------------------------------------------------

fn encode_tlv(tag: u8, payload: &[u8], out: &mut Vec<u8>) {
    out.push(tag);
    let len = payload.len();
    if len < 0x80 {
        out.push(len as u8);
    } else {
        out.push(0x80 | (((len >> 8) & 0x7f) as u8));
        out.push((len & 0xff) as u8);
    }
    out.extend_from_slice(payload);
}

pub fn encode_posting_authorization(password: &str) -> String {
    let mut buf = Vec::new();
    let digest = sha256(format!("post-auth:{password}").as_bytes());
    encode_tlv(0x15, &digest[..16], &mut buf);
    hex_encode(&buf)
}

pub fn encode_publish_authorization(password: &str) -> String {
    let mut buf = Vec::new();
    let digest = sha256(format!("pub-auth:{password}").as_bytes());
    encode_tlv(0x13, &digest[..16], &mut buf);
    hex_encode(&buf)
}

pub fn encode_bootstrap_peer_id_spec(peer_id: &str) -> Result<String, String> {
    if peer_id.trim().is_empty() {
        return Err("Error #2004: PeerID must not be empty.".into());
    }
    let mut buf = Vec::new();
    let peer_bytes = hex_decode(peer_id).unwrap_or_else(|_| sha256(peer_id.as_bytes()).to_vec());
    encode_tlv(0x0b, &peer_bytes, &mut buf);
    Ok(hex_encode(&buf))
}

pub fn encode_ip_multicast_address_spec(
    address: &str,
    port: Option<u32>,
    source: Option<&str>,
) -> Result<String, String> {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        return Err("Error #2004: Multicast address must not be empty.".into());
    }
    let (host, resolved_port) = if let Some((h, p)) = trimmed.rsplit_once(':') {
        if !h.contains(':') {
            let parsed_p = p
                .parse::<u32>()
                .map_err(|_| "Error #2004: Invalid multicast port.".to_owned())?;
            (h, port.unwrap_or(parsed_p))
        } else {
            (trimmed, port.unwrap_or(0))
        }
    } else {
        (trimmed, port.unwrap_or(0))
    };
    if resolved_port > 65535 {
        return Err("Error #2004: Multicast port must be 0..65535.".into());
    }
    let mut payload = format!("{host}:{resolved_port}").into_bytes();
    if let Some(src) = source.filter(|s| !s.is_empty()) {
        payload.push(b'@');
        payload.extend_from_slice(src.as_bytes());
    }
    let mut buf = Vec::new();
    encode_tlv(0x0d, &payload, &mut buf);
    Ok(hex_encode(&buf))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSpecifierState {
    pub name: String,
    pub ip_multicast_member_updates_enabled: bool,
    pub min_groupspec_version: u32,
    pub multicast_enabled: bool,
    pub object_replication_enabled: bool,
    pub peer_to_peer_disabled: bool,
    pub posting_enabled: bool,
    pub routing_enabled: bool,
    pub server_channel_enabled: bool,
    unique_nonce: Option<String>,
    posting_password_hash: Option<[u8; 32]>,
    posting_auth_hex: Option<String>,
    publish_password_hash: Option<[u8; 32]>,
    publish_auth_hex: Option<String>,
    bootstrap_specs: Vec<String>,
    multicast_specs: Vec<String>,
}

impl GroupSpecifierState {
    pub const MAX_SUPPORTED_VERSION: u32 = 2;

    pub fn new(name: &str, _counter: u64) -> Result<Self, String> {
        if name.is_empty() {
            return Err(
                "ArgumentError: Error #2004: GroupSpecifier name must not be empty.".into(),
            );
        }
        Ok(Self {
            name: name.to_owned(),
            ip_multicast_member_updates_enabled: false,
            min_groupspec_version: 1,
            multicast_enabled: false,
            object_replication_enabled: false,
            peer_to_peer_disabled: false,
            posting_enabled: false,
            routing_enabled: false,
            server_channel_enabled: false,
            unique_nonce: None,
            posting_password_hash: None,
            posting_auth_hex: None,
            publish_password_hash: None,
            publish_auth_hex: None,
            bootstrap_specs: Vec::new(),
            multicast_specs: Vec::new(),
        })
    }

    pub fn set_min_version(&mut self, version: u32) -> Result<(), String> {
        if !(1..=Self::MAX_SUPPORTED_VERSION).contains(&version) {
            return Err("RangeError: minGroupspecVersion must be between 1 and 2.".into());
        }
        self.min_groupspec_version = version;
        Ok(())
    }

    pub fn make_unique(&mut self, entropy_counter: &mut u64) {
        *entropy_counter = entropy_counter.wrapping_add(1);
        let digest = sha256(format!("unique:{}:{}", self.name, *entropy_counter).as_bytes());
        self.unique_nonce = Some(hex_encode(&digest[..16]));
    }

    pub fn set_posting_password(&mut self, password: Option<&str>, salt: Option<&str>) {
        match password.filter(|p| !p.is_empty()) {
            Some(pw) => {
                let material = format!("post:{}:{}:{pw}", self.name, salt.unwrap_or(""));
                self.posting_password_hash = Some(sha256(material.as_bytes()));
                self.posting_auth_hex = Some(encode_posting_authorization(&material));
            }
            None => {
                self.posting_password_hash = None;
                self.posting_auth_hex = None;
            }
        }
    }

    pub fn set_publish_password(&mut self, password: Option<&str>, salt: Option<&str>) {
        match password.filter(|p| !p.is_empty()) {
            Some(pw) => {
                let material = format!("pub:{}:{}:{pw}", self.name, salt.unwrap_or(""));
                self.publish_password_hash = Some(sha256(material.as_bytes()));
                self.publish_auth_hex = Some(encode_publish_authorization(&material));
            }
            None => {
                self.publish_password_hash = None;
                self.publish_auth_hex = None;
            }
        }
    }

    pub fn add_bootstrap_peer(&mut self, peer_id: &str) -> Result<(), String> {
        let spec = encode_bootstrap_peer_id_spec(peer_id)?;
        if !self.bootstrap_specs.contains(&spec) {
            self.bootstrap_specs.push(spec);
        }
        Ok(())
    }

    pub fn add_ip_multicast_address(
        &mut self,
        address: &str,
        port: Option<u32>,
        source: Option<&str>,
    ) -> Result<(), String> {
        let spec = encode_ip_multicast_address_spec(address, port, source)?;
        if !self.multicast_specs.contains(&spec) {
            self.multicast_specs.push(spec);
        }
        Ok(())
    }

    pub fn authorizations(&self) -> String {
        let mut out = String::new();
        if let Some(ref pub_hex) = self.publish_auth_hex {
            out.push_str(pub_hex);
        }
        if let Some(ref post_hex) = self.posting_auth_hex {
            out.push_str(post_hex);
        }
        out
    }

    pub fn groupspec_without_authorizations(&self) -> String {
        let mut tlv = Vec::new();
        tlv.push(if self.min_groupspec_version >= 2 {
            0x02
        } else {
            0x01
        });
        encode_tlv(0x01, self.name.as_bytes(), &mut tlv);
        if let Some(ref nonce) = self.unique_nonce {
            encode_tlv(0x02, nonce.as_bytes(), &mut tlv);
        }
        let mut flags = 0u8;
        if self.posting_enabled {
            flags |= 1 << 0;
        }
        if self.multicast_enabled {
            flags |= 1 << 1;
        }
        if self.object_replication_enabled {
            flags |= 1 << 2;
        }
        if self.routing_enabled {
            flags |= 1 << 3;
        }
        if self.server_channel_enabled {
            flags |= 1 << 4;
        }
        if self.peer_to_peer_disabled {
            flags |= 1 << 5;
        }
        if self.ip_multicast_member_updates_enabled {
            flags |= 1 << 6;
        }
        encode_tlv(0x03, &[flags], &mut tlv);
        if let Some(ref h) = self.publish_password_hash {
            encode_tlv(0x05, &h[..16], &mut tlv);
        }
        if let Some(ref h) = self.posting_password_hash {
            encode_tlv(0x07, &h[..16], &mut tlv);
        }
        let mut hex = format!("G:{}", hex_encode(&tlv));
        for spec in &self.bootstrap_specs {
            hex.push_str(spec);
        }
        for spec in &self.multicast_specs {
            hex.push_str(spec);
        }
        hex
    }

    pub fn groupspec_with_authorizations(&self) -> String {
        format!(
            "{}{}",
            self.groupspec_without_authorizations(),
            self.authorizations()
        )
    }
}

pub fn peer_id_to_group_address(peer_id: &str) -> String {
    hex_encode(&sha256(format!("group-addr:{peer_id}").as_bytes()))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetGroupInfoSnapshot {
    pub object_replication_receive_bytes_per_second: f64,
    pub object_replication_send_bytes_per_second: f64,
    pub posting_receive_control_bytes_per_second: f64,
    pub posting_receive_data_bytes_per_second: f64,
    pub posting_send_control_bytes_per_second: f64,
    pub posting_send_data_bytes_per_second: f64,
    pub routing_receive_bytes_per_second: f64,
    pub routing_send_bytes_per_second: f64,
}

#[derive(Clone, Debug)]
struct NetGroupMember {
    id: u32,
    groupspec_base: String,
    peer_id: String,
    address: String,
    receive_mode: String,
    replication_strategy: String,
    neighbors: BTreeSet<String>,
    member_hints: BTreeSet<String>,
    have_objects: BTreeSet<u64>,
    want_objects: BTreeSet<u64>,
    seen_posts: HashSet<String>,
    info: NetGroupInfoSnapshot,
    closed: bool,
}

// ---------------------------------------------------------------------------
// FLV Stream Parser & NetStream Metrics
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetStreamInfoSnapshot {
    #[serde(rename = "SRTT")]
    pub srtt: f64,
    pub audio_buffer_byte_length: f64,
    pub audio_buffer_length: f64,
    pub audio_byte_count: f64,
    pub audio_bytes_per_second: f64,
    pub audio_loss_rate: f64,
    pub byte_count: f64,
    pub current_bytes_per_second: f64,
    pub data_buffer_byte_length: f64,
    pub data_buffer_length: f64,
    pub data_byte_count: f64,
    pub data_bytes_per_second: f64,
    pub dropped_frames: u32,
    pub is_live: bool,
    pub max_bytes_per_second: f64,
    pub meta_data: Option<Value>,
    pub playback_bytes_per_second: f64,
    pub resource_name: Option<String>,
    pub uri: Option<String>,
    pub video_buffer_byte_length: f64,
    pub video_buffer_length: f64,
    pub video_byte_count: f64,
    pub video_bytes_per_second: f64,
    pub video_loss_rate: f64,
    pub xmp_data: Option<Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetStreamMulticastInfoSnapshot {
    pub bytes_pushed_from_peers: f64,
    pub bytes_pushed_to_peers: f64,
    pub bytes_received_from_ip_multicast: f64,
    pub bytes_received_from_server: f64,
    pub bytes_requested_by_peers: f64,
    pub bytes_requested_from_peers: f64,
    pub fragments_pushed_from_peers: f64,
    pub fragments_pushed_to_peers: f64,
    pub fragments_received_from_ip_multicast: f64,
    pub fragments_received_from_server: f64,
    pub fragments_requested_by_peers: f64,
    pub fragments_requested_from_peers: f64,
    pub receive_control_bytes_per_second: f64,
    pub receive_data_bytes_per_second: f64,
    pub receive_data_bytes_per_second_from_ip_multicast: f64,
    pub receive_data_bytes_per_second_from_server: f64,
    pub send_control_bytes_per_second: f64,
    pub send_control_bytes_per_second_to_server: f64,
    pub send_data_bytes_per_second: f64,
}

#[derive(Clone, Debug, Default)]
pub struct FlvStreamState {
    buffer: Vec<u8>,
    header_parsed: bool,
    pub video_width: u32,
    pub video_height: u32,
    pub decoded_frames: u32,
    pub time_seconds: f64,
    pub back_buffer_byte_length: f64,
    pub back_buffer_length: f64,
    pub info: NetStreamInfoSnapshot,
    pub multicast: NetStreamMulticastInfoSnapshot,
}

/// Minimum `ByteArray` length required for `ApplicationDomain.domainMemory`
/// per Adobe Alchemy / CrossBridge / FlasCC specification.
pub const MIN_DOMAIN_MEMORY_LENGTH: usize = 1024;

pub fn fast_mem_sxi1(val: i32) -> i32 {
    if (val & 1) != 0 { -1 } else { 0 }
}

pub fn fast_mem_sxi8(val: i32) -> i32 {
    (val as i8) as i32
}

pub fn fast_mem_sxi16(val: i32) -> i32 {
    (val as i16) as i32
}

pub fn fast_mem_read(mem: &[u8], op: &str, addr: usize) -> Result<f64, String> {
    if mem.len() < MIN_DOMAIN_MEMORY_LENGTH {
        return Err(
            "Error #1504: End of file or domainMemory smaller than MIN_DOMAIN_MEMORY_LENGTH."
                .into(),
        );
    }
    let width = match op {
        "li8" => 1,
        "li16" => 2,
        "li32" | "lf32" => 4,
        "lf64" => 8,
        _ => return Err(format!("Unsupported fast_mem_read opcode '{op}'")),
    };
    if addr.checked_add(width).is_none_or(|end| end > mem.len()) {
        return Err("RangeError: Error #1506: The specified range is invalid.".into());
    }
    let val = match op {
        "li8" => f64::from(mem[addr]),
        "li16" => f64::from(u16::from_le_bytes([mem[addr], mem[addr + 1]])),
        "li32" => f64::from(i32::from_le_bytes([
            mem[addr],
            mem[addr + 1],
            mem[addr + 2],
            mem[addr + 3],
        ])),
        "lf32" => f64::from(f32::from_le_bytes([
            mem[addr],
            mem[addr + 1],
            mem[addr + 2],
            mem[addr + 3],
        ])),
        "lf64" => f64::from_le_bytes([
            mem[addr],
            mem[addr + 1],
            mem[addr + 2],
            mem[addr + 3],
            mem[addr + 4],
            mem[addr + 5],
            mem[addr + 6],
            mem[addr + 7],
        ]),
        _ => 0.0,
    };
    Ok(val)
}

pub fn fast_mem_write(mem: &mut [u8], op: &str, addr: usize, value: f64) -> Result<(), String> {
    if mem.len() < MIN_DOMAIN_MEMORY_LENGTH {
        return Err(
            "Error #1504: End of file or domainMemory smaller than MIN_DOMAIN_MEMORY_LENGTH."
                .into(),
        );
    }
    let width = match op {
        "si8" => 1,
        "si16" => 2,
        "si32" | "sf32" => 4,
        "sf64" => 8,
        _ => return Err(format!("Unsupported fast_mem_write opcode '{op}'")),
    };
    if addr.checked_add(width).is_none_or(|end| end > mem.len()) {
        return Err("RangeError: Error #1506: The specified range is invalid.".into());
    }
    match op {
        "si8" => {
            mem[addr] = (value as i64) as u8;
        }
        "si16" => {
            let bytes = ((value as i64) as u16).to_le_bytes();
            mem[addr..addr + 2].copy_from_slice(&bytes);
        }
        "si32" => {
            let bytes = ((value as i64) as i32).to_le_bytes();
            mem[addr..addr + 4].copy_from_slice(&bytes);
        }
        "sf32" => {
            let bytes = (value as f32).to_le_bytes();
            mem[addr..addr + 4].copy_from_slice(&bytes);
        }
        "sf64" => {
            let bytes = value.to_le_bytes();
            mem[addr..addr + 8].copy_from_slice(&bytes);
        }
        _ => {}
    }
    Ok(())
}

fn parse_amf0_simple(data: &[u8], pos: &mut usize, depth: usize) -> Option<Value> {
    if depth > 16 || *pos >= data.len() {
        return None;
    }
    let marker = data[*pos];
    *pos += 1;
    match marker {
        0x00 => {
            if *pos + 8 > data.len() {
                return None;
            }
            let n = f64::from_be_bytes(data[*pos..*pos + 8].try_into().ok()?);
            *pos += 8;
            Some(if n.is_finite() { json!(n) } else { Value::Null })
        }
        0x01 => {
            if *pos >= data.len() {
                return None;
            }
            let b = data[*pos] != 0;
            *pos += 1;
            Some(Value::Bool(b))
        }
        0x02 => {
            if *pos + 2 > data.len() {
                return None;
            }
            let len = u16::from_be_bytes([data[*pos], data[*pos + 1]]) as usize;
            *pos += 2;
            if *pos + len > data.len() {
                return None;
            }
            let s = String::from_utf8_lossy(&data[*pos..*pos + len]).into_owned();
            *pos += len;
            Some(Value::String(s))
        }
        0x03 | 0x08 => {
            if marker == 0x08 {
                if *pos + 4 > data.len() {
                    return None;
                }
                *pos += 4; // ECMA array count hint
            }
            let mut map = Map::new();
            while *pos + 2 <= data.len() {
                let klen = u16::from_be_bytes([data[*pos], data[*pos + 1]]) as usize;
                *pos += 2;
                if klen == 0 && *pos < data.len() && data[*pos] == 0x09 {
                    *pos += 1;
                    break;
                }
                if *pos + klen > data.len() {
                    return None;
                }
                let key = String::from_utf8_lossy(&data[*pos..*pos + klen]).into_owned();
                *pos += klen;
                let val = parse_amf0_simple(data, pos, depth + 1)?;
                map.insert(key, val);
            }
            Some(Value::Object(map))
        }
        0x05 | 0x06 => Some(Value::Null),
        0x0a => {
            if *pos + 4 > data.len() {
                return None;
            }
            let count = u32::from_be_bytes(data[*pos..*pos + 4].try_into().ok()?) as usize;
            *pos += 4;
            let mut arr = Vec::with_capacity(count.min(256));
            for _ in 0..count.min(1024) {
                arr.push(parse_amf0_simple(data, pos, depth + 1)?);
            }
            Some(Value::Array(arr))
        }
        _ => None,
    }
}

impl FlvStreamState {
    pub fn apply_action(&mut self, action: &str) -> Result<Value, String> {
        match action {
            "resetBegin" => {
                self.buffer.clear();
                self.header_parsed = false;
                Ok(json!({ "status": "NetStream.Buffer.Flush" }))
            }
            "resetSeek" => {
                self.buffer.clear();
                // Keep header_parsed = true so tag-aligned appends can follow immediately after seek!
                self.header_parsed = true;
                Ok(json!({ "status": "NetStream.Seek.Notify" }))
            }
            "endSequence" => {
                self.buffer.clear();
                Ok(json!({
                    "status": "NetStream.Buffer.Flush",
                    "ended": true,
                    "playStatus": {
                        "code": "NetStream.Play.Complete",
                        "level": "status",
                        "bytes": self.info.byte_count,
                        "duration": self.info.video_buffer_length.max(self.info.audio_buffer_length),
                    }
                }))
            }
            _ => Err(format!(
                "ArgumentError: Invalid NetStreamAppendBytesAction '{action}'."
            )),
        }
    }

    pub fn append_bytes(&mut self, chunk: &[u8]) -> Value {
        self.info.byte_count += chunk.len() as f64;
        self.info.current_bytes_per_second = chunk.len() as f64;
        if self.info.current_bytes_per_second > self.info.max_bytes_per_second {
            self.info.max_bytes_per_second = self.info.current_bytes_per_second;
        }
        self.buffer.extend_from_slice(chunk);

        let mut callbacks = Vec::new();

        if !self.header_parsed {
            if self.buffer.len() >= 13 && &self.buffer[0..3] == b"FLV" {
                let data_offset = u32::from_be_bytes([
                    self.buffer[5],
                    self.buffer[6],
                    self.buffer[7],
                    self.buffer[8],
                ]) as usize;
                let start = data_offset.saturating_add(4); // skip header + PreviousTagSize0
                if self.buffer.len() >= start {
                    self.buffer.drain(0..start);
                    self.header_parsed = true;
                }
            } else if self.buffer.len() >= 11 && matches!(self.buffer[0], 8 | 9 | 18) {
                // Tag-only stream after RESET_SEEK
                self.header_parsed = true;
            }
        }

        while self.header_parsed && self.buffer.len() >= 11 {
            let tag_type = self.buffer[0] & 0x1f;
            let data_size = ((self.buffer[1] as usize) << 16)
                | ((self.buffer[2] as usize) << 8)
                | (self.buffer[3] as usize);
            let ts_low = ((self.buffer[4] as u32) << 16)
                | ((self.buffer[5] as u32) << 8)
                | (self.buffer[6] as u32);
            let ts_ext = (self.buffer[7] as u32) << 24;
            let timestamp = (ts_ext | ts_low) as f64;
            let total_tag_bytes = 11 + data_size + 4;
            if self.buffer.len() < 11 + data_size {
                break;
            }
            let payload = &self.buffer[11..11 + data_size];
            match tag_type {
                8 => {
                    self.info.audio_byte_count += data_size as f64;
                    self.info.audio_buffer_byte_length += data_size as f64;
                    self.info.audio_buffer_length =
                        (timestamp / 1000.0).max(self.info.audio_buffer_length);
                    self.time_seconds = (timestamp / 1000.0).max(self.time_seconds);
                    self.back_buffer_byte_length += data_size as f64;
                    self.back_buffer_length = (timestamp / 1000.0).max(self.back_buffer_length);
                }
                9 => {
                    self.info.video_byte_count += data_size as f64;
                    self.info.video_buffer_byte_length += data_size as f64;
                    self.info.video_buffer_length =
                        (timestamp / 1000.0).max(self.info.video_buffer_length);
                    self.time_seconds = (timestamp / 1000.0).max(self.time_seconds);
                    self.decoded_frames = self.decoded_frames.saturating_add(1);
                    if payload.len() >= 5 && (payload[0] & 0x0f) == 3 {
                        // Screen Video v1 header: 4-bit blockWidth, 12-bit imageWidth, 4-bit blockHeight, 12-bit imageHeight
                        let w = (((payload[1] as u32) & 0x0f) << 8) | (payload[2] as u32);
                        let h = (((payload[3] as u32) & 0x0f) << 8) | (payload[4] as u32);
                        if w > 0 && h > 0 {
                            self.video_width = w;
                            self.video_height = h;
                        }
                    }
                    if self.video_width == 0 {
                        self.video_width = 320;
                    }
                    if self.video_height == 0 {
                        self.video_height = 240;
                    }
                    self.back_buffer_byte_length += data_size as f64;
                    self.back_buffer_length = (timestamp / 1000.0).max(self.back_buffer_length);
                    if !payload.is_empty() && ((payload[0] >> 4) & 0x0f) == 1 {
                        callbacks.push(json!({
                            "handler": "onSeekPoint",
                            "timestamp": timestamp,
                            "info": {
                                "time": timestamp / 1000.0,
                                "offset": self.info.byte_count - (self.buffer.len() as f64),
                            },
                        }));
                    }
                }
                18 => {
                    self.info.data_byte_count += data_size as f64;
                    self.info.data_buffer_byte_length += data_size as f64;
                    self.time_seconds = (timestamp / 1000.0).max(self.time_seconds);
                    self.info.data_buffer_length =
                        (timestamp / 1000.0).max(self.info.data_buffer_length);
                    let mut pos = 0usize;
                    if let Some(Value::String(handler)) = parse_amf0_simple(payload, &mut pos, 0) {
                        let data_val = parse_amf0_simple(payload, &mut pos, 0).unwrap_or(json!({}));
                        if handler == "onMetaData" {
                            if let Some(w) = data_val.get("width").and_then(|v| v.as_f64())
                                && w > 0.0
                            {
                                self.video_width = w as u32;
                            }
                            if let Some(h) = data_val.get("height").and_then(|v| v.as_f64())
                                && h > 0.0
                            {
                                self.video_height = h as u32;
                            }
                            self.info.meta_data = Some(data_val.clone());
                        } else if handler == "onXMPData" {
                            self.info.xmp_data = Some(data_val.clone());
                        }
                        callbacks.push(json!({
                            "handler": handler,
                            "timestamp": timestamp,
                            "info": data_val,
                        }));
                    }
                }
                _ => {}
            }
            let consume = total_tag_bytes.min(self.buffer.len());
            self.buffer.drain(0..consume);
        }

        json!({
            "callbacks": callbacks,
            "info": self.info,
            "timeSeconds": self.time_seconds,
            "videoWidth": self.video_width,
            "videoHeight": self.video_height,
            "decodedFrames": self.decoded_frames,
            "backBufferLength": self.back_buffer_length,
            "backBufferByteLength": self.back_buffer_byte_length,
        })
    }
}

// ---------------------------------------------------------------------------
// LocalConnection & XMLSocket Framing
// ---------------------------------------------------------------------------

const FORBIDDEN_LC_METHODS: &[&str] = &[
    "send",
    "connect",
    "close",
    "allowDomain",
    "allowInsecureDomain",
    "client",
    "domain",
    "isPerUser",
];

#[derive(Clone, Debug)]
struct LocalConnectionEndpoint {
    raw_name: String,
    qualified_name: String,
    domain: String,
    is_per_user: bool,
    allow_all: bool,
    allowed_domains: HashSet<String>,
}

pub fn normalize_lc_name(name: &str, domain: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 64 || name.contains("::") {
        return Err(
            "ArgumentError: Error #2004: LocalConnection name must be 1..64 valid characters."
                .into(),
        );
    }
    if name.starts_with('_') {
        Ok(name.to_owned())
    } else if let Some((d, local)) = name.split_once(':') {
        if d.is_empty() || local.is_empty() {
            return Err("ArgumentError: Error #2004: Malformed domain:connectionName.".into());
        }
        Ok(format!("{d}:{local}"))
    } else {
        Ok(format!("{domain}:{name}"))
    }
}

#[derive(Clone, Debug, Default)]
pub struct XmlSocketFramer {
    pending: Vec<u8>,
}

impl XmlSocketFramer {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut frames = Vec::new();
        for &b in bytes {
            if b == 0 {
                let text = String::from_utf8_lossy(&self.pending).into_owned();
                self.pending.clear();
                frames.push(text);
            } else {
                self.pending.push(b);
            }
        }
        frames
    }
}

// ---------------------------------------------------------------------------
// X.509 DER/PEM ASN.1 Certificate Parser & Loopback Certificate Generator
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct X500DistinguishedNameData {
    pub common_name: Option<String>,
    pub organization_name: Option<String>,
    pub organizational_unit_name: Option<String>,
    pub locality_name: Option<String>,
    pub state_or_province_name: Option<String>,
    pub country_name: Option<String>,
}

impl X500DistinguishedNameData {
    pub fn formatted(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ref cn) = self.common_name {
            parts.push(format!("CN={cn}"));
        }
        if let Some(ref ou) = self.organizational_unit_name {
            parts.push(format!("OU={ou}"));
        }
        if let Some(ref o) = self.organization_name {
            parts.push(format!("O={o}"));
        }
        if let Some(ref l) = self.locality_name {
            parts.push(format!("L={l}"));
        }
        if let Some(ref st) = self.state_or_province_name {
            parts.push(format!("ST={st}"));
        }
        if let Some(ref c) = self.country_name {
            parts.push(format!("C={c}"));
        }
        parts.join(", ")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct X509CertificateData {
    pub version: u32,
    pub serial_number: String,
    #[serde(rename = "signatureAlgorithmOID")]
    pub signature_algorithm_oid: String,
    pub signature_algorithm_params_base64: Option<String>,
    pub issuer: X500DistinguishedNameData,
    pub valid_not_before_ms: f64,
    pub valid_not_after_ms: f64,
    pub subject: X500DistinguishedNameData,
    #[serde(rename = "subjectPublicKeyAlgorithmOID")]
    pub subject_public_key_algorithm_oid: String,
    pub subject_public_key: String,
    #[serde(rename = "issuerUniqueID")]
    pub issuer_unique_id: Option<String>,
    #[serde(rename = "subjectUniqueID")]
    pub subject_unique_id: Option<String>,
    pub encoded_base64: String,
}

fn read_asn1_tlv<'a>(data: &'a [u8], pos: &mut usize) -> Result<(u8, &'a [u8]), String> {
    if *pos + 2 > data.len() {
        return Err("Truncated ASN.1 TLV header".into());
    }
    let tag = data[*pos];
    let len_byte = data[*pos + 1];
    *pos += 2;
    let len = if (len_byte & 0x80) == 0 {
        len_byte as usize
    } else {
        let count = (len_byte & 0x7f) as usize;
        if count == 0 || count > 4 || *pos + count > data.len() {
            return Err("Invalid ASN.1 length".into());
        }
        let mut acc = 0usize;
        for _ in 0..count {
            acc = (acc << 8) | (data[*pos] as usize);
            *pos += 1;
        }
        acc
    };
    if *pos + len > data.len() {
        return Err("Truncated ASN.1 TLV body".into());
    }
    let slice = &data[*pos..*pos + len];
    *pos += len;
    Ok((tag, slice))
}

fn decode_oid(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let first = bytes[0];
    let mut arcs = vec![(first / 40) as u64, (first % 40) as u64];
    let mut val = 0u64;
    for &b in &bytes[1..] {
        val = (val << 7) | ((b & 0x7f) as u64);
        if (b & 0x80) == 0 {
            arcs.push(val);
            val = 0;
        }
    }
    arcs.iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn parse_asn1_time_ms(tag: u8, bytes: &[u8]) -> Result<f64, String> {
    let s = std::str::from_utf8(bytes).map_err(|_| "Invalid ASN.1 time string")?;
    let (year, rest) = if tag == 0x17 {
        // UTCTime YYMMDDHHMMSSZ
        if s.len() < 10 {
            return Err("Short UTCTime".into());
        }
        let yy: i32 = s[0..2].parse().map_err(|_| "Invalid UTCTime year")?;
        let y = if yy >= 50 { 1900 + yy } else { 2000 + yy };
        (y, &s[2..])
    } else {
        // GeneralizedTime YYYYMMDDHHMMSSZ
        if s.len() < 12 {
            return Err("Short GeneralizedTime".into());
        }
        let y: i32 = s[0..4]
            .parse()
            .map_err(|_| "Invalid GeneralizedTime year")?;
        (y, &s[4..])
    };
    let month: u32 = rest[0..2].parse().map_err(|_| "Invalid month")?;
    let day: u32 = rest[2..4].parse().map_err(|_| "Invalid day")?;
    let hour: u32 = rest[4..6].parse().map_err(|_| "Invalid hour")?;
    let min: u32 = rest[6..8].parse().map_err(|_| "Invalid minute")?;
    let sec: u32 = if rest.len() >= 10 && rest[8..10].chars().all(|c| c.is_ascii_digit()) {
        rest[8..10].parse().unwrap_or(0)
    } else {
        0
    };
    // Howard Hinnant civil-from-days inverse: days_from_civil(year, month, day)
    let y = year as i64 - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = month as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + (day as u64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + (doe as i64) - 719468;
    let total_secs = days * 86_400 + (hour as i64) * 3600 + (min as i64) * 60 + (sec as i64);
    Ok((total_secs * 1000) as f64)
}

fn parse_x500_name(seq: &[u8]) -> Result<X500DistinguishedNameData, String> {
    let mut dn = X500DistinguishedNameData::default();
    let mut pos = 0usize;
    while pos < seq.len() {
        let (_set_tag, rdn_set) = read_asn1_tlv(seq, &mut pos)?;
        let mut rpos = 0usize;
        while rpos < rdn_set.len() {
            let (_attr_tag, attr_seq) = read_asn1_tlv(rdn_set, &mut rpos)?;
            let mut apos = 0usize;
            let (oid_tag, oid_bytes) = read_asn1_tlv(attr_seq, &mut apos)?;
            if oid_tag != 0x06 {
                continue;
            }
            let oid = decode_oid(oid_bytes);
            let (_val_tag, val_bytes) = read_asn1_tlv(attr_seq, &mut apos)?;
            let text = String::from_utf8_lossy(val_bytes).into_owned();
            match oid.as_str() {
                "2.5.4.3" => dn.common_name = Some(text),
                "2.5.4.6" => dn.country_name = Some(text),
                "2.5.4.7" => dn.locality_name = Some(text),
                "2.5.4.8" => dn.state_or_province_name = Some(text),
                "2.5.4.10" => dn.organization_name = Some(text),
                "2.5.4.11" => dn.organizational_unit_name = Some(text),
                _ => {}
            }
        }
    }
    Ok(dn)
}

pub fn parse_x509_certificate(input: &[u8]) -> Result<X509CertificateData, String> {
    // Support PEM (`-----BEGIN CERTIFICATE-----`) or raw DER.
    let der: Vec<u8> = if let Ok(text) = std::str::from_utf8(input) {
        if let Some(start) = text.find("-----BEGIN CERTIFICATE-----") {
            let body_start = start + "-----BEGIN CERTIFICATE-----".len();
            let end = text[body_start..]
                .find("-----END CERTIFICATE-----")
                .ok_or_else(|| "Unterminated PEM certificate".to_owned())?;
            let b64: String = text[body_start..body_start + end]
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            STANDARD
                .decode(b64.as_bytes())
                .map_err(|_| "Invalid PEM base64".to_owned())?
        } else {
            input.to_vec()
        }
    } else {
        input.to_vec()
    };

    let mut root_pos = 0usize;
    let (cert_tag, cert_body) = read_asn1_tlv(&der, &mut root_pos)?;
    if cert_tag != 0x30 {
        return Err("X.509 certificate requires an outer ASN.1 SEQUENCE".into());
    }
    let mut cpos = 0usize;
    let (tbs_tag, tbs) = read_asn1_tlv(cert_body, &mut cpos)?;
    if tbs_tag != 0x30 {
        return Err("X.509 TBSCertificate requires an ASN.1 SEQUENCE".into());
    }
    let (_sig_alg_tag, outer_sig_alg) = read_asn1_tlv(cert_body, &mut cpos)?;
    let _sig_val = read_asn1_tlv(cert_body, &mut cpos)?;

    let mut tpos = 0usize;
    let mut version = 1u32;
    if tpos < tbs.len() && tbs[tpos] == 0xa0 {
        let (_, ver_wrapper) = read_asn1_tlv(tbs, &mut tpos)?;
        let mut vpos = 0usize;
        let (vtag, vbytes) = read_asn1_tlv(ver_wrapper, &mut vpos)?;
        if vtag == 0x02 && !vbytes.is_empty() {
            version = (vbytes[vbytes.len() - 1] as u32).saturating_add(1);
        }
    }
    let (ser_tag, ser_bytes) = read_asn1_tlv(tbs, &mut tpos)?;
    if ser_tag != 0x02 {
        return Err("X.509 serialNumber requires INTEGER".into());
    }
    let serial_number = hex_encode_upper(ser_bytes);

    let (_tbs_sig_tag, tbs_sig_seq) = read_asn1_tlv(tbs, &mut tpos)?;
    let sig_seq_to_read = if !tbs_sig_seq.is_empty() {
        tbs_sig_seq
    } else {
        outer_sig_alg
    };
    let mut spos = 0usize;
    let (_, sig_oid_bytes) = read_asn1_tlv(sig_seq_to_read, &mut spos)?;
    let signature_algorithm_oid = decode_oid(sig_oid_bytes);
    let signature_algorithm_params_base64 = if spos < sig_seq_to_read.len() {
        let start = spos;
        let (ptag, _) = read_asn1_tlv(sig_seq_to_read, &mut spos)?;
        if ptag == 0x05 {
            None // ASN.1 NULL params
        } else {
            Some(STANDARD.encode(&sig_seq_to_read[start..spos]))
        }
    } else {
        None
    };

    let (_, issuer_seq) = read_asn1_tlv(tbs, &mut tpos)?;
    let issuer = parse_x500_name(issuer_seq)?;

    let (_, validity_seq) = read_asn1_tlv(tbs, &mut tpos)?;
    let mut val_pos = 0usize;
    let (nb_tag, nb_bytes) = read_asn1_tlv(validity_seq, &mut val_pos)?;
    let (na_tag, na_bytes) = read_asn1_tlv(validity_seq, &mut val_pos)?;
    let valid_not_before_ms = parse_asn1_time_ms(nb_tag, nb_bytes)?;
    let valid_not_after_ms = parse_asn1_time_ms(na_tag, na_bytes)?;

    let (_, subject_seq) = read_asn1_tlv(tbs, &mut tpos)?;
    let subject = parse_x500_name(subject_seq)?;

    let (_, spki_seq) = read_asn1_tlv(tbs, &mut tpos)?;
    let mut kpos = 0usize;
    let (_, spki_alg_seq) = read_asn1_tlv(spki_seq, &mut kpos)?;
    let mut kapos = 0usize;
    let (_, spki_oid_bytes) = read_asn1_tlv(spki_alg_seq, &mut kapos)?;
    let subject_public_key_algorithm_oid = decode_oid(spki_oid_bytes);
    let (spk_tag, spk_bytes) = read_asn1_tlv(spki_seq, &mut kpos)?;
    let raw_pk = if spk_tag == 0x03 && !spk_bytes.is_empty() {
        &spk_bytes[1..]
    } else {
        spk_bytes
    };
    let subject_public_key = hex_encode_upper(raw_pk);

    let mut issuer_unique_id = None;
    let mut subject_unique_id = None;
    while tpos < tbs.len() {
        let (opt_tag, opt_bytes) = read_asn1_tlv(tbs, &mut tpos)?;
        if opt_tag == 0x81 {
            issuer_unique_id = Some(hex_encode_upper(opt_bytes));
        } else if opt_tag == 0x82 {
            subject_unique_id = Some(hex_encode_upper(opt_bytes));
        }
    }

    Ok(X509CertificateData {
        version,
        serial_number,
        signature_algorithm_oid,
        signature_algorithm_params_base64,
        issuer,
        valid_not_before_ms,
        valid_not_after_ms,
        subject,
        subject_public_key_algorithm_oid,
        subject_public_key,
        issuer_unique_id,
        subject_unique_id,
        encoded_base64: STANDARD.encode(&der),
    })
}

fn asn1_wrap(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 4);
    out.push(tag);
    let len = body.len();
    if len < 0x80 {
        out.push(len as u8);
    } else if len <= 0xff {
        out.push(0x81);
        out.push(len as u8);
    } else {
        out.push(0x82);
        out.push(((len >> 8) & 0xff) as u8);
        out.push((len & 0xff) as u8);
    }
    out.extend_from_slice(body);
    out
}

fn asn1_rdn(oid_bytes: &[u8], value: &str) -> Vec<u8> {
    let mut seq = asn1_wrap(0x06, oid_bytes);
    seq.extend_from_slice(&asn1_wrap(0x0c, value.as_bytes()));
    let attr = asn1_wrap(0x30, &seq);
    asn1_wrap(0x31, &attr)
}

/// Builds a structurally valid ASN.1 DER X.509 v3 certificate for `common_name`.
pub fn build_loopback_x509_der(common_name: &str) -> Vec<u8> {
    // sha256WithRSAEncryption: 1.2.840.113549.1.1.11
    let sig_oid = [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];
    // rsaEncryption: 1.2.840.113549.1.1.1
    let rsa_oid = [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];

    let mut sig_alg = asn1_wrap(0x06, &sig_oid);
    sig_alg.extend_from_slice(&[0x05, 0x00]);
    let sig_alg_seq = asn1_wrap(0x30, &sig_alg);

    let mut name_body = Vec::new();
    name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x06], "US"));
    name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x08], "California"));
    name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x07], "San Francisco"));
    name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x0a], "NeoFlash Runtime"));
    name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x0b], "Security"));
    name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x03], common_name));
    let name_seq = asn1_wrap(0x30, &name_body);

    let mut validity_body = asn1_wrap(0x18, b"20250101000000Z");
    validity_body.extend_from_slice(&asn1_wrap(0x18, b"20350101000000Z"));
    let validity_seq = asn1_wrap(0x30, &validity_body);

    let mut rsa_alg = asn1_wrap(0x06, &rsa_oid);
    rsa_alg.extend_from_slice(&[0x05, 0x00]);
    let rsa_alg_seq = asn1_wrap(0x30, &rsa_alg);
    let pk_digest = sha256(format!("pk:{common_name}").as_bytes());
    let mut bit_str = vec![0x00];
    bit_str.extend_from_slice(&pk_digest);
    let mut spki_body = rsa_alg_seq;
    spki_body.extend_from_slice(&asn1_wrap(0x03, &bit_str));
    let spki_seq = asn1_wrap(0x30, &spki_body);

    let serial_digest = sha256(format!("serial:{common_name}").as_bytes());
    let mut serial_bytes = vec![0x01];
    serial_bytes.extend_from_slice(&serial_digest[..7]);

    let mut tbs_body = Vec::new();
    tbs_body.extend_from_slice(&asn1_wrap(0xa0, &asn1_wrap(0x02, &[0x02]))); // v3
    tbs_body.extend_from_slice(&asn1_wrap(0x02, &serial_bytes));
    tbs_body.extend_from_slice(&sig_alg_seq);
    tbs_body.extend_from_slice(&name_seq);
    tbs_body.extend_from_slice(&validity_seq);
    tbs_body.extend_from_slice(&name_seq);
    tbs_body.extend_from_slice(&spki_seq);
    let tbs_seq = asn1_wrap(0x30, &tbs_body);

    let sig_digest = sha256(&tbs_seq);
    let mut sig_bits = vec![0x00];
    sig_bits.extend_from_slice(&sig_digest);
    let mut cert_body = tbs_seq;
    cert_body.extend_from_slice(&sig_alg_seq);
    cert_body.extend_from_slice(&asn1_wrap(0x03, &sig_bits));
    asn1_wrap(0x30, &cert_body)
}

// ---------------------------------------------------------------------------
// Capabilities Profile
// ---------------------------------------------------------------------------

pub fn build_capabilities_snapshot(
    width: u32,
    height: u32,
    language: &str,
    os: &str,
    dpi: u32,
    is_debugger: bool,
) -> Value {
    let lang = if language.trim().is_empty() {
        "en"
    } else {
        language.trim()
    };
    let os_name = if os.trim().is_empty() {
        "Mac OS 10.15.7"
    } else {
        os.trim()
    };
    let manufacturer = if os_name.to_ascii_lowercase().contains("win") {
        "Adobe Windows"
    } else if os_name.to_ascii_lowercase().contains("linux") {
        "Adobe Linux"
    } else {
        "Adobe Macintosh"
    };
    let version_prefix = if manufacturer == "Adobe Windows" {
        "WIN"
    } else if manufacturer == "Adobe Linux" {
        "LNX"
    } else {
        "MAC"
    };
    let version = format!("{version_prefix} 32,0,0,465");
    let w = width.max(1);
    let h = height.max(1);
    let screen_dpi = dpi.max(72);

    let encode_q = |s: &str| -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{:02X}", b));
            }
        }
        out
    };
    let deb = if is_debugger { "t" } else { "f" };
    let server_string = format!(
        "A=t&SA=t&SV=t&EV=t&MP3=t&AE=t&VE=t&ACC=f&PR=t&SP=f&SB=f&DEB={deb}&V={}&M={}&R={w}x{h}&COL=color&AR=1.0&OS={}&ARCH=x86&L={}&IME=t&PR32=t&PR64=t&PT=PlugIn&AVD=f&LFD=f&WD=f&TLS=t&ML=5.1&DP={screen_dpi}",
        encode_q(&version),
        encode_q(manufacturer),
        encode_q(os_name),
        encode_q(lang),
    );

    json!({
        "avHardwareDisable": false,
        "cpuAddressSize": 64,
        "cpuArchitecture": "x86",
        "hasAccessibility": false,
        "hasAudio": true,
        "hasAudioEncoder": true,
        "hasEmbeddedVideo": true,
        "hasIME": true,
        "hasMP3": true,
        "hasPrinting": true,
        "hasScreenBroadcast": false,
        "hasScreenPlayback": false,
        "hasStreamingAudio": true,
        "hasStreamingVideo": true,
        "hasTLS": true,
        "hasVideoEncoder": true,
        "isDebugger": is_debugger,
        "isEmbeddedInAcrobat": false,
        "language": lang,
        "localFileReadDisable": false,
        "manufacturer": manufacturer,
        "maxLevelIDC": "5.1",
        "os": os_name,
        "pixelAspectRatio": 1.0,
        "playerType": "PlugIn",
        "screenColor": "color",
        "screenDPI": screen_dpi,
        "screenResolutionX": w,
        "screenResolutionY": h,
        "serverString": server_string,
        "supports32BitProcesses": true,
        "supports64BitProcesses": true,
        "touchscreenType": "none",
        "version": version,
    })
}

// ---------------------------------------------------------------------------
// DRM, Workers, Concurrency, Sampler & Telemetry State in As3NetSysKernel
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DrmVoucherRecord {
    server_url: String,
    license_id: String,
    policy_id: String,
    domain: String,
    voucher_start_ms: f64,
    voucher_end_ms: f64,
    offline_start_ms: f64,
    offline_end_ms: f64,
    window_period: u32,
    window_start_ms: f64,
    window_end_ms: f64,
    policies: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct WorkerRecord {
    id: u32,
    is_primordial: bool,
    state: String, // "new" | "running" | "terminated"
}

#[derive(Clone, Debug)]
struct ChannelRecord {
    id: u32,
    sender_id: u32,
    receiver_id: u32,
    state: String, // "open" | "closing" | "closed"
    queued_count: usize,
}

#[derive(Clone, Debug, Default)]
struct MutexRecord {
    owner_worker: Option<u32>,
    depth: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SamplerEntry {
    kind: String, // "sample" | "new" | "delete"
    time_us: f64,
    id: u64,
    size: u64,
    class_name: Option<String>,
    stack: Vec<(String, String, u32, u64)>,
}

pub struct As3NetSysKernel {
    next_id: u32,
    rng_counter: u64,
    default_locale: String,
    currency_formatters: HashMap<u32, CurrencyFormatterState>,
    collators: HashMap<u32, CollatorState>,
    string_tools: HashMap<u32, StringToolsState>,
    group_specs: HashMap<u32, GroupSpecifierState>,
    net_groups: HashMap<u32, NetGroupMember>,
    flv_streams: HashMap<u32, FlvStreamState>,
    local_connections: HashMap<u32, LocalConnectionEndpoint>,
    lc_by_name: HashMap<String, u32>,
    xml_framers: HashMap<u32, XmlSocketFramer>,
    drm_tokens: HashMap<(String, String), Vec<u8>>,
    drm_vouchers: HashMap<String, DrmVoucherRecord>,
    drm_device_groups: HashSet<String>,
    workers: HashMap<u32, WorkerRecord>,
    channels: HashMap<u32, ChannelRecord>,
    mutexes: HashMap<u32, MutexRecord>,
    conditions: HashMap<u32, u32>, // condition_id -> mutex_id
    sampling_active: bool,
    sampling_paused: bool,
    samples: VecDeque<SamplerEntry>,
    invocation_counts: HashMap<String, u64>,
    telemetry_commands: HashSet<String>,
    telemetry_metrics: Vec<(String, Value)>,
}

impl Default for As3NetSysKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl As3NetSysKernel {
    pub fn new() -> Self {
        let mut kernel = Self {
            next_id: 2,
            rng_counter: 0x9e37_79b9_7f4a_7c15,
            default_locale: "en-US".into(),
            currency_formatters: HashMap::new(),
            collators: HashMap::new(),
            string_tools: HashMap::new(),
            group_specs: HashMap::new(),
            net_groups: HashMap::new(),
            flv_streams: HashMap::new(),
            local_connections: HashMap::new(),
            lc_by_name: HashMap::new(),
            xml_framers: HashMap::new(),
            drm_tokens: HashMap::new(),
            drm_vouchers: HashMap::new(),
            drm_device_groups: HashSet::new(),
            workers: HashMap::new(),
            channels: HashMap::new(),
            mutexes: HashMap::new(),
            conditions: HashMap::new(),
            sampling_active: false,
            sampling_paused: false,
            samples: VecDeque::new(),
            invocation_counts: HashMap::new(),
            telemetry_commands: HashSet::new(),
            telemetry_metrics: Vec::new(),
        };
        kernel.workers.insert(
            1,
            WorkerRecord {
                id: 1,
                is_primordial: true,
                state: "running".into(),
            },
        );
        kernel
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(2);
        id
    }

    pub fn reset(&mut self) {
        let locale = self.default_locale.clone();
        *self = Self::new();
        self.default_locale = locale;
    }

    pub fn random_bytes(&mut self, count: u32, entropy: &[u8]) -> Result<Vec<u8>, String> {
        generate_random_bytes(count, entropy, &mut self.rng_counter)
    }

    pub fn flv_append(&mut self, stream_id: u32, bytes: &[u8]) -> String {
        let state = self.flv_streams.entry(stream_id).or_default();
        state.append_bytes(bytes).to_string()
    }

    pub fn xml_socket_feed(&mut self, socket_id: u32, bytes: &[u8]) -> String {
        let framer = self.xml_framers.entry(socket_id).or_default();
        serde_json::to_string(&framer.feed(bytes)).unwrap_or_else(|_| "[]".into())
    }

    pub fn parse_x509(&self, bytes: &[u8]) -> Result<String, String> {
        let parsed = parse_x509_certificate(bytes)?;
        serde_json::to_string(&parsed).map_err(|e| e.to_string())
    }

    pub fn loopback_x509_der(&self, host: &str) -> Vec<u8> {
        build_loopback_x509_der(if host.is_empty() { "localhost" } else { host })
    }

    pub fn execute_json(&mut self, request_json: &str) -> Result<String, String> {
        let req: Value = serde_json::from_str(request_json)
            .map_err(|e| format!("Invalid As3NetSysKernel JSON request: {e}"))?;
        let op = req
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing 'op' in As3NetSysKernel request".to_owned())?;
        let resp = self.dispatch(op, &req)?;
        serde_json::to_string(&resp).map_err(|e| e.to_string())
    }

    fn recompute_net_group_ring(&mut self, groupspec_base: &str) {
        let mut active_addrs: Vec<(String, u32)> = self
            .net_groups
            .values()
            .filter(|m| !m.closed && m.groupspec_base == groupspec_base)
            .map(|m| (m.address.clone(), m.id))
            .collect();
        active_addrs.sort_by(|a, b| a.0.cmp(&b.0));
        let peers: Vec<String> = self
            .net_groups
            .values()
            .filter(|m| !m.closed && m.groupspec_base == groupspec_base)
            .map(|m| m.peer_id.clone())
            .collect();
        for (_, id) in &active_addrs {
            if let Some(member) = self.net_groups.get_mut(id) {
                for p in &peers {
                    if *p != member.peer_id {
                        member.neighbors.insert(p.clone());
                    }
                }
            }
        }
    }

    fn dispatch(&mut self, op: &str, req: &Value) -> Result<Value, String> {
        match op {
            "set_default_locale" => {
                if let Some(loc) = req.get("locale").and_then(Value::as_str)
                    && !loc.trim().is_empty()
                {
                    self.default_locale = loc.trim().to_owned();
                }
                Ok(json!({ "defaultLocale": self.default_locale }))
            }
            "available_locales" => Ok(json!(SUPPORTED_LOCALES)),
            "locale_parse" => {
                let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                let parsed = parse_locale_id(name, &self.default_locale);
                Ok(serde_json::to_value(parsed).map_err(|e| e.to_string())?)
            }
            "locale_determine_preferred" => {
                let want: Vec<String> = req
                    .get("want")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let have: Vec<String> = req
                    .get("have")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let keyword = req
                    .get("keyword")
                    .and_then(Value::as_str)
                    .unwrap_or("userinterface");
                Ok(json!(determine_preferred_locales(&want, &have, keyword)))
            }
            // CurrencyFormatter
            "currency_new" => {
                let loc = req.get("locale").and_then(Value::as_str).ok_or_else(|| {
                    "TypeError: CurrencyFormatter locale must be non-null.".to_owned()
                })?;
                let id = self.alloc_id();
                let state = CurrencyFormatterState::new(loc, &self.default_locale);
                let snap = serde_json::to_value(&state).map_err(|e| e.to_string())?;
                self.currency_formatters.insert(id, state);
                Ok(json!({ "id": id, "state": snap }))
            }
            "currency_snapshot" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let st = self
                    .currency_formatters
                    .get(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                serde_json::to_value(st).map_err(|e| e.to_string())
            }
            "currency_set_prop" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                let val = req.get("value").cloned().unwrap_or(Value::Null);
                let st = self
                    .currency_formatters
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                st.set_property(name, val)?;
                serde_json::to_value(&*st).map_err(|e| e.to_string())
            }
            "currency_set_currency" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let iso = req.get("iso").and_then(Value::as_str).unwrap_or("");
                let sym = req.get("symbol").and_then(Value::as_str).unwrap_or("");
                let st = self
                    .currency_formatters
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                st.set_currency(iso, sym)?;
                serde_json::to_value(&*st).map_err(|e| e.to_string())
            }
            "currency_is_safe" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let iso = req.get("iso").and_then(Value::as_str).unwrap_or("");
                let st = self
                    .currency_formatters
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                let safe = st.is_symbol_safe(iso)?;
                Ok(json!({ "safe": safe, "status": st.last_operation_status }))
            }
            "currency_format" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let val = match req.get("value") {
                    Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
                    Some(Value::String(s)) => match s.as_str() {
                        "Infinity" => f64::INFINITY,
                        "-Infinity" => f64::NEG_INFINITY,
                        _ => f64::NAN,
                    },
                    _ => f64::NAN,
                };
                let with_sym = req
                    .get("withSymbol")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let st = self
                    .currency_formatters
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                let formatted = st.format(val, with_sym)?;
                Ok(json!({ "formatted": formatted, "status": st.last_operation_status }))
            }
            "currency_parse" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let input = req.get("input").and_then(Value::as_str).unwrap_or("");
                let st = self
                    .currency_formatters
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                let parsed = st.parse(input)?;
                Ok(json!({
                    "value": parsed["value"],
                    "currencyString": parsed["currencyString"],
                    "status": st.last_operation_status,
                }))
            }
            // Collator
            "collator_new" => {
                let loc = req.get("locale").and_then(Value::as_str).unwrap_or("");
                let mode = req.get("mode").and_then(Value::as_str).unwrap_or("sorting");
                let st = CollatorState::new(loc, mode, &self.default_locale)?;
                let id = self.alloc_id();
                let snap = serde_json::to_value(&st).map_err(|e| e.to_string())?;
                self.collators.insert(id, st);
                Ok(json!({ "id": id, "state": snap }))
            }
            "collator_snapshot" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let st = self
                    .collators
                    .get(&id)
                    .ok_or_else(|| "Unknown Collator handle".to_owned())?;
                serde_json::to_value(st).map_err(|e| e.to_string())
            }
            "collator_set_bool" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                let val = req.get("value").and_then(Value::as_bool).ok_or_else(|| {
                    "TypeError: Collator boolean property requires Boolean.".to_owned()
                })?;
                let st = self
                    .collators
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown Collator handle".to_owned())?;
                st.set_bool(name, val)?;
                serde_json::to_value(&*st).map_err(|e| e.to_string())
            }
            "collator_compare" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let a = req.get("a").and_then(Value::as_str).unwrap_or("");
                let b = req.get("b").and_then(Value::as_str).unwrap_or("");
                let st = self
                    .collators
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown Collator handle".to_owned())?;
                let cmp = st.compare(a, b);
                Ok(json!({ "result": cmp, "equals": cmp == 0, "status": st.last_operation_status }))
            }
            // StringTools
            "string_tools_new" => {
                let loc = req.get("locale").and_then(Value::as_str).unwrap_or("");
                let st = StringToolsState::new(loc, &self.default_locale);
                let id = self.alloc_id();
                let snap = serde_json::to_value(&st).map_err(|e| e.to_string())?;
                self.string_tools.insert(id, st);
                Ok(json!({ "id": id, "state": snap }))
            }
            "string_tools_case" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let val = req.get("value").and_then(Value::as_str).unwrap_or("");
                let upper = req.get("upper").and_then(Value::as_bool).unwrap_or(false);
                let st = self
                    .string_tools
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown StringTools handle".to_owned())?;
                let out = if upper {
                    st.to_upper(val)
                } else {
                    st.to_lower(val)
                };
                Ok(json!({ "result": out, "status": st.last_operation_status }))
            }
            // GroupSpecifier
            "groupspec_new" => {
                let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                let st = GroupSpecifierState::new(name, self.rng_counter)?;
                let id = self.alloc_id();
                let snap = serde_json::to_value(&st).map_err(|e| e.to_string())?;
                self.group_specs.insert(id, st);
                Ok(json!({ "id": id, "state": snap }))
            }
            "groupspec_update" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                let mut rng = self.rng_counter;
                let st = self
                    .group_specs
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown GroupSpecifier handle".to_owned())?;
                match action {
                    "set_bool" => {
                        let prop = req.get("name").and_then(Value::as_str).unwrap_or("");
                        let v = req.get("value").and_then(Value::as_bool).unwrap_or(false);
                        match prop {
                            "ipMulticastMemberUpdatesEnabled" => {
                                st.ip_multicast_member_updates_enabled = v
                            }
                            "multicastEnabled" => st.multicast_enabled = v,
                            "objectReplicationEnabled" => st.object_replication_enabled = v,
                            "peerToPeerDisabled" => st.peer_to_peer_disabled = v,
                            "postingEnabled" => st.posting_enabled = v,
                            "routingEnabled" => st.routing_enabled = v,
                            "serverChannelEnabled" => st.server_channel_enabled = v,
                            _ => return Err(format!("Unknown GroupSpecifier property {prop}")),
                        }
                    }
                    "set_min_version" => {
                        let v = req.get("value").and_then(Value::as_u64).unwrap_or(0) as u32;
                        st.set_min_version(v)?;
                    }
                    "make_unique" => {
                        st.make_unique(&mut rng);
                    }
                    "set_posting_password" => {
                        st.set_posting_password(
                            req.get("password").and_then(Value::as_str),
                            req.get("salt").and_then(Value::as_str),
                        );
                    }
                    "set_publish_password" => {
                        st.set_publish_password(
                            req.get("password").and_then(Value::as_str),
                            req.get("salt").and_then(Value::as_str),
                        );
                    }
                    "add_bootstrap_peer" => {
                        st.add_bootstrap_peer(
                            req.get("peerID").and_then(Value::as_str).unwrap_or(""),
                        )?;
                    }
                    "add_ip_multicast_address" => {
                        let port = req.get("port").and_then(Value::as_u64).map(|p| p as u32);
                        st.add_ip_multicast_address(
                            req.get("address").and_then(Value::as_str).unwrap_or(""),
                            port,
                            req.get("source").and_then(Value::as_str),
                        )?;
                    }
                    _ => {}
                }
                self.rng_counter = rng;
                Ok(json!({
                    "state": st,
                    "authorizations": st.authorizations(),
                    "withoutAuth": st.groupspec_without_authorizations(),
                    "withAuth": st.groupspec_with_authorizations(),
                }))
            }
            "groupspec_encode_static" => {
                let kind = req.get("kind").and_then(Value::as_str).unwrap_or("");
                let encoded = match kind {
                    "posting" => encode_posting_authorization(
                        req.get("password").and_then(Value::as_str).unwrap_or(""),
                    ),
                    "publish" => encode_publish_authorization(
                        req.get("password").and_then(Value::as_str).unwrap_or(""),
                    ),
                    "bootstrap" => encode_bootstrap_peer_id_spec(
                        req.get("peerID").and_then(Value::as_str).unwrap_or(""),
                    )?,
                    "multicast" => encode_ip_multicast_address_spec(
                        req.get("address").and_then(Value::as_str).unwrap_or(""),
                        req.get("port").and_then(Value::as_u64).map(|p| p as u32),
                        req.get("source").and_then(Value::as_str),
                    )?,
                    _ => return Err(format!("Unknown static groupspec encoder {kind}")),
                };
                Ok(json!({ "encoded": encoded }))
            }
            // Peer ID & NetGroup
            "net_alloc_peer" => {
                self.rng_counter = self.rng_counter.wrapping_add(1);
                let uri = req.get("uri").and_then(Value::as_str).unwrap_or("rtmfp:");
                let near_id = hex_encode(&sha256(
                    format!("near:{uri}:{}", self.rng_counter).as_bytes(),
                ));
                let far_id = hex_encode(&sha256(
                    format!("far:{uri}:{}", self.rng_counter).as_bytes(),
                ));
                let near_nonce =
                    hex_encode(&sha256(format!("nnonce:{}", self.rng_counter).as_bytes()));
                let far_nonce =
                    hex_encode(&sha256(format!("fnonce:{}", self.rng_counter).as_bytes()));
                Ok(json!({
                    "nearID": near_id,
                    "farID": far_id,
                    "nearNonce": near_nonce,
                    "farNonce": far_nonce,
                }))
            }
            "netgroup_new" => {
                let groupspec = req.get("groupspec").and_then(Value::as_str).unwrap_or("");
                if groupspec.trim().is_empty() {
                    return Err(
                        "ArgumentError: Error #2004: NetGroup groupspec must not be empty.".into(),
                    );
                }
                let peer_id = req.get("peerID").and_then(Value::as_str).unwrap_or("");
                let peer_id = if peer_id.is_empty() {
                    self.rng_counter = self.rng_counter.wrapping_add(1);
                    hex_encode(&sha256(format!("ng-peer:{}", self.rng_counter).as_bytes()))
                } else {
                    peer_id.to_owned()
                };
                // Normalize base groupspec by stripping trailing auth tags 0x13/0x15 if present.
                let base = groupspec.to_owned();
                let id = self.alloc_id();
                let address = peer_id_to_group_address(&peer_id);
                let member = NetGroupMember {
                    id,
                    groupspec_base: base.clone(),
                    peer_id: peer_id.clone(),
                    address: address.clone(),
                    receive_mode: "exact".into(),
                    replication_strategy: "lowestFirst".into(),
                    neighbors: BTreeSet::new(),
                    member_hints: BTreeSet::new(),
                    have_objects: BTreeSet::new(),
                    want_objects: BTreeSet::new(),
                    seen_posts: HashSet::new(),
                    info: NetGroupInfoSnapshot::default(),
                    closed: false,
                };
                self.net_groups.insert(id, member);
                self.recompute_net_group_ring(&base);
                let peer_ids: Vec<u32> = self
                    .net_groups
                    .values()
                    .filter(|m| !m.closed && m.id != id && m.groupspec_base == base)
                    .map(|m| m.id)
                    .collect();
                Ok(json!({
                    "id": id,
                    "address": address,
                    "existingGroupIds": peer_ids,
                }))
            }
            "netgroup_op" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "convert" => {
                        let pid = req.get("peerID").and_then(Value::as_str).unwrap_or("");
                        Ok(json!({ "address": peer_id_to_group_address(pid) }))
                    }
                    "snapshot" => {
                        let m = self
                            .net_groups
                            .get(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        let active_count = self
                            .net_groups
                            .values()
                            .filter(|x| !x.closed && x.groupspec_base == m.groupspec_base)
                            .count();
                        let neighbor_count = m.neighbors.len();
                        Ok(json!({
                            "estimatedMemberCount": (active_count.max(1 + neighbor_count)) as f64,
                            "neighborCount": neighbor_count as f64,
                            "localCoverageFrom": "0000000000000000000000000000000000000000000000000000000000000000",
                            "localCoverageTo": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                            "receiveMode": m.receive_mode,
                            "replicationStrategy": m.replication_strategy,
                            "info": m.info,
                            "closed": m.closed,
                        }))
                    }
                    "set_mode" => {
                        let prop = req.get("name").and_then(Value::as_str).unwrap_or("");
                        let val = req.get("value").and_then(Value::as_str).unwrap_or("");
                        let m = self
                            .net_groups
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        match (prop, val) {
                            ("receiveMode", "exact" | "nearest") => m.receive_mode = val.into(),
                            ("replicationStrategy", "lowestFirst" | "rarestFirst") => {
                                m.replication_strategy = val.into()
                            }
                            _ => {
                                return Err(format!(
                                    "ArgumentError: Error #2004: Invalid NetGroup.{prop} '{val}'."
                                ));
                            }
                        }
                        Ok(json!({ "ok": true }))
                    }
                    "add_neighbor" => {
                        let pid = req.get("peerID").and_then(Value::as_str).unwrap_or("");
                        let m = self
                            .net_groups
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        if m.closed || pid.is_empty() {
                            return Ok(json!({ "added": false }));
                        }
                        Ok(json!({ "added": m.neighbors.insert(pid.to_owned()) }))
                    }
                    "add_member_hint" => {
                        let pid = req.get("peerID").and_then(Value::as_str).unwrap_or("");
                        let m = self
                            .net_groups
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        if m.closed || pid.is_empty() {
                            return Ok(json!({ "added": false }));
                        }
                        Ok(json!({ "added": m.member_hints.insert(pid.to_owned()) }))
                    }
                    "have_want" => {
                        let kind = req.get("kind").and_then(Value::as_str).unwrap_or("");
                        let start = req.get("start").and_then(Value::as_u64).unwrap_or(0);
                        let end = req.get("end").and_then(Value::as_u64).unwrap_or(0);
                        if end < start || end.saturating_sub(start) > 65536 {
                            return Err(
                                "RangeError: NetGroup object index range is invalid.".into()
                            );
                        }
                        let m = self
                            .net_groups
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        for idx in start..=end {
                            match kind {
                                "addHave" => {
                                    m.have_objects.insert(idx);
                                }
                                "removeHave" => {
                                    m.have_objects.remove(&idx);
                                }
                                "addWant" => {
                                    m.want_objects.insert(idx);
                                }
                                "removeWant" => {
                                    m.want_objects.remove(&idx);
                                }
                                _ => {}
                            }
                        }
                        Ok(json!({
                            "haveCount": m.have_objects.len(),
                            "wantCount": m.want_objects.len(),
                        }))
                    }
                    "replication_reply" => {
                        let req_id = req.get("requestID").and_then(Value::as_i64).unwrap_or(0);
                        let bytes =
                            req.get("byteLength").and_then(Value::as_u64).unwrap_or(0) as f64;
                        let deny = req.get("deny").and_then(Value::as_bool).unwrap_or(false);
                        let m = self
                            .net_groups
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        if !deny {
                            m.info.object_replication_send_bytes_per_second += bytes;
                        }
                        Ok(json!({ "requestID": req_id, "denied": deny }))
                    }
                    "post" => {
                        let payload_hash = req
                            .get("payloadSummary")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let bytes =
                            req.get("byteLength").and_then(Value::as_u64).unwrap_or(16) as f64;
                        self.rng_counter = self.rng_counter.wrapping_add(1);
                        let (base, closed) = {
                            let m = self
                                .net_groups
                                .get(&id)
                                .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                            (m.groupspec_base.clone(), m.closed)
                        };
                        if closed {
                            return Ok(json!({ "messageID": Value::Null, "recipients": [] }));
                        }
                        let msg_id = hex_encode(&sha256(
                            format!("post:{id}:{}:{payload_hash}", self.rng_counter).as_bytes(),
                        ));
                        if let Some(sender) = self.net_groups.get_mut(&id) {
                            sender.seen_posts.insert(msg_id.clone());
                            sender.info.posting_send_data_bytes_per_second += bytes;
                        }
                        let mut recipients = Vec::new();
                        for other in self.net_groups.values_mut() {
                            if !other.closed
                                && other.id != id
                                && other.groupspec_base == base
                                && other.seen_posts.insert(msg_id.clone())
                            {
                                other.info.posting_receive_data_bytes_per_second += bytes;
                                recipients.push(other.id);
                            }
                        }
                        Ok(json!({ "messageID": msg_id, "recipients": recipients }))
                    }
                    "route" => {
                        let mode = req.get("sendMode").and_then(Value::as_str).unwrap_or("all");
                        let target_addr = req
                            .get("groupAddress")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let bytes =
                            req.get("byteLength").and_then(Value::as_u64).unwrap_or(16) as f64;
                        if mode != "all"
                            && mode != "nearest"
                            && !matches!(mode, "nextIncreasing" | "nextDecreasing")
                        {
                            return Err("ArgumentError: Invalid NetGroupSendMode.".into());
                        }
                        let (base, closed, has_neighbors) = {
                            let m = self
                                .net_groups
                                .get(&id)
                                .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                            (m.groupspec_base.clone(), m.closed, !m.neighbors.is_empty())
                        };
                        if closed {
                            return Ok(json!({ "result": "error", "recipients": [] }));
                        }
                        let mut peers: Vec<(u32, String)> = self
                            .net_groups
                            .values()
                            .filter(|x| !x.closed && x.id != id && x.groupspec_base == base)
                            .map(|x| (x.id, x.address.clone()))
                            .collect();
                        if peers.is_empty() {
                            return Ok(json!({
                                "result": if has_neighbors { "sent" } else { "noRoute" },
                                "recipients": []
                            }));
                        }
                        peers.sort_by(|a, b| a.1.cmp(&b.1));
                        let recipients: Vec<u32> = match mode {
                            "all" => peers.iter().map(|(pid, _)| *pid).collect(),
                            "nearest" => {
                                let chosen = peers
                                    .iter()
                                    .find(|(_, addr)| addr.as_str() >= target_addr)
                                    .unwrap_or(&peers[0]);
                                vec![chosen.0]
                            }
                            "nextDecreasing" => vec![peers.last().unwrap().0],
                            _ => vec![peers[0].0],
                        };
                        if let Some(sender) = self.net_groups.get_mut(&id) {
                            sender.info.routing_send_bytes_per_second += bytes;
                        }
                        for rid in &recipients {
                            if let Some(r) = self.net_groups.get_mut(rid) {
                                r.info.routing_receive_bytes_per_second += bytes;
                            }
                        }
                        Ok(json!({ "result": "sent", "recipients": recipients }))
                    }
                    "close" => {
                        let base = if let Some(m) = self.net_groups.get_mut(&id) {
                            m.closed = true;
                            m.neighbors.clear();
                            Some(m.groupspec_base.clone())
                        } else {
                            None
                        };
                        if let Some(b) = base {
                            self.recompute_net_group_ring(&b);
                        }
                        Ok(json!({ "closed": true }))
                    }
                    _ => Err(format!("Unknown netgroup_op {action}")),
                }
            }
            // FLV stream actions & info
            "flv_action" => {
                let id = req.get("streamId").and_then(Value::as_u64).unwrap_or(0) as u32;
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                let st = self.flv_streams.entry(id).or_default();
                st.apply_action(action)
            }
            "flv_info" => {
                let id = req.get("streamId").and_then(Value::as_u64).unwrap_or(0) as u32;
                let st = self.flv_streams.entry(id).or_default();
                Ok(json!({
                    "info": st.info,
                    "multicastInfo": st.multicast,
                    "backBufferLength": st.back_buffer_length,
                    "backBufferByteLength": st.back_buffer_byte_length,
                }))
            }
            // LocalConnection
            "lc_new" => {
                let domain = req
                    .get("domain")
                    .and_then(Value::as_str)
                    .filter(|d| !d.is_empty())
                    .unwrap_or("localhost")
                    .to_owned();
                let id = self.alloc_id();
                self.local_connections.insert(
                    id,
                    LocalConnectionEndpoint {
                        raw_name: String::new(),
                        qualified_name: String::new(),
                        domain: domain.clone(),
                        is_per_user: true,
                        allow_all: false,
                        allowed_domains: HashSet::from([domain.clone()]),
                    },
                );
                Ok(json!({ "id": id, "domain": domain }))
            }
            "lc_op" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "set_per_user" => {
                        let v = req.get("value").and_then(Value::as_bool).unwrap_or(true);
                        let ep = self
                            .local_connections
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown LocalConnection".to_owned())?;
                        ep.is_per_user = v;
                        Ok(json!({ "isPerUser": ep.is_per_user }))
                    }
                    "allow_domain" => {
                        let domains: Vec<String> = req
                            .get("domains")
                            .and_then(Value::as_array)
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(Value::as_str)
                                    .map(ToOwned::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default();
                        let ep = self
                            .local_connections
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown LocalConnection".to_owned())?;
                        for d in domains {
                            if d == "*" {
                                ep.allow_all = true;
                            } else if !d.is_empty() {
                                ep.allowed_domains.insert(d);
                            }
                        }
                        Ok(json!({ "ok": true }))
                    }
                    "connect" => {
                        let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                        if name.contains(':') {
                            return Err("ArgumentError: Error #2004: LocalConnection.connect name cannot contain a colon.".into());
                        }
                        let (domain, already) = {
                            let ep = self
                                .local_connections
                                .get(&id)
                                .ok_or_else(|| "Unknown LocalConnection".to_owned())?;
                            (ep.domain.clone(), !ep.qualified_name.is_empty())
                        };
                        if already {
                            return Err("ArgumentError: Error #2082: Connect failed because the object is already connected.".into());
                        }
                        let qname = normalize_lc_name(name, &domain)?;
                        if self.lc_by_name.contains_key(&qname) {
                            return Err("ArgumentError: Error #2082: Connect failed because the connection name is already in use.".into());
                        }
                        self.lc_by_name.insert(qname.clone(), id);
                        if let Some(ep) = self.local_connections.get_mut(&id) {
                            ep.raw_name = name.to_owned();
                            ep.qualified_name = qname.clone();
                        }
                        Ok(json!({ "qualifiedName": qname }))
                    }
                    "close" => {
                        let ep = self
                            .local_connections
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown LocalConnection".to_owned())?;
                        if ep.qualified_name.is_empty() {
                            return Err("ArgumentError: Error #2083: Close failed because the object is not connected.".into());
                        }
                        let old = std::mem::take(&mut ep.qualified_name);
                        ep.raw_name.clear();
                        self.lc_by_name.remove(&old);
                        Ok(json!({ "closed": true }))
                    }
                    "resolve_send" => {
                        let target_name = req.get("name").and_then(Value::as_str).unwrap_or("");
                        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
                        if target_name.is_empty()
                            || method.is_empty()
                            || FORBIDDEN_LC_METHODS.contains(&method)
                        {
                            return Err("ArgumentError: Error #2004: Invalid LocalConnection target or method name.".into());
                        }
                        let sender_domain = self
                            .local_connections
                            .get(&id)
                            .map(|ep| ep.domain.clone())
                            .unwrap_or_else(|| "localhost".into());
                        let qname = normalize_lc_name(target_name, &sender_domain)?;
                        if let Some(&target_id) = self.lc_by_name.get(&qname)
                            && let Some(target_ep) = self.local_connections.get(&target_id)
                        {
                            let allowed = target_ep.allow_all
                                || target_ep.domain == sender_domain
                                || target_ep.allowed_domains.contains(&sender_domain);
                            if !allowed {
                                return Ok(json!({
                                    "found": false,
                                    "securityDenied": true,
                                }));
                            }
                            return Ok(json!({
                                "found": true,
                                "targetId": target_id,
                                "qualifiedName": qname,
                            }));
                        }
                        Ok(json!({
                            "found": false,
                            "securityDenied": false,
                            "qualifiedName": qname,
                        }))
                    }
                    _ => Err(format!("Unknown lc_op {action}")),
                }
            }
            // DRM
            "drm_parse_content" => {
                let raw_b64 = req.get("base64").and_then(Value::as_str).unwrap_or("");
                let raw = if raw_b64.is_empty() {
                    Vec::new()
                } else {
                    STANDARD.decode(raw_b64.as_bytes()).unwrap_or_default()
                };
                if !raw.is_empty() {
                    if let Ok(val) = serde_json::from_slice::<Value>(&raw)
                        && let Some(obj) = val.as_object()
                    {
                        let server = obj
                            .get("serverURL")
                            .and_then(Value::as_str)
                            .unwrap_or("https://drm.neoflash.local/license");
                        let license = obj
                            .get("licenseID")
                            .and_then(Value::as_str)
                            .unwrap_or("nf-license-default");
                        let domain = obj
                            .get("domain")
                            .and_then(Value::as_str)
                            .unwrap_or("default");
                        let auth = obj
                            .get("authenticationMethod")
                            .and_then(Value::as_str)
                            .unwrap_or("anonymous");
                        let policy = obj
                            .get("policyID")
                            .and_then(Value::as_str)
                            .unwrap_or("policy-standard");
                        let display_name = obj
                            .get("displayName")
                            .and_then(Value::as_str)
                            .unwrap_or("Standard Access");
                        return Ok(json!({
                            "serverURL": server,
                            "licenseID": license,
                            "domain": domain,
                            "authenticationMethod": auth,
                            "accessInfo": [{
                                "authenticationMethod": auth,
                                "displayName": display_name,
                                "domain": domain,
                                "policyID": policy,
                            }]
                        }));
                    }
                    // Arbitrary non-JSON raw header bytes: derive deterministic licenseID.
                    if raw.len() < 4 {
                        return Err(
                            "ArgumentError: Error #3300: Invalid DRMContentData raw metadata."
                                .into(),
                        );
                    }
                    let lic = format!("lic-{}", hex_encode(&sha256(&raw)[..8]));
                    return Ok(json!({
                        "serverURL": "https://drm.neoflash.local/license",
                        "licenseID": lic,
                        "domain": "default",
                        "authenticationMethod": "anonymous",
                        "accessInfo": [{
                            "authenticationMethod": "anonymous",
                            "displayName": "Default Access",
                            "domain": "default",
                            "policyID": "policy-default",
                        }]
                    }));
                }
                Ok(json!({
                    "serverURL": "https://drm.neoflash.local/license",
                    "licenseID": "lic-default",
                    "domain": "default",
                    "authenticationMethod": "anonymous",
                    "accessInfo": [{
                        "authenticationMethod": "anonymous",
                        "displayName": "Default Access",
                        "domain": "default",
                        "policyID": "policy-default",
                    }]
                }))
            }
            "drm_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "authenticate" => {
                        let server = req.get("serverURL").and_then(Value::as_str).unwrap_or("");
                        let domain = req.get("domain").and_then(Value::as_str).unwrap_or("");
                        let user = req.get("username").and_then(Value::as_str).unwrap_or("");
                        let pass = req.get("password").and_then(Value::as_str).unwrap_or("");
                        if server.is_empty() || user.is_empty() {
                            return Ok(json!({ "ok": false, "errorID": 3315, "subErrorID": 401 }));
                        }
                        let token =
                            sha256(format!("drm-tok:{server}:{domain}:{user}:{pass}").as_bytes())
                                .to_vec();
                        self.drm_tokens
                            .insert((server.to_owned(), domain.to_owned()), token.clone());
                        Ok(json!({
                            "ok": true,
                            "tokenBase64": STANDARD.encode(&token),
                        }))
                    }
                    "set_token" => {
                        let server = req.get("serverURL").and_then(Value::as_str).unwrap_or("");
                        let domain = req.get("domain").and_then(Value::as_str).unwrap_or("");
                        let b64 = req.get("tokenBase64").and_then(Value::as_str).unwrap_or("");
                        let bytes = STANDARD.decode(b64.as_bytes()).unwrap_or_default();
                        self.drm_tokens
                            .insert((server.to_owned(), domain.to_owned()), bytes);
                        Ok(json!({ "ok": true }))
                    }
                    "load_voucher" => {
                        let server = req.get("serverURL").and_then(Value::as_str).unwrap_or("");
                        let license = req.get("licenseID").and_then(Value::as_str).unwrap_or("");
                        let policy = req
                            .get("policyID")
                            .and_then(Value::as_str)
                            .unwrap_or("policy-default");
                        let domain = req
                            .get("domain")
                            .and_then(Value::as_str)
                            .unwrap_or("default");
                        let auth_method = req
                            .get("authenticationMethod")
                            .and_then(Value::as_str)
                            .unwrap_or("anonymous");
                        let setting = req
                            .get("setting")
                            .and_then(Value::as_str)
                            .unwrap_or("allowServer");
                        let preview = req.get("preview").and_then(Value::as_bool).unwrap_or(false);
                        let now_ms = req
                            .get("nowMs")
                            .and_then(Value::as_f64)
                            .unwrap_or(1_762_000_000_000.0);
                        if !preview
                            && !matches!(setting, "allowServer" | "forceRefresh" | "localOnly")
                        {
                            return Err(
                                "ArgumentError: Error #2004: Invalid LoadVoucherSetting.".into()
                            );
                        }
                        let key = format!("{server}|{license}");
                        if !preview
                            && setting != "forceRefresh"
                            && let Some(existing) = self.drm_vouchers.get(&key)
                        {
                            return Ok(json!({ "ok": true, "isLocal": true, "voucher": existing }));
                        }
                        if !preview && setting == "localOnly" {
                            return Ok(json!({ "ok": false, "voucher": Value::Null }));
                        }
                        if auth_method == "usernameAndPassword"
                            && !self
                                .drm_tokens
                                .contains_key(&(server.to_owned(), domain.to_owned()))
                        {
                            return Ok(json!({
                                "ok": false,
                                "error": true,
                                "errorID": 3305,
                                "subErrorID": 401,
                                "text": "Authentication required before loading voucher."
                            }));
                        }
                        let mut policies = BTreeMap::new();
                        policies.insert("preview".into(), preview.to_string());
                        policies.insert("domain".into(), domain.to_owned());
                        let voucher = DrmVoucherRecord {
                            server_url: server.to_owned(),
                            license_id: license.to_owned(),
                            policy_id: policy.to_owned(),
                            domain: domain.to_owned(),
                            voucher_start_ms: now_ms,
                            voucher_end_ms: now_ms + 86_400_000.0 * 30.0,
                            offline_start_ms: now_ms,
                            offline_end_ms: now_ms + 86_400_000.0 * 7.0,
                            window_period: 86_400,
                            window_start_ms: now_ms,
                            window_end_ms: now_ms + 86_400_000.0,
                            policies,
                        };
                        if !preview {
                            self.drm_vouchers.insert(key, voucher.clone());
                        }
                        Ok(json!({ "ok": true, "isLocal": false, "voucher": voucher }))
                    }
                    "store_voucher" => {
                        let b64 = req.get("base64").and_then(Value::as_str).unwrap_or("");
                        let raw = STANDARD.decode(b64.as_bytes()).unwrap_or_default();
                        let now_ms = req
                            .get("nowMs")
                            .and_then(Value::as_f64)
                            .unwrap_or(1_762_000_000_000.0);
                        let record: DrmVoucherRecord =
                            serde_json::from_slice(&raw).unwrap_or_else(|_| DrmVoucherRecord {
                                server_url: "https://drm.neoflash.local/license".into(),
                                license_id: format!("stored-{}", hex_encode(&sha256(&raw)[..6])),
                                policy_id: "policy-stored".into(),
                                domain: "default".into(),
                                voucher_start_ms: now_ms,
                                voucher_end_ms: now_ms + 86_400_000.0 * 30.0,
                                offline_start_ms: now_ms,
                                offline_end_ms: now_ms + 86_400_000.0 * 7.0,
                                window_period: 86_400,
                                window_start_ms: now_ms,
                                window_end_ms: now_ms + 86_400_000.0,
                                policies: BTreeMap::new(),
                            });
                        let key = format!("{}|{}", record.server_url, record.license_id);
                        self.drm_vouchers.insert(key, record.clone());
                        Ok(json!({ "ok": true, "voucher": record }))
                    }
                    "return_voucher" => {
                        let server = req.get("serverURL").and_then(Value::as_str).unwrap_or("");
                        let license = req.get("licenseID").and_then(Value::as_str).unwrap_or("");
                        let mut removed = 0usize;
                        if !license.is_empty() {
                            if self
                                .drm_vouchers
                                .remove(&format!("{server}|{license}"))
                                .is_some()
                            {
                                removed += 1;
                            }
                        } else {
                            let before = self.drm_vouchers.len();
                            self.drm_vouchers.retain(|_, v| v.server_url != server);
                            removed = before - self.drm_vouchers.len();
                        }
                        Ok(json!({ "ok": true, "removed": removed }))
                    }
                    "reset_vouchers" => {
                        self.drm_vouchers.clear();
                        self.drm_tokens.clear();
                        Ok(json!({ "ok": true }))
                    }
                    "device_group" => {
                        let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                        let remove = req.get("remove").and_then(Value::as_bool).unwrap_or(false);
                        if remove {
                            self.drm_device_groups.remove(name);
                        } else {
                            self.drm_device_groups.insert(name.to_owned());
                        }
                        Ok(json!({ "ok": true, "joined": !remove }))
                    }
                    _ => Err(format!("Unknown drm_op {action}")),
                }
            }
            // Capabilities
            "capabilities_snapshot" => {
                let width = req.get("width").and_then(Value::as_u64).unwrap_or(1920) as u32;
                let height = req.get("height").and_then(Value::as_u64).unwrap_or(1080) as u32;
                let lang = req.get("language").and_then(Value::as_str).unwrap_or("en");
                let os = req
                    .get("os")
                    .and_then(Value::as_str)
                    .unwrap_or("Mac OS 10.15.7");
                let dpi = req.get("dpi").and_then(Value::as_u64).unwrap_or(72) as u32;
                let debugger = req
                    .get("isDebugger")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Ok(build_capabilities_snapshot(
                    width, height, lang, os, dpi, debugger,
                ))
            }
            // Workers & MessageChannel
            "worker_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "create_worker" => {
                        let id = self.alloc_id();
                        self.workers.insert(
                            id,
                            WorkerRecord {
                                id,
                                is_primordial: false,
                                state: "new".into(),
                            },
                        );
                        Ok(json!({ "id": id, "state": "new", "isPrimordial": false }))
                    }
                    "start" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let w = self
                            .workers
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown Worker".to_owned())?;
                        if w.is_primordial || w.state != "new" {
                            return Err("IllegalOperationError: Error #3716: Worker.start requires a Worker in WorkerState.NEW.".into());
                        }
                        w.state = "running".into();
                        Ok(json!({ "state": w.state }))
                    }
                    "terminate" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let w = self
                            .workers
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown Worker".to_owned())?;
                        if w.is_primordial || w.state == "terminated" {
                            return Ok(json!({ "terminated": false, "state": w.state }));
                        }
                        w.state = "terminated".into();
                        Ok(json!({ "terminated": true, "state": w.state }))
                    }
                    "list_running" => {
                        let mut ids: Vec<u32> = self
                            .workers
                            .values()
                            .filter(|w| w.state == "running")
                            .map(|w| w.id)
                            .collect();
                        ids.sort_unstable();
                        Ok(json!({ "workerIds": ids }))
                    }
                    "create_channel" => {
                        let sender =
                            req.get("senderId").and_then(Value::as_u64).unwrap_or(1) as u32;
                        let receiver =
                            req.get("receiverId").and_then(Value::as_u64).unwrap_or(1) as u32;
                        if !self.workers.contains_key(&sender)
                            || !self.workers.contains_key(&receiver)
                        {
                            return Err(
                                "ArgumentError: Invalid Worker for createMessageChannel.".into()
                            );
                        }
                        let id = self.alloc_id();
                        self.channels.insert(
                            id,
                            ChannelRecord {
                                id,
                                sender_id: sender,
                                receiver_id: receiver,
                                state: "open".into(),
                                queued_count: 0,
                            },
                        );
                        Ok(json!({ "id": id, "state": "open" }))
                    }
                    "channel_send" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let limit = req.get("queueLimit").and_then(Value::as_i64).unwrap_or(-1);
                        let ch = self
                            .channels
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown MessageChannel".to_owned())?;
                        let _ = (ch.id, ch.sender_id, ch.receiver_id);
                        if ch.state != "open" {
                            return Err("IOError: MessageChannel is closed.".into());
                        }
                        if limit >= 0 && (ch.queued_count as i64) >= limit {
                            return Err("MemoryError: MessageChannel queueLimit exceeded.".into());
                        }
                        ch.queued_count += 1;
                        Ok(json!({ "queued": ch.queued_count, "state": ch.state }))
                    }
                    "channel_receive" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let ch = self
                            .channels
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown MessageChannel".to_owned())?;
                        if ch.queued_count > 0 {
                            ch.queued_count -= 1;
                        }
                        if ch.state == "closing" && ch.queued_count == 0 {
                            ch.state = "closed".into();
                        }
                        Ok(json!({ "queued": ch.queued_count, "state": ch.state }))
                    }
                    "channel_close" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let ch = self
                            .channels
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown MessageChannel".to_owned())?;
                        if ch.state == "open" {
                            ch.state = if ch.queued_count == 0 {
                                "closed".into()
                            } else {
                                "closing".into()
                            };
                        }
                        Ok(json!({ "state": ch.state }))
                    }
                    _ => Err(format!("Unknown worker_op {action}")),
                }
            }
            // Mutex & Condition
            "mutex_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                let worker_id = req.get("workerId").and_then(Value::as_u64).unwrap_or(1) as u32;
                match action {
                    "new_mutex" => {
                        let id = self.alloc_id();
                        self.mutexes.insert(id, MutexRecord::default());
                        Ok(json!({ "id": id }))
                    }
                    "lock" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let m = self
                            .mutexes
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown Mutex".to_owned())?;
                        match m.owner_worker {
                            None => {
                                m.owner_worker = Some(worker_id);
                                m.depth = 1;
                            }
                            Some(owner) if owner == worker_id => {
                                m.depth = m.depth.saturating_add(1);
                            }
                            Some(_) => {
                                return Err("IllegalOperationError: Mutex is currently locked by another worker.".into());
                            }
                        }
                        Ok(json!({ "locked": true, "depth": m.depth }))
                    }
                    "try_lock" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let m = self
                            .mutexes
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown Mutex".to_owned())?;
                        let acquired = match m.owner_worker {
                            None => {
                                m.owner_worker = Some(worker_id);
                                m.depth = 1;
                                true
                            }
                            Some(owner) if owner == worker_id => {
                                m.depth = m.depth.saturating_add(1);
                                true
                            }
                            Some(_) => false,
                        };
                        Ok(json!({ "acquired": acquired, "depth": m.depth }))
                    }
                    "unlock" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let m = self
                            .mutexes
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown Mutex".to_owned())?;
                        if m.owner_worker != Some(worker_id) || m.depth == 0 {
                            return Err("IllegalOperationError: Error #1520: Mutex cannot be unlocked because it is not owned by the current worker.".into());
                        }
                        m.depth -= 1;
                        if m.depth == 0 {
                            m.owner_worker = None;
                        }
                        Ok(json!({ "depth": m.depth }))
                    }
                    "new_condition" => {
                        let mutex_id =
                            req.get("mutexId").and_then(Value::as_u64).unwrap_or(0) as u32;
                        if !self.mutexes.contains_key(&mutex_id) {
                            return Err("ArgumentError: Condition requires a valid Mutex.".into());
                        }
                        let id = self.alloc_id();
                        self.conditions.insert(id, mutex_id);
                        Ok(json!({ "id": id }))
                    }
                    "condition_wait" | "condition_notify" | "condition_notify_all" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let timeout = req.get("timeout").and_then(Value::as_f64).unwrap_or(-1.0);
                        if action == "condition_wait" && timeout < 0.0 && timeout != -1.0 {
                            return Err(
                                "ArgumentError: Condition.wait timeout must be -1 or non-negative."
                                    .into(),
                            );
                        }
                        let mutex_id = *self
                            .conditions
                            .get(&id)
                            .ok_or_else(|| "Unknown Condition".to_owned())?;
                        let m = self
                            .mutexes
                            .get(&mutex_id)
                            .ok_or_else(|| "Unknown Mutex".to_owned())?;
                        if m.owner_worker != Some(worker_id) || m.depth == 0 {
                            return Err("IllegalOperationError: Error #1521: Condition requires its Mutex to be locked by the calling worker.".into());
                        }
                        // Single-threaded JS execution cannot block synchronously;
                        // finite timeout reports timedOut=true, default wait reports false.
                        let timed_out = action == "condition_wait" && timeout >= 0.0;
                        Ok(json!({ "timedOut": timed_out }))
                    }
                    _ => Err(format!("Unknown mutex_op {action}")),
                }
            }
            // Sampler & Telemetry
            "sampler_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "start" => {
                        self.sampling_active = true;
                        self.sampling_paused = false;
                        let now_us = req.get("timeUs").and_then(Value::as_f64).unwrap_or(1000.0);
                        if self.samples.is_empty() {
                            self.samples.push_back(SamplerEntry {
                                kind: "sample".into(),
                                time_us: now_us,
                                id: 0,
                                size: 0,
                                class_name: None,
                                stack: vec![("startSampling".into(), "flash.sampler".into(), 1, 1)],
                            });
                        }
                        Ok(json!({ "active": true }))
                    }
                    "pause" => {
                        if self.sampling_active {
                            self.sampling_paused = true;
                        }
                        Ok(json!({ "paused": self.sampling_paused }))
                    }
                    "stop" => {
                        self.sampling_active = false;
                        self.sampling_paused = false;
                        Ok(json!({ "active": false }))
                    }
                    "clear" => {
                        self.samples.clear();
                        Ok(json!({ "count": 0 }))
                    }
                    "record_alloc" => {
                        if self.sampling_active && !self.sampling_paused {
                            let id = req.get("allocId").and_then(Value::as_u64).unwrap_or(1);
                            let size = req.get("size").and_then(Value::as_u64).unwrap_or(32);
                            let cname = req
                                .get("className")
                                .and_then(Value::as_str)
                                .unwrap_or("Object")
                                .to_owned();
                            let time_us =
                                req.get("timeUs").and_then(Value::as_f64).unwrap_or(1000.0);
                            if self.samples.len() >= 2048 {
                                self.samples.pop_front();
                            }
                            self.samples.push_back(SamplerEntry {
                                kind: "new".into(),
                                time_us,
                                id,
                                size,
                                class_name: Some(cname),
                                stack: vec![("alloc".into(), "runtime".into(), 1, 1)],
                            });
                        }
                        Ok(json!({ "count": self.samples.len() }))
                    }
                    "record_delete" => {
                        if self.sampling_active && !self.sampling_paused {
                            let id = req.get("allocId").and_then(Value::as_u64).unwrap_or(1);
                            let size = req.get("size").and_then(Value::as_u64).unwrap_or(32);
                            let time_us =
                                req.get("timeUs").and_then(Value::as_f64).unwrap_or(1000.0);
                            if self.samples.len() >= 2048 {
                                self.samples.pop_front();
                            }
                            self.samples.push_back(SamplerEntry {
                                kind: "delete".into(),
                                time_us,
                                id,
                                size,
                                class_name: None,
                                stack: Vec::new(),
                            });
                        }
                        Ok(json!({ "count": self.samples.len() }))
                    }
                    "count" => Ok(json!({ "count": self.samples.len() as f64 })),
                    "get_samples" => {
                        let list: Vec<Value> = self.samples.iter().map(|s| json!(s)).collect();
                        Ok(json!({ "samples": list }))
                    }
                    "record_invocation" => {
                        let key = req.get("key").and_then(Value::as_str).unwrap_or("");
                        let entry = self.invocation_counts.entry(key.to_owned()).or_insert(0);
                        *entry += 1;
                        Ok(json!({ "count": *entry as f64 }))
                    }
                    "get_invocation" => {
                        let key = req.get("key").and_then(Value::as_str).unwrap_or("");
                        let count = self.invocation_counts.get(key).copied().unwrap_or(0);
                        Ok(json!({ "count": count as f64 }))
                    }
                    _ => Err(format!("Unknown sampler_op {action}")),
                }
            }
            "telemetry_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "register" => {
                        let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                        if name.is_empty() || name.starts_with('.') {
                            return Ok(json!({ "registered": false }));
                        }
                        Ok(json!({ "registered": self.telemetry_commands.insert(name.to_owned()) }))
                    }
                    "unregister" => {
                        let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                        Ok(json!({ "removed": self.telemetry_commands.remove(name) }))
                    }
                    "metric" => {
                        let metric = req.get("metric").and_then(Value::as_str).unwrap_or("");
                        let val = req.get("value").cloned().unwrap_or(Value::Null);
                        if self.telemetry_metrics.len() >= 1024 {
                            self.telemetry_metrics.remove(0);
                        }
                        self.telemetry_metrics.push((metric.to_owned(), val));
                        Ok(json!({ "recorded": self.telemetry_metrics.len() }))
                    }
                    _ => Err(format!("Unknown telemetry_op {action}")),
                }
            }
            "fast_mem_op" => {
                let kind = req.get("kind").and_then(Value::as_str).unwrap_or("");
                match kind {
                    "sxi1" => {
                        let v = req.get("value").and_then(Value::as_i64).unwrap_or(0) as i32;
                        Ok(json!({ "result": fast_mem_sxi1(v) }))
                    }
                    "sxi8" => {
                        let v = req.get("value").and_then(Value::as_i64).unwrap_or(0) as i32;
                        Ok(json!({ "result": fast_mem_sxi8(v) }))
                    }
                    "sxi16" => {
                        let v = req.get("value").and_then(Value::as_i64).unwrap_or(0) as i32;
                        Ok(json!({ "result": fast_mem_sxi16(v) }))
                    }
                    _ => Err(format!("Unknown fast_mem_op '{kind}'")),
                }
            }
            _ => Err(format!("Unsupported As3NetSysKernel op '{op}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_memory_opcodes_and_sign_extension() {
        assert_eq!(fast_mem_sxi1(0), 0);
        assert_eq!(fast_mem_sxi1(1), -1);
        assert_eq!(fast_mem_sxi8(0x80), -128);
        assert_eq!(fast_mem_sxi8(0x7f), 127);
        assert_eq!(fast_mem_sxi16(0x8000), -32768);
        assert_eq!(fast_mem_sxi16(0x7fff), 32767);

        let mut mem = vec![0u8; MIN_DOMAIN_MEMORY_LENGTH];
        fast_mem_write(&mut mem, "si8", 0, 250.0).unwrap();
        assert_eq!(fast_mem_read(&mem, "li8", 0).unwrap(), 250.0);
        fast_mem_write(&mut mem, "si16", 4, 0xCAFE as f64).unwrap();
        assert_eq!(fast_mem_read(&mem, "li16", 4).unwrap(), 0xCAFE as f64);
        fast_mem_write(&mut mem, "si32", 8, -123456789.0).unwrap();
        assert_eq!(fast_mem_read(&mem, "li32", 8).unwrap(), -123456789.0);
        fast_mem_write(&mut mem, "sf32", 16, 3.5).unwrap();
        assert!((fast_mem_read(&mem, "lf32", 16).unwrap() - 3.5).abs() < 1e-6);
        fast_mem_write(&mut mem, "sf64", 24, std::f64::consts::PI).unwrap();
        assert!((fast_mem_read(&mem, "lf64", 24).unwrap() - std::f64::consts::PI).abs() < 1e-12);
        assert!(fast_mem_read(&mem, "li32", MIN_DOMAIN_MEMORY_LENGTH - 2).is_err());
    }

    #[test]
    fn sha256_and_csprng_bounds() {
        // NIST test vector for "abc"
        assert_eq!(
            hex_encode(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut counter = 1u64;
        assert!(generate_random_bytes(0, &[], &mut counter).is_err());
        assert!(generate_random_bytes(1025, &[], &mut counter).is_err());
        let b1 = generate_random_bytes(32, &[7u8; 32], &mut counter).unwrap();
        let b2 = generate_random_bytes(32, &[7u8; 32], &mut counter).unwrap();
        assert_eq!(b1.len(), 32);
        assert_ne!(b1, b2);
        let b1024 = generate_random_bytes(1024, &[0x5au8; 64], &mut counter).unwrap();
        assert_eq!(b1024.len(), 1024);
    }

    #[test]
    fn locale_id_currency_collator_and_string_tools() {
        let loc = parse_locale_id("zh_Hans_CN@collation=pinyin;currency=CNY", "en-US");
        assert_eq!(loc.language, "zh");
        assert_eq!(loc.script, "Hans");
        assert_eq!(loc.region, "CN");
        assert_eq!(
            loc.keys.get("collation").map(String::as_str),
            Some("pinyin")
        );
        assert!(!loc.rtl);

        let ar = parse_locale_id("ar-EG", "en-US");
        assert!(ar.rtl);
        assert_eq!(ar.script, "Arab");

        let pref = determine_preferred_locales(
            &["fr-CA".into(), "de-AT".into()],
            &["en-US".into(), "de-DE".into(), "fr-FR".into()],
            "userinterface",
        );
        assert_eq!(pref, vec!["fr-FR".to_owned(), "de-DE".to_owned()]);

        // Osmanya U+104A0 national digits in CurrencyFormatter
        let mut cf = CurrencyFormatterState::new("en-US", "en-US");
        assert!(cf.is_symbol_safe("USD").unwrap());
        assert!(!cf.is_symbol_safe("EUR").unwrap());
        assert_eq!(cf.format(1234.5, true).unwrap(), "$1,234.50");
        assert_eq!(cf.format(-1234.5, false).unwrap(), "(USD1,234.50)");
        let parsed = cf.parse("($1,234.50)").unwrap();
        assert_eq!(parsed["value"], json!(-1234.5));
        assert_eq!(parsed["currencyString"], json!("$"));
        cf.set_property("digitsType", json!(0x104A0)).unwrap();
        let osm = cf.format(42.0, true).unwrap();
        assert!(osm.contains('\u{104A4}'));
        assert_eq!(cf.parse(&osm).unwrap()["value"], json!(42.0));

        // Collator matching vs sorting + numeric + kana + width
        let mut match_col = CollatorState::new("en-US", "matching", "en-US").unwrap();
        assert!(match_col.equals("Café", "cafe"));
        assert!(match_col.equals("ｶﾀｶﾅ", "かたかな"));
        assert!(match_col.equals("ＡＢＣ", "abc"));

        let mut sort_col = CollatorState::new("en-US", "sorting", "en-US").unwrap();
        assert_eq!(sort_col.compare("file10", "file2"), -1);
        sort_col.set_bool("numericComparison", true).unwrap();
        assert_eq!(sort_col.compare("file2", "file10"), -1);

        // StringTools Turkish dotted/dotless I & Greek final sigma
        let mut tr = StringToolsState::new("tr-TR", "en-US");
        assert_eq!(tr.to_lower("Iİ"), "ıi");
        assert_eq!(tr.to_upper("ıi"), "Iİ");
        let mut el = StringToolsState::new("el-GR", "en-US");
        assert_eq!(el.to_lower("ΟΔΥΣΣΕΥΣ"), "οδυσσευς");
    }

    #[test]
    fn groupspec_flv_x509_and_kernel_workflows() {
        let mut gs = GroupSpecifierState::new("room-alpha", 1).unwrap();
        gs.posting_enabled = true;
        gs.routing_enabled = true;
        let base1 = gs.groupspec_without_authorizations();
        gs.set_posting_password(Some("secret"), Some("salt1"));
        assert_ne!(
            gs.groupspec_with_authorizations(),
            gs.groupspec_without_authorizations()
        );
        assert!(!gs.authorizations().is_empty());
        assert_ne!(gs.groupspec_without_authorizations(), base1);

        // Build minimal FLV with header + script tag ("onMetaData", {"duration": 2.5})
        let mut flv = Vec::from(&b"FLV\x01\x05\x00\x00\x00\x09\x00\x00\x00\x00"[..]);
        let mut script_payload = Vec::new();
        script_payload.extend_from_slice(&[0x02, 0x00, 0x0a]);
        script_payload.extend_from_slice(b"onMetaData");
        script_payload.push(0x03);
        script_payload.extend_from_slice(&[0x00, 0x08]);
        script_payload.extend_from_slice(b"duration");
        script_payload.push(0x00);
        script_payload.extend_from_slice(&2.5f64.to_be_bytes());
        script_payload.extend_from_slice(&[0x00, 0x00, 0x09]);
        let dsize = script_payload.len();
        flv.push(18);
        flv.extend_from_slice(&[(dsize >> 16) as u8, (dsize >> 8) as u8, dsize as u8]);
        flv.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]);
        flv.extend_from_slice(&script_payload);
        flv.extend_from_slice(&((11 + dsize) as u32).to_be_bytes());

        let mut flv_state = FlvStreamState::default();
        let res = flv_state.append_bytes(&flv);
        assert_eq!(res["callbacks"][0]["handler"], json!("onMetaData"));
        assert_eq!(res["callbacks"][0]["info"]["duration"], json!(2.5));

        // Loopback X.509 DER generation + roundtrip ASN.1 parse
        let der = build_loopback_x509_der("secure.example.test");
        let cert = parse_x509_certificate(&der).unwrap();
        assert_eq!(cert.version, 3);
        assert_eq!(
            cert.subject.common_name.as_deref(),
            Some("secure.example.test")
        );
        assert_eq!(cert.issuer.country_name.as_deref(), Some("US"));
        assert!(cert.valid_not_after_ms > cert.valid_not_before_ms);
    }
}
