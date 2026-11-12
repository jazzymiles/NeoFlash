//! Versioned FLA/XFL target profiles, compatibility preflight, derived-project
//! baking/conversion, and atomic round-trip-verified export (`T-188`).

use crate::ExportError;
use crate::fonts::{FontExportPolicy, apply_font_policy, convert_project_text_to_outlines};
use neoflash_core::{
    BlendMode, Color, Easing, Element, ElementKind, FilterSpec, GradientKind, GradientPaint, Id,
    InstancePlayback, Keyframe, Layer, LayerRole, MaskMode, ObjectEvent, ObjectScript, PathCommand,
    Project, ScriptLanguage, SoundEffect, SoundSync, Symbol, SymbolKind, TextAlign, Timeline,
    Transform, TweenKind, sample_layer_in, transform_matrix,
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

/// Run compatibility preflight for `project` against `profile` using default `FlaExportOptions`.
pub fn preflight_fla_export(
    project: &Project,
    profile: FlaTargetProfile,
) -> FlaCompatibilityReport {
    preflight_fla_export_with_options(project, profile, &FlaExportOptions::default())
}

/// Run compatibility preflight for `project` against `profile` with explicit `options`.
pub fn preflight_fla_export_with_options(
    project: &Project,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
) -> FlaCompatibilityReport {
    let mut report = FlaCompatibilityReport::new(profile);

    // 1. Document settings & scenes
    let max_dim = if matches!(profile, FlaTargetProfile::LegacyFlash8) {
        2880
    } else {
        8192
    };
    if project.width > max_dim || project.height > max_dim {
        report.push(
            "stage_dimensions_clamped",
            FlaFeatureCategory::DocumentSettings,
            if options.allow_approximations {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Blocking
            },
            "Project",
            format!(
                "Stage size {}x{} exceeds {} maximum {}x{} px.",
                project.width,
                project.height,
                profile.label(),
                max_dim,
                max_dim
            ),
        );
    } else {
        report.push(
            "document_settings_exact",
            FlaFeatureCategory::DocumentSettings,
            FlaFeatureDisposition::Exact,
            "Project",
            format!(
                "Stage {}x{} and background RGBA {:?} are supported.",
                project.width, project.height, project.background
            ),
        );
    }

    if project.fps > 120 {
        report.push(
            "frame_rate_clamped",
            FlaFeatureCategory::DocumentSettings,
            if options.allow_approximations {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Blocking
            },
            "Project.fps",
            format!(
                "Frame rate {} fps exceeds the FLA/XFL export maximum of 120 fps; export uses 120 fps.",
                project.fps
            ),
        );
    } else {
        report.push(
            "document_frame_rate_exact",
            FlaFeatureCategory::DocumentSettings,
            FlaFeatureDisposition::Exact,
            "Project.fps",
            format!("Frame rate {} fps is supported.", project.fps),
        );
    }

    if project.scenes.len() > 1 {
        report.push(
            "scenes_preserved",
            FlaFeatureCategory::Scenes,
            FlaFeatureDisposition::Exact,
            "Project.scenes",
            format!(
                "{} scenes are preserved in play order.",
                project.scenes.len()
            ),
        );
    }

    // 2. Collect script languages across the project
    let mut has_avm1 = false;
    let mut has_avm2 = false;
    let mut has_web_scripts = false;

    for src in &project.sources {
        let loc = format!("SourceUnit({})", src.path);
        classify_script_language(
            src.language,
            &loc,
            profile,
            options,
            &mut has_avm1,
            &mut has_avm2,
            &mut has_web_scripts,
            &mut report,
        );
        let lang_supported = matches!(
            src.language,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        ) || (profile.supports_as3()
            && src.language == ScriptLanguage::ActionScript3);
        if profile.is_legacy_binary() && lang_supported {
            report.push(
                "legacy_fla_external_source_approximated",
                FlaFeatureCategory::Scripts,
                if options.allow_approximations || options.strip_unsupported_scripts {
                    FlaFeatureDisposition::Approximated
                } else {
                    FlaFeatureDisposition::Blocking
                },
                &loc,
                format!(
                    "External source file {:?} cannot be stored as a standalone file inside a single-file binary CFB .fla archive ({}); it is preserved in Contents document properties.",
                    src.path,
                    profile.label()
                ),
            );
        }
    }

    if let Some(doc_class) = &project.document_class
        && !doc_class.trim().is_empty()
    {
        if profile.supports_as3() {
            report.push(
                "document_class_preserved",
                FlaFeatureCategory::Scripts,
                FlaFeatureDisposition::Exact,
                "Project.document_class",
                format!("AS3 document class {:?} is preserved.", doc_class),
            );
        } else {
            report.push(
                "document_class_unsupported",
                FlaFeatureCategory::Scripts,
                if options.strip_unsupported_scripts || options.allow_approximations {
                    FlaFeatureDisposition::Omitted
                } else {
                    FlaFeatureDisposition::Blocking
                },
                "Project.document_class",
                format!(
                    "Document class {:?} requires ActionScript 3.0 and is omitted in {}.",
                    doc_class,
                    profile.label()
                ),
            );
        }
    }

    // 3. Library folders & resources
    if !project.library_folders.is_empty() {
        report.push(
            "library_folders_preserved",
            FlaFeatureCategory::SymbolsAndLibrary,
            FlaFeatureDisposition::Exact,
            "Project.library_folders",
            format!(
                "{} library folders are preserved.",
                project.library_folders.len()
            ),
        );
    }

    for res in &project.resources {
        let loc = format!("Resource({})", res.path);
        let mime = res.media_type.to_ascii_lowercase();
        if mime.starts_with("image/") || mime.starts_with("audio/") {
            report.push(
                "media_resource_preserved",
                FlaFeatureCategory::MediaAndSound,
                FlaFeatureDisposition::Exact,
                loc,
                format!("Media asset ({}) is bundled.", res.media_type),
            );
        } else if crate::fonts::is_font_resource(res) {
            let disp = if options.outline_unsupported_text
                || options.font_policy == FontExportPolicy::Outlines
            {
                FlaFeatureDisposition::Outlined
            } else if profile.is_legacy_binary() {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "font_resource",
                FlaFeatureCategory::TextAndFonts,
                disp,
                loc,
                "Embedded font resource inspected for target profile.".to_owned(),
            );
        }
    }

    // 4. Symbols, timelines, layers, keyframes, elements, scripts
    for sym in &project.symbols {
        let sym_loc = format!("Symbol({})", sym.name);
        if sym.scale9_grid().is_some() {
            let disp = if profile.is_legacy_binary() {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "symbol_scale9_grid",
                FlaFeatureCategory::SymbolsAndLibrary,
                disp,
                &sym_loc,
                "Symbol 9-slice scaling grid inspected.".to_owned(),
            );
        }

        if let Some(cls) = &sym.class_name
            && !cls.trim().is_empty()
            && !profile.supports_as3()
        {
            report.push(
                "symbol_as3_class_converted",
                FlaFeatureCategory::SymbolsAndLibrary,
                FlaFeatureDisposition::Converted,
                &sym_loc,
                format!(
                    "Symbol class {:?} converted to AVM1 linkage identifier for {}.",
                    cls,
                    profile.label()
                ),
            );
        }

        for fs in &sym.timeline.scripts {
            let fs_loc = format!("{}:frame[{}]", sym_loc, fs.frame);
            classify_script_language(
                fs.script.language,
                &fs_loc,
                profile,
                options,
                &mut has_avm1,
                &mut has_avm2,
                &mut has_web_scripts,
                &mut report,
            );
        }

        for layer in &sym.timeline.layers {
            let layer_loc = format!("{}/Layer({})", sym_loc, layer.name);
            preflight_layer(
                sym,
                layer,
                &layer_loc,
                profile,
                options,
                &mut has_avm1,
                &mut has_avm2,
                &mut has_web_scripts,
                &mut report,
            );
        }
    }

    // 5. Mixed AVM1 + AVM2 check
    if has_avm1 && has_avm2 {
        report.push(
            "mixed_avm1_avm2_scripts",
            FlaFeatureCategory::Scripts,
            if options.strip_unsupported_scripts {
                FlaFeatureDisposition::Omitted
            } else {
                FlaFeatureDisposition::Blocking
            },
            "Project.scripts",
            "Project mixes ActionScript 1/2 (AVM1) and ActionScript 3 (AVM2) scripts in one document.".to_owned(),
        );
    }

    report
}

#[allow(clippy::too_many_arguments)]
fn classify_script_language(
    lang: ScriptLanguage,
    location: &str,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
    has_avm1: &mut bool,
    has_avm2: &mut bool,
    has_web_scripts: &mut bool,
    report: &mut FlaCompatibilityReport,
) {
    match lang {
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
            *has_avm1 = true;
            let disp = if matches!(profile, FlaTargetProfile::AnimateCC) {
                // Animate CC can still open XFL with AS2 frame/object scripts (with PublishSettings ActionScriptVersion=2)
                FlaFeatureDisposition::Exact
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "script_avm1",
                FlaFeatureCategory::Scripts,
                disp,
                location,
                format!("{:?} script preserved.", lang),
            );
        }
        ScriptLanguage::ActionScript3 => {
            *has_avm2 = true;
            if profile.supports_as3() {
                report.push(
                    "script_avm2",
                    FlaFeatureCategory::Scripts,
                    FlaFeatureDisposition::Exact,
                    location,
                    "ActionScript 3.0 script preserved.".to_owned(),
                );
            } else {
                report.push(
                    "script_avm2_in_avm1_profile",
                    FlaFeatureCategory::Scripts,
                    if options.strip_unsupported_scripts {
                        FlaFeatureDisposition::Omitted
                    } else {
                        FlaFeatureDisposition::Blocking
                    },
                    location,
                    format!(
                        "ActionScript 3.0 is not supported by {} (AVM1 only).",
                        profile.label()
                    ),
                );
            }
        }
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
            *has_web_scripts = true;
            report.push(
                "script_web_language",
                FlaFeatureCategory::Scripts,
                if options.strip_unsupported_scripts {
                    FlaFeatureDisposition::Omitted
                } else {
                    FlaFeatureDisposition::Blocking
                },
                location,
                format!(
                    "{:?} script is a NeoFlash web script and cannot run natively in Flash/Animate.",
                    lang
                ),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn preflight_layer(
    _sym: &Symbol,
    layer: &Layer,
    layer_loc: &str,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
    has_avm1: &mut bool,
    has_avm2: &mut bool,
    has_web_scripts: &mut bool,
    report: &mut FlaCompatibilityReport,
) {
    let props = &layer.properties;

    // Mask modes & invert
    if props.role == LayerRole::Mask && (props.mask_mode != MaskMode::Geometry || props.mask_invert)
    {
        report.push(
            "layer_advanced_mask_mode",
            FlaFeatureCategory::LayersAndMasks,
            if options.allow_approximations {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Blocking
            },
            layer_loc,
            format!(
                "Mask mode {:?} (invert={}) is approximated as a classic geometric mask in {}.",
                props.mask_mode,
                props.mask_invert,
                profile.label()
            ),
        );
    }

    // Layer-level filters & blend mode
    preflight_filter_stack(&props.filters, layer_loc, profile, report);
    if !props.filters.is_empty() || !props.blend_mode.is_normal() {
        let disp = if options.bake_unsupported_tweens || options.allow_approximations {
            FlaFeatureDisposition::Converted
        } else {
            FlaFeatureDisposition::Blocking
        };
        report.push(
            "layer_filters_or_blend",
            FlaFeatureCategory::FiltersAndBlends,
            disp,
            layer_loc,
            "Layer-level filters and blend mode are propagated to child display instances for Flash compatibility.".to_owned(),
        );
    }

    // Layer Z-depth & camera attachment
    if props.z_depth != 0.0 || props.attach_to_camera {
        let disp = if profile.supports_modern_layer_effects() {
            FlaFeatureDisposition::Approximated
        } else if options.bake_unsupported_tweens {
            FlaFeatureDisposition::Baked
        } else {
            FlaFeatureDisposition::Approximated
        };
        report.push(
            "layer_z_depth_camera",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            layer_loc,
            "Layer Z-depth / camera attachment inspected for target profile.".to_owned(),
        );
    }

    // Motion guide link
    if props.motion_guide.is_some() {
        let disp = if options.bake_unsupported_tweens && profile.is_legacy_binary() {
            FlaFeatureDisposition::Baked
        } else {
            FlaFeatureDisposition::Exact
        };
        report.push(
            "layer_motion_guide",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            layer_loc,
            "Motion guide layer link inspected.".to_owned(),
        );
    }

    for kf in &layer.keyframes {
        let kf_loc = format!("{}:frame[{}]", layer_loc, kf.frame);
        preflight_filter_stack(&kf.filters, &kf_loc, profile, report);
        for el in &kf.elements {
            let effective_filter_count = props.filters.len() + kf.filters.len() + el.filters.len();
            if effective_filter_count > neoflash_core::MAX_OBJECT_FILTERS {
                report.push(
                    "effective_filter_stack_too_large",
                    FlaFeatureCategory::FiltersAndBlends,
                    FlaFeatureDisposition::Blocking,
                    format!("{kf_loc}:element:{}", el.id),
                    format!(
                        "Element, keyframe and layer filters combine to {effective_filter_count}; the portable display stack supports at most {}. Export is blocked instead of truncating the stack.",
                        neoflash_core::MAX_OBJECT_FILTERS
                    ),
                );
            }
        }
        preflight_keyframe(
            kf,
            &kf_loc,
            profile,
            options,
            has_avm1,
            has_avm2,
            has_web_scripts,
            report,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn preflight_keyframe(
    kf: &Keyframe,
    kf_loc: &str,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
    has_avm1: &mut bool,
    has_avm2: &mut bool,
    has_web_scripts: &mut bool,
    report: &mut FlaCompatibilityReport,
) {
    // Tween & easing inspection
    if let Some(easing) = kf.tween {
        match kf.tween_kind {
            TweenKind::Classic => {
                let ease_disp = match easing {
                    Easing::Linear | Easing::EaseIn | Easing::EaseOut | Easing::Flash { .. } => {
                        FlaFeatureDisposition::Exact
                    }
                    Easing::EaseInOut | Easing::CubicBezier { .. } => {
                        if profile.is_legacy_binary() {
                            if options.bake_unsupported_tweens {
                                FlaFeatureDisposition::Baked
                            } else {
                                FlaFeatureDisposition::Approximated
                            }
                        } else {
                            FlaFeatureDisposition::Exact
                        }
                    }
                    Easing::Bounce | Easing::Elastic => {
                        if matches!(
                            profile,
                            FlaTargetProfile::AnimateCC | FlaTargetProfile::UncompressedXfl
                        ) {
                            FlaFeatureDisposition::Exact
                        } else if options.bake_unsupported_tweens {
                            FlaFeatureDisposition::Baked
                        } else {
                            FlaFeatureDisposition::Approximated
                        }
                    }
                };
                report.push(
                    "classic_tween",
                    FlaFeatureCategory::AnimationAndRigging,
                    ease_disp,
                    kf_loc,
                    format!("Classic tween with {:?} easing.", easing),
                );
            }
            TweenKind::Shape => {
                let disp = if profile.is_legacy_binary() && options.bake_unsupported_tweens {
                    FlaFeatureDisposition::Baked
                } else {
                    FlaFeatureDisposition::Exact
                };
                report.push(
                    "shape_tween",
                    FlaFeatureCategory::AnimationAndRigging,
                    disp,
                    kf_loc,
                    "Shape tween span inspected.".to_owned(),
                );
            }
            TweenKind::Motion => {
                let disp = if options.bake_unsupported_tweens {
                    FlaFeatureDisposition::Baked
                } else if options.allow_approximations {
                    FlaFeatureDisposition::Approximated
                } else {
                    FlaFeatureDisposition::Blocking
                };
                report.push(
                    "motion_tween_span",
                    FlaFeatureCategory::AnimationAndRigging,
                    disp,
                    kf_loc,
                    "Property-track Motion tween span is baked into frame-by-frame keyframes for portable round-trip.".to_owned(),
                );
            }
        }
    }

    // Rigging / IK / Asset Warp / Camera / Parenting / Appearances / Vector Effects / Expressions
    if !kf.ik_armatures.is_empty() {
        let disp = if options.bake_unsupported_tweens {
            FlaFeatureDisposition::Baked
        } else {
            FlaFeatureDisposition::Blocking
        };
        report.push(
            "ik_armature",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            kf_loc,
            "IK bone armature poses are baked into keyframe element transforms.".to_owned(),
        );
    }

    if !kf.asset_warps.is_empty() {
        let disp = if options.bake_unsupported_tweens {
            FlaFeatureDisposition::Baked
        } else {
            FlaFeatureDisposition::Blocking
        };
        report.push(
            "asset_warp_rig",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            kf_loc,
            "Asset Warp / Puppet pin deformations are baked into vector contours.".to_owned(),
        );
    }

    if kf.camera.is_some() {
        let disp = if options.bake_unsupported_tweens {
            FlaFeatureDisposition::Baked
        } else if profile.supports_modern_layer_effects() {
            FlaFeatureDisposition::Approximated
        } else {
            FlaFeatureDisposition::Blocking
        };
        report.push(
            "camera_pose",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            kf_loc,
            "Timeline camera pan/zoom/rotation is baked into stage transforms.".to_owned(),
        );
    }

    if !kf.element_parents.is_empty()
        || !kf.appearances.is_empty()
        || !kf.vector_effects.is_empty()
        || !kf.property_expressions.is_empty()
    {
        let disp = if options.bake_unsupported_tweens {
            FlaFeatureDisposition::Baked
        } else {
            FlaFeatureDisposition::Approximated
        };
        report.push(
            "advanced_keyframe_modifiers",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            kf_loc,
            "Element parenting, appearances, vector effects, or expressions are baked into concrete elements.".to_owned(),
        );
    }

    // Frame sound
    if let Some(snd) = &kf.sound {
        let disp = if (snd.envelope.is_some() || snd.in_point.is_some() || snd.out_point.is_some())
            && profile.is_legacy_binary()
        {
            FlaFeatureDisposition::Approximated
        } else {
            FlaFeatureDisposition::Exact
        };
        report.push(
            "frame_sound",
            FlaFeatureCategory::MediaAndSound,
            disp,
            kf_loc,
            format!("Frame sound ({:?}) inspected.", snd.sync),
        );
    }

    if kf.video.is_some() {
        report.push(
            "timeline_video_not_serialized",
            FlaFeatureCategory::MediaAndSound,
            if options.allow_approximations {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Blocking
            },
            kf_loc,
            "Timeline video playback, deployment mode and cue points are not serialized by the current FLA/XFL writer; approximation keeps only authored viewport artwork."
                .to_owned(),
        );
    }

    // Elements
    for el in &kf.elements {
        let el_loc = format!("{}/Element({})", kf_loc, el.id);
        preflight_element(
            el,
            &el_loc,
            profile,
            options,
            has_avm1,
            has_avm2,
            has_web_scripts,
            report,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn preflight_element(
    el: &Element,
    el_loc: &str,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
    has_avm1: &mut bool,
    has_avm2: &mut bool,
    has_web_scripts: &mut bool,
    report: &mut FlaCompatibilityReport,
) {
    // Object scripts
    for os in &el.scripts {
        let os_loc = format!("{}:on({:?})", el_loc, os.event);
        classify_script_language(
            os.script.language,
            &os_loc,
            profile,
            options,
            has_avm1,
            has_avm2,
            has_web_scripts,
            report,
        );
    }

    // Filters & Blend modes
    preflight_filter_stack(&el.filters, el_loc, profile, report);
    if !el.filters.is_empty() || !el.blend_mode.is_normal() {
        let is_instance_or_text = matches!(
            el.kind,
            ElementKind::Instance { .. } | ElementKind::Text { .. } | ElementKind::TextBox { .. }
        );
        let disp = if profile.is_legacy_binary() && !is_instance_or_text {
            FlaFeatureDisposition::Approximated
        } else {
            FlaFeatureDisposition::Exact
        };
        report.push(
            "element_filters_or_blend",
            FlaFeatureCategory::FiltersAndBlends,
            disp,
            el_loc,
            "Display filters / blend mode inspected.".to_owned(),
        );
    }

    // Variable-width stroke profile
    if !el.stroke_width_profile().is_default() {
        let disp = if profile.supports_variable_width_strokes() && !profile.is_legacy_binary() {
            FlaFeatureDisposition::Converted
        } else {
            FlaFeatureDisposition::Baked
        };
        report.push(
            "variable_width_stroke",
            FlaFeatureCategory::ShapesAndPaints,
            disp,
            el_loc,
            "Variable-width stroke profile expanded to filled vector contour.".to_owned(),
        );
    }

    match &el.kind {
        ElementKind::Rectangle { .. } | ElementKind::Ellipse { .. } => {
            let disp = if el.has_parametric_options() && profile.is_legacy_binary() {
                FlaFeatureDisposition::Converted
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "primitive_shape",
                FlaFeatureCategory::ShapesAndPaints,
                disp,
                el_loc,
                "Vector primitive shape inspected.".to_owned(),
            );
        }
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            let has_cubic = commands
                .iter()
                .any(|c| matches!(c, PathCommand::CubicTo { .. }));
            let disp = if has_cubic {
                FlaFeatureDisposition::Converted
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "vector_path",
                FlaFeatureCategory::ShapesAndPaints,
                disp,
                el_loc,
                if has_cubic {
                    "Cubic Bezier segments converted to quadratic curves for Flash/XFL.".to_owned()
                } else {
                    "Quadratic/linear vector path preserved.".to_owned()
                },
            );
        }
        ElementKind::Text { .. } | ElementKind::TextBox { .. } => {
            let disp = if options.outline_unsupported_text
                || options.font_policy == FontExportPolicy::Outlines
            {
                FlaFeatureDisposition::Outlined
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "text_element",
                FlaFeatureCategory::TextAndFonts,
                disp,
                el_loc,
                if disp == FlaFeatureDisposition::Outlined {
                    "Text element converted to vector glyph outlines.".to_owned()
                } else {
                    "Text element preserved.".to_owned()
                },
            );
        }
        ElementKind::Bitmap { .. } => {
            report.push(
                "bitmap_instance",
                FlaFeatureCategory::MediaAndSound,
                FlaFeatureDisposition::Exact,
                el_loc,
                "Bitmap instance preserved.".to_owned(),
            );
        }
        ElementKind::Video { .. } => {
            report.push(
                "video_instance",
                FlaFeatureCategory::MediaAndSound,
                if options.allow_approximations {
                    FlaFeatureDisposition::Approximated
                } else {
                    FlaFeatureDisposition::Blocking
                },
                el_loc,
                "Video media is not serialized by the current FLA/XFL writer; approximation keeps only a viewport placeholder."
                    .to_owned(),
            );
        }
        ElementKind::Instance { .. } => {
            report.push(
                "symbol_instance",
                FlaFeatureCategory::SymbolsAndLibrary,
                FlaFeatureDisposition::Exact,
                el_loc,
                "Library symbol instance preserved.".to_owned(),
            );
        }
    }
}

/// Prepare a derived copy of `project` tailored to `profile` and `options`, returning
/// both the derived `Project` and its `FlaCompatibilityReport`. Never mutates `project`.
pub fn prepare_project_for_fla_profile(
    project: &Project,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
) -> (Project, FlaCompatibilityReport) {
    let report = preflight_fla_export_with_options(project, profile, options);
    let mut derived = project.clone();

    // 1. Clamp stage dimensions if needed
    let max_dim = if matches!(profile, FlaTargetProfile::LegacyFlash8) {
        2880
    } else {
        8192
    };
    derived.width = derived.width.clamp(1, max_dim);
    derived.height = derived.height.clamp(1, max_dim);
    derived.fps = derived.fps.clamp(1, 120);

    // 2. Strip unsupported scripts if requested
    if options.strip_unsupported_scripts {
        let allow_as3 = profile.supports_as3();
        derived.sources.retain(|s| {
            matches!(
                s.language,
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
            ) || (allow_as3 && s.language == ScriptLanguage::ActionScript3)
        });
        if !allow_as3 {
            derived.document_class = None;
        }
        for sym in &mut derived.symbols {
            if !allow_as3 && sym.linkage_id.is_none() && sym.class_name.is_some() {
                sym.linkage_id = sym.class_name.take();
            }
            sym.timeline.scripts.retain(|fs| {
                matches!(
                    fs.script.language,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) || (allow_as3 && fs.script.language == ScriptLanguage::ActionScript3)
            });
            for layer in &mut sym.timeline.layers {
                for kf in &mut layer.keyframes {
                    for el in &mut kf.elements {
                        el.scripts.retain(|os| {
                            matches!(
                                os.script.language,
                                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                            ) || (allow_as3 && os.script.language == ScriptLanguage::ActionScript3)
                        });
                    }
                }
            }
        }
    } else if !profile.supports_as3() {
        for sym in &mut derived.symbols {
            if sym.linkage_id.is_none() && sym.class_name.is_some() {
                sym.linkage_id = sym.class_name.take();
            }
        }
    }

    // 3. Apply font policy / text outlining on the derived project
    if options.outline_unsupported_text || options.font_policy == FontExportPolicy::Outlines {
        convert_project_text_to_outlines(&mut derived);
        outline_remaining_textboxes(&mut derived);
    } else if options.font_policy != FontExportPolicy::Device {
        let _ = apply_font_policy(&mut derived, options.font_policy);
    }

    // 4. Bake non-classic animation spans & modifiers when requested
    if options.bake_unsupported_tweens {
        bake_derived_timelines(&mut derived, profile);
    }

    // 5. Propagate layer filters/blends & expand parametric/variable-width strokes
    normalize_derived_elements(&mut derived, profile);

    (derived, report)
}

fn outline_remaining_textboxes(project: &mut Project) {
    for sym in &mut project.symbols {
        for layer in &mut sym.timeline.layers {
            for kf in &mut layer.keyframes {
                for el in &mut kf.elements {
                    if matches!(el.kind, ElementKind::TextBox { .. })
                        && let Some(field) = neoflash_core::TextFieldState::from_kind(&el.kind)
                    {
                        let commands = field.to_outline_commands(None);
                        let rgb = field.format.color;
                        let fill = [
                            ((rgb >> 16) & 0xFF) as u8,
                            ((rgb >> 8) & 0xFF) as u8,
                            (rgb & 0xFF) as u8,
                            field.alpha,
                        ];
                        el.kind = ElementKind::Path {
                            commands,
                            fill_rule: neoflash_core::FillRule::NonZero,
                            fill,
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        };
                    }
                }
            }
        }
    }
}

fn layer_needs_baking(layer: &Layer, timeline: &Timeline, profile: FlaTargetProfile) -> bool {
    if layer.properties.z_depth != 0.0 || layer.properties.attach_to_camera {
        return true;
    }
    if timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .any(|k| k.camera.is_some())
    {
        return true;
    }
    if profile.is_legacy_binary() && layer.properties.motion_guide.is_some() {
        return true;
    }
    for kf in &layer.keyframes {
        if kf.tween.is_some() && kf.tween_kind == TweenKind::Motion {
            return true;
        }
        if kf.tween.is_some() && kf.tween_kind == TweenKind::Shape && profile.is_legacy_binary() {
            return true;
        }
        if let Some(easing) = kf.tween
            && matches!(easing, Easing::Bounce | Easing::Elastic)
            && !matches!(
                profile,
                FlaTargetProfile::AnimateCC | FlaTargetProfile::UncompressedXfl
            )
        {
            return true;
        }
        if !kf.ik_armatures.is_empty()
            || !kf.asset_warps.is_empty()
            || !kf.element_parents.is_empty()
            || !kf.appearances.is_empty()
            || !kf.vector_effects.is_empty()
            || !kf.property_expressions.is_empty()
        {
            return true;
        }
    }
    false
}

fn bake_derived_timelines(project: &mut Project, profile: FlaTargetProfile) {
    for sym in &mut project.symbols {
        let timeline_snapshot = sym.timeline.clone();
        let frame_count = timeline_snapshot.frame_count.max(1);
        for (li, layer) in sym.timeline.layers.iter_mut().enumerate() {
            if matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide) {
                continue;
            }
            if !layer_needs_baking(layer, &timeline_snapshot, profile) {
                continue;
            }
            let orig_layer = &timeline_snapshot.layers[li];
            let max_frame = orig_layer
                .properties
                .end_frame
                .unwrap_or(frame_count)
                .min(frame_count);
            if max_frame <= 1 && orig_layer.keyframes.len() <= 1 {
                if let Some(kf) = layer.keyframes.first_mut() {
                    kf.elements = sample_layer_in(&timeline_snapshot, orig_layer, 0);
                    clear_baked_keyframe_fields(kf);
                }
                continue;
            }
            let mut baked_keyframes = Vec::with_capacity(max_frame as usize);
            for f in 0..max_frame {
                let sampled_elements = sample_layer_in(&timeline_snapshot, orig_layer, f);
                let orig_kf = orig_layer.keyframes.iter().find(|k| k.frame == f);
                let mut kf = Keyframe {
                    frame: f,
                    elements: sampled_elements,
                    tween: None,
                    tween_kind: TweenKind::Classic,
                    label: orig_kf.map(|k| k.label.clone()).unwrap_or_default(),
                    sound: orig_kf.and_then(|k| k.sound.clone()),
                    ..Default::default()
                };
                clear_baked_keyframe_fields(&mut kf);
                baked_keyframes.push(kf);
            }
            layer.keyframes = baked_keyframes;
            if profile.is_legacy_binary() {
                layer.properties.motion_guide = None;
            }
            layer.properties.z_depth = 0.0;
            layer.properties.attach_to_camera = false;
        }
    }
}

fn clear_baked_keyframe_fields(kf: &mut Keyframe) {
    kf.tween = None;
    kf.tween_kind = TweenKind::Classic;
    kf.keyed_properties = None;
    kf.motion_keys = None;
    kf.motion_tracks.clear();
    kf.motion_path = None;
    kf.property_eases = Vec::new();
    kf.roving = false;
    kf.orient_to_path = false;
    kf.ik_armatures.clear();
    kf.asset_warps.clear();
    kf.camera = None;
    kf.element_parents.clear();
    kf.appearances.clear();
    kf.vector_effects.clear();
    kf.property_expressions.clear();
}

fn normalize_derived_elements(project: &mut Project, profile: FlaTargetProfile) {
    for sym in &mut project.symbols {
        for layer in &mut sym.timeline.layers {
            let layer_blend = layer.properties.blend_mode;
            let layer_filters = std::mem::take(&mut layer.properties.filters);
            layer.properties.blend_mode = BlendMode::Normal;
            if layer.properties.role == LayerRole::Mask {
                layer.properties.mask_mode = MaskMode::Geometry;
                layer.properties.mask_invert = false;
            }
            for kf in &mut layer.keyframes {
                let keyframe_filters = std::mem::take(&mut kf.filters);
                for el in &mut kf.elements {
                    if el.blend_mode.is_normal() && !layer_blend.is_normal() {
                        el.blend_mode = layer_blend;
                    }
                    el.filters.extend(keyframe_filters.iter().cloned());
                    el.filters.extend(layer_filters.iter().cloned());
                    // Expand variable-width stroke profiles or legacy parametric primitives
                    if (!el.stroke_width_profile().is_default()
                        || (profile.is_legacy_binary() && el.has_parametric_options()))
                        && let Some(expanded_kind) = el.parametric_path_kind()
                    {
                        el.kind = expanded_kind;
                        el.transform.primitive = None;
                    }
                }
            }
        }
    }
}

fn remove_path_if_exists(path: &Path) -> std::io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn replace_staged_path_recoverably_with(
    staged_path: &Path,
    target_path: &Path,
    backup_path: &Path,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    if fs::symlink_metadata(backup_path).is_ok() {
        let _ = remove_path_if_exists(staged_path);
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "refusing to replace '{}' because backup path '{}' already exists",
                target_path.display(),
                backup_path.display()
            ),
        ));
    }

    let target_exists = match fs::symlink_metadata(target_path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            let _ = remove_path_if_exists(staged_path);
            return Err(error);
        }
    };
    if !target_exists {
        let result = rename(staged_path, target_path);
        if result.is_err() {
            let _ = remove_path_if_exists(staged_path);
        }
        return result;
    }

    if let Err(error) = rename(target_path, backup_path) {
        let _ = remove_path_if_exists(staged_path);
        return Err(error);
    }

    match rename(staged_path, target_path) {
        Ok(()) => {
            if let Err(error) = remove_path_if_exists(backup_path) {
                return Err(std::io::Error::new(
                    error.kind(),
                    format!(
                        "replaced '{}', but could not remove recoverable backup '{}': {error}",
                        target_path.display(),
                        backup_path.display()
                    ),
                ));
            }
            Ok(())
        }
        Err(install_error) => {
            let rollback_result = rename(backup_path, target_path);
            let cleanup_result = remove_path_if_exists(staged_path);
            if let Err(rollback_error) = rollback_result {
                return Err(std::io::Error::new(
                    rollback_error.kind(),
                    format!(
                        "could not install staged XFL at '{}': {install_error}; rollback also failed, but the previous document remains at '{}': {rollback_error}",
                        target_path.display(),
                        backup_path.display()
                    ),
                ));
            }
            if let Err(cleanup_error) = cleanup_result {
                return Err(std::io::Error::new(
                    cleanup_error.kind(),
                    format!(
                        "could not install staged XFL at '{}': {install_error}; the previous document was restored, but temporary '{}' could not be removed: {cleanup_error}",
                        target_path.display(),
                        staged_path.display()
                    ),
                ));
            }
            Err(install_error)
        }
    }
}

fn replace_staged_path_recoverably(
    staged_path: &Path,
    target_path: &Path,
    backup_path: &Path,
) -> std::io::Result<()> {
    replace_staged_path_recoverably_with(staged_path, target_path, backup_path, |from, to| {
        fs::rename(from, to)
    })
}

/// Export `project` to `path` using `profile` and `options`, verifying the written artifact
/// via `neoflash_import::import_project` before atomically replacing `path` (`T-188`).
pub fn export_fla_with_profile(
    project: &Project,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
    path: impl AsRef<Path>,
) -> Result<FlaCompatibilityReport, ExportError> {
    let target_path = path.as_ref();
    let (derived, report) = prepare_project_for_fla_profile(project, profile, options);
    if report.has_blocking() {
        let first = report.blocking_items()[0];
        return Err(ExportError::UnsupportedScript(format!(
            "FLA export blocked for {} at {}: {}",
            profile.label(),
            first.location,
            first.message
        )));
    }

    let parent_dir = target_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent_dir)?;

    let stem = target_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Document");
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    if profile.is_directory() {
        let temp_dir = parent_dir.join(format!(".{stem}.xfl-tmp-{pid}-{nanos}"));
        let backup_path = parent_dir.join(format!(".{stem}.xfl-backup-{pid}-{nanos}"));
        remove_path_if_exists(&temp_dir)?;
        remove_path_if_exists(&backup_path)?;
        if let Err(error) = write_uncompressed_xfl_dir(&derived, profile, stem, &temp_dir) {
            let _ = remove_path_if_exists(&temp_dir);
            return Err(error);
        }
        if let Err(error) = verify_reimported_fla(&temp_dir, profile) {
            let _ = remove_path_if_exists(&temp_dir);
            return Err(error);
        }
        replace_staged_path_recoverably(&temp_dir, target_path, &backup_path)?;
        return Ok(report);
    }

    let ext = target_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("fla");
    let temp_file = parent_dir.join(format!(".{stem}.tmp-{pid}-{nanos}.{ext}"));
    let bytes = if profile.is_legacy_binary() {
        crate::legacy_fla::write_legacy_binary_fla_from_prepared(&derived, profile)?
    } else {
        write_zipped_xfl_bytes(&derived, profile, stem)?
    };
    fs::write(&temp_file, &bytes)?;
    if let Err(err) = verify_reimported_fla(&temp_file, profile) {
        let _ = fs::remove_file(&temp_file);
        return Err(err);
    }
    fs::rename(&temp_file, target_path)?;
    Ok(report)
}

/// Export `project` to in-memory bytes using `profile` and `options` (for `.fla` archives and
/// legacy binary `.fla` streams), verifying the bytes through `neoflash_import::import_fla_bytes`.
pub fn export_fla_bytes_with_profile(
    project: &Project,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
) -> Result<(Vec<u8>, FlaCompatibilityReport), ExportError> {
    let (derived, report) = prepare_project_for_fla_profile(project, profile, options);
    if report.has_blocking() {
        let first = report.blocking_items()[0];
        return Err(ExportError::UnsupportedScript(format!(
            "FLA export blocked for {} at {}: {}",
            profile.label(),
            first.location,
            first.message
        )));
    }
    let bytes = if profile.is_legacy_binary() {
        crate::legacy_fla::write_legacy_binary_fla_from_prepared(&derived, profile)?
    } else {
        write_zipped_xfl_bytes(&derived, profile, &derived.name)?
    };
    let _reimported = crate::legacy_fla::import_fla_bytes(&bytes)?;
    Ok((bytes, report))
}

fn verify_reimported_fla(path: &Path, profile: FlaTargetProfile) -> Result<(), ExportError> {
    neoflash_import::import_project(path).map_err(|e| {
        ExportError::InvalidProject(format!(
            "Generated {} at {} failed round-trip import verification: {e}",
            profile.label(),
            path.display()
        ))
    })?;
    Ok(())
}

struct XflPackage {
    text_files: BTreeMap<String, String>,
    binary_files: BTreeMap<String, Vec<u8>>,
}

fn write_uncompressed_xfl_dir(
    project: &Project,
    profile: FlaTargetProfile,
    stem: &str,
    dir: &Path,
) -> Result<(), ExportError> {
    let pkg = build_xfl_package(project, profile, stem)?;
    fs::create_dir_all(dir)?;
    for (rel, content) in &pkg.text_files {
        let full = dir.join(rel);
        if let Some(p) = full.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(full, content.as_bytes())?;
    }
    for (rel, bytes) in &pkg.binary_files {
        let full = dir.join(rel);
        if let Some(p) = full.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(full, bytes)?;
    }
    Ok(())
}

fn write_zipped_xfl_bytes(
    project: &Project,
    profile: FlaTargetProfile,
    stem: &str,
) -> Result<Vec<u8>, ExportError> {
    let pkg = build_xfl_package(project, profile, stem)?;
    let cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for (rel, content) in &pkg.text_files {
        zip.start_file(rel, opts)
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        zip.write_all(content.as_bytes())?;
    }
    for (rel, bytes) in &pkg.binary_files {
        zip.start_file(rel, opts)
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        zip.write_all(bytes)?;
    }
    let finished = zip
        .finish()
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
    Ok(finished.into_inner())
}

fn detect_project_script_language(project: &Project, profile: FlaTargetProfile) -> ScriptLanguage {
    for src in &project.sources {
        if matches!(
            src.language,
            ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3
        ) {
            if src.language == ScriptLanguage::ActionScript3 && !profile.supports_as3() {
                return ScriptLanguage::ActionScript2;
            }
            return src.language;
        }
    }
    for sym in &project.symbols {
        for fs in &sym.timeline.scripts {
            if fs.script.language == ScriptLanguage::ActionScript3 && profile.supports_as3() {
                return ScriptLanguage::ActionScript3;
            }
            if matches!(
                fs.script.language,
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
            ) {
                return fs.script.language;
            }
        }
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    for os in &el.scripts {
                        if os.script.language == ScriptLanguage::ActionScript3
                            && profile.supports_as3()
                        {
                            return ScriptLanguage::ActionScript3;
                        }
                        if matches!(
                            os.script.language,
                            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                        ) {
                            return os.script.language;
                        }
                    }
                }
            }
        }
    }
    if profile.supports_as3() {
        ScriptLanguage::ActionScript3
    } else {
        ScriptLanguage::ActionScript2
    }
}

fn build_xfl_package(
    project: &Project,
    profile: FlaTargetProfile,
    stem: &str,
) -> Result<XflPackage, ExportError> {
    let mut text_files = BTreeMap::new();
    let mut binary_files = BTreeMap::new();

    let clean_stem: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let marker_stem = if clean_stem.is_empty() {
        "Document"
    } else {
        &clean_stem
    };
    text_files.insert(format!("{marker_stem}.xfl"), "PROXY-CS5\n".to_owned());

    // META-INF/metadata.xml
    let metadata_xml = format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/">
      <xmp:CreatorTool>{}</xmp:CreatorTool>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#,
        xml_escape(profile.creator_info())
    );
    text_files.insert("META-INF/metadata.xml".to_owned(), metadata_xml);

    // PublishSettings.xml
    let script_lang = detect_project_script_language(project, profile);
    let as_version_num = match script_lang {
        ScriptLanguage::ActionScript1 => "1",
        ScriptLanguage::ActionScript2 => "2",
        _ => "3",
    };
    let doc_class_xml = if as_version_num == "3"
        && let Some(dc) = project
            .document_class
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        format!("\n      <DocumentClass>{}</DocumentClass>", xml_escape(dc))
    } else {
        String::new()
    };
    let publish_xml = format!(
        r#"<flash_profiles>
  <flash_profile version="1.0" name="Default" current="true">
    <PublishFlashProperties enabled="true">
      <ActionScriptVersion>{as_version_num}</ActionScriptVersion>
      <InvisibleLayer>1</InvisibleLayer>{doc_class_xml}
    </PublishFlashProperties>
  </flash_profile>
</flash_profiles>
"#
    );
    text_files.insert("PublishSettings.xml".to_owned(), publish_xml);

    // Bundled .as / .js / .ts source units
    for (idx, src) in project.sources.iter().enumerate() {
        let rel_path = sanitize_source_rel_path(&src.path, idx);
        let marker = match src.language {
            ScriptLanguage::JavaScript => "/* neoflash:language=javascript */\n",
            ScriptLanguage::TypeScript => "/* neoflash:language=typescript */\n",
            ScriptLanguage::ActionScript1 => "/* neoflash:language=actionscript1 */\n",
            ScriptLanguage::ActionScript2 => "/* neoflash:language=actionscript2 */\n",
            ScriptLanguage::ActionScript3 => "/* neoflash:language=actionscript3 */\n",
        };
        text_files.insert(rel_path, format!("{marker}{}", src.source));
    }

    // Collect scene symbol IDs vs library symbols
    let scene_ids: Vec<Id> = if project.scenes.is_empty() {
        vec![project.root.clone()]
    } else {
        project.scenes.clone()
    };
    let scene_set: BTreeSet<&str> = scene_ids.iter().map(String::as_str).collect();

    // Assign unique library item names for non-scene symbols
    let mut symbol_lib_names: BTreeMap<Id, String> = BTreeMap::new();
    let mut used_lib_names: BTreeSet<String> = BTreeSet::new();
    for sym in &project.symbols {
        if scene_set.contains(sym.id.as_str()) {
            continue;
        }
        let base_name = sanitize_xfl_item_name(&sym.name, "Symbol");
        let full_name = if sym.folder.trim().is_empty() {
            base_name
        } else {
            format!("{}/{}", sym.folder.trim_matches('/'), base_name)
        };
        let mut candidate = full_name.clone();
        let mut suffix = 2usize;
        while !used_lib_names.insert(candidate.clone()) {
            candidate = format!("{full_name}_{suffix}");
            suffix += 1;
        }
        symbol_lib_names.insert(sym.id.clone(), candidate);
    }

    // Media resources (bitmaps & sounds)
    let mut bitmap_names: BTreeMap<Id, String> = BTreeMap::new();
    let mut sound_names: BTreeMap<Id, String> = BTreeMap::new();
    let mut media_xml = String::new();
    for (idx, res) in project.resources.iter().enumerate() {
        let Ok(bytes) = res.decoded_bytes() else {
            continue;
        };
        let mime = res.media_type.to_ascii_lowercase();
        if mime.starts_with("image/") {
            let ext = if mime.contains("jpeg") || mime.contains("jpg") {
                "jpg"
            } else {
                "png"
            };
            let item_name = format!("bitmap_{}.{ext}", idx + 1);
            bitmap_names.insert(res.id.clone(), item_name.clone());
            binary_files.insert(format!("LIBRARY/{item_name}"), bytes);
            media_xml.push_str(&format!(
                "    <DOMBitmapItem name=\"{}\" href=\"{}\" allowSmoothing=\"true\"/>\n",
                xml_escape(&item_name),
                xml_escape(&item_name)
            ));
        } else if mime.starts_with("audio/") {
            let ext = if mime.contains("mpeg") || mime.contains("mp3") {
                "mp3"
            } else {
                "wav"
            };
            let item_name = format!("sound_{}.{ext}", idx + 1);
            sound_names.insert(res.id.clone(), item_name.clone());
            binary_files.insert(format!("LIBRARY/{item_name}"), bytes);
            media_xml.push_str(&format!(
                "    <DOMSoundItem name=\"{}\" href=\"{}\" format=\"44kHz 16bit Stereo\"/>\n",
                xml_escape(&item_name),
                xml_escape(&item_name)
            ));
        }
    }

    // Write LIBRARY/*.xml for each library symbol
    let mut includes_xml = String::new();
    for sym in &project.symbols {
        let Some(lib_name) = symbol_lib_names.get(&sym.id) else {
            continue;
        };
        let href = format!("{lib_name}.xml");
        includes_xml.push_str(&format!(
            "    <Include href=\"{}\" loadImmediate=\"false\"/>\n",
            xml_escape(&href)
        ));
        let sym_xml = write_dom_symbol_item_xml(
            sym,
            lib_name,
            project,
            &symbol_lib_names,
            &bitmap_names,
            &sound_names,
        );
        text_files.insert(format!("LIBRARY/{href}"), sym_xml);
    }

    // Folders XML
    let mut all_folders: BTreeSet<String> = BTreeSet::new();
    for f in &project.library_folders {
        let norm = f.trim().trim_matches('/');
        if !norm.is_empty() {
            all_folders.insert(norm.to_owned());
        }
    }
    for sym in &project.symbols {
        if !scene_set.contains(sym.id.as_str()) {
            let norm = sym.folder.trim().trim_matches('/');
            if !norm.is_empty() {
                all_folders.insert(norm.to_owned());
            }
        }
    }
    let folders_section = if all_folders.is_empty() {
        String::new()
    } else {
        let mut s = String::from("  <folders>\n");
        for folder in &all_folders {
            s.push_str(&format!(
                "    <DOMFolderItem name=\"{}\" isExpanded=\"true\"/>\n",
                xml_escape(folder)
            ));
        }
        s.push_str("  </folders>\n");
        s
    };

    let media_section = if media_xml.is_empty() {
        String::new()
    } else {
        format!("  <media>\n{media_xml}  </media>\n")
    };

    let symbols_section = if includes_xml.is_empty() {
        String::new()
    } else {
        format!("  <symbols>\n{includes_xml}  </symbols>\n")
    };

    // Scene timelines XML
    let mut timelines_xml = String::from("  <timelines>\n");
    for (scene_idx, scene_id) in scene_ids.iter().enumerate() {
        if let Some(scene_sym) = project.symbols.iter().find(|s| &s.id == scene_id) {
            let scene_name = if scene_sym.name.trim().is_empty() {
                format!("Scene {}", scene_idx + 1)
            } else {
                scene_sym.name.clone()
            };
            timelines_xml.push_str(&write_dom_timeline_xml(
                &scene_sym.timeline,
                &scene_name,
                project,
                &symbol_lib_names,
                &bitmap_names,
                &sound_names,
                "    ",
            ));
        }
    }
    timelines_xml.push_str("  </timelines>\n");

    let bg_hex = format!(
        "#{:02X}{:02X}{:02X}",
        project.background[0], project.background[1], project.background[2]
    );
    let doc_class_attr = project
        .document_class
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|dc| format!(" documentClass=\"{}\"", xml_escape(dc)))
        .unwrap_or_default();
    let dom_document_xml = format!(
        r#"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="{}" height="{}" frameRate="{}" backgroundColor="{}" xflVersion="{}" creatorInfo="{}"{}>
{}{}{}{}</DOMDocument>
"#,
        project.width.max(1),
        project.height.max(1),
        project.fps.max(1),
        bg_hex,
        profile.xfl_version(),
        xml_escape(profile.creator_info()),
        doc_class_attr,
        folders_section,
        media_section,
        symbols_section,
        timelines_xml
    );
    text_files.insert("DOMDocument.xml".to_owned(), dom_document_xml);

    Ok(XflPackage {
        text_files,
        binary_files,
    })
}

fn write_dom_symbol_item_xml(
    sym: &Symbol,
    lib_name: &str,
    project: &Project,
    symbol_lib_names: &BTreeMap<Id, String>,
    bitmap_names: &BTreeMap<Id, String>,
    sound_names: &BTreeMap<Id, String>,
) -> String {
    let sym_type = match sym.kind {
        SymbolKind::MovieClip => "movie clip",
        SymbolKind::Graphic => "graphic",
        SymbolKind::Button => "button",
    };
    let mut linkage_attrs = String::new();
    let has_class = sym
        .class_name
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let has_id = sym
        .linkage_id
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    if has_class || has_id {
        linkage_attrs.push_str(" linkageExportForAS=\"true\" linkageExportInFirstFrame=\"true\"");
        if let Some(cls) = sym.class_name.as_deref().filter(|s| !s.trim().is_empty()) {
            linkage_attrs.push_str(&format!(" linkageClassName=\"{}\"", xml_escape(cls.trim())));
        }
        if let Some(lid) = sym.linkage_id.as_deref().filter(|s| !s.trim().is_empty()) {
            linkage_attrs.push_str(&format!(
                " linkageIdentifier=\"{}\"",
                xml_escape(lid.trim())
            ));
        }
    }
    let inner_name = lib_name.rsplit_once('/').map_or(lib_name, |(_, b)| b);
    let timeline_xml = write_dom_timeline_xml(
        &sym.timeline,
        inner_name,
        project,
        symbol_lib_names,
        bitmap_names,
        sound_names,
        "    ",
    );
    format!(
        r#"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="{}" symbolType="{}"{}>
  <timeline>
{}  </timeline>
</DOMSymbolItem>
"#,
        xml_escape(lib_name),
        sym_type,
        linkage_attrs,
        timeline_xml
    )
}

#[allow(clippy::too_many_arguments)]
fn write_dom_timeline_xml(
    timeline: &Timeline,
    name: &str,
    project: &Project,
    symbol_lib_names: &BTreeMap<Id, String>,
    bitmap_names: &BTreeMap<Id, String>,
    sound_names: &BTreeMap<Id, String>,
    indent: &str,
) -> String {
    let mut out = format!(
        "{indent}<DOMTimeline name=\"{}\">\n{indent}  <layers>\n",
        xml_escape(name)
    );

    // XFL lists topmost layers first; NeoFlash stores painter order (bottom-to-top).
    let xfl_layers: Vec<&Layer> = timeline.layers.iter().rev().collect();
    let id_to_xfl_idx: BTreeMap<&str, usize> = xfl_layers
        .iter()
        .enumerate()
        .map(|(i, l)| (l.id.as_str(), i))
        .collect();

    // Map frame scripts by frame number so they can be attached to the first normal layer
    let mut scripts_by_frame: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for fs in &timeline.scripts {
        let marker = match fs.script.language {
            ScriptLanguage::JavaScript => "/* neoflash:language=javascript */\n",
            ScriptLanguage::TypeScript => "/* neoflash:language=typescript */\n",
            ScriptLanguage::ActionScript1 => "/* neoflash:language=actionscript1 */\n",
            ScriptLanguage::ActionScript2 => "/* neoflash:language=actionscript2 */\n",
            ScriptLanguage::ActionScript3 => "/* neoflash:language=actionscript3 */\n",
        };
        scripts_by_frame
            .entry(fs.frame)
            .or_default()
            .push(format!("{marker}{}", fs.script.source));
    }

    let first_normal_idx = xfl_layers
        .iter()
        .position(|l| l.properties.role != LayerRole::Folder);

    if xfl_layers.is_empty() || (first_normal_idx.is_none() && !scripts_by_frame.is_empty()) {
        let synth_layer = Layer {
            id: "__nf_synth_actions_layer__".to_owned(),
            name: if scripts_by_frame.is_empty() {
                "Layer 1".to_owned()
            } else {
                "Actions".to_owned()
            },
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: Vec::new(),
                ..Default::default()
            }],
            properties: neoflash_core::LayerProperties::default(),
        };
        out.push_str(&write_dom_layer_xml(
            &synth_layer,
            timeline.frame_count.max(1),
            &id_to_xfl_idx,
            Some(&scripts_by_frame),
            project,
            symbol_lib_names,
            bitmap_names,
            sound_names,
            &format!("{indent}    "),
        ));
    }
    for (xfl_idx, layer) in xfl_layers.iter().copied().enumerate() {
        let attach_scripts = if Some(xfl_idx) == first_normal_idx {
            Some(&scripts_by_frame)
        } else {
            None
        };
        out.push_str(&write_dom_layer_xml(
            layer,
            timeline.frame_count.max(1),
            &id_to_xfl_idx,
            attach_scripts,
            project,
            symbol_lib_names,
            bitmap_names,
            sound_names,
            &format!("{indent}    "),
        ));
    }

    out.push_str(&format!("{indent}  </layers>\n{indent}</DOMTimeline>\n"));
    out
}

#[allow(clippy::too_many_arguments)]
fn write_dom_layer_xml(
    layer: &Layer,
    timeline_frame_count: u32,
    id_to_xfl_idx: &BTreeMap<&str, usize>,
    frame_scripts: Option<&BTreeMap<u32, Vec<String>>>,
    project: &Project,
    symbol_lib_names: &BTreeMap<Id, String>,
    bitmap_names: &BTreeMap<Id, String>,
    sound_names: &BTreeMap<Id, String>,
    indent: &str,
) -> String {
    let mut attrs = format!(
        "name=\"{}\"",
        xml_escape(if layer.name.trim().is_empty() {
            "Layer"
        } else {
            &layer.name
        })
    );
    match layer.properties.role {
        LayerRole::Normal => {}
        LayerRole::Guide => attrs.push_str(" layerType=\"guide\""),
        LayerRole::Mask => attrs.push_str(" layerType=\"mask\""),
        LayerRole::Folder => {
            attrs.push_str(" layerType=\"folder\"");
            if layer.properties.collapsed {
                attrs.push_str(" open=\"false\"");
            }
        }
    }
    if !layer.visible {
        attrs.push_str(" visible=\"false\"");
    }
    if layer.locked {
        attrs.push_str(" locked=\"true\"");
    }
    let parent_target = layer
        .properties
        .mask
        .as_deref()
        .or(layer.properties.motion_guide.as_deref())
        .or(layer.properties.parent.as_deref());
    if let Some(pid) = parent_target
        && let Some(&p_idx) = id_to_xfl_idx.get(pid)
    {
        attrs.push_str(&format!(" parentLayerIndex=\"{p_idx}\""));
    }

    if layer.properties.role == LayerRole::Folder {
        return format!("{indent}<DOMLayer {attrs}>\n{indent}  <frames/>\n{indent}</DOMLayer>\n");
    }

    let mut out = format!("{indent}<DOMLayer {attrs}>\n{indent}  <frames>\n");
    let effective_end = layer
        .properties
        .end_frame
        .unwrap_or(timeline_frame_count)
        .min(timeline_frame_count)
        .max(1);

    let mut keyframes = layer.keyframes.clone();
    keyframes.sort_by_key(|k| k.frame);
    if keyframes.is_empty() || keyframes[0].frame > 0 {
        keyframes.insert(
            0,
            Keyframe {
                frame: 0,
                elements: Vec::new(),
                ..Default::default()
            },
        );
    }

    // Ensure keyframes exist for any attached frame scripts
    if let Some(scripts_map) = frame_scripts {
        for &script_frame in scripts_map.keys() {
            if script_frame < effective_end && !keyframes.iter().any(|k| k.frame == script_frame) {
                let held_elements = keyframes
                    .iter()
                    .rev()
                    .find(|k| k.frame < script_frame)
                    .map(|k| k.elements.clone())
                    .unwrap_or_default();
                keyframes.push(Keyframe {
                    frame: script_frame,
                    elements: held_elements,
                    ..Default::default()
                });
                keyframes.sort_by_key(|k| k.frame);
            }
        }
    }

    for (idx, kf) in keyframes.iter().enumerate() {
        if kf.frame >= effective_end {
            break;
        }
        let next_start = keyframes
            .get(idx + 1)
            .map(|n| n.frame.min(effective_end))
            .unwrap_or(effective_end);
        let duration = next_start.saturating_sub(kf.frame).max(1);

        let mut f_attrs = format!("index=\"{}\" duration=\"{duration}\"", kf.frame);
        if !kf.label.trim().is_empty() {
            f_attrs.push_str(&format!(
                " name=\"{}\" labelType=\"name\"",
                xml_escape(&kf.label)
            ));
        }

        if let Some(easing) = kf.tween {
            match kf.tween_kind {
                TweenKind::Shape => f_attrs.push_str(" tweenType=\"shape\""),
                TweenKind::Classic | TweenKind::Motion => f_attrs.push_str(" tweenType=\"motion\""),
            }
            let accel = match easing {
                Easing::Linear => 0,
                Easing::EaseIn => -100,
                Easing::EaseOut => 100,
                Easing::Flash { acceleration } => acceleration.round().clamp(-100.0, 100.0) as i32,
                _ => 0,
            };
            if accel != 0 {
                f_attrs.push_str(&format!(" acceleration=\"{accel}\""));
            }
            if kf.orient_to_path {
                f_attrs.push_str(" motionTweenOrientToPath=\"true\"");
            }
        }

        if let Some(snd) = &kf.sound
            && let Some(snd_name) = sound_names.get(&snd.resource)
        {
            let sync_str = match snd.sync {
                SoundSync::Event => "event",
                SoundSync::Start => "start",
                SoundSync::Stop => "stop",
                SoundSync::Stream => "stream",
            };
            f_attrs.push_str(&format!(
                " soundName=\"{}\" soundSync=\"{sync_str}\"",
                xml_escape(snd_name)
            ));
            if snd.loop_forever {
                f_attrs.push_str(" soundLoopMode=\"loop\"");
            } else if snd.loops > 1 {
                f_attrs.push_str(&format!(
                    " soundLoopMode=\"repeat\" soundLoop=\"{}\"",
                    snd.loops
                ));
            }
            let effect_str = match snd.effect {
                SoundEffect::None => None,
                SoundEffect::LeftChannel => Some("left channel"),
                SoundEffect::RightChannel => Some("right channel"),
                SoundEffect::FadeLeftToRight => Some("fade left to right"),
                SoundEffect::FadeRightToLeft => Some("fade right to left"),
                SoundEffect::FadeIn => Some("fade in"),
                SoundEffect::FadeOut => Some("fade out"),
                SoundEffect::Custom => None,
            };
            if let Some(eff) = effect_str {
                f_attrs.push_str(&format!(" soundEffect=\"{eff}\""));
            }
        }

        out.push_str(&format!("{indent}    <DOMFrame {f_attrs}>\n"));

        if let Some(scripts_map) = frame_scripts
            && let Some(sources) = scripts_map.get(&kf.frame)
            && !sources.is_empty()
        {
            let joined = sources.join("\n");
            out.push_str(&format!(
                "{indent}      <Actionscript>\n{indent}        <script>{}</script>\n{indent}      </Actionscript>\n",
                xml_escape(&joined)
            ));
        }

        if !kf.elements.is_empty() {
            out.push_str(&format!("{indent}      <elements>\n"));
            for el in &kf.elements {
                out.push_str(&write_dom_element_xml(
                    el,
                    project,
                    symbol_lib_names,
                    bitmap_names,
                    &format!("{indent}        "),
                ));
            }
            out.push_str(&format!("{indent}      </elements>\n"));
        }

        out.push_str(&format!("{indent}    </DOMFrame>\n"));
    }

    out.push_str(&format!("{indent}  </frames>\n{indent}</DOMLayer>\n"));
    out
}

fn write_dom_element_xml(
    el: &Element,
    project: &Project,
    symbol_lib_names: &BTreeMap<Id, String>,
    bitmap_names: &BTreeMap<Id, String>,
    indent: &str,
) -> String {
    let matrix_xml = write_matrix_xml(&el.transform, &format!("{indent}  "));
    let color_xml = write_color_transform_xml(&el.transform, &format!("{indent}  "));
    let filters_xml = write_filters_xml(&el.filters, &format!("{indent}  "));

    match &el.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let mut attrs = format!(
                "objectWidth=\"{}\" objectHeight=\"{}\"",
                fmt_num(*width),
                fmt_num(*height)
            );
            if !el.name.trim().is_empty() && el.name != "DOMRectangleObject" {
                attrs.push_str(&format!(" name=\"{}\"", xml_escape(&el.name)));
            }
            let rect_opts = el.rectangle_options();
            if rect_opts.has_rounded_corners() {
                attrs.push_str(&format!(
                    " topLeftRadius=\"{}\" topRightRadius=\"{}\" bottomLeftRadius=\"{}\" bottomRightRadius=\"{}\"",
                    fmt_num(rect_opts.corner_radii[0]),
                    fmt_num(rect_opts.corner_radii[1]),
                    fmt_num(rect_opts.corner_radii[2]),
                    fmt_num(rect_opts.corner_radii[3]),
                ));
                if !rect_opts.corners_locked {
                    attrs.push_str(" lockFlag=\"false\"");
                }
            }
            let fill_xml = if fill[3] > 0 {
                format!(
                    "{indent}  <fill>\n{indent}    <SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>\n{indent}  </fill>\n",
                    fill[0],
                    fill[1],
                    fill[2],
                    fmt_num(f64::from(fill[3]) / 255.0)
                )
            } else {
                String::new()
            };
            let stroke_xml = if *stroke_width > 0.0 && stroke[3] > 0 {
                format!(
                    "{indent}  <stroke>\n{indent}    <SolidStroke scaleMode=\"normal\" weight=\"{}\">\n{indent}      <fill>\n{indent}        <SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>\n{indent}      </fill>\n{indent}    </SolidStroke>\n{indent}  </stroke>\n",
                    fmt_num(*stroke_width),
                    stroke[0],
                    stroke[1],
                    stroke[2],
                    fmt_num(f64::from(stroke[3]) / 255.0)
                )
            } else {
                String::new()
            };
            format!(
                "{indent}<DOMRectangleObject {attrs}>\n{matrix_xml}{color_xml}{filters_xml}{fill_xml}{stroke_xml}{indent}</DOMRectangleObject>\n"
            )
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let mut attrs = format!(
                "objectWidth=\"{}\" objectHeight=\"{}\"",
                fmt_num(*width),
                fmt_num(*height)
            );
            if !el.name.trim().is_empty() && el.name != "DOMOvalObject" {
                attrs.push_str(&format!(" name=\"{}\"", xml_escape(&el.name)));
            }
            let oval_opts = el.oval_options();
            if !oval_opts.is_default() {
                attrs.push_str(&format!(
                    " startAngle=\"{}\" endAngle=\"{}\" innerRadius=\"{}\" closePath=\"{}\"",
                    fmt_num(oval_opts.start_angle),
                    fmt_num(oval_opts.end_angle),
                    fmt_num(oval_opts.inner_radius),
                    oval_opts.close_path
                ));
            }
            let fill_xml = if fill[3] > 0 {
                format!(
                    "{indent}  <fill>\n{indent}    <SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>\n{indent}  </fill>\n",
                    fill[0],
                    fill[1],
                    fill[2],
                    fmt_num(f64::from(fill[3]) / 255.0)
                )
            } else {
                String::new()
            };
            let stroke_xml = if *stroke_width > 0.0 && stroke[3] > 0 {
                format!(
                    "{indent}  <stroke>\n{indent}    <SolidStroke scaleMode=\"normal\" weight=\"{}\">\n{indent}      <fill>\n{indent}        <SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>\n{indent}      </fill>\n{indent}    </SolidStroke>\n{indent}  </stroke>\n",
                    fmt_num(*stroke_width),
                    stroke[0],
                    stroke[1],
                    stroke[2],
                    fmt_num(f64::from(stroke[3]) / 255.0)
                )
            } else {
                String::new()
            };
            format!(
                "{indent}<DOMOvalObject {attrs}>\n{matrix_xml}{color_xml}{filters_xml}{fill_xml}{stroke_xml}{indent}</DOMOvalObject>\n"
            )
        }
        ElementKind::Path {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        } => write_dom_shape_xml(
            el,
            commands,
            *fill,
            None,
            *stroke,
            None,
            *stroke_width,
            indent,
        ),
        ElementKind::PaintedPath {
            commands,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => write_dom_shape_xml(
            el,
            commands,
            *fill,
            fill_gradient.as_ref(),
            *stroke,
            stroke_gradient.as_ref(),
            *stroke_width,
            indent,
        ),
        ElementKind::Instance {
            symbol_id,
            playback,
        } => {
            let Some(lib_name) = symbol_lib_names.get(symbol_id) else {
                return String::new();
            };
            let target_kind = project
                .symbols
                .iter()
                .find(|s| &s.id == symbol_id)
                .map(|s| s.kind)
                .unwrap_or(SymbolKind::MovieClip);
            let mut attrs = format!("libraryItemName=\"{}\"", xml_escape(lib_name));
            if !el.name.trim().is_empty() && el.name != "DOMSymbolInstance" {
                attrs.push_str(&format!(" name=\"{}\"", xml_escape(&el.name)));
            }
            match (target_kind, playback) {
                (SymbolKind::Button, _) => attrs.push_str(" symbolType=\"button\""),
                (_, InstancePlayback::SingleFrame(f)) => {
                    attrs.push_str(&format!(
                        " symbolType=\"graphic\" loop=\"single frame\" firstFrame=\"{f}\""
                    ));
                }
                (_, InstancePlayback::Loop) => {
                    attrs.push_str(" symbolType=\"graphic\" loop=\"loop\"");
                }
                (_, InstancePlayback::LoopFrom(f)) => {
                    attrs.push_str(&format!(
                        " symbolType=\"graphic\" loop=\"loop\" firstFrame=\"{f}\""
                    ));
                }
                (_, InstancePlayback::PlayOnce) => {
                    attrs.push_str(" symbolType=\"graphic\" loop=\"play once\"");
                }
                (_, InstancePlayback::PlayOnceFrom(f)) => {
                    attrs.push_str(&format!(
                        " symbolType=\"graphic\" loop=\"play once\" firstFrame=\"{f}\""
                    ));
                }
                (_, InstancePlayback::Independent) => {
                    if target_kind == SymbolKind::Graphic {
                        attrs.push_str(" symbolType=\"graphic\" loop=\"loop\"");
                    } else {
                        attrs.push_str(" symbolType=\"movie clip\"");
                    }
                }
            }
            if !el.visible {
                attrs.push_str(" isVisible=\"false\"");
            }
            if el.cache_as_bitmap {
                attrs.push_str(" cacheAsBitmap=\"true\"");
            }
            if !el.blend_mode.is_normal() {
                attrs.push_str(&format!(" blendMode=\"{}\"", el.blend_mode.as_str()));
            }
            let scripts_xml = write_object_scripts_xml(&el.scripts, &format!("{indent}  "));
            format!(
                "{indent}<DOMSymbolInstance {attrs}>\n{matrix_xml}{color_xml}{filters_xml}{scripts_xml}{indent}</DOMSymbolInstance>\n"
            )
        }
        ElementKind::Bitmap { resource_id, .. } => {
            let Some(bmp_name) = bitmap_names.get(resource_id) else {
                return String::new();
            };
            format!(
                "{indent}<DOMBitmapInstance libraryItemName=\"{}\">\n{matrix_xml}{color_xml}{indent}</DOMBitmapInstance>\n",
                xml_escape(bmp_name)
            )
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            let hex = format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]);
            let alpha_attr = if color[3] < 255 {
                format!(" alpha=\"{}\"", fmt_num(f64::from(color[3]) / 255.0))
            } else {
                String::new()
            };
            format!(
                "{indent}<DOMStaticText isSelectable=\"false\">\n{matrix_xml}{color_xml}{filters_xml}{indent}  <textRuns>\n{indent}    <DOMTextRun>\n{indent}      <characters>{}</characters>\n{indent}      <textAttrs>\n{indent}        <DOMTextAttrs face=\"{}\" size=\"{}\" fillColor=\"{hex}\"{alpha_attr}/>\n{indent}      </textAttrs>\n{indent}    </DOMTextRun>\n{indent}  </textRuns>\n{indent}</DOMStaticText>\n",
                xml_escape(text),
                xml_escape(if font_family.trim().is_empty() {
                    "Arial"
                } else {
                    font_family
                }),
                fmt_num(*font_size)
            )
        }
        ElementKind::TextBox {
            text,
            width,
            height,
            format,
            word_wrap,
            multiline,
        } => {
            let tag = if !el.name.trim().is_empty() && el.name != "DOMStaticText" {
                "DOMDynamicText"
            } else {
                "DOMStaticText"
            };
            let line_type = if *multiline {
                if *word_wrap {
                    "multiline"
                } else {
                    "multiline no wrap"
                }
            } else {
                "single line"
            };
            let mut attrs = format!(
                "width=\"{}\" height=\"{}\" lineType=\"{line_type}\"",
                fmt_num(*width),
                fmt_num(*height)
            );
            if !el.name.trim().is_empty()
                && !matches!(el.name.as_str(), "DOMStaticText" | "DOMDynamicText")
            {
                attrs.push_str(&format!(" name=\"{}\"", xml_escape(&el.name)));
            }
            let r = ((format.color >> 16) & 0xFF) as u8;
            let g = ((format.color >> 8) & 0xFF) as u8;
            let b = (format.color & 0xFF) as u8;
            let hex = format!("#{r:02X}{g:02X}{b:02X}");
            let align = match format.align {
                TextAlign::Left => "left",
                TextAlign::Center => "center",
                TextAlign::Right => "right",
            };
            let mut t_attrs = format!(
                "face=\"{}\" size=\"{}\" fillColor=\"{hex}\" alignment=\"{align}\"",
                xml_escape(if format.font.trim().is_empty() {
                    "Arial"
                } else {
                    &format.font
                }),
                fmt_num(format.size)
            );
            if format.bold {
                t_attrs.push_str(" bold=\"true\"");
            }
            if format.italic {
                t_attrs.push_str(" italic=\"true\"");
            }
            if format.underline {
                t_attrs.push_str(" underline=\"true\"");
            }
            if format.letter_spacing != 0.0 {
                t_attrs.push_str(&format!(
                    " letterSpacing=\"{}\"",
                    fmt_num(format.letter_spacing)
                ));
            }
            if format.leading != 0.0 {
                t_attrs.push_str(&format!(" lineSpacing=\"{}\"", fmt_num(format.leading)));
            }
            format!(
                "{indent}<{tag} {attrs}>\n{matrix_xml}{color_xml}{filters_xml}{indent}  <textRuns>\n{indent}    <DOMTextRun>\n{indent}      <characters>{}</characters>\n{indent}      <textAttrs>\n{indent}        <DOMTextAttrs {t_attrs}/>\n{indent}      </textAttrs>\n{indent}    </DOMTextRun>\n{indent}  </textRuns>\n{indent}</{tag}>\n",
                xml_escape(text)
            )
        }
        ElementKind::Video { width, height } => {
            // Represent Video viewport as a rectangle object if no video media item is present
            format!(
                "{indent}<DOMRectangleObject objectWidth=\"{}\" objectHeight=\"{}\">\n{matrix_xml}{indent}  <fill>\n{indent}    <SolidColor color=\"#000000\" alpha=\"1\"/>\n{indent}  </fill>\n{indent}</DOMRectangleObject>\n",
                fmt_num(*width),
                fmt_num(*height)
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_dom_shape_xml(
    el: &Element,
    commands: &[PathCommand],
    fill: Color,
    fill_gradient: Option<&GradientPaint>,
    stroke: Color,
    stroke_gradient: Option<&GradientPaint>,
    stroke_width: f64,
    indent: &str,
) -> String {
    let has_fill = fill_gradient.is_some() || fill[3] > 0;
    let has_stroke = stroke_width > 0.0 && (stroke_gradient.is_some() || stroke[3] > 0);
    let edge_str = path_commands_to_xfl_edges(commands, has_fill);
    if edge_str.is_empty() || (!has_fill && !has_stroke) {
        return String::new();
    }

    let mut attrs = String::new();
    if !el.name.trim().is_empty() && el.name != "DOMShape" {
        attrs.push_str(&format!(" name=\"{}\"", xml_escape(&el.name)));
    }
    let matrix_xml = write_matrix_xml(&el.transform, &format!("{indent}  "));

    let fills_xml = if has_fill {
        let paint_xml = write_paint_xml(fill, fill_gradient, &format!("{indent}      "));
        format!(
            "{indent}  <fills>\n{indent}    <FillStyle index=\"1\">\n{paint_xml}{indent}    </FillStyle>\n{indent}  </fills>\n"
        )
    } else {
        String::new()
    };

    let strokes_xml = if has_stroke {
        let paint_xml = write_paint_xml(stroke, stroke_gradient, &format!("{indent}          "));
        format!(
            "{indent}  <strokes>\n{indent}    <StrokeStyle index=\"1\">\n{indent}      <SolidStroke scaleMode=\"normal\" weight=\"{}\">\n{indent}        <fill>\n{paint_xml}{indent}        </fill>\n{indent}      </SolidStroke>\n{indent}    </StrokeStyle>\n{indent}  </strokes>\n",
            fmt_num(stroke_width)
        )
    } else {
        String::new()
    };

    let mut edge_attrs = String::new();
    if has_fill {
        edge_attrs.push_str(" fillStyle1=\"1\"");
    }
    if has_stroke {
        edge_attrs.push_str(" strokeStyle=\"1\"");
    }

    format!(
        "{indent}<DOMShape{attrs}>\n{matrix_xml}{fills_xml}{strokes_xml}{indent}  <edges>\n{indent}    <Edge{edge_attrs} edges=\"{edge_str}\"/>\n{indent}  </edges>\n{indent}</DOMShape>\n"
    )
}

fn write_paint_xml(color: Color, gradient: Option<&GradientPaint>, indent: &str) -> String {
    if let Some(grad) = gradient
        && !grad.stops.is_empty()
    {
        let tag = match grad.kind {
            GradientKind::Linear => "LinearGradient",
            GradientKind::Radial => "RadialGradient",
        };
        let focal_attr = if grad.kind == GradientKind::Radial && grad.focal_point_ratio != 0.0 {
            format!(
                " focalPointRatio=\"{}\"",
                fmt_num(grad.focal_point_ratio.clamp(-1.0, 1.0))
            )
        } else {
            String::new()
        };
        let m = grad.matrix;
        let mut out = format!(
            "{indent}<{tag}{focal_attr}>\n{indent}  <matrix>\n{indent}    <Matrix a=\"{}\" b=\"{}\" c=\"{}\" d=\"{}\" tx=\"{}\" ty=\"{}\"/>\n{indent}  </matrix>\n",
            fmt_num(m[0]),
            fmt_num(m[1]),
            fmt_num(m[2]),
            fmt_num(m[3]),
            fmt_num(m[4]),
            fmt_num(m[5])
        );
        for stop in &grad.stops {
            out.push_str(&format!(
                "{indent}  <GradientEntry color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\" ratio=\"{}\"/>\n",
                stop.color[0],
                stop.color[1],
                stop.color[2],
                fmt_num(f64::from(stop.color[3]) / 255.0),
                fmt_num(f64::from(stop.ratio) / 255.0)
            ));
        }
        out.push_str(&format!("{indent}</{tag}>\n"));
        out
    } else {
        format!(
            "{indent}<SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>\n",
            color[0],
            color[1],
            color[2],
            fmt_num(f64::from(color[3]) / 255.0)
        )
    }
}

fn path_commands_to_xfl_edges(commands: &[PathCommand], close_for_fill: bool) -> String {
    let to_twips = |v: f64| -> i32 { (v * 20.0).round().clamp(-1_000_000.0, 1_000_000.0) as i32 };
    let mut out = String::new();
    let mut cur = [0i32, 0i32];
    let mut contour_start = [0i32, 0i32];
    let mut has_contour = false;

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                if close_for_fill && has_contour && cur != contour_start {
                    out.push_str(&format!("|{} {}", contour_start[0], contour_start[1]));
                }
                cur = [to_twips(x), to_twips(y)];
                contour_start = cur;
                has_contour = true;
                out.push_str(&format!("!{} {}", cur[0], cur[1]));
            }
            PathCommand::LineTo { x, y } => {
                let next = [to_twips(x), to_twips(y)];
                if !has_contour {
                    contour_start = cur;
                    has_contour = true;
                    out.push_str(&format!("!{} {}", cur[0], cur[1]));
                }
                if next != cur {
                    out.push_str(&format!("|{} {}", next[0], next[1]));
                    cur = next;
                }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let ctrl = [to_twips(cx), to_twips(cy)];
                let next = [to_twips(x), to_twips(y)];
                if !has_contour {
                    contour_start = cur;
                    has_contour = true;
                    out.push_str(&format!("!{} {}", cur[0], cur[1]));
                }
                if next != cur || ctrl != cur {
                    out.push_str(&format!("[{} {} {} {}", ctrl[0], ctrl[1], next[0], next[1]));
                    cur = next;
                }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if !has_contour {
                    contour_start = cur;
                    has_contour = true;
                    out.push_str(&format!("!{} {}", cur[0], cur[1]));
                }
                // Approximate cubic with two quadratics
                let p0 = [cur[0] as f64 / 20.0, cur[1] as f64 / 20.0];
                let c1 = [cx1, cy1];
                let c2 = [cx2, cy2];
                let p3 = [x, y];
                let q1 = [0.25 * p0[0] + 0.75 * c1[0], 0.25 * p0[1] + 0.75 * c1[1]];
                let q2 = [0.25 * p3[0] + 0.75 * c2[0], 0.25 * p3[1] + 0.75 * c2[1]];
                let mid = [0.5 * (q1[0] + q2[0]), 0.5 * (q1[1] + q2[1])];
                let q1_t = [to_twips(q1[0]), to_twips(q1[1])];
                let mid_t = [to_twips(mid[0]), to_twips(mid[1])];
                let q2_t = [to_twips(q2[0]), to_twips(q2[1])];
                let end_t = [to_twips(p3[0]), to_twips(p3[1])];
                if mid_t != cur {
                    out.push_str(&format!(
                        "[{} {} {} {}",
                        q1_t[0], q1_t[1], mid_t[0], mid_t[1]
                    ));
                    cur = mid_t;
                }
                if end_t != cur {
                    out.push_str(&format!(
                        "[{} {} {} {}",
                        q2_t[0], q2_t[1], end_t[0], end_t[1]
                    ));
                    cur = end_t;
                }
            }
            PathCommand::Close => {
                if has_contour && cur != contour_start {
                    out.push_str(&format!("|{} {}", contour_start[0], contour_start[1]));
                    cur = contour_start;
                }
            }
        }
    }
    if close_for_fill && has_contour && cur != contour_start {
        out.push_str(&format!("|{} {}", contour_start[0], contour_start[1]));
    }
    // Fallback if all commands collapsed to a single point
    if !out.contains('|') && !out.contains('[') {
        if close_for_fill {
            return format!(
                "!{} {}|{} {}|{} {}|{} {}",
                contour_start[0],
                contour_start[1],
                contour_start[0] + 20,
                contour_start[1],
                contour_start[0] + 20,
                contour_start[1] + 20,
                contour_start[0],
                contour_start[1]
            );
        } else {
            return format!(
                "!{} {}|{} {}",
                contour_start[0],
                contour_start[1],
                contour_start[0] + 20,
                contour_start[1]
            );
        }
    }
    out
}

fn write_matrix_xml(t: &Transform, indent: &str) -> String {
    let m = transform_matrix(*t);
    if (m[0] - 1.0).abs() < 1e-6
        && m[1].abs() < 1e-6
        && m[2].abs() < 1e-6
        && (m[3] - 1.0).abs() < 1e-6
        && m[4].abs() < 1e-6
        && m[5].abs() < 1e-6
    {
        return String::new();
    }
    format!(
        "{indent}<matrix>\n{indent}  <Matrix a=\"{}\" b=\"{}\" c=\"{}\" d=\"{}\" tx=\"{}\" ty=\"{}\"/>\n{indent}</matrix>\n",
        fmt_num(m[0]),
        fmt_num(m[1]),
        fmt_num(m[2]),
        fmt_num(m[3]),
        fmt_num(m[4]),
        fmt_num(m[5])
    )
}

fn write_color_transform_xml(t: &Transform, indent: &str) -> String {
    let ct = &t.color_transform;
    if (t.alpha - 1.0).abs() < 1e-6 && ct.is_identity() {
        return String::new();
    }
    if ct.is_identity() {
        return format!(
            "{indent}<color>\n{indent}  <Color alphaMultiplier=\"{}\"/>\n{indent}</color>\n",
            fmt_num(t.alpha.clamp(0.0, 1.0))
        );
    }
    format!(
        "{indent}<color>\n{indent}  <Color alphaMultiplier=\"{}\" redMultiplier=\"{}\" greenMultiplier=\"{}\" blueMultiplier=\"{}\" redOffset=\"{}\" greenOffset=\"{}\" blueOffset=\"{}\" alphaOffset=\"{}\"/>\n{indent}</color>\n",
        fmt_num(t.alpha.clamp(0.0, 1.0)),
        fmt_num(ct.multipliers[0]),
        fmt_num(ct.multipliers[1]),
        fmt_num(ct.multipliers[2]),
        fmt_num(ct.offsets[0]),
        fmt_num(ct.offsets[1]),
        fmt_num(ct.offsets[2]),
        fmt_num(ct.offsets[3])
    )
}

fn write_filters_xml(filters: &[FilterSpec], indent: &str) -> String {
    let mut inner = String::new();
    for f in filters {
        let enabled = f.is_enabled();
        let quality_level = f.quality_level().clamp(1, 15);
        match f {
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                inner: is_inner,
                knockout,
                hide_object,
            } => {
                inner.push_str(&format!(
                    "{indent}  <DropShadowFilter distance=\"{}\" angle=\"{}\" color=\"#{:06X}\" alpha=\"{}\" blurX=\"{}\" blurY=\"{}\" strength=\"{}\" quality=\"{}\" inner=\"{}\" knockout=\"{}\" hideObject=\"{}\" isEnabled=\"{}\"/>\n",
                    fmt_num(*distance),
                    fmt_num(*angle),
                    color & 0xFFFFFF,
                    fmt_num(*alpha),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*strength),
                    quality_level,
                    is_inner,
                    knockout,
                    hide_object,
                    enabled
                ));
            }
            FilterSpec::Blur {
                blur_x,
                blur_y,
                quality: _,
            } => {
                inner.push_str(&format!(
                    "{indent}  <BlurFilter blurX=\"{}\" blurY=\"{}\" quality=\"{}\" isEnabled=\"{}\"/>\n",
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    quality_level,
                    enabled
                ));
            }
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                inner: is_inner,
                knockout,
            } => {
                inner.push_str(&format!(
                    "{indent}  <GlowFilter color=\"#{:06X}\" alpha=\"{}\" blurX=\"{}\" blurY=\"{}\" strength=\"{}\" quality=\"{}\" inner=\"{}\" knockout=\"{}\" isEnabled=\"{}\"/>\n",
                    color & 0xFFFFFF,
                    fmt_num(*alpha),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*strength),
                    quality_level,
                    is_inner,
                    knockout,
                    enabled
                ));
            }
            FilterSpec::Bevel {
                distance,
                angle,
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                placement,
                knockout,
            } => {
                inner.push_str(&format!(
                    "{indent}  <BevelFilter distance=\"{}\" angle=\"{}\" highlightColor=\"#{:06X}\" highlightAlpha=\"{}\" shadowColor=\"#{:06X}\" shadowAlpha=\"{}\" blurX=\"{}\" blurY=\"{}\" strength=\"{}\" quality=\"{}\" type=\"{}\" knockout=\"{}\" isEnabled=\"{}\"/>\n",
                    fmt_num(*distance),
                    fmt_num(*angle),
                    highlight_color & 0xFFFFFF,
                    fmt_num(*highlight_alpha),
                    shadow_color & 0xFFFFFF,
                    fmt_num(*shadow_alpha),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*strength),
                    quality_level,
                    xml_escape(placement),
                    knockout,
                    enabled
                ));
            }
            unsupported => unreachable!(
                "{} is blocked by FLA/XFL preflight before XML serialization",
                filter_kind_name(unsupported)
            ),
        }
    }
    if inner.is_empty() {
        String::new()
    } else {
        format!("{indent}<filters>\n{inner}{indent}</filters>\n")
    }
}

pub(crate) fn skip_leading_ws_and_comments(mut s: &str) -> &str {
    loop {
        s = s.trim_start();
        if let Some(after_line_comment) = s.strip_prefix("//") {
            match after_line_comment.find('\n') {
                Some(nl) => s = &after_line_comment[nl + 1..],
                None => return "",
            }
        } else if let Some(after_block_start) = s.strip_prefix("/*") {
            match after_block_start.find("*/") {
                Some(end) => s = &after_block_start[end + 2..],
                None => return "",
            }
        } else {
            return s;
        }
    }
}

pub(crate) fn script_starts_with_event_wrapper(source: &str) -> bool {
    let s = skip_leading_ws_and_comments(source);
    let after_kw = if let Some(prefix) = s.get(..11)
        && prefix.eq_ignore_ascii_case("onClipEvent")
    {
        let rest = &s[11..];
        if rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
        {
            return false;
        }
        rest
    } else if let Some(prefix) = s.get(..2)
        && prefix.eq_ignore_ascii_case("on")
    {
        let rest = &s[2..];
        if rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
        {
            return false;
        }
        rest
    } else {
        return false;
    };
    skip_leading_ws_and_comments(after_kw).starts_with('(')
}

fn write_object_scripts_xml(scripts: &[ObjectScript], indent: &str) -> String {
    if scripts.is_empty() {
        return String::new();
    }
    let mut blocks = Vec::with_capacity(scripts.len());
    for os in scripts {
        let marker = match os.script.language {
            ScriptLanguage::JavaScript => "/* neoflash:language=javascript */\n",
            ScriptLanguage::TypeScript => "/* neoflash:language=typescript */\n",
            ScriptLanguage::ActionScript1 => "/* neoflash:language=actionscript1 */\n",
            ScriptLanguage::ActionScript2 => "/* neoflash:language=actionscript2 */\n",
            ScriptLanguage::ActionScript3 => "/* neoflash:language=actionscript3 */\n",
        };
        let trimmed = os.script.source.trim();
        if script_starts_with_event_wrapper(trimmed) {
            blocks.push(format!("{marker}{trimmed}"));
        } else {
            let header = match os.event {
                ObjectEvent::Click => "on (release) {\n",
                ObjectEvent::Load => "onClipEvent (load) {\n",
                ObjectEvent::EnterFrame => "onClipEvent (enterFrame) {\n",
            };
            blocks.push(format!("{header}{marker}{}\n}}", os.script.source));
        }
    }
    format!(
        "{indent}<Actionscript>\n{indent}  <script>{}</script>\n{indent}</Actionscript>\n",
        xml_escape(&blocks.join("\n"))
    )
}

fn sanitize_xfl_item_name(raw: &str, fallback: &str) -> String {
    let base = raw.rsplit_once('/').map_or(raw, |(_, b)| b).trim();
    let mut out = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub(crate) fn sanitize_source_rel_path(raw: &str, idx: usize) -> String {
    let clean: Vec<String> = raw
        .split(['/', '\\'])
        .filter(|seg| !seg.is_empty() && *seg != "." && *seg != "..")
        .map(|seg| {
            seg.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        })
        .collect();
    let mut joined = if clean.is_empty() {
        format!("script_{}.as", idx + 1)
    } else {
        clean.join("/")
    };
    let lower = joined.to_ascii_lowercase();
    if !lower.ends_with(".as") && !lower.ends_with(".js") && !lower.ends_with(".ts") {
        joined.push_str(".as");
    }
    joined
}

fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let rounded = (v * 10000.0).round() / 10000.0;
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded}")
    }
}

fn xml_escape(s: &str) -> String {
    crate::xml_escape(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> std::path::PathBuf {
        static ID: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-xfl-replace-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create XFL replacement test directory");
        path
    }

    #[test]
    fn staged_xfl_install_failure_restores_previous_target_and_cleans_swap_paths() {
        let root = temp_dir();
        let target = root.join("movie.xfl");
        let staged = root.join(".movie.xfl-tmp");
        let backup = root.join(".movie.xfl-backup");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("marker.txt"), b"previous document").unwrap();
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("marker.txt"), b"new document").unwrap();

        let mut rename_count = 0usize;
        let error = replace_staged_path_recoverably_with(&staged, &target, &backup, |from, to| {
            rename_count += 1;
            if rename_count == 2 {
                Err(std::io::Error::other("injected staged install failure"))
            } else {
                fs::rename(from, to)
            }
        })
        .expect_err("second rename is injected to fail");

        assert!(
            error
                .to_string()
                .contains("injected staged install failure")
        );
        assert_eq!(rename_count, 3, "target move, failed install, rollback");
        assert_eq!(
            fs::read(target.join("marker.txt")).unwrap(),
            b"previous document"
        );
        assert!(!staged.exists(), "staged directory must be cleaned");
        assert!(!backup.exists(), "rollback must consume backup directory");

        fs::remove_dir_all(root).unwrap();
    }
}
