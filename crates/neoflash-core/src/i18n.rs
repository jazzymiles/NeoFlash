//! Interface localization architecture (`platform-extensions`: `localization`).
//!
//! Provides BCP-47 locale parsing and normalization (`LocaleId`), deterministic fallback chains
//! (`LocaleFallbackChain`), CLDR-compatible cardinal plural category selection (`PluralCategory`,
//! `select_plural_category`), named parameter interpolation, JSON translation catalogs
//! (`TranslationCatalog`, `LocalizedMessage`), completeness verification against required UI keys
//! (`verify_catalog_completeness`), and built-in seed catalogs for `en-US` and `ru-RU`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// Maximum serialized size of a JSON translation catalog (2 MiB).
pub const MAX_CATALOG_JSON_BYTES: usize = 2 * 1024 * 1024;

/// Default canonical fallback locale tag for the NeoFlash interface.
pub const DEFAULT_FALLBACK_LOCALE_TAG: &str = "en-US";

/// Required core UI translation keys across NeoFlash's primary interface domains (`menu.*`,
/// `panel.*`, `tool.*`, `timeline.*`, `publish.*`).
pub const CORE_UI_TRANSLATION_KEYS: &[&str] = &[
    "menu.file.new",
    "menu.file.open",
    "menu.file.save",
    "menu.file.publish",
    "menu.edit.undo",
    "menu.edit.redo",
    "panel.timeline.title",
    "panel.library.title",
    "panel.library.items_count",
    "panel.properties.title",
    "panel.actions.title",
    "tool.selection.name",
    "tool.subselection.name",
    "tool.pen.name",
    "tool.brush.name",
    "tool.text.name",
    "timeline.frames.count",
    "timeline.fps.label",
    "publish.html5.status",
    "publish.warnings.count",
];

/// Errors produced by locale parsing, catalog serialization, or message formatting.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum LocalizationError {
    #[error("Invalid BCP-47 locale tag '{0}'.")]
    InvalidLocaleTag(String),
    #[error(
        "Invalid translation key '{0}': expected dot-separated ASCII identifier (e.g. 'menu.file.open')."
    )]
    InvalidTranslationKey(String),
    #[error("Invalid localized message for key '{key}': {reason}")]
    InvalidMessage { key: String, reason: String },
    #[error("Catalog JSON error: {0}")]
    CatalogJson(String),
}

/// Normalized BCP-47 locale identifier (`language[-Script][-REGION]`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocaleId {
    /// ISO 639 lowercase 2- or 3-letter language subtag (e.g. `"en"`, `"ru"`, `"ja"`, `"zh"`).
    pub language: String,
    /// Optional ISO 15924 4-letter script subtag in TitleCase (e.g. `"Latn"`, `"Cyrl"`, `"Hans"`).
    pub script: Option<String>,
    /// Optional ISO 3166-1 2-letter uppercase or UN M.49 3-digit region subtag (e.g. `"US"`, `"RU"`, `"419"`).
    pub region: Option<String>,
}

impl LocaleId {
    pub fn new(
        language: impl Into<String>,
        script: Option<String>,
        region: Option<String>,
    ) -> Result<Self, LocalizationError> {
        let mut raw = language.into();
        if let Some(sc) = script {
            raw.push('-');
            raw.push_str(&sc);
        }
        if let Some(reg) = region {
            raw.push('-');
            raw.push_str(&reg);
        }
        Self::parse(&raw)
    }

    /// Default fallback locale (`en-US`).
    pub fn default_en_us() -> Self {
        Self {
            language: "en".into(),
            script: None,
            region: Some("US".into()),
        }
    }

    /// Parse and normalize a BCP-47 locale tag (also accepting `'_'` separators such as `"ru_RU"`).
    pub fn parse(raw: &str) -> Result<Self, LocalizationError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > 35 {
            return Err(LocalizationError::InvalidLocaleTag(raw.to_string()));
        }
        let normalized_sep = trimmed.replace('_', "-");
        let parts: Vec<&str> = normalized_sep.split('-').collect();
        if parts.is_empty() || parts.len() > 3 {
            return Err(LocalizationError::InvalidLocaleTag(raw.to_string()));
        }

        let lang_part = parts[0];
        if !(2..=3).contains(&lang_part.len())
            || !lang_part.bytes().all(|b| b.is_ascii_alphabetic())
        {
            return Err(LocalizationError::InvalidLocaleTag(raw.to_string()));
        }
        let language = lang_part.to_ascii_lowercase();

        let mut script = None;
        let mut region = None;

        if parts.len() == 2 {
            let sub = parts[1];
            if is_valid_script_subtag(sub) {
                script = Some(normalize_script_subtag(sub));
            } else if is_valid_region_subtag(sub) {
                region = Some(sub.to_ascii_uppercase());
            } else {
                return Err(LocalizationError::InvalidLocaleTag(raw.to_string()));
            }
        } else if parts.len() == 3 {
            if !is_valid_script_subtag(parts[1]) || !is_valid_region_subtag(parts[2]) {
                return Err(LocalizationError::InvalidLocaleTag(raw.to_string()));
            }
            script = Some(normalize_script_subtag(parts[1]));
            region = Some(parts[2].to_ascii_uppercase());
        }

        Ok(Self {
            language,
            script,
            region,
        })
    }

    /// Canonical BCP-47 string representation (e.g. `"en-US"`, `"zh-Hans-CN"`, `"ru"`).
    pub fn canonical_tag(&self) -> String {
        let mut out = self.language.clone();
        if let Some(script) = &self.script {
            out.push('-');
            out.push_str(script);
        }
        if let Some(region) = &self.region {
            out.push('-');
            out.push_str(region);
        }
        out
    }

    /// Language-only base locale (e.g. `ru-RU` -> `ru`).
    pub fn language_only(&self) -> Self {
        Self {
            language: self.language.clone(),
            script: None,
            region: None,
        }
    }

    /// Build a deterministic fallback chain from `self` down to `default_locale` and its base
    /// language (e.g. `ru-RU` -> `ru` -> `en-US` -> `en`).
    pub fn fallback_chain(&self, default_locale: &LocaleId) -> LocaleFallbackChain {
        let mut candidates = Vec::new();
        let mut push_unique = |loc: LocaleId| {
            if !candidates.contains(&loc) {
                candidates.push(loc);
            }
        };

        push_unique(self.clone());
        if let Some(sc) = &self.script
            && self.region.is_some()
        {
            push_unique(Self {
                language: self.language.clone(),
                script: Some(sc.clone()),
                region: None,
            });
        }
        if let Some(reg) = &self.region
            && self.script.is_some()
        {
            push_unique(Self {
                language: self.language.clone(),
                script: None,
                region: Some(reg.clone()),
            });
        }
        push_unique(self.language_only());

        push_unique(default_locale.clone());
        push_unique(default_locale.language_only());

        LocaleFallbackChain {
            locales: candidates,
        }
    }
}

fn is_valid_script_subtag(s: &str) -> bool {
    s.len() == 4 && s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn normalize_script_subtag(s: &str) -> String {
    let mut chars = s.chars();
    let mut out = String::with_capacity(4);
    if let Some(first) = chars.next() {
        out.push(first.to_ascii_uppercase());
    }
    for c in chars {
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn is_valid_region_subtag(s: &str) -> bool {
    (s.len() == 2 && s.bytes().all(|b| b.is_ascii_alphabetic()))
        || (s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()))
}

impl fmt::Display for LocaleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical_tag())
    }
}

impl FromStr for LocaleId {
    type Err = LocalizationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for LocaleId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.canonical_tag())
    }
}

impl<'de> Deserialize<'de> for LocaleId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Ordered sequence of `LocaleId`s consulted when resolving a translation key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocaleFallbackChain {
    pub locales: Vec<LocaleId>,
}

impl LocaleFallbackChain {
    pub fn as_tags(&self) -> Vec<String> {
        self.locales.iter().map(LocaleId::canonical_tag).collect()
    }
}

/// CLDR cardinal plural categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluralCategory {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

/// Select the CLDR cardinal `PluralCategory` for a given ISO 639 language subtag and integer count.
///
/// Covered rule families:
/// - East Asian (`ja`, `zh`, `ko`, `vi`, `th`, `id`, `ms`): `Other`.
/// - East/South Slavic (`ru`, `uk`, `be`, `sr`, `hr`, `bs`): `One` (1, 21, 31...), `Few` (2..=4,
///   22..=24...), `Many` (0, 5..=20, 25..=30...).
/// - West Slavic Polish (`pl`): `One` (1), `Few` (2..=4 except 12..=14), `Many` (all other integers).
/// - Arabic (`ar`): `Zero` (0), `One` (1), `Two` (2), `Few` (n % 100 in 3..=10),
///   `Many` (n % 100 in 11..=99), `Other` (100, 101, 102...).
/// - French / Brazilian Portuguese (`fr`, `pt`): `One` (0, 1), `Other` (2+).
/// - English / Germanic / Romance (`en`, `de`, `es`, `it`, `nl`, `sv`, `da`, `no`, default):
///   `One` (1), `Other` (all other counts).
pub fn select_plural_category(language: &str, count: u64) -> PluralCategory {
    let lang = language
        .split(['-', '_'])
        .next()
        .unwrap_or(language)
        .trim()
        .to_ascii_lowercase();
    let mod10 = count % 10;
    let mod100 = count % 100;

    match lang.as_str() {
        "ja" | "zh" | "ko" | "vi" | "th" | "id" | "ms" => PluralCategory::Other,
        "ru" | "uk" | "be" | "sr" | "hr" | "bs" => {
            if mod10 == 1 && mod100 != 11 {
                PluralCategory::One
            } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                PluralCategory::Few
            } else {
                PluralCategory::Many
            }
        }
        "pl" => {
            if count == 1 {
                PluralCategory::One
            } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                PluralCategory::Few
            } else {
                PluralCategory::Many
            }
        }
        "ar" => match count {
            0 => PluralCategory::Zero,
            1 => PluralCategory::One,
            2 => PluralCategory::Two,
            _ if (3..=10).contains(&mod100) => PluralCategory::Few,
            _ if (11..=99).contains(&mod100) => PluralCategory::Many,
            _ => PluralCategory::Other,
        },
        "fr" | "pt" => {
            if count <= 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        _ => {
            if count == 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
    }
}

/// Return the plural categories required for a complete pluralized message in `language`.
pub fn required_plural_categories_for_language(language: &str) -> &'static [PluralCategory] {
    let lang = language
        .split(['-', '_'])
        .next()
        .unwrap_or(language)
        .trim()
        .to_ascii_lowercase();
    match lang.as_str() {
        "ja" | "zh" | "ko" | "vi" | "th" | "id" | "ms" => &[PluralCategory::Other],
        "ru" | "uk" | "be" | "sr" | "hr" | "bs" | "pl" => &[
            PluralCategory::One,
            PluralCategory::Few,
            PluralCategory::Many,
            PluralCategory::Other,
        ],
        "ar" => &[
            PluralCategory::Zero,
            PluralCategory::One,
            PluralCategory::Two,
            PluralCategory::Few,
            PluralCategory::Many,
            PluralCategory::Other,
        ],
        _ => &[PluralCategory::One, PluralCategory::Other],
    }
}

/// Validate a stable dot-separated translation key (e.g. `menu.file.open`, `timeline.frames.count`).
pub fn is_valid_translation_key(key: &str) -> bool {
    if !(3..=128).contains(&key.len()) {
        return false;
    }
    let segments: Vec<&str> = key.split('.').collect();
    if segments.len() < 2 {
        return false;
    }
    for seg in segments {
        if seg.is_empty() || seg.len() > 48 {
            return false;
        }
        let mut chars = seg.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        if !first.is_ascii_lowercase() {
            return false;
        }
        if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-')) {
            return false;
        }
    }
    true
}

/// Extract named `{placeholder}` tokens from a message template string.
pub fn extract_placeholders(template: &str) -> BTreeSet<String> {
    let mut placeholders = BTreeSet::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{'
            && let Some(rel_end) = template[i + 1..].find('}')
        {
            let token = &template[i + 1..i + 1 + rel_end];
            if !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                placeholders.insert(token.to_string());
            }
            i += 1 + rel_end + 1;
            continue;
        }
        i += 1;
    }
    placeholders
}

/// Interpolate named `{param}` placeholders in `template` using `params`.
///
/// Unmatched placeholders are preserved verbatim as `{name}` so missing arguments remain visible
/// in diagnostics rather than silently disappearing.
pub fn interpolate_template(template: &str, params: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open_idx) = rest.find('{') {
        out.push_str(&rest[..open_idx]);
        let after_open = &rest[open_idx + 1..];
        if let Some(close_idx) = after_open.find('}') {
            let key = &after_open[..close_idx];
            if let Some((_, val)) = params.iter().find(|(k, _)| *k == key) {
                out.push_str(val);
            } else {
                out.push('{');
                out.push_str(key);
                out.push('}');
            }
            rest = &after_open[close_idx + 1..];
        } else {
            out.push_str(&rest[open_idx..]);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

/// A single localized entry inside a `TranslationCatalog`, supporting either a single template
/// string or pluralized forms keyed by `PluralCategory`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LocalizedMessage {
    /// Single message template with optional `{param}` placeholders.
    Simple(String),
    /// Pluralized message forms keyed by `PluralCategory` (must always include `Other`).
    Plural {
        forms: BTreeMap<PluralCategory, String>,
    },
}

impl LocalizedMessage {
    pub fn simple(template: impl Into<String>) -> Self {
        Self::Simple(template.into())
    }

    pub fn plural(forms: impl IntoIterator<Item = (PluralCategory, impl Into<String>)>) -> Self {
        Self::Plural {
            forms: forms.into_iter().map(|(k, v)| (k, v.into())).collect(),
        }
    }

    pub fn is_plural(&self) -> bool {
        matches!(self, Self::Plural { .. })
    }

    pub fn validate(&self, key: &str) -> Result<(), LocalizationError> {
        match self {
            Self::Simple(text) => {
                if text.trim().is_empty() {
                    return Err(LocalizationError::InvalidMessage {
                        key: key.to_string(),
                        reason: "Message template cannot be empty.".into(),
                    });
                }
            }
            Self::Plural { forms } => {
                if !forms.contains_key(&PluralCategory::Other) {
                    return Err(LocalizationError::InvalidMessage {
                        key: key.to_string(),
                        reason: "Plural message must define at least the 'other' category.".into(),
                    });
                }
                for (cat, tmpl) in forms {
                    if tmpl.trim().is_empty() {
                        return Err(LocalizationError::InvalidMessage {
                            key: key.to_string(),
                            reason: format!("Plural form '{cat:?}' cannot be empty."),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Collect the union of all `{placeholder}` names across this message's template(s).
    pub fn placeholders(&self) -> BTreeSet<String> {
        match self {
            Self::Simple(text) => extract_placeholders(text),
            Self::Plural { forms } => {
                let mut set = BTreeSet::new();
                for tmpl in forms.values() {
                    set.extend(extract_placeholders(tmpl));
                }
                set
            }
        }
    }

    /// Resolve the template for a non-plural lookup.
    pub fn resolve_simple(&self, params: &[(&str, &str)]) -> String {
        match self {
            Self::Simple(tmpl) => interpolate_template(tmpl, params),
            Self::Plural { forms } => {
                let tmpl = forms
                    .get(&PluralCategory::Other)
                    .or_else(|| forms.values().next())
                    .map_or("", String::as_str);
                interpolate_template(tmpl, params)
            }
        }
    }

    /// Resolve the template for a plural lookup using CLDR rules for `language` and `count`.
    /// Automatically supplies `count` as `{count}` if not explicitly overridden in `params`.
    pub fn resolve_plural(&self, language: &str, count: u64, params: &[(&str, &str)]) -> String {
        let count_str = count.to_string();
        let mut combined: Vec<(&str, &str)> = Vec::with_capacity(params.len() + 1);
        if !params.iter().any(|(k, _)| *k == "count") {
            combined.push(("count", count_str.as_str()));
        }
        combined.extend_from_slice(params);

        match self {
            Self::Simple(tmpl) => interpolate_template(tmpl, &combined),
            Self::Plural { forms } => {
                let category = select_plural_category(language, count);
                let tmpl = forms
                    .get(&category)
                    .or_else(|| forms.get(&PluralCategory::Other))
                    .or_else(|| forms.values().next())
                    .map_or("", String::as_str);
                interpolate_template(tmpl, &combined)
            }
        }
    }
}

/// Versioned translation catalog for a single `LocaleId`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationCatalog {
    pub locale: LocaleId,
    pub messages: BTreeMap<String, LocalizedMessage>,
}

impl TranslationCatalog {
    pub fn new(locale: LocaleId) -> Self {
        Self {
            locale,
            messages: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, key: impl Into<String>, message: LocalizedMessage) {
        self.messages.insert(key.into(), message);
    }

    pub fn get(&self, key: &str) -> Option<&LocalizedMessage> {
        self.messages.get(key)
    }

    pub fn validate(&self) -> Result<(), LocalizationError> {
        for (key, msg) in &self.messages {
            if !is_valid_translation_key(key) {
                return Err(LocalizationError::InvalidTranslationKey(key.clone()));
            }
            msg.validate(key)?;
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, LocalizationError> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map_err(|e| LocalizationError::CatalogJson(e.to_string()))
    }

    pub fn from_json(raw: &str) -> Result<Self, LocalizationError> {
        if raw.len() > MAX_CATALOG_JSON_BYTES {
            return Err(LocalizationError::CatalogJson(format!(
                "Catalog JSON exceeds {MAX_CATALOG_JSON_BYTES} bytes."
            )));
        }
        let catalog: Self =
            serde_json::from_str(raw).map_err(|e| LocalizationError::CatalogJson(e.to_string()))?;
        catalog.validate()?;
        Ok(catalog)
    }
}

/// Resolution status for a localized message lookup across a `LocaleFallbackChain`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizationLookupResult {
    pub key: String,
    pub requested_locale: LocaleId,
    pub resolved_locale: Option<LocaleId>,
    pub used_fallback: bool,
    pub text: String,
    pub missing_key_diagnostic: Option<String>,
}

/// Multi-locale catalog registry supporting fallback resolution and missing-key diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalizationRegistry {
    default_locale: LocaleId,
    catalogs: BTreeMap<LocaleId, TranslationCatalog>,
}

impl LocalizationRegistry {
    pub fn new(default_locale: LocaleId) -> Self {
        Self {
            default_locale,
            catalogs: BTreeMap::new(),
        }
    }

    pub fn default_locale(&self) -> &LocaleId {
        &self.default_locale
    }

    pub fn register_catalog(
        &mut self,
        catalog: TranslationCatalog,
    ) -> Result<(), LocalizationError> {
        catalog.validate()?;
        self.catalogs.insert(catalog.locale.clone(), catalog);
        Ok(())
    }

    pub fn catalog(&self, locale: &LocaleId) -> Option<&TranslationCatalog> {
        self.catalogs.get(locale)
    }

    /// Resolve a simple or parameterized message across the locale fallback chain.
    pub fn format_message(
        &self,
        requested_locale: &LocaleId,
        key: &str,
        params: &[(&str, &str)],
    ) -> LocalizationLookupResult {
        let chain = requested_locale.fallback_chain(&self.default_locale);
        for (idx, candidate_locale) in chain.locales.iter().enumerate() {
            if let Some(cat) = self.catalogs.get(candidate_locale)
                && let Some(msg) = cat.get(key)
            {
                return LocalizationLookupResult {
                    key: key.to_string(),
                    requested_locale: requested_locale.clone(),
                    resolved_locale: Some(candidate_locale.clone()),
                    used_fallback: idx > 0,
                    text: msg.resolve_simple(params),
                    missing_key_diagnostic: None,
                };
            }
        }
        LocalizationLookupResult {
            key: key.to_string(),
            requested_locale: requested_locale.clone(),
            resolved_locale: None,
            used_fallback: true,
            text: format!("[[{key}]]"),
            missing_key_diagnostic: Some(format!(
                "Missing translation key '{key}' across fallback chain {:?}.",
                chain.as_tags()
            )),
        }
    }

    /// Resolve a pluralized message across the locale fallback chain using the resolved catalog's
    /// language plural rules.
    pub fn format_plural(
        &self,
        requested_locale: &LocaleId,
        key: &str,
        count: u64,
        params: &[(&str, &str)],
    ) -> LocalizationLookupResult {
        let chain = requested_locale.fallback_chain(&self.default_locale);
        for (idx, candidate_locale) in chain.locales.iter().enumerate() {
            if let Some(cat) = self.catalogs.get(candidate_locale)
                && let Some(msg) = cat.get(key)
            {
                return LocalizationLookupResult {
                    key: key.to_string(),
                    requested_locale: requested_locale.clone(),
                    resolved_locale: Some(candidate_locale.clone()),
                    used_fallback: idx > 0,
                    text: msg.resolve_plural(&candidate_locale.language, count, params),
                    missing_key_diagnostic: None,
                };
            }
        }
        LocalizationLookupResult {
            key: key.to_string(),
            requested_locale: requested_locale.clone(),
            resolved_locale: None,
            used_fallback: true,
            text: format!("[[{key}:{count}]]"),
            missing_key_diagnostic: Some(format!(
                "Missing plural translation key '{key}' across fallback chain {:?}.",
                chain.as_tags()
            )),
        }
    }
}

/// Placeholder mismatch between a reference catalog entry and a translated target catalog entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceholderMismatch {
    pub key: String,
    pub expected_placeholders: Vec<String>,
    pub actual_placeholders: Vec<String>,
}

/// Missing plural category for a pluralized key in a target locale.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingPluralForm {
    pub key: String,
    pub missing_categories: Vec<PluralCategory>,
}

/// Completeness and consistency report comparing a `target` catalog against a `reference` catalog
/// and a list of required source keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCompletenessReport {
    pub target_locale: LocaleId,
    pub is_complete: bool,
    pub missing_keys: Vec<String>,
    pub extra_keys: Vec<String>,
    pub placeholder_mismatches: Vec<PlaceholderMismatch>,
    pub missing_plural_forms: Vec<MissingPluralForm>,
}

/// Verify that `target` covers all keys in `reference` and `required_source_keys`, preserves exact
/// `{placeholder}` sets, and defines all required CLDR plural categories for `target.locale`.
pub fn verify_catalog_completeness(
    reference: &TranslationCatalog,
    target: &TranslationCatalog,
    required_source_keys: &[&str],
) -> CatalogCompletenessReport {
    let mut expected_keys: BTreeSet<String> = reference.messages.keys().cloned().collect();
    for req in required_source_keys {
        expected_keys.insert((*req).to_string());
    }

    let mut missing_keys = Vec::new();
    for key in &expected_keys {
        if !target.messages.contains_key(key) {
            missing_keys.push(key.clone());
        }
    }

    let mut extra_keys = Vec::new();
    for key in target.messages.keys() {
        if !expected_keys.contains(key) {
            extra_keys.push(key.clone());
        }
    }

    let mut placeholder_mismatches = Vec::new();
    let mut missing_plural_forms = Vec::new();
    let required_cats = required_plural_categories_for_language(&target.locale.language);

    for (key, ref_msg) in &reference.messages {
        let Some(target_msg) = target.messages.get(key) else {
            continue;
        };
        let expected_ph = ref_msg.placeholders();
        let actual_ph = target_msg.placeholders();
        if expected_ph != actual_ph {
            placeholder_mismatches.push(PlaceholderMismatch {
                key: key.clone(),
                expected_placeholders: expected_ph.into_iter().collect(),
                actual_placeholders: actual_ph.into_iter().collect(),
            });
        }
        if ref_msg.is_plural() {
            match target_msg {
                LocalizedMessage::Simple(_) => {
                    missing_plural_forms.push(MissingPluralForm {
                        key: key.clone(),
                        missing_categories: required_cats.to_vec(),
                    });
                }
                LocalizedMessage::Plural { forms } => {
                    let missing: Vec<PluralCategory> = required_cats
                        .iter()
                        .copied()
                        .filter(|cat| !forms.contains_key(cat))
                        .collect();
                    if !missing.is_empty() {
                        missing_plural_forms.push(MissingPluralForm {
                            key: key.clone(),
                            missing_categories: missing,
                        });
                    }
                }
            }
        }
    }

    let is_complete = missing_keys.is_empty()
        && extra_keys.is_empty()
        && placeholder_mismatches.is_empty()
        && missing_plural_forms.is_empty();

    CatalogCompletenessReport {
        target_locale: target.locale.clone(),
        is_complete,
        missing_keys,
        extra_keys,
        placeholder_mismatches,
        missing_plural_forms,
    }
}

/// Built-in English (`en-US`) seed translation catalog covering `CORE_UI_TRANSLATION_KEYS`.
pub fn builtin_en_us_catalog() -> TranslationCatalog {
    let mut cat = TranslationCatalog::new(LocaleId::default_en_us());
    cat.insert("menu.file.new", LocalizedMessage::simple("New Document"));
    cat.insert("menu.file.open", LocalizedMessage::simple("Open…"));
    cat.insert("menu.file.save", LocalizedMessage::simple("Save"));
    cat.insert(
        "menu.file.publish",
        LocalizedMessage::simple("Publish HTML5…"),
    );
    cat.insert("menu.edit.undo", LocalizedMessage::simple("Undo {action}"));
    cat.insert("menu.edit.redo", LocalizedMessage::simple("Redo {action}"));
    cat.insert("panel.timeline.title", LocalizedMessage::simple("Timeline"));
    cat.insert("panel.library.title", LocalizedMessage::simple("Library"));
    cat.insert(
        "panel.library.items_count",
        LocalizedMessage::plural([
            (PluralCategory::One, "{count} item in Library"),
            (PluralCategory::Other, "{count} items in Library"),
        ]),
    );
    cat.insert(
        "panel.properties.title",
        LocalizedMessage::simple("Properties"),
    );
    cat.insert("panel.actions.title", LocalizedMessage::simple("Actions"));
    cat.insert(
        "tool.selection.name",
        LocalizedMessage::simple("Selection (V)"),
    );
    cat.insert(
        "tool.subselection.name",
        LocalizedMessage::simple("Subselection (A)"),
    );
    cat.insert("tool.pen.name", LocalizedMessage::simple("Pen (P)"));
    cat.insert("tool.brush.name", LocalizedMessage::simple("Brush (B)"));
    cat.insert("tool.text.name", LocalizedMessage::simple("Text (T)"));
    cat.insert(
        "timeline.frames.count",
        LocalizedMessage::plural([
            (PluralCategory::One, "{count} frame in {scene}"),
            (PluralCategory::Other, "{count} frames in {scene}"),
        ]),
    );
    cat.insert("timeline.fps.label", LocalizedMessage::simple("{fps} fps"));
    cat.insert(
        "publish.html5.status",
        LocalizedMessage::simple("Published {project} to {target}"),
    );
    cat.insert(
        "publish.warnings.count",
        LocalizedMessage::plural([
            (PluralCategory::One, "{count} publish warning"),
            (PluralCategory::Other, "{count} publish warnings"),
        ]),
    );
    cat
}

/// Built-in Russian (`ru-RU`) seed translation catalog covering `CORE_UI_TRANSLATION_KEYS`.
pub fn builtin_ru_ru_catalog() -> TranslationCatalog {
    let locale = LocaleId::parse("ru-RU").expect("valid ru-RU locale");
    let mut cat = TranslationCatalog::new(locale);
    cat.insert("menu.file.new", LocalizedMessage::simple("Новый документ"));
    cat.insert("menu.file.open", LocalizedMessage::simple("Открыть…"));
    cat.insert("menu.file.save", LocalizedMessage::simple("Сохранить"));
    cat.insert(
        "menu.file.publish",
        LocalizedMessage::simple("Опубликовать HTML5…"),
    );
    cat.insert(
        "menu.edit.undo",
        LocalizedMessage::simple("Отменить: {action}"),
    );
    cat.insert(
        "menu.edit.redo",
        LocalizedMessage::simple("Повторить: {action}"),
    );
    cat.insert(
        "panel.timeline.title",
        LocalizedMessage::simple("Монтажный кадр"),
    );
    cat.insert(
        "panel.library.title",
        LocalizedMessage::simple("Библиотека"),
    );
    cat.insert(
        "panel.library.items_count",
        LocalizedMessage::plural([
            (PluralCategory::One, "{count} элемент в библиотеке"),
            (PluralCategory::Few, "{count} элемента в библиотеке"),
            (PluralCategory::Many, "{count} элементов в библиотеке"),
            (PluralCategory::Other, "{count} элементов в библиотеке"),
        ]),
    );
    cat.insert(
        "panel.properties.title",
        LocalizedMessage::simple("Свойства"),
    );
    cat.insert("panel.actions.title", LocalizedMessage::simple("Действия"));
    cat.insert(
        "tool.selection.name",
        LocalizedMessage::simple("Выделение (V)"),
    );
    cat.insert(
        "tool.subselection.name",
        LocalizedMessage::simple("Частичное выделение (A)"),
    );
    cat.insert("tool.pen.name", LocalizedMessage::simple("Перо (P)"));
    cat.insert("tool.brush.name", LocalizedMessage::simple("Кисть (B)"));
    cat.insert("tool.text.name", LocalizedMessage::simple("Текст (T)"));
    cat.insert(
        "timeline.frames.count",
        LocalizedMessage::plural([
            (PluralCategory::One, "{count} кадр в сцене {scene}"),
            (PluralCategory::Few, "{count} кадра в сцене {scene}"),
            (PluralCategory::Many, "{count} кадров в сцене {scene}"),
            (PluralCategory::Other, "{count} кадров в сцене {scene}"),
        ]),
    );
    cat.insert(
        "timeline.fps.label",
        LocalizedMessage::simple("{fps} кадр/с"),
    );
    cat.insert(
        "publish.html5.status",
        LocalizedMessage::simple("Опубликован проект {project} в {target}"),
    );
    cat.insert(
        "publish.warnings.count",
        LocalizedMessage::plural([
            (PluralCategory::One, "{count} предупреждение публикации"),
            (PluralCategory::Few, "{count} предупреждения публикации"),
            (PluralCategory::Many, "{count} предупреждений публикации"),
            (PluralCategory::Other, "{count} предупреждений публикации"),
        ]),
    );
    cat
}

/// Construct a pre-populated `LocalizationRegistry` with the built-in `en-US` (plus base `en`)
/// and `ru-RU` (plus base `ru`) catalogs.
pub fn builtin_localization_registry() -> LocalizationRegistry {
    let en_us = builtin_en_us_catalog();
    let mut en_base = en_us.clone();
    en_base.locale = en_us.locale.language_only();

    let ru_ru = builtin_ru_ru_catalog();
    let mut ru_base = ru_ru.clone();
    ru_base.locale = ru_ru.locale.language_only();

    let mut reg = LocalizationRegistry::new(LocaleId::default_en_us());
    reg.register_catalog(en_us).expect("valid en-US catalog");
    reg.register_catalog(en_base).expect("valid en catalog");
    reg.register_catalog(ru_ru).expect("valid ru-RU catalog");
    reg.register_catalog(ru_base).expect("valid ru catalog");
    reg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcp47_parsing_normalization_and_fallback_chains() {
        let ru = LocaleId::parse("ru_ru").unwrap();
        assert_eq!(ru.canonical_tag(), "ru-RU");
        assert_eq!(ru.language, "ru");
        assert_eq!(ru.script, None);
        assert_eq!(ru.region.as_deref(), Some("RU"));

        let zh = LocaleId::parse("ZH-hans-cn").unwrap();
        assert_eq!(zh.canonical_tag(), "zh-Hans-CN");
        assert_eq!(zh.language, "zh");
        assert_eq!(zh.script.as_deref(), Some("Hans"));
        assert_eq!(zh.region.as_deref(), Some("CN"));

        let en_us = LocaleId::default_en_us();
        let ru_chain = ru.fallback_chain(&en_us);
        assert_eq!(ru_chain.as_tags(), vec!["ru-RU", "ru", "en-US", "en"]);

        let zh_chain = zh.fallback_chain(&en_us);
        assert_eq!(
            zh_chain.as_tags(),
            vec!["zh-Hans-CN", "zh-Hans", "zh-CN", "zh", "en-US", "en"]
        );

        assert!(LocaleId::parse("").is_err());
        assert!(LocaleId::parse("invalid_language_tag").is_err());
    }

    #[test]
    fn cldr_plural_rules_across_language_families() {
        // English / Germanic
        assert_eq!(select_plural_category("en-US", 0), PluralCategory::Other);
        assert_eq!(select_plural_category("en-US", 1), PluralCategory::One);
        assert_eq!(select_plural_category("en-US", 2), PluralCategory::Other);
        assert_eq!(select_plural_category("de-DE", 1), PluralCategory::One);
        assert_eq!(select_plural_category("de-DE", 5), PluralCategory::Other);

        // Russian / Slavic
        assert_eq!(select_plural_category("ru", 1), PluralCategory::One);
        assert_eq!(select_plural_category("ru", 21), PluralCategory::One);
        assert_eq!(select_plural_category("ru", 2), PluralCategory::Few);
        assert_eq!(select_plural_category("ru", 4), PluralCategory::Few);
        assert_eq!(select_plural_category("ru", 23), PluralCategory::Few);
        assert_eq!(select_plural_category("ru", 0), PluralCategory::Many);
        assert_eq!(select_plural_category("ru", 5), PluralCategory::Many);
        assert_eq!(select_plural_category("ru", 11), PluralCategory::Many);
        assert_eq!(select_plural_category("ru", 14), PluralCategory::Many);
        assert_eq!(select_plural_category("ru", 25), PluralCategory::Many);

        // Polish
        assert_eq!(select_plural_category("pl", 1), PluralCategory::One);
        assert_eq!(select_plural_category("pl", 21), PluralCategory::Many);
        assert_eq!(select_plural_category("pl", 2), PluralCategory::Few);
        assert_eq!(select_plural_category("pl", 12), PluralCategory::Many);

        // Arabic
        assert_eq!(select_plural_category("ar", 0), PluralCategory::Zero);
        assert_eq!(select_plural_category("ar", 1), PluralCategory::One);
        assert_eq!(select_plural_category("ar", 2), PluralCategory::Two);
        assert_eq!(select_plural_category("ar", 5), PluralCategory::Few);
        assert_eq!(select_plural_category("ar", 25), PluralCategory::Many);
        assert_eq!(select_plural_category("ar", 100), PluralCategory::Other);

        // East Asian
        assert_eq!(select_plural_category("ja-JP", 0), PluralCategory::Other);
        assert_eq!(select_plural_category("ja-JP", 1), PluralCategory::Other);
        assert_eq!(select_plural_category("zh-CN", 10), PluralCategory::Other);
    }

    #[test]
    fn builtin_catalogs_pass_completeness_round_trip_and_fallback_lookup() {
        let en = builtin_en_us_catalog();
        let ru = builtin_ru_ru_catalog();
        en.validate().expect("en-US valid");
        ru.validate().expect("ru-RU valid");

        let report = verify_catalog_completeness(&en, &ru, CORE_UI_TRANSLATION_KEYS);
        assert!(report.is_complete, "report: {report:?}");

        let json = ru.to_json().expect("serialize ru-RU catalog");
        let restored = TranslationCatalog::from_json(&json).expect("deserialize ru-RU catalog");
        assert_eq!(ru, restored);

        let reg = builtin_localization_registry();
        let ru_loc = LocaleId::parse("ru-RU").unwrap();
        let ja_loc = LocaleId::parse("ja-JP").unwrap();

        // Pluralized + parameterized lookup in Russian (1, 3, 12 frames)
        let f1 = reg.format_plural(&ru_loc, "timeline.frames.count", 1, &[("scene", "Scene 1")]);
        assert_eq!(f1.text, "1 кадр в сцене Scene 1");
        assert!(!f1.used_fallback);

        let f3 = reg.format_plural(&ru_loc, "timeline.frames.count", 3, &[("scene", "Scene 1")]);
        assert_eq!(f3.text, "3 кадра в сцене Scene 1");

        let f12 = reg.format_plural(
            &ru_loc,
            "timeline.frames.count",
            12,
            &[("scene", "Scene 1")],
        );
        assert_eq!(f12.text, "12 кадров в сцене Scene 1");

        // Fallback from unregistered ja-JP -> en-US
        let fallback_res = reg.format_message(&ja_loc, "menu.file.open", &[]);
        assert_eq!(fallback_res.text, "Open…");
        assert!(fallback_res.used_fallback);
        assert_eq!(
            fallback_res.resolved_locale,
            Some(LocaleId::default_en_us())
        );

        // Missing key diagnostic
        let missing = reg.format_message(&ru_loc, "menu.nonexistent.item", &[]);
        assert_eq!(missing.text, "[[menu.nonexistent.item]]");
        assert!(missing.missing_key_diagnostic.is_some());
    }

    #[test]
    fn completeness_verifier_detects_missing_keys_placeholder_mismatches_and_missing_plurals() {
        let en = builtin_en_us_catalog();
        let mut incomplete_ru = builtin_ru_ru_catalog();
        incomplete_ru.messages.remove("menu.file.save");
        incomplete_ru.insert(
            "menu.edit.undo",
            LocalizedMessage::simple("Отменить {wrong_param}"),
        );
        incomplete_ru.insert(
            "publish.warnings.count",
            LocalizedMessage::plural([
                (PluralCategory::One, "{count} предупреждение"),
                (PluralCategory::Other, "{count} предупреждений"),
            ]),
        );

        let report = verify_catalog_completeness(&en, &incomplete_ru, CORE_UI_TRANSLATION_KEYS);
        assert!(!report.is_complete);
        assert_eq!(report.missing_keys, vec!["menu.file.save".to_string()]);
        assert_eq!(report.placeholder_mismatches.len(), 1);
        assert_eq!(report.placeholder_mismatches[0].key, "menu.edit.undo");
        assert_eq!(report.missing_plural_forms.len(), 1);
        assert_eq!(report.missing_plural_forms[0].key, "publish.warnings.count");
        assert_eq!(
            report.missing_plural_forms[0].missing_categories,
            vec![PluralCategory::Few, PluralCategory::Many]
        );
    }
}
