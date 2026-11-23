//! Versioned FLA/XFL target profiles, compatibility preflight, derived-project
//! baking/conversion, and atomic round-trip-verified export (`T-188`).

use crate::ExportError;
use crate::fonts::{FontExportPolicy, apply_font_policy, convert_project_text_to_outlines};
use neoflash_core::{
    AuthoredBitmapFill, BlendMode, Color, Easing, Element, ElementKind, FilterSpec, FrameSound,
    GradientKind, GradientPaint, GradientSpread, Id, InstancePlayback, Keyframe, Layer, LayerRole,
    MaskMode, ObjectEvent, ObjectScript, PathCommand, Project, ResourceUnit, ScriptLanguage,
    SoundEffect, SoundSync, Symbol, SymbolKind, TextAlign, Timeline, Transform, TweenKind,
    sample_layer_in, transform_matrix,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;

/// Target authoring format and version profile for FLA / XFL Save As (`T-188`, `T-189`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlaTargetProfile {
    /// Adobe Animate CC (`.fla` ZIP archive, XFL `2.97`, `creatorInfo="Adobe Animate"`).
    #[default]
    AnimateCC,
    /// Adobe Flash Professional CS6 (`.fla` ZIP archive, XFL `2.2`, `creatorInfo="Adobe Flash Professional CS6"`).
    FlashCS6,
    /// Adobe Flash Professional CS5.5 (`.fla` ZIP archive, XFL `2.1`, `creatorInfo="Adobe Flash Professional CS5.5"`).
    FlashCS5_5,
    /// Adobe Flash Professional CS5 (`.fla` ZIP archive, XFL `2.0`, `creatorInfo="Adobe Flash Professional CS5"`).
    FlashCS5,
    /// Uncompressed XFL folder (`<stem>.xfl` marker + `DOMDocument.xml` + `LIBRARY/` + `META-INF/metadata.xml` + `PublishSettings.xml`).
    UncompressedXfl,
    /// Legacy binary OLE2/CFB `.fla` document for Adobe Flash CS4 (Flash 10, AVM1/AVM2).
    LegacyFlashCS4,
    /// Legacy binary OLE2/CFB `.fla` document for Macromedia/Adobe Flash 8 (Flash 8, AVM1 only).
    LegacyFlash8,
}

impl FlaTargetProfile {
    pub const ALL: [Self; 7] = [
        Self::AnimateCC,
        Self::FlashCS6,
        Self::FlashCS5_5,
        Self::FlashCS5,
        Self::UncompressedXfl,
        Self::LegacyFlashCS4,
        Self::LegacyFlash8,
    ];

    /// Human-readable label for UI and reports.
    pub fn label(self) -> &'static str {
        match self {
            Self::AnimateCC => "Adobe Animate CC (XFL 2.97)",
            Self::FlashCS6 => "Adobe Flash Professional CS6 (XFL 2.2)",
            Self::FlashCS5_5 => "Adobe Flash Professional CS5.5 (XFL 2.1)",
            Self::FlashCS5 => "Adobe Flash Professional CS5 (XFL 2.0)",
            Self::UncompressedXfl => "Uncompressed XFL Directory (XFL 2.97)",
            Self::LegacyFlashCS4 => "Adobe Flash CS4 Legacy Binary (.fla v10)",
            Self::LegacyFlash8 => "Macromedia Flash 8 Legacy Binary (.fla v8)",
        }
    }

    /// XFL `xflVersion` attribute string for XML-based profiles.
    pub fn xfl_version(self) -> &'static str {
        match self {
            Self::AnimateCC | Self::UncompressedXfl => "2.97",
            Self::FlashCS6 => "2.2",
            Self::FlashCS5_5 => "2.1",
            Self::FlashCS5 => "2.0",
            Self::LegacyFlashCS4 => "10.0",
            Self::LegacyFlash8 => "8.0",
        }
    }

    /// `creatorInfo` attribute written to `DOMDocument.xml`.
    pub fn creator_info(self) -> &'static str {
        match self {
            Self::AnimateCC | Self::UncompressedXfl => "Adobe Animate",
            Self::FlashCS6 => "Adobe Flash Professional CS6",
            Self::FlashCS5_5 => "Adobe Flash Professional CS5.5",
            Self::FlashCS5 => "Adobe Flash Professional CS5",
            Self::LegacyFlashCS4 => "Adobe Flash CS4 Professional",
            Self::LegacyFlash8 => "Macromedia Flash 8",
        }
    }

    /// True if this profile writes an OLE2 Compound File Binary (`.fla`) stream inventory.
    pub fn is_legacy_binary(self) -> bool {
        matches!(self, Self::LegacyFlashCS4 | Self::LegacyFlash8)
    }

    /// True if this profile writes an uncompressed directory rather than a single file.
    pub fn is_directory(self) -> bool {
        matches!(self, Self::UncompressedXfl)
    }

    /// Maximum SWF/Flash Player version associated with the authoring tool.
    pub fn target_player_version(self) -> u8 {
        match self {
            Self::AnimateCC | Self::UncompressedXfl => 32,
            Self::FlashCS6 => 11,
            Self::FlashCS5_5 => 10,
            Self::FlashCS5 => 10,
            Self::LegacyFlashCS4 => 10,
            Self::LegacyFlash8 => 8,
        }
    }

    /// True if the target profile supports ActionScript 3.0 (AVM2).
    pub fn supports_as3(self) -> bool {
        !matches!(self, Self::LegacyFlash8)
    }

    /// True if the target profile supports variable-width stroke profiles natively (CS5.5+).
    pub fn supports_variable_width_strokes(self) -> bool {
        matches!(
            self,
            Self::AnimateCC | Self::UncompressedXfl | Self::FlashCS6 | Self::FlashCS5_5
        )
    }

    /// True if the target profile supports layer-level filters/blend modes and camera/Z-depth (Animate CC).
    pub fn supports_modern_layer_effects(self) -> bool {
        matches!(self, Self::AnimateCC | Self::UncompressedXfl)
    }
}

fn filter_is_serialized_by_profile(profile: FlaTargetProfile, filter: &FilterSpec) -> bool {
    match filter {
        FilterSpec::DropShadow { .. }
        | FilterSpec::Blur { .. }
        | FilterSpec::Glow { .. }
        | FilterSpec::Bevel { .. } => true,
        FilterSpec::GradientGlow { .. } | FilterSpec::GradientBevel { .. } => {
            profile.is_legacy_binary()
        }
        FilterSpec::ColorMatrix { .. } | FilterSpec::Convolution { .. } => false,
    }
}

fn filter_kind_name(filter: &FilterSpec) -> &'static str {
    match filter {
        FilterSpec::DropShadow { .. } => "DropShadow",
        FilterSpec::Blur { .. } => "Blur",
        FilterSpec::Glow { .. } => "Glow",
        FilterSpec::Bevel { .. } => "Bevel",
        FilterSpec::GradientGlow { .. } => "GradientGlow",
        FilterSpec::GradientBevel { .. } => "GradientBevel",
        FilterSpec::ColorMatrix { .. } => "ColorMatrix",
        FilterSpec::Convolution { .. } => "Convolution",
    }
}

fn preflight_filter_stack(
    filters: &[FilterSpec],
    location: &str,
    profile: FlaTargetProfile,
    report: &mut FlaCompatibilityReport,
) {
    for (index, filter) in filters.iter().enumerate() {
        let supported = filter_is_serialized_by_profile(profile, filter);
        report.push(
            if supported {
                "filter_serialized"
            } else {
                "filter_not_serialized"
            },
            FlaFeatureCategory::FiltersAndBlends,
            if supported {
                FlaFeatureDisposition::Exact
            } else {
                FlaFeatureDisposition::Blocking
            },
            format!("{location}:filter[{index}]"),
            if supported {
                format!(
                    "{} filter has a native representation in {}.",
                    filter_kind_name(filter),
                    profile.label()
                )
            } else {
                format!(
                    "{} filter has no lossless representation in the current {} writer; export is blocked instead of silently dropping or replacing it.",
                    filter_kind_name(filter),
                    profile.label()
                )
            },
        );
        let gradient_stop_count = match filter {
            FilterSpec::GradientGlow {
                colors,
                alphas,
                ratios,
                ..
            }
            | FilterSpec::GradientBevel {
                colors,
                alphas,
                ratios,
                ..
            } => colors.len().min(alphas.len()).min(ratios.len()),
            _ => 0,
        };
        if profile.is_legacy_binary() && gradient_stop_count > 15 {
            report.push(
                "legacy_filter_gradient_stop_limit",
                FlaFeatureCategory::FiltersAndBlends,
                FlaFeatureDisposition::Blocking,
                format!("{location}:filter[{index}]"),
                format!(
                    "{} has {gradient_stop_count} gradient stops, but the legacy binary FLA structure stores at most 15; export is blocked instead of truncating the last stop.",
                    filter_kind_name(filter)
                ),
            );
        }
    }
}

pub(crate) fn is_valid_as3_qname(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.split('.').all(|seg| {
        let mut chars = seg.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        (first.is_ascii_alphabetic() || matches!(first, '_' | '$'))
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
    })
}

pub(crate) fn is_internal_provenance_resource(res: &ResourceUnit) -> bool {
    res.id == "fla-provenance"
        || matches!(
            res.path.as_str(),
            "legacy/provenance.json" | "legacy/import-provenance.json"
        )
        || res.path.starts_with("legacy/source.fla")
        || res.path.starts_with("legacy/source.swf")
        || matches!(
            res.media_type.to_ascii_lowercase().as_str(),
            "application/x-neoflash-fla-source"
                | "application/x-neoflash-fla-provenance"
                | "application/x-neoflash-swf-source"
                | "application/x-neoflash-swf-provenance"
                | "application/x-neoflash-swc-source"
                | "application/x-neoflash-swc-provenance"
        )
}

pub(crate) fn is_actionscript_resource(res: &ResourceUnit) -> bool {
    let mime = res.media_type.trim().to_ascii_lowercase();
    let is_as_ext = res.path.trim().to_ascii_lowercase().ends_with(".as");
    mime == "text/x-actionscript"
        || mime == "application/actionscript"
        || mime == "application/x-actionscript"
        || (is_as_ext
            && (mime.is_empty() || mime == "text/plain" || mime == "application/javascript"))
}

pub(crate) fn is_unreferenced_sidecar_resource(project: &Project, res: &ResourceUnit) -> bool {
    if is_actionscript_resource(res) {
        return false;
    }
    let mime = res.media_type.trim().to_ascii_lowercase();
    if !matches!(mime.as_str(), "text/plain" | "application/json") {
        return false;
    }
    !project.symbols.iter().any(|sym| {
        sym.timeline.layers.iter().any(|layer| {
            layer.keyframes.iter().any(|kf| {
                kf.sound.as_ref().is_some_and(|s| s.resource == res.id)
                    || kf.bitmap_fills.iter().any(|bf| bf.resource_id == res.id)
                    || kf.elements.iter().any(|el| {
                        matches!(&el.kind, ElementKind::Bitmap { resource_id, .. } if resource_id == &res.id)
                    })
            })
        })
    })
}

pub(crate) fn normalize_fla_bitmap_bytes(
    bytes: &[u8],
) -> Option<(Vec<u8>, &'static str, u32, u32)> {
    let format = image::guess_format(bytes).ok()?;
    let decoded = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let (w, h) = (decoded.width(), decoded.height());
    if w == 0 || h == 0 {
        return None;
    }
    match format {
        image::ImageFormat::Png => Some((bytes.to_vec(), "png", w, h)),
        image::ImageFormat::Jpeg => Some((bytes.to_vec(), "jpg", w, h)),
        _ => {
            let mut png = Vec::new();
            let mut cursor = std::io::Cursor::new(&mut png);
            decoded
                .into_rgba8()
                .write_to(&mut cursor, image::ImageFormat::Png)
                .ok()?;
            Some((png, "png", w, h))
        }
    }
}

pub(crate) fn inspect_wav_pcm_duration_44(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut fmt_info: Option<(u32, usize)> = None;
    let mut data_len: Option<usize> = None;
    while pos + 8 <= bytes.len() {
        let tag = &bytes[pos..pos + 4];
        let chunk_len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let payload_start = pos + 8;
        let payload_end = payload_start.checked_add(chunk_len)?;
        if payload_end > bytes.len() {
            return None;
        }
        let payload = &bytes[payload_start..payload_end];
        if tag == b"fmt " {
            if payload.len() < 16 {
                return None;
            }
            let audio_format = u16::from_le_bytes(payload[0..2].try_into().ok()?);
            if audio_format != 1 {
                return None;
            }
            let channels = u16::from_le_bytes(payload[2..4].try_into().ok()?);
            let sample_rate = u32::from_le_bytes(payload[4..8].try_into().ok()?);
            let block_align = u16::from_le_bytes(payload[12..14].try_into().ok()?);
            let bits = u16::from_le_bytes(payload[14..16].try_into().ok()?);
            if channels == 0 || sample_rate == 0 || bits == 0 {
                return None;
            }
            let derived_align = usize::from(channels) * usize::from(bits.max(8)).div_ceil(8);
            let frame_bytes = usize::from(block_align).max(derived_align).max(1);
            fmt_info = Some((sample_rate, frame_bytes));
        } else if tag == b"data" {
            data_len = Some(payload.len());
        }
        pos = payload_end.saturating_add(chunk_len & 1);
    }
    let (sample_rate, frame_bytes) = fmt_info?;
    let data_bytes = data_len?;
    if data_bytes < frame_bytes {
        return None;
    }
    let sample_frames = (data_bytes / frame_bytes) as u64;
    Some((sample_frames * 44_100) / u64::from(sample_rate))
}

pub(crate) fn looks_like_mp3_bytes(bytes: &[u8]) -> bool {
    let mut start = 0usize;
    if bytes.len() >= 10
        && &bytes[..3] == b"ID3"
        && (bytes[6] | bytes[7] | bytes[8] | bytes[9]) & 0x80 == 0
    {
        let tag_len = ((bytes[6] as usize) << 21)
            | ((bytes[7] as usize) << 14)
            | ((bytes[8] as usize) << 7)
            | (bytes[9] as usize);
        let footer = if (bytes[5] & 0x10) != 0 { 10 } else { 0 };
        start = (10 + tag_len + footer).min(bytes.len());
    }
    let end = bytes
        .len()
        .saturating_sub(3)
        .min(start.saturating_add(4096));
    (start..end).any(|i| {
        let h = &bytes[i..i + 4];
        h[0] == 0xff
            && (h[1] & 0xe0) == 0xe0
            && ((h[1] >> 3) & 3) != 1
            && ((h[1] >> 1) & 3) != 0
            && ((h[2] >> 4) & 0xf) != 0xf
            && ((h[2] >> 2) & 3) != 3
    })
}

pub(crate) fn inspect_audio_codec(bytes: &[u8]) -> Option<(&'static str, u64)> {
    if let Some(duration_44) = inspect_wav_pcm_duration_44(bytes) {
        return Some(("wav", duration_44));
    }
    looks_like_mp3_bytes(bytes).then_some(("mp3", 44_100))
}

pub(crate) fn encode_sound_envelope_points(
    snd: &FrameSound,
    duration_44: u64,
) -> Vec<(u64, u32, u32)> {
    let raw = snd.envelope_points();
    let denom = if duration_44 > 0 { duration_44 } else { 44_100 };
    let mut encoded: Vec<(u64, u32, u32)> = raw
        .into_iter()
        .take(FrameSound::MAX_ENVELOPE_POINTS)
        .map(|[pos, left, right]| {
            let p = if pos.is_finite() {
                pos.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let l = if left.is_finite() {
                left.clamp(0.0, 1.0)
            } else {
                1.0
            };
            let r = if right.is_finite() {
                right.clamp(0.0, 1.0)
            } else {
                1.0
            };
            let mark44 = (p * denom as f64).round() as u64;
            let level0 = (l * 32_768.0).round() as u32;
            let level1 = (r * 32_768.0).round() as u32;
            (mark44, level0, level1)
        })
        .collect();
    encoded.sort_by_key(|pt| pt.0);
    encoded
}

/// How an authored feature is handled for a target FLA/XFL or SWF profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlaFeatureDisposition {
    /// Preserved 1:1 in the target format without loss.
    Exact,
    /// Converted to an equivalent target representation (e.g., cubic curves subdivided to quadratics).
    Converted,
    /// Evaluated/baked into frame-by-frame keyframes or static geometry (e.g., Motion/IK/AssetWarp spans).
    Baked,
    /// Text converted to vector glyph outlines.
    Outlined,
    /// Approximated with the closest representable feature in the target profile.
    Approximated,
    /// Omitted because the target profile has no representation and omission is non-fatal.
    Omitted,
    /// Blocks export unless the user enables conversion/baking/stripping or selects a newer profile.
    Blocking,
}

impl FlaFeatureDisposition {
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Blocking)
    }
}

/// Category of an inspected document feature in the preflight report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlaFeatureCategory {
    DocumentSettings,
    Scenes,
    SymbolsAndLibrary,
    LayersAndMasks,
    ShapesAndPaints,
    TextAndFonts,
    FiltersAndBlends,
    MediaAndSound,
    AnimationAndRigging,
    Scripts,
}

/// Alias for `FlaFeatureDisposition`.
pub type FlaCompatibilityStatus = FlaFeatureDisposition;

/// Alias for `FlaFeatureCategory`.
pub type FlaCompatibilityCategory = FlaFeatureCategory;

/// One located compatibility finding in a `FlaCompatibilityReport`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlaCompatibilityItem {
    pub code: String,
    pub category: FlaFeatureCategory,
    pub disposition: FlaFeatureDisposition,
    pub location: String,
    pub message: String,
}

/// Structured preflight report for versioned FLA/XFL Save As and SWF publishing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlaCompatibilityReport {
    pub profile: FlaTargetProfile,
    pub items: Vec<FlaCompatibilityItem>,
}

impl FlaCompatibilityReport {
    pub fn new(profile: FlaTargetProfile) -> Self {
        Self {
            profile,
            items: Vec::new(),
        }
    }

    pub fn push(
        &mut self,
        code: impl Into<String>,
        category: FlaFeatureCategory,
        disposition: FlaFeatureDisposition,
        location: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.items.push(FlaCompatibilityItem {
            code: code.into(),
            category,
            disposition,
            location: location.into(),
            message: message.into(),
        });
    }

    /// True if any item has `FlaFeatureDisposition::Blocking`.
    pub fn has_blocking(&self) -> bool {
        self.items.iter().any(|i| i.disposition.is_blocking())
    }

    /// True if export can proceed without blocking issues.
    pub fn can_export(&self) -> bool {
        !self.has_blocking()
    }

    /// Alias for `can_export()`.
    pub fn can_proceed(&self) -> bool {
        !self.has_blocking()
    }

    /// Number of blocking items in the report.
    pub fn blocking_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.disposition.is_blocking())
            .count()
    }

    /// Count of items with the given disposition.
    pub fn count_by_disposition(&self, disposition: FlaFeatureDisposition) -> usize {
        self.items
            .iter()
            .filter(|i| i.disposition == disposition)
            .count()
    }

    /// All blocking items.
    pub fn blocking_items(&self) -> Vec<&FlaCompatibilityItem> {
        self.items
            .iter()
            .filter(|i| i.disposition.is_blocking())
            .collect()
    }
}

/// Options controlling how versioned FLA/XFL export handles non-native features.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlaExportOptions {
    /// Bake Motion tweens, IK armatures, Asset Warp rigs, 3D camera, and parenting into classic frame poses.
    pub bake_unsupported_tweens: bool,
    /// Convert text elements to vector outlines when requested or when targeting profiles without matching font support.
    pub outline_unsupported_text: bool,
    /// Strip incompatible scripts (e.g. JS/TS in Flash targets, or AS3 in Flash 8) instead of blocking.
    pub strip_unsupported_scripts: bool,
    /// Allow non-blocking approximations/omissions.
    pub allow_approximations: bool,
    /// Optional font policy override (defaults to `FontExportPolicy::Embed`).
    pub font_policy: FontExportPolicy,
}

impl Default for FlaExportOptions {
    fn default() -> Self {
        Self {
            bake_unsupported_tweens: true,
            outline_unsupported_text: false,
            strip_unsupported_scripts: false,
            allow_approximations: true,
            font_policy: FontExportPolicy::Embed,
        }
    }
}

fn bitmap_smoothing_usage(project: &Project) -> BTreeMap<&str, (bool, bool)> {
    let mut usage = BTreeMap::new();
    for key in project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
    {
        let placements = key.elements.iter().filter_map(|el| match &el.kind {
            ElementKind::Bitmap {
                resource_id,
                smoothing,
                ..
            } => Some((resource_id, *smoothing)),
            _ => None,
        });
        let fills = key
            .bitmap_fills
            .iter()
            .map(|f| (&f.resource_id, f.smoothing));
        for (id, smoothing) in placements.chain(fills) {
            let flags = usage.entry(id.as_str()).or_insert((false, false));
            flags.0 |= smoothing;
            flags.1 |= !smoothing;
        }
    }
    usage
}

const FLASH8_STAGE_LIMIT: u32 = 2880;

fn flash8_stage_fit(width: u32, height: u32) -> Option<(u32, u32, f64)> {
    let longest = width.max(height);
    if longest <= FLASH8_STAGE_LIMIT {
        return None;
    }
    let scale = f64::from(FLASH8_STAGE_LIMIT) / f64::from(longest);
    let fit = |v| (f64::from(v) * scale).round().max(1.) as u32;
    Some((fit(width), fit(height), scale))
}

mod preflight_project;
mod preflight_timeline;
mod prepare_export;
#[cfg(test)]
mod tests;
mod xfl_dom_shapes;
mod xfl_dom_timeline;
mod xfl_package;

pub use self::preflight_project::{preflight_fla_export, preflight_fla_export_with_options};
pub use self::prepare_export::{
    export_fla_bytes_with_profile, export_fla_with_profile, prepare_project_for_fla_profile,
};
#[allow(unused_imports)]
pub(crate) use self::xfl_dom_shapes::{
    sanitize_source_rel_path, script_starts_with_event_wrapper, skip_leading_ws_and_comments,
};

#[allow(unused_imports)]
use self::{
    preflight_project::*, preflight_timeline::*, prepare_export::*, xfl_dom_shapes::*,
    xfl_dom_timeline::*, xfl_package::*,
};
