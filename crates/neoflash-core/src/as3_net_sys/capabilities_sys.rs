use super::*;

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
