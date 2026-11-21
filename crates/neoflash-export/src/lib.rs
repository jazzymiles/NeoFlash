//! Self-contained HTML5 publication. Authored evaluation remains in the Rust WASM runtime.

pub mod aep;
pub mod ai_http;
pub mod atlas;
pub mod audio;
pub mod banner;
pub mod feedback_http;
pub mod fla_preflight;
pub mod fonts;
pub mod html;
pub mod image_export;
mod image_fill_layers;
pub mod legacy_fla;
pub mod lottie;
pub mod profiles;
pub mod publish;
pub mod release_http;
pub mod svg;
mod svg_bitmap;
pub mod swf;
pub mod video;
pub mod video_audio;
mod video_frames;
pub use aep::*;
pub mod bitmap_encode;
mod sequence_policy;
pub mod sequence_set;
pub mod size_report;
mod source_map;
pub use atlas::*;
pub use audio::*;
pub use banner::*;
pub use feedback_http::*;
pub use fla_preflight::*;
pub use fonts::*;
pub use html::*;
pub use image_export::*;
pub use legacy_fla::*;
pub use lottie::*;
pub use neoflash_core::{HtmlRenderMode, PublishBreakpoint, PublishSettings};
pub use profiles::*;
pub use publish::{
    HtmlAcceleration, HtmlExportOptions, HtmlPublishConfig, HtmlScaleMode, HtmlStageAlign,
    HtmlTemplate, PlayerChromeConfig, PublishOptions, SequencePublishPolicy, SizeEstimate,
    SizeLevel, SizePart, gzip_len, validate_runtime_url,
};
pub use sequence_set::*;
pub use svg::*;
pub use swf::*;
pub use video::*;

use base64::{Engine, engine::general_purpose::STANDARD};
pub use bitmap_encode::{
    BitmapPublishStats, apply_bitmap_export_settings, sha256_bytes, sha256_hex,
};
use neoflash_core::{ElementKind, Project, SceneRange, Script, ScriptLanguage};
use sequence_policy::apply_sequence_publish_policy;
use serde::Serialize;
pub use size_report::{
    LoadSummary, ResourceLoadItem, compute_load_summary, format_bytes, format_size_report,
};
use size_report::{RuntimeFigures, size_report_from};
use source_map::{
    apply_js_publish_transforms, apply_publish_source_map_shift, build_fallback_bundle_source_map,
};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use thiserror::Error;

const HOST: &str = include_str!("../../../web/runtime.js");
const DOM_LAYOUT: &str = include_str!("../../../web/dom-layout.js");
const PATH_GEOMETRY: &str = include_str!("../../../web/path.js");
const ACCESSIBILITY: &str = include_str!("../../../web/accessibility.js");
const PREVIEW: &str = include_str!("../../../web/preview.js");
const EVENTS: &str = include_str!("../../../web/events.js");
const STAGE: &str = include_str!("../../../web/stage.js");
const LEGACY_INPUT: &str = include_str!("../../../web/legacy-input.js");
const GEOMETRY: &str = include_str!("../../../web/geom.js");
const GEOMETRY_3D: &str = include_str!("../../../web/geom3d.js");
const AS2_NETWORK: &str = include_str!("../../../web/as2-network.js");
const AS2_FILES: &str = include_str!("../../../web/as2-files.js");
const AS2_LOCALE: &str = include_str!("../../../web/as2-locale.js");
const AS2_TRANSITIONS: &str = include_str!("../../../web/as2-transitions.js");
const AS2_DEPTH_MANAGER: &str = include_str!("../../../web/as2-depth-manager.js");
const AS2_PRINT: &str = include_str!("../../../web/as2-print.js");
const AS2_BOXING: &str = include_str!("../../../web/as2-boxing.js");
const AS2_CAPTURE: &str = include_str!("../../../web/as2-capture.js");
const AS2_CONNECTIONS: &str = include_str!("../../../web/as2-connections.js");
const AS2_STAGE: &str = include_str!("../../../web/as2-stage.js");
const AS2_MEDIA: &str = include_str!("../../../web/as2-media.js");
const AS2_SERVICES: &str = include_str!("../../../web/as2-services.js");
const AS2_OBJECT: &str = include_str!("../../../web/as2-object.js");
const AS2_TEXT: &str = include_str!("../../../web/as2-text.js");
const AS2_BINDING: &str = include_str!("../../../web/as2-binding.js");
const AS2_DATA: &str = include_str!("../../../web/as2-data.js");
const AS2_PLATFORM: &str = include_str!("../../../web/as2-platform.js");
const AS2_STRINGS: &str = include_str!("../../../web/as2-strings.js");
const AS3_STRINGS: &str = include_str!("../../../web/as3-strings.js");
const AS3_FTE_3D: &str = include_str!("../../../web/as3-fte-3d.js");
const SCRIPT_EASING: &str = include_str!("../../../web/easing.js");
const SCRIPT_BROADCASTER: &str = include_str!("../../../web/broadcaster.js");
const SCRIPT_MATH: &str = include_str!("../../../web/math.js");
const SCRIPT_DATE: &str = include_str!("../../../web/date.js");
const SCRIPT_NUMBER_FORMAT: &str = include_str!("../../../web/number-format.js");
const ALGORITHM_NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.txt");
const TIMERS: &str = include_str!("../../../web/timers.js");
const BYTE_ARRAY: &str = include_str!("../../../web/byte-array.js");
const NET: &str = include_str!("../../../web/net.js");
const EXTERNAL: &str = include_str!("../../../web/external.js");
const BITMAP: &str = include_str!("../../../web/bitmap.js");
const TEXT: &str = include_str!("../../../web/text.js");
const TEXT_INPUT: &str = include_str!("../../../web/text-input.js");
const FILTERS: &str = include_str!("../../../web/filters.js");
const AUDIO_FORMATS: &str = include_str!("../../../web/audio-formats.js");
const AUDIO: &str = include_str!("../../../web/audio.js");
const VIDEO: &str = include_str!("../../../web/video.js");
const XML: &str = include_str!("../../../web/xml.js");
const LOADER: &str = include_str!("../../../web/loader.js");
const AMF: &str = include_str!("../../../web/amf.js");
const STORAGE: &str = include_str!("../../../web/storage.js");
const AIR: &str = include_str!("../../../web/air.js");
const AS3_NET_SYS: &str = include_str!("../../../web/as3-net-sys.js");
const AS3_MEDIA_EVENTS: &str = include_str!("../../../web/as3-media-events.js");
const DISPLAY: &str = include_str!("../../../web/display.js");
const MARKER: &str = "neoflash-export.json";

/// Best-effort cleanup for a task-owned temporary file or directory on every early return.
pub(crate) struct TemporaryPathGuard {
    path: PathBuf,
}

impl TemporaryPathGuard {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for TemporaryPathGuard {
    fn drop(&mut self) {
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            let _ = fs::remove_dir_all(&self.path);
        } else {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("Invalid project: {0}")]
    InvalidProject(String),
    #[error("Script compilation failed:\n{0}")]
    Scripts(String),
    #[error("Unsupported script for target profile: {0}")]
    UnsupportedScript(String),
    #[error("Missing root symbol: {0}")]
    MissingRootSymbol(String),
    #[error(
        "Compatibility preflight blocked export for {profile} ({blocking_count} blocking item(s))"
    )]
    CompatibilityBlocked {
        profile: &'static str,
        blocking_count: usize,
    },
    #[error("Round-trip verification failed: {0}")]
    RoundTripVerification(String),
    #[error("HTML5 runtime is missing or incompatible: {0}. Run scripts/build-web.sh first.")]
    MissingRuntime(String),
    #[error(
        "Export destination contains files that do not belong to NeoFlash: {0}. Choose an empty folder or a previous NeoFlash export."
    )]
    Destination(String),
    #[error("Could not write HTML5 export: {0}")]
    Io(#[from] std::io::Error),
    #[error("Could not serialize HTML5 export: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid publish options: {0}")]
    Options(String),
}

#[derive(Clone, Serialize)]
pub(crate) struct CompiledScript {
    language: ScriptLanguage,
    source: String,
    code: String,
    location: String,
    locations: Vec<String>,
    source_url: String,
    source_map: neoflash_script::SourceMap,
    source_locations: Vec<neoflash_script::RuntimeSourceLocation>,
}

/// Browser host modules in load order, between the bindings with `bundle.js` and `runtime.js`.
const HOST_MODULES: &[(&str, &str)] = &[
    ("path.js", PATH_GEOMETRY),
    ("accessibility.js", ACCESSIBILITY),
    ("preview.js", PREVIEW),
    ("events.js", EVENTS),
    ("stage.js", STAGE),
    ("legacy-input.js", LEGACY_INPUT),
    ("geom.js", GEOMETRY),
    ("geom3d.js", GEOMETRY_3D),
    ("as2-strings.js", AS2_STRINGS),
    ("as2-platform.js", AS2_PLATFORM),
    ("as2-object.js", AS2_OBJECT),
    ("as2-transitions.js", AS2_TRANSITIONS),
    ("as2-depth-manager.js", AS2_DEPTH_MANAGER),
    ("as2-print.js", AS2_PRINT),
    ("as2-boxing.js", AS2_BOXING),
    ("as2-capture.js", AS2_CAPTURE),
    ("as2-connections.js", AS2_CONNECTIONS),
    ("as2-stage.js", AS2_STAGE),
    ("as2-media.js", AS2_MEDIA),
    ("as2-services.js", AS2_SERVICES),
    ("as2-locale.js", AS2_LOCALE),
    ("as2-files.js", AS2_FILES),
    ("as2-network.js", AS2_NETWORK),
    ("as2-text.js", AS2_TEXT),
    ("as2-data.js", AS2_DATA),
    ("as2-binding.js", AS2_BINDING),
    ("as3-strings.js", AS3_STRINGS),
    ("as3-fte-3d.js", AS3_FTE_3D),
    ("easing.js", SCRIPT_EASING),
    ("math.js", SCRIPT_MATH),
    ("date.js", SCRIPT_DATE),
    ("number-format.js", SCRIPT_NUMBER_FORMAT),
    ("broadcaster.js", SCRIPT_BROADCASTER),
    ("timers.js", TIMERS),
    ("byte-array.js", BYTE_ARRAY),
    ("net.js", NET),
    ("external.js", EXTERNAL),
    ("bitmap.js", BITMAP),
    ("text.js", TEXT),
    ("text-input.js", TEXT_INPUT),
    ("filters.js", FILTERS),
    ("audio-formats.js", AUDIO_FORMATS),
    ("audio.js", AUDIO),
    ("video.js", VIDEO),
    ("xml.js", XML),
    ("loader.js", LOADER),
    ("amf.js", AMF),
    ("storage.js", STORAGE),
    ("air.js", AIR),
    ("as3-net-sys.js", AS3_NET_SYS),
    ("as3-media-events.js", AS3_MEDIA_EVENTS),
    ("display.js", DISPLAY),
    ("dom-layout.js", DOM_LAYOUT),
];
const WASM_FILE: &str = "neoflash_web_bg.wasm";
const BINDINGS_FILE: &str = "neoflash_web.js";
const NOTICES_FILE: &str = "THIRD_PARTY_NOTICES.txt";
const RUNTIME_ID_FILE: &str = "runtime-id.js";
const MODULE_FILE: &str = "module.mjs";

pub(crate) fn static_runtime_js_bytes() -> usize {
    HOST.len()
        + HOST_MODULES
            .iter()
            .map(|(_, source)| source.len())
            .sum::<usize>()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SpriteSheetBundle {
    pub image_url: String,
    pub manifest_url: String,
}

#[derive(Serialize)]
struct Bundle<'a> {
    /// Single main timeline: authored scenes are concatenated before publishing.
    project: &'a Project,
    /// Scene names and frame ranges inside the flattened main timeline.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scenes: Vec<SceneRange>,
    scripts: Vec<&'a CompiledScript>,
    // The Full profile also ships a program prelude for documents with no authored scripts.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    scriptless: bool,
    /// Absent when a smaller size level drops the prelude of a project without scripts.
    #[serde(skip_serializing_if = "Option::is_none")]
    program: Option<ProgramBundle<'a>>,
    /// Base64 WebAssembly at the Full level; otherwise the runtime fetches `wasm_url`.
    #[serde(skip_serializing_if = "Option::is_none")]
    wasm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wasm_url: Option<String>,
    /// Shared level: the runtime build this project was published against.
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    player_config: Option<&'a HtmlPublishConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    video_manifest: &'a Vec<neoflash_core::VideoManifestEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sprite_sheet: Option<SpriteSheetBundle>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    asset_urls: std::collections::BTreeMap<String, String>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    resource_aliases: std::collections::BTreeMap<String, String>,
    publish: PublishInfo,
    warnings: Vec<String>,
    load_summary: LoadSummary,
    size_report: String,
    fonts: &'a FontBundle,
}

#[derive(Serialize)]
struct PublishInfo {
    level: &'static str,
    /// Titles of runtime families a Compact package leaves out.
    omitted_modules: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "renderMode")]
    render_mode: Option<&'static str>,
}

/// Inspects a project for graphics features that cannot be represented as pure semantic HTML/SVG
/// in [`HtmlRenderMode::Dom`] and will therefore render inside scoped `<canvas>` islands (`T-307`).
pub fn dom_publish_diagnostics(project: &Project) -> Vec<String> {
    let mut diagnostics = Vec::new();
    for symbol in &project.symbols {
        let mut has_filters = false;
        let mut has_non_css_blend = false;
        let mut has_bitmap_fill = false;
        for layer in &symbol.timeline.layers {
            if !layer.properties.filters.is_empty() {
                has_filters = true;
            }
            for key in &layer.keyframes {
                if !key.filters.is_empty() {
                    has_filters = true;
                }
                if !key.bitmap_fills.is_empty() {
                    has_bitmap_fill = true;
                }
                for element in &key.elements {
                    if !element.filters.is_empty() {
                        has_filters = true;
                    }
                    if matches!(
                        element.blend_mode,
                        neoflash_core::BlendMode::Add
                            | neoflash_core::BlendMode::Subtract
                            | neoflash_core::BlendMode::Invert
                            | neoflash_core::BlendMode::Alpha
                            | neoflash_core::BlendMode::Erase
                    ) {
                        has_non_css_blend = true;
                    }
                }
            }
        }
        let mut reasons = Vec::new();
        if has_filters {
            reasons.push("bitmap filters");
        }
        if has_non_css_blend {
            reasons.push("composited blend modes");
        }
        if has_bitmap_fill {
            reasons.push("bitmap pattern fills");
        }
        if !reasons.is_empty() {
            diagnostics.push(format!(
                "DOM publish mode: symbol '{}' uses {} that cannot be expressed as pure semantic DOM; rendering affected artwork in scoped <canvas> islands.",
                symbol.name,
                reasons.join(", ")
            ));
        }
    }
    diagnostics
}

#[derive(Serialize)]
struct ProgramBundle<'a> {
    code: &'a str,
    class_names: &'a [String],
    source_url: &'static str,
    source_map: Option<&'a neoflash_script::SourceMap>,
    source_locations: Vec<neoflash_script::RuntimeSourceLocation>,
}

/// Reports current publish limitations before the user exports their work, excluding located
/// foreign-script policy warnings when a caller (`Compiler errors`) already emits script-located
/// rows for them.
pub fn export_warnings_without_foreign_scripts(project: &Project) -> Vec<String> {
    let embedded = embedded_font_families(project);
    let mut fonts = BTreeSet::new();
    let mut warnings = Vec::new();
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    match &element.kind {
                        ElementKind::Text { font_family, .. } => {
                            fonts.insert(font_family.clone());
                        }
                        ElementKind::TextBox { format, .. } => {
                            let embedded_only = format
                                .field_options()
                                .is_some_and(|options| options.embed_fonts);
                            if embedded_only {
                                if !has_embedded_font_face(
                                    project,
                                    &format.font,
                                    format.bold,
                                    format.italic,
                                ) {
                                    let style = match (format.bold, format.italic) {
                                        (true, true) => "Bold Italic",
                                        (true, false) => "Bold",
                                        (false, true) => "Italic",
                                        (false, false) => "Regular",
                                    };
                                    let field = if element.name.trim().is_empty() {
                                        element.id.as_str()
                                    } else {
                                        element.name.as_str()
                                    };
                                    warnings.push(format!(
                                        "Text field '{field}' requires embedded font '{}' ({style}), but no matching embedded face is configured. Select the field, open Properties ▸ Embed font ▸ Embedding…, and add that face before publishing; embedded-only text can otherwise be blank.",
                                        format.font
                                    ));
                                }
                            } else {
                                fonts.insert(format.font.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    warnings.extend(
        fonts
            .into_iter()
            .filter(|font| !embedded.contains(&font.to_ascii_lowercase()))
            .map(|font| format!(
                "Font '{font}' is referenced by name, not embedded. Visitors need this font installed; otherwise the browser uses a fallback."
            )),
    );
    let non_font_resources = project
        .resources
        .iter()
        .filter(|res| {
            !is_font_resource(res)
                && !res
                    .path
                    .eq_ignore_ascii_case(Project::PUBLISH_SETTINGS_RESOURCE_PATH)
        })
        .count();
    if non_font_resources > 0 {
        let user_resource_total = project
            .resources
            .iter()
            .filter(|res| {
                !res.path
                    .eq_ignore_ascii_case(Project::PUBLISH_SETTINGS_RESOURCE_PATH)
            })
            .count();
        warnings.push(format!("{} project resource(s) are embedded and available by their relative paths. Embedding bytes alone does not create display placements or install fonts.", user_resource_total));
    }
    if project.html_render_mode() == HtmlRenderMode::Dom {
        warnings.extend(dom_publish_diagnostics(project));
    }
    warnings
}

/// Reports current publish limitations before the user exports their work.
pub fn export_warnings(project: &Project) -> Vec<String> {
    let mut warnings = export_warnings_without_foreign_scripts(project);
    for (loc, msg, _) in foreign_script_policy_entries(project) {
        warnings.push(format!("{loc}: {msg}"));
    }
    warnings
}

pub(crate) fn foreign_script_policy_entries(project: &Project) -> Vec<(String, String, bool)> {
    let Some(proj_lang) = project.script_language else {
        return Vec::new();
    };
    let also_compat = project.also_compile_compatible_scripts;
    let mut entries = Vec::new();
    let mut record = |loc: String, lang: neoflash_core::ScriptLanguage| match project
        .script_disposition_for(proj_lang, also_compat, lang)
    {
        neoflash_core::ScriptDisposition::CompiledPrimary => {}
        neoflash_core::ScriptDisposition::CompiledCompatibleWithWarning => {
            entries.push((
                loc,
                format!(
                    "Compiled as compatible language: script is {}, project uses {}.",
                    lang.display_label(),
                    proj_lang.display_label()
                ),
                false,
            ));
        }
        neoflash_core::ScriptDisposition::SkippedForeign => {
            entries.push((
                loc,
                format!(
                    "Skipped when publishing: script is {}, project uses {}.",
                    lang.display_label(),
                    proj_lang.display_label()
                ),
                true,
            ));
        }
    };
    for unit in &project.sources {
        record(unit.path.clone(), unit.language);
    }
    for symbol in &project.symbols {
        for frame in &symbol.timeline.scripts {
            if !frame.script.source.trim().is_empty() {
                record(
                    format!("{} / frame {}", symbol.name, frame.frame + 1),
                    frame.script.language,
                );
            }
        }
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    for handler in &element.scripts {
                        if !handler.script.source.trim().is_empty() {
                            let elem_label = if element.name.trim().is_empty() {
                                element.id.as_str()
                            } else {
                                element.name.as_str()
                            };
                            record(
                                format!(
                                    "{} / {} / frame {} / {} / {:?}",
                                    symbol.name,
                                    layer.name,
                                    key.frame + 1,
                                    elem_label,
                                    handler.event
                                ),
                                handler.script.language,
                            );
                        }
                    }
                }
            }
        }
    }
    entries
}

fn source_unit_defines_class(unit: &neoflash_core::SourceUnit, short: &str) -> bool {
    if short.is_empty() {
        return false;
    }
    if unit.language.is_actionscript()
        && unit
            .path
            .rsplit(['/', '\\'])
            .next()
            .and_then(|file| file.strip_suffix(".as"))
            .is_some_and(|stem| stem == short)
    {
        return true;
    }
    let needle = format!("class {short}");
    let mut rest = unit.source.as_str();
    while let Some(pos) = rest.find(&needle) {
        let before_ok = pos == 0
            || !rest[..pos]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$');
        let after = &rest[pos + needle.len()..];
        let after_ok = !after
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$');
        if before_ok && after_ok {
            return true;
        }
        rest = &rest[pos + 5..];
    }
    false
}

pub(crate) fn strip_skipped_foreign_scripts(project: &Project) -> std::borrow::Cow<'_, Project> {
    let Some(proj_lang) = project.script_language else {
        return std::borrow::Cow::Borrowed(project);
    };
    let also_compat = project.also_compile_compatible_scripts;
    let is_skipped = |lang: neoflash_core::ScriptLanguage| {
        !project
            .script_disposition_for(proj_lang, also_compat, lang)
            .is_compiled()
    };
    let has_skipped = project.sources.iter().any(|u| is_skipped(u.language))
        || project.symbols.iter().any(|s| {
            s.timeline
                .scripts
                .iter()
                .any(|fs| is_skipped(fs.script.language))
                || s.timeline.layers.iter().any(|l| {
                    l.keyframes.iter().any(|k| {
                        k.elements
                            .iter()
                            .any(|e| e.scripts.iter().any(|os| is_skipped(os.script.language)))
                    })
                })
        });
    if !has_skipped {
        return std::borrow::Cow::Borrowed(project);
    }
    let skipped_units: Vec<&neoflash_core::SourceUnit> = project
        .sources
        .iter()
        .filter(|u| is_skipped(u.language))
        .collect();
    let mut copy = project.clone();
    copy.sources.retain(|u| !is_skipped(u.language));
    if !skipped_units.is_empty() {
        let defined_only_in_skipped = |qname: &str| {
            let short = qname.rsplit('.').next().unwrap_or(qname).trim();
            !short.is_empty()
                && skipped_units
                    .iter()
                    .any(|u| source_unit_defines_class(u, short))
                && !copy
                    .sources
                    .iter()
                    .any(|u| source_unit_defines_class(u, short))
        };
        if copy
            .document_class
            .as_deref()
            .is_some_and(defined_only_in_skipped)
        {
            copy.document_class = None;
        }
        for sym in &mut copy.symbols {
            if sym
                .class_name
                .as_deref()
                .is_some_and(defined_only_in_skipped)
            {
                sym.class_name = None;
            }
        }
    }
    for sym in &mut copy.symbols {
        sym.timeline
            .scripts
            .retain(|fs| !is_skipped(fs.script.language));
        for layer in &mut sym.timeline.layers {
            for key in &mut layer.keyframes {
                for el in &mut key.elements {
                    el.scripts.retain(|os| !is_skipped(os.script.language));
                }
            }
        }
    }
    std::borrow::Cow::Owned(copy)
}

pub(crate) fn compile_scripts(
    project: &Project,
    program: &neoflash_script::ProjectProgram,
) -> Result<Vec<CompiledScript>, ExportError> {
    let mut scripts: Vec<CompiledScript> = Vec::new();
    let mut failures = Vec::new();
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    let mut add = |script: &Script, location: String| {
        if script.source.trim().is_empty() {
            return;
        }
        if let Some(proj_lang) = project.script_language
            && !project
                .script_disposition_for(
                    proj_lang,
                    project.also_compile_compatible_scripts,
                    script.language,
                )
                .is_compiled()
        {
            return;
        }
        let key = serde_json::to_string(script).expect("Script contains only serializable strings");
        if let Some(&index) = seen.get(&key) {
            if !scripts[index].locations.contains(&location) {
                scripts[index].locations.push(location);
            }
            return;
        }
        let source_url = format!("neoflash-script-{}.js", scripts.len());
        match neoflash_script::compile_with_program_source_map(script, program, &location) {
            Ok(compiled) => {
                seen.insert(key, scripts.len());
                scripts.push(CompiledScript {
                    language: script.language,
                    source: script.source.clone(),
                    code: compiled.code,
                    locations: vec![location.clone()],
                    location,
                    source_url,
                    source_locations: compiled.source_map.runtime_locations(),
                    source_map: compiled.source_map,
                });
            }
            Err(errors) => {
                for error in errors {
                    failures.push(format!(
                        "{location}, line {}, column {}: {}",
                        error.line, error.column, error.message
                    ));
                }
            }
        }
    };
    for symbol in &project.symbols {
        for frame in &symbol.timeline.scripts {
            add(
                &frame.script,
                format!("{} / frame {}", symbol.name, frame.frame + 1),
            );
        }
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    for handler in &element.scripts {
                        add(
                            &handler.script,
                            format!(
                                "{} / {} / frame {} / {} / {:?}",
                                symbol.name,
                                layer.name,
                                key.frame + 1,
                                element.name,
                                handler.event
                            ),
                        );
                    }
                }
            }
        }
    }
    // Runtime dispatch currently identifies script text, not its authored placement.
    // Give deduplicated text an honest shared identity instead of claiming the first frame.
    for (index, script) in scripts.iter_mut().enumerate() {
        if script.locations.len() > 1 {
            let name = format!("Shared script {}", index + 1);
            for source in &mut script.source_map.sources {
                if *source == script.location {
                    *source = name.clone();
                }
            }
            for location in &mut script.source_locations {
                if location.source == script.location {
                    location.source = name.clone();
                }
            }
            script.location = name;
        }
    }
    if failures.is_empty() {
        Ok(scripts)
    } else {
        Err(ExportError::Scripts(failures.join("\n")))
    }
}

/// Authoring-only data that never reaches the published bundle: ruler guides and foreign scripts
/// skipped by `R-102` policy. Returns a stripped copy only when there is something to strip.
pub(crate) fn publishable(project: &Project) -> Option<Project> {
    let has_guides = project
        .symbols
        .iter()
        .any(|symbol| !symbol.timeline.guides.is_empty());
    let stripped = strip_skipped_foreign_scripts(project);
    if !has_guides && matches!(stripped, std::borrow::Cow::Borrowed(_)) {
        return None;
    }
    let mut copy = stripped.into_owned();
    if has_guides {
        for symbol in &mut copy.symbols {
            symbol.timeline.guides.clear();
        }
    }
    Some(copy)
}

/// Export an entirely local package that also works when index.html is opened directly.
/// `runtime_dir` contains wasm-bindgen's `--target no-modules` output, global `NeoFlashWasm`.
/// Existing unrelated directories are never overwritten.
pub fn export_project(
    project: &Project,
    output_dir: &Path,
    runtime_dir: &Path,
) -> Result<PathBuf, ExportError> {
    export_project_with_policy(project, output_dir, runtime_dir, FontExportPolicy::Embed)
}

/// Export an HTML5 package with an explicit font export policy (`Device`, `Embed`, `Subset`, `Outlines`).
pub fn export_project_with_policy(
    project: &Project,
    output_dir: &Path,
    runtime_dir: &Path,
    font_policy: FontExportPolicy,
) -> Result<PathBuf, ExportError> {
    let options = PublishOptions {
        fonts: font_policy,
        size: SizeLevel::Full,
        audio: AudioExportPolicy::Original,
    };
    export_project_with_options(project, output_dir, runtime_dir, &options)
}

/// Export an HTML5 package at a size level (see [`SizeLevel`]) with a font policy.
pub fn export_project_with_options(
    project: &Project,
    output_dir: &Path,
    runtime_dir: &Path,
    options: &PublishOptions,
) -> Result<PathBuf, ExportError> {
    if project.html_render_mode() == HtmlRenderMode::Dom {
        let dom_cfg = HtmlPublishConfig {
            render_mode: HtmlRenderMode::Dom,
            breakpoints: project.publish_settings().breakpoints,
            ..HtmlPublishConfig::default()
        };
        return export_project_with_config(project, output_dir, runtime_dir, options, &dom_cfg);
    }
    let options = options.validated().map_err(ExportError::Options)?;
    let prepared = prepare(project, runtime_dir, options.fonts, options.audio)?;
    let package = assemble(project, &prepared, &options.size)?;
    write_package(output_dir, &package.files, &project.name, "project")
}

/// Export an HTML5 package with unified [`HtmlExportOptions`] (`options`, `config`, and
/// `render_mode`: `Canvas` default or `Dom` for interactive websites and applications, `T-307`).
pub fn export_html_with_options(
    project: &Project,
    output_dir: &Path,
    runtime_dir: &Path,
    html_options: &HtmlExportOptions,
) -> Result<PathBuf, ExportError> {
    let effective_mode = if html_options.render_mode != HtmlRenderMode::Canvas {
        html_options.render_mode
    } else if html_options.config.render_mode != HtmlRenderMode::Canvas {
        html_options.config.render_mode
    } else {
        project.html_render_mode()
    };
    if effective_mode == HtmlRenderMode::Canvas
        && html_options.config == HtmlPublishConfig::default()
    {
        return export_project_with_options(
            project,
            output_dir,
            runtime_dir,
            &html_options.options,
        );
    }
    let mut cfg = html_options.config.clone();
    cfg.render_mode = effective_mode;
    if cfg.breakpoints.is_empty() && effective_mode == HtmlRenderMode::Dom {
        cfg.breakpoints = project.publish_settings().breakpoints;
    }
    export_project_with_config(
        project,
        output_dir,
        runtime_dir,
        &html_options.options,
        &cfg,
    )
}

/// Export an HTML5 package with explicit [`PublishOptions`] and [`HtmlPublishConfig`]
/// (Clean publish vs. Preview chrome, HTML wrapper template, playback flags, source maps,
/// JPEG quality, sprite sheets, and script minification).
pub fn export_project_with_config(
    project: &Project,
    output_dir: &Path,
    runtime_dir: &Path,
    options: &PublishOptions,
    config: &HtmlPublishConfig,
) -> Result<PathBuf, ExportError> {
    let options = options.validated().map_err(ExportError::Options)?;
    let mut owned_cfg: Option<HtmlPublishConfig> = None;
    if config.render_mode == HtmlRenderMode::Canvas
        && project.html_render_mode() == HtmlRenderMode::Dom
    {
        let mut next = config.clone();
        next.render_mode = HtmlRenderMode::Dom;
        if next.breakpoints.is_empty() {
            next.breakpoints = project.publish_settings().breakpoints;
        }
        owned_cfg = Some(next);
    } else if config.render_mode == HtmlRenderMode::Dom && config.breakpoints.is_empty() {
        let doc_bps = project.publish_settings().breakpoints;
        if !doc_bps.is_empty() {
            let mut next = config.clone();
            next.breakpoints = doc_bps;
            owned_cfg = Some(next);
        }
    }
    let effective_cfg = owned_cfg.as_ref().unwrap_or(config);
    let prepared = prepare_with_config(
        project,
        runtime_dir,
        options.fonts,
        options.audio,
        Some(effective_cfg),
    )?;
    let package = assemble_with_config(project, &prepared, &options.size, Some(effective_cfg))?;
    write_package(output_dir, &package.files, &project.name, "project")
}

/// Export a self-contained Desktop application bundle (`.app` on macOS plus portable Windows
/// `.bat` and Linux `.sh` launchers) wrapping the Clean HTML5 package in `output_dir`.
pub fn export_desktop_app(
    project: &Project,
    output_dir: &Path,
    runtime_dir: &Path,
    options: &PublishOptions,
    config: &HtmlPublishConfig,
) -> Result<PathBuf, ExportError> {
    let mut app_config = config.clone();
    if app_config.render_mode == HtmlRenderMode::Canvas
        && project.html_render_mode() == HtmlRenderMode::Dom
    {
        app_config.render_mode = HtmlRenderMode::Dom;
    }
    if app_config.render_mode == HtmlRenderMode::Dom && app_config.breakpoints.is_empty() {
        app_config.breakpoints = project.publish_settings().breakpoints;
    }
    app_config.include_html = true;
    app_config.sprite_sheets = false;
    app_config.assets_folder = false;
    app_config.chrome = PlayerChromeConfig::clean_publish();
    let safe_name: String = project
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_owned();
    let app_stem = if safe_name.is_empty() {
        "NeoFlashApp".to_owned()
    } else {
        safe_name
    };
    let app_dir = output_dir.join(format!("{app_stem}.app"));
    let contents_dir = app_dir.join("Contents");
    let macos_dir = contents_dir.join("MacOS");
    let web_dir = contents_dir.join("Resources").join("web");
    if has_non_ignorable_dir_entries(&app_dir)? && !web_dir.join(MARKER).exists() {
        return Err(ExportError::Destination(app_dir.display().to_string()));
    }
    let options = options.validated().map_err(ExportError::Options)?;
    let prepared = prepare_with_config(
        project,
        runtime_dir,
        options.fonts,
        options.audio,
        Some(&app_config),
    )?;
    let package = assemble_with_config(project, &prepared, &options.size, Some(&app_config))?;
    fs::create_dir_all(&macos_dir)?;
    fs::create_dir_all(&web_dir)?;
    write_package(&web_dir, &package.files, &project.name, "project")?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>{}</string>
  <key>CFBundleIdentifier</key>
  <string>org.neoflash.published.{}</string>
  <key>CFBundleName</key>
  <string>{}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
"#,
        html_escape(&app_stem),
        app_stem
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase(),
        html_escape(&project.name),
    );
    atomic_write(&contents_dir.join("Info.plist"), plist.as_bytes())?;
    let launcher = "#!/bin/sh\nDIR=\"$(cd \"$(dirname \"$0\")/../Resources/web\" && pwd)\"\nopen \"$DIR/index.html\"\n";
    let exec_path = macos_dir.join(&app_stem);
    atomic_write(&exec_path, launcher.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&exec_path, fs::Permissions::from_mode(0o755));
    }
    let win_bat = format!(
        "@echo off\r\nstart \"\" \"%~dp0{app_stem}.app\\Contents\\Resources\\web\\index.html\"\r\n"
    );
    atomic_write(
        &output_dir.join(format!("Launch {app_stem}.bat")),
        win_bat.as_bytes(),
    )?;
    let linux_launcher = format!(
        r#"#!/bin/sh
set -eu
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
INDEX="$SCRIPT_DIR/{app_stem}.app/Contents/Resources/web/index.html"
if command -v xdg-open >/dev/null 2>&1; then
    if xdg-open "$INDEX"; then
        exit 0
    fi
fi
if command -v gio >/dev/null 2>&1; then
    exec gio open "$INDEX"
fi
printf '%s\n' 'NeoFlash could not find xdg-open or gio to open the published application.' >&2
exit 127
"#
    );
    let linux_launcher_path = output_dir.join(format!("Launch {app_stem}.sh"));
    atomic_write(&linux_launcher_path, linux_launcher.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&linux_launcher_path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(app_dir)
}

/// Measure every size level for the publish dialog by assembling each package in memory.
/// `runtime_url` is where the Shared level would load the runtime from.
pub fn estimate_publish_sizes(
    project: &Project,
    runtime_dir: &Path,
    font_policy: FontExportPolicy,
    runtime_url: &str,
) -> Result<Vec<SizeEstimate>, ExportError> {
    Ok(estimate_publish_levels_cancellable(
        project,
        runtime_dir,
        font_policy,
        &SizeLevel::all(runtime_url),
        || false,
    )?
    .expect("uncancelled estimate"))
}

/// Measure the exact options used by a publication, without writing any output files.
pub fn estimate_publish_options(
    project: &Project,
    runtime_dir: &Path,
    options: &PublishOptions,
) -> Result<SizeEstimate, ExportError> {
    let options = options.validated().map_err(ExportError::Options)?;
    Ok(estimate_publish_levels_with_audio_cancellable(
        project,
        runtime_dir,
        options.fonts,
        options.audio,
        &[options.size],
        || false,
    )?
    .expect("uncancelled estimate")
    .remove(0))
}

/// Measure the exact [`PublishOptions`] and [`HtmlPublishConfig`] used by a publication,
/// with cooperative cancellation.
pub fn estimate_publish_options_cancellable_with_config(
    project: &Project,
    runtime_dir: &Path,
    options: &PublishOptions,
    config: &HtmlPublishConfig,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<SizeEstimate>, ExportError> {
    if cancelled() {
        return Ok(None);
    }
    let options = options.validated().map_err(ExportError::Options)?;
    let prepared = prepare_with_config(
        project,
        runtime_dir,
        options.fonts,
        options.audio,
        Some(config),
    )?;
    if cancelled() {
        return Ok(None);
    }
    let shared = if matches!(options.size, SizeLevel::Shared(_)) {
        let files =
            shared_runtime_package_files(&prepared.bindings, &prepared.wasm, &prepared.runtime_id);
        let marker = package_marker(&files, "shared runtime", "runtime")?;
        let mut bytes = marker.len();
        let mut gzip = gzip_len(&marker);
        for (_, file) in &files {
            if cancelled() {
                return Ok(None);
            }
            bytes += file.len();
            gzip += gzip_len(file);
        }
        Some((bytes, gzip))
    } else {
        None
    };
    let package = assemble_with_config(project, &prepared, &options.size, Some(config))?;
    measure(
        options.size,
        &package,
        &project.name,
        shared,
        &mut cancelled,
    )
}

/// Measure requested levels in order, sharing preparation, without writing output files.
/// `None` means cancelled, never a partial result. Cancellation is cooperative between
/// preparation, package assembly and individual file compression; those operations finish
/// before cancellation is observed. Call from a worker when keeping the UI responsive.
pub fn estimate_publish_levels_cancellable(
    project: &Project,
    runtime_dir: &Path,
    font_policy: FontExportPolicy,
    levels: &[SizeLevel],
    cancelled: impl FnMut() -> bool,
) -> Result<Option<Vec<SizeEstimate>>, ExportError> {
    estimate_publish_levels_with_audio_cancellable(
        project,
        runtime_dir,
        font_policy,
        AudioExportPolicy::Original,
        levels,
        cancelled,
    )
}

/// Measure requested levels with both font and audio export policies.
pub fn estimate_publish_levels_with_audio_cancellable(
    project: &Project,
    runtime_dir: &Path,
    font_policy: FontExportPolicy,
    audio_policy: AudioExportPolicy,
    levels: &[SizeLevel],
    mut cancelled: impl FnMut() -> bool,
) -> Result<Option<Vec<SizeEstimate>>, ExportError> {
    if cancelled() {
        return Ok(None);
    }
    let levels = levels
        .iter()
        .map(|size| {
            PublishOptions {
                fonts: font_policy,
                size: size.clone(),
                audio: audio_policy,
            }
            .validated()
            .map(|options| options.size)
            .map_err(ExportError::Options)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if levels.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let prepared = prepare(project, runtime_dir, font_policy, audio_policy)?;
    if cancelled() {
        return Ok(None);
    }
    let mut shared_weight = None;
    let mut estimates = Vec::with_capacity(levels.len());
    for level in levels {
        if cancelled() {
            return Ok(None);
        }
        let shared = if matches!(level, SizeLevel::Shared(_)) {
            if shared_weight.is_none() {
                let files = shared_runtime_package_files(
                    &prepared.bindings,
                    &prepared.wasm,
                    &prepared.runtime_id,
                );
                let marker = package_marker(&files, "shared runtime", "runtime")?;
                let mut bytes = marker.len();
                let mut gzip = gzip_len(&marker);
                for (_, file) in &files {
                    if cancelled() {
                        return Ok(None);
                    }
                    bytes += file.len();
                    gzip += gzip_len(file);
                }
                shared_weight = Some((bytes, gzip));
            }
            shared_weight
        } else {
            None
        };
        let package = assemble(project, &prepared, &level)?;
        let Some(estimate) = measure(level, &package, &project.name, shared, &mut cancelled)?
        else {
            return Ok(None);
        };
        estimates.push(estimate);
    }
    Ok((!cancelled()).then_some(estimates))
}

/// Publish the runtime folder that Shared-level projects load from `--runtime-url`. Publish it
/// again from each NeoFlash build: projects check that they were published against the same one.
pub fn export_shared_runtime(
    output_dir: &Path,
    runtime_dir: &Path,
) -> Result<PathBuf, ExportError> {
    let (bindings, wasm) = read_runtime(runtime_dir)?;
    let prepared_id = shared_runtime_id(&bindings, &wasm);
    let files = shared_runtime_package_files(&bindings, &wasm, &prepared_id);
    write_package(output_dir, &files, "shared runtime", "runtime")?;
    Ok(output_dir.to_path_buf())
}

/// The compiled project and runtime files every size level starts from.
pub(crate) struct Prepared {
    pub(crate) program: neoflash_script::ProjectProgram,
    pub(crate) scripts: Vec<CompiledScript>,
    pub(crate) published: Project,
    pub(crate) scenes: Vec<SceneRange>,
    pub(crate) fonts: FontBundle,
    pub(crate) external_videos: Vec<neoflash_core::ExternalVideoAsset>,
    pub(crate) video_manifest: Vec<neoflash_core::VideoManifestEntry>,
    pub(crate) sequence_pack_atlas: bool,
    pub(crate) resource_aliases: std::collections::BTreeMap<String, String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) bindings: String,
    pub(crate) wasm: Vec<u8>,
    pub(crate) runtime_id: String,
}

/// An assembled package: files in write order (index.html last) and what went into them.
pub(crate) struct Package {
    pub(crate) files: Vec<(String, Vec<u8>)>,
    pub(crate) script_srcs: Vec<String>,
    pub(crate) level: SizeLevel,
    pub(crate) omitted: Vec<&'static str>,
    pub(crate) wasm_bytes: usize,
    pub(crate) runtime_js_bytes: usize,
    /// Scripts, content, resources and fonts inside `bundle.js`, each compressed on its own.
    pub(crate) embedded: Vec<SizePart>,
}

fn reserved_publish_path(path: &str) -> bool {
    const GENERATED: &[&str] = &[
        MARKER,
        BINDINGS_FILE,
        WASM_FILE,
        RUNTIME_ID_FILE,
        NOTICES_FILE,
        "runtime.js",
        "bundle.js",
        "bundle.js.map",
        MODULE_FILE,
        "sprite_sheet.png",
        "sprite_sheet.json",
        "assets/sprite_sheet.png",
        "assets/sprite_sheet.json",
        "README.txt",
        "index.html",
    ];
    GENERATED
        .iter()
        .any(|name| package_paths_collide(path, name))
        || HOST_MODULES
            .iter()
            .any(|(name, _)| package_paths_collide(path, name))
}

fn package_paths_collide(left: &str, right: &str) -> bool {
    fn component_descendant(path: &str, ancestor: &str) -> bool {
        path.len() > ancestor.len()
            && path.starts_with(ancestor)
            && path.as_bytes().get(ancestor.len()) == Some(&b'/')
    }

    let left = left.to_lowercase();
    let right = right.to_lowercase();
    left == right || component_descendant(&left, &right) || component_descendant(&right, &left)
}

fn windows_device_name(name: &str) -> bool {
    let base = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_uppercase();
    matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || base
        .strip_prefix("COM")
        .or_else(|| base.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

pub(crate) fn validate_portable_path_component(component: &str) -> Result<(), String> {
    if component.is_empty() || matches!(component, "." | "..") {
        return Err("path component must not be empty, '.' or '..'".into());
    }
    if component.ends_with([' ', '.']) {
        return Err("path component must not end in a space or dot".into());
    }
    if component.chars().any(|character| {
        character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
    }) {
        return Err("path component contains characters forbidden on Windows".into());
    }
    if component.len() > 255 || component.encode_utf16().count() > 255 {
        return Err("path component exceeds portable filesystem limits".into());
    }
    if windows_device_name(component) {
        return Err("path component is a reserved Windows device name".into());
    }
    Ok(())
}

fn validate_package_relative_path(name: &str) -> Result<(), ExportError> {
    neoflash_core::validate_external_video_path(name).map_err(|reason| {
        ExportError::Options(format!("invalid package file path '{name}': {reason}"))
    })?;
    for component in name.split('/') {
        validate_portable_path_component(component).map_err(|reason| {
            ExportError::Options(format!("invalid package file path '{name}': {reason}"))
        })?;
    }
    if name.eq_ignore_ascii_case(MARKER) {
        return Err(ExportError::Options(format!(
            "package file '{name}' collides with the NeoFlash ownership marker"
        )));
    }
    Ok(())
}

fn validate_package_path_set<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> Result<(), ExportError> {
    let mut validated: Vec<&str> = Vec::new();
    for name in names {
        validate_package_relative_path(name)?;
        if let Some(previous) = validated
            .iter()
            .find(|previous| package_paths_collide(name, previous))
        {
            return Err(ExportError::Options(format!(
                "package paths '{previous}' and '{name}' collide by name or file/directory prefix"
            )));
        }
        validated.push(name);
    }
    Ok(())
}

fn validate_package_file_names(files: &[(String, Vec<u8>)]) -> Result<(), ExportError> {
    validate_package_path_set(files.iter().map(|(name, _)| name.as_str()))
}

fn read_runtime(runtime_dir: &Path) -> Result<(String, Vec<u8>), ExportError> {
    let bindings_path = runtime_dir.join(BINDINGS_FILE);
    let wasm_path = runtime_dir.join(WASM_FILE);
    let bindings = fs::read_to_string(&bindings_path)
        .map_err(|_| ExportError::MissingRuntime(bindings_path.display().to_string()))?;
    if !bindings.contains("NeoFlashWasm") {
        return Err(ExportError::MissingRuntime(
            "bindings must use --target no-modules --no-modules-global NeoFlashWasm".into(),
        ));
    }
    let wasm = fs::read(&wasm_path)
        .map_err(|_| ExportError::MissingRuntime(wasm_path.display().to_string()))?;
    if !wasm.starts_with(b"\0asm") {
        return Err(ExportError::MissingRuntime(
            "invalid WebAssembly header".into(),
        ));
    }
    Ok((bindings, wasm))
}

fn shared_runtime_id(bindings: &str, wasm: &[u8]) -> String {
    let mut parts: Vec<&[u8]> = vec![bindings.as_bytes(), wasm, HOST.as_bytes()];
    parts.extend(HOST_MODULES.iter().map(|(_, source)| source.as_bytes()));
    publish::runtime_id(&parts)
}

pub(crate) fn prepare(
    project: &Project,
    runtime_dir: &Path,
    font_policy: FontExportPolicy,
    audio_policy: AudioExportPolicy,
) -> Result<Prepared, ExportError> {
    prepare_with_config(project, runtime_dir, font_policy, audio_policy, None)
}

pub(crate) fn prepare_with_config(
    project: &Project,
    runtime_dir: &Path,
    font_policy: FontExportPolicy,
    audio_policy: AudioExportPolicy,
    config: Option<&HtmlPublishConfig>,
) -> Result<Prepared, ExportError> {
    let issues = neoflash_core::validate(project);
    if !issues.is_empty() {
        return Err(ExportError::InvalidProject(format!("{issues:?}")));
    }
    let program = neoflash_script::compile_project(project).map_err(|errors| {
        ExportError::Scripts(
            errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;
    // Scripts compile from the authored scenes so diagnostics name the scene
    // and its own frame; flattening keeps every script source unchanged.
    let scripts = compile_scripts(project, &program)?;
    let (published, scenes) = neoflash_core::flatten_scenes_with_ranges(project);
    let (bindings, wasm) = read_runtime(runtime_dir)?;
    let mut published = publishable(&published).unwrap_or(published);
    let (avi_warnings, resource_aliases) = transcode_avi_resources_for_html5(&mut published);
    let mut seq_warnings = Vec::new();
    let mut sequence_pack_atlas = false;
    if let Some(cfg) = config {
        let (pack_atlas, warnings) =
            apply_sequence_publish_policy(&mut published, cfg.sequence_policy);
        sequence_pack_atlas = pack_atlas;
        seq_warnings = warnings;
    }
    let video_manifest = neoflash_core::video_manifest_entries(&published);
    let external_videos = neoflash_core::strip_non_embedded_video_bytes(&mut published);
    let fonts = apply_font_policy(&mut published, font_policy);
    apply_audio_policy(&mut published, audio_policy)?;
    let default_jpeg_q = config.map(|c| c.jpeg_quality).unwrap_or(80);
    let (_, bitmap_warnings) = apply_bitmap_export_settings(&mut published, default_jpeg_q);
    let runtime_id = shared_runtime_id(&bindings, &wasm);
    let mut warnings = export_warnings(project);
    if config.is_some_and(|c| c.render_mode == HtmlRenderMode::Dom)
        && project.html_render_mode() != HtmlRenderMode::Dom
    {
        warnings.extend(dom_publish_diagnostics(project));
    }
    warnings.extend(avi_warnings);
    warnings.extend(seq_warnings);
    warnings.extend(bitmap_warnings);
    if font_policy == FontExportPolicy::Subset {
        for stats in &fonts.subset_stats {
            if !stats.subset_applied
                && let Some(diagnostic) = &stats.diagnostic
            {
                warnings.push(format!("Font '{}': {}", stats.family, diagnostic));
            }
        }
    }
    warnings.extend(fonts.outline_diagnostics.clone());
    Ok(Prepared {
        program,
        scripts,
        published,
        scenes,
        fonts,
        external_videos,
        video_manifest,
        sequence_pack_atlas,
        resource_aliases,
        warnings,
        bindings,
        wasm,
        runtime_id,
    })
}

fn transcode_avi_resources_for_html5(
    project: &mut Project,
) -> (Vec<String>, std::collections::BTreeMap<String, String>) {
    let mut warnings = Vec::new();
    let ffmpeg = video::detect_ffmpeg();
    let mut path_renames: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut used_paths_lower: std::collections::HashSet<String> = project
        .resources
        .iter()
        .map(|r| r.path.to_ascii_lowercase())
        .collect();
    for resource in &mut project.resources {
        let is_avi_mime = resource.media_type.eq_ignore_ascii_case("video/x-msvideo")
            || resource.media_type.eq_ignore_ascii_case("video/avi");
        let is_flv_mime = resource.media_type.eq_ignore_ascii_case("video/x-flv")
            || resource.media_type.eq_ignore_ascii_case("video/flv");
        let Ok(raw_bytes) = resource.decoded_bytes() else {
            continue;
        };
        if raw_bytes.is_empty() {
            continue;
        }
        let is_avi = is_avi_mime || neoflash_core::is_avi_bytes(&raw_bytes);
        let is_flv =
            !is_avi && (is_flv_mime || neoflash_core::timeline_video::is_flv_bytes(&raw_bytes));
        if !is_avi && !is_flv {
            continue;
        }
        let Some(ffmpeg_bin) = ffmpeg.as_ref() else {
            if is_flv {
                warnings.push(format!(
                    "FLV video '{}' kept in FLV container (install ffmpeg for H.264 MP4 / VP9 WebM HTML5 transcoding).",
                    resource.path
                ));
            } else {
                warnings.push(format!(
                    "AVI video '{}' kept in RIFF/AVI container (install ffmpeg for H.264 MP4 transcoding; MJPEG/PNG/RGB AVI plays via Canvas fallback).",
                    resource.path
                ));
            }
            continue;
        };
        let use_webm_alpha =
            is_flv && neoflash_core::timeline_video::flv_has_vp6a_alpha(&raw_bytes);
        static AVI_TEMP_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = AVI_TEMP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp_dir = std::env::temp_dir().join(format!(
            "neoflash-avi-transcode-{}-{nonce}-{seq}",
            std::process::id()
        ));
        if std::fs::create_dir_all(&temp_dir).is_err() {
            continue;
        }
        let in_path = temp_dir.join(if is_flv { "input.flv" } else { "input.avi" });
        let (target_ext, target_mime) = if use_webm_alpha {
            ("webm", "video/webm")
        } else {
            ("mp4", "video/mp4")
        };
        let out_path = temp_dir.join(format!("output.{target_ext}"));
        let transcoded = std::fs::write(&in_path, &raw_bytes).ok().and_then(|()| {
            let mut cmd = std::process::Command::new(ffmpeg_bin);
            video::configure_media_subprocess_command(&mut cmd, ffmpeg_bin);
            cmd.args(["-y", "-hide_banner", "-loglevel", "error"]);
            if is_flv {
                cmd.args(["-f", "flv"]);
            }
            cmd.args(["-i", in_path.to_str()?]);
            if use_webm_alpha {
                cmd.args([
                    "-c:v",
                    "libvpx-vp9",
                    "-b:v",
                    "0",
                    "-crf",
                    "30",
                    "-pix_fmt",
                    "yuva420p",
                    "-c:a",
                    "libopus",
                    "-ar",
                    "48000",
                    "-b:a",
                    "128k",
                ]);
            } else {
                cmd.args([
                    "-vf",
                    "scale=trunc(iw/2)*2:trunc(ih/2)*2",
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                    "-movflags",
                    "+faststart",
                    "-c:a",
                    "aac",
                    "-ar",
                    "44100",
                    "-b:a",
                    "128k",
                ]);
            }
            let status = cmd.arg(out_path.to_str()?).status().ok()?;
            if status.success() {
                std::fs::read(&out_path).ok()
            } else {
                None
            }
        });
        let _ = std::fs::remove_dir_all(&temp_dir);
        if let Some(out_bytes) = transcoded
            && !out_bytes.is_empty()
            && out_bytes.len() <= neoflash_core::MAX_RESOURCE_BYTES
        {
            let old_path = resource.path.clone();
            used_paths_lower.remove(&old_path.to_ascii_lowercase());
            let stem = old_path
                .strip_suffix(".avi")
                .or_else(|| old_path.strip_suffix(".AVI"))
                .or_else(|| old_path.strip_suffix(".flv"))
                .or_else(|| old_path.strip_suffix(".FLV"))
                .unwrap_or(&old_path);
            let mut next_path = format!("{stem}.{target_ext}");
            let mut seq = 2usize;
            while next_path != old_path
                && (used_paths_lower.contains(&next_path.to_ascii_lowercase())
                    || path_renames.values().any(|v| v == &next_path))
            {
                next_path = format!("{stem}-{seq}.{target_ext}");
                seq += 1;
            }
            used_paths_lower.insert(next_path.to_ascii_lowercase());
            path_renames.insert(old_path, next_path.clone());
            resource.path = next_path;
            resource.media_type = target_mime.to_owned();
            resource.data_base64 = STANDARD.encode(&out_bytes).into();
        } else if is_flv {
            warnings.push(format!(
                "FLV video '{}' could not be transcoded to {target_mime} by ffmpeg; preserving FLV container.",
                resource.path
            ));
        } else {
            warnings.push(format!(
                "AVI video '{}' could not be transcoded to H.264 MP4 by ffmpeg; preserving RIFF/AVI fallback.",
                resource.path
            ));
        }
    }
    if !path_renames.is_empty() {
        for symbol in &mut project.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    if let Some(video) = &mut key.video
                        && let Some(ext) = &mut video.external_path
                        && let Some(next) = path_renames.get(ext)
                    {
                        *ext = next.clone();
                    }
                }
            }
        }
    }
    (warnings, path_renames)
}

pub(crate) fn assemble(
    project: &Project,
    prepared: &Prepared,
    level: &SizeLevel,
) -> Result<Package, ExportError> {
    assemble_with_config(project, prepared, level, None)
}

pub(crate) fn assemble_with_config(
    project: &Project,
    prepared: &Prepared,
    level: &SizeLevel,
    config: Option<&HtmlPublishConfig>,
) -> Result<Package, ExportError> {
    let shared_url = match level {
        SizeLevel::Shared(url) => {
            Some(publish::validate_runtime_url(url).map_err(ExportError::Options)?)
        }
        _ => None,
    };
    let omitted = if *level == SizeLevel::Compact {
        let sources = HOST_MODULES.iter().copied().collect();
        publish::omitted_modules(&prepared.published, &sources)
    } else {
        Default::default()
    };
    let modules: Vec<(&str, &str)> = HOST_MODULES
        .iter()
        .copied()
        .filter(|(file, _)| !omitted.contains_key(file))
        .collect();
    let stripped;
    let wasm: &[u8] = if *level == SizeLevel::Compact {
        let words = publish::referenced_words(
            modules
                .iter()
                .map(|(_, source)| *source)
                .chain([HOST, prepared.program.code.as_str()])
                .chain(prepared.scripts.iter().map(|script| script.code.as_str())),
        );
        stripped = publish::strip_wasm(&prepared.wasm, &prepared.bindings, &words)
            .map_err(ExportError::MissingRuntime)?;
        &stripped
    } else {
        &prepared.wasm
    };
    let reusable_bindings = wrap_reusable_wasm_bindings(&prepared.bindings);
    let bindings_bytes = if shared_url.is_none() && !level.wasm_inline() {
        format!(
            "{reusable_bindings}\nglobalThis.NeoFlashWasmBase64 = \"{}\";\n",
            STANDARD.encode(wasm)
        )
        .into_bytes()
    } else {
        reusable_bindings.into_bytes()
    };
    let runtime_js_bytes = if shared_url.is_some() {
        0
    } else {
        bindings_bytes.len()
            + HOST.len()
            + modules
                .iter()
                .map(|(_, source)| source.len())
                .sum::<usize>()
    };
    let figures = RuntimeFigures {
        wasm_bytes: if shared_url.is_some() { 0 } else { wasm.len() },
        wasm_inline: level.wasm_inline(),
        runtime_js_bytes,
    };
    let (effective_program_code, program_map_shift) =
        apply_js_publish_transforms(&prepared.program.code, config);
    let effective_program_map = prepared
        .program
        .bundle_source_map
        .as_ref()
        .map(|map| apply_publish_source_map_shift(map, program_map_shift));
    let effective_scripts: Vec<CompiledScript> = prepared
        .scripts
        .iter()
        .cloned()
        .map(|mut script| {
            let (code, map_shift) = apply_js_publish_transforms(&script.code, config);
            script.code = code;
            script.source_map = apply_publish_source_map_shift(&script.source_map, map_shift);
            script.source_locations = script.source_map.runtime_locations();
            script
        })
        .collect();
    let (size_report, load_summary) = size_report_from(
        &prepared.published,
        &prepared.program,
        &effective_scripts,
        Some(figures),
    )?;
    let omitted_titles: Vec<&'static str> = omitted
        .values()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut warnings = prepared.warnings.clone();
    warnings.extend(level_tradeoffs(level, &omitted_titles));
    // Without scripts or classes the compiled prelude has nothing to run; the runtime supplies
    // its built-in class lookup instead.
    let scriptless = publish::script_corpus(&prepared.published)
        .trim()
        .is_empty()
        && prepared.program.class_names.is_empty();
    let ships_program = !(scriptless && level.trims_content());
    let mapped_program_code = (ships_program && config.is_some_and(|config| config.source_maps))
        .then(|| {
            format!(
                "{}\n//# sourceMappingURL=bundle.js.map",
                effective_program_code
            )
        });
    let program = ships_program.then(|| ProgramBundle {
        code: mapped_program_code
            .as_deref()
            .unwrap_or(&effective_program_code),
        class_names: &prepared.program.class_names,
        source_url: "neoflash-project.js",
        source_map: effective_program_map.as_ref(),
        source_locations: effective_program_map
            .as_ref()
            .map(|map| map.runtime_locations())
            .unwrap_or_default(),
    });
    let assets_folder = config.is_some_and(|c| c.assets_folder);
    let effective_sprite_sheets =
        config.is_some_and(|c| c.sprite_sheets) || prepared.sequence_pack_atlas;
    let mut placed_bitmap_ids = BTreeSet::<String>::new();
    let mut fill_bitmap_ids = BTreeSet::<String>::new();
    for symbol in &prepared.published.symbols {
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for el in &key.elements {
                    if let ElementKind::Bitmap { resource_id, .. } = &el.kind {
                        placed_bitmap_ids.insert(resource_id.clone());
                    }
                }
                for bf in &key.bitmap_fills {
                    fill_bitmap_ids.insert(bf.resource_id.clone());
                }
            }
        }
    }
    let packed_sprite_sheet = if effective_sprite_sheets {
        atlas::pack_project_bitmap_resources_sprite_sheet_filtered(
            &prepared.published,
            None,
            config.is_some_and(|c| !c.minify),
        )?
    } else {
        None
    };

    let mut effective_published = prepared.published.clone();
    let mut effective_video_manifest = prepared.video_manifest.clone();
    let mut asset_urls: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut extra_asset_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut sprite_sheet_bundle: Option<SpriteSheetBundle> = None;

    if let Some((png_bytes, sheet_manifest)) = packed_sprite_sheet {
        let (img_rel, json_rel) = if assets_folder {
            (
                "assets/sprite_sheet.png".to_owned(),
                "assets/sprite_sheet.json".to_owned(),
            )
        } else {
            (
                "sprite_sheet.png".to_owned(),
                "sprite_sheet.json".to_owned(),
            )
        };
        extra_asset_files.push((img_rel.clone(), png_bytes));
        extra_asset_files.push((json_rel.clone(), sheet_manifest.into_bytes()));
        sprite_sheet_bundle = Some(SpriteSheetBundle {
            image_url: img_rel,
            manifest_url: json_rel,
        });

        for res in &mut effective_published.resources {
            if res.is_image()
                && placed_bitmap_ids.contains(&res.id)
                && !fill_bitmap_ids.contains(&res.id)
            {
                res.data_base64 = std::sync::Arc::from("");
            }
        }
    }

    let mut effective_external_videos = prepared.external_videos.clone();
    if assets_folder {
        let mut ext_relocations: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for ext_vid in &mut effective_external_videos {
            neoflash_core::validate_external_video_path(&ext_vid.relative_path)
                .map_err(ExportError::Options)?;
            if reserved_publish_path(&ext_vid.relative_path) {
                return Err(ExportError::Options(format!(
                    "external video path '{}' collides with a generated package file",
                    ext_vid.relative_path
                )));
            }
            if !ext_vid
                .relative_path
                .to_ascii_lowercase()
                .starts_with("assets/")
            {
                let next = format!("assets/{}", ext_vid.relative_path);
                ext_relocations.insert(ext_vid.relative_path.clone(), next.clone());
                ext_vid.relative_path = next;
            }
        }
        if !ext_relocations.is_empty() {
            for entry in &mut effective_video_manifest {
                if let Some(next) = ext_relocations.get(&entry.source.url_or_path) {
                    entry.source.url_or_path = next.clone();
                }
            }
            for symbol in &mut effective_published.symbols {
                for layer in &mut symbol.timeline.layers {
                    for key in &mut layer.keyframes {
                        if let Some(video) = &mut key.video
                            && let Some(ext) = &mut video.external_path
                            && let Some(next) = ext_relocations.get(ext)
                        {
                            *ext = next.clone();
                        }
                    }
                }
            }
        }

        let mut written_digests: std::collections::HashMap<[u8; 32], String> =
            std::collections::HashMap::new();
        let mut used_asset_names: BTreeSet<String> = extra_asset_files
            .iter()
            .map(|(n, _)| n.to_ascii_lowercase())
            .chain(
                effective_external_videos
                    .iter()
                    .map(|v| v.relative_path.to_ascii_lowercase()),
            )
            .collect();
        let mut extracted_video_paths: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        for res in &mut effective_published.resources {
            let is_audio = res.media_type.starts_with("audio/");
            let is_video = res.is_video();
            if (!is_audio && !is_video) || res.data_base64.is_empty() {
                continue;
            }
            let Ok(raw) = res.decoded_bytes() else {
                continue;
            };
            if raw.is_empty() {
                continue;
            }
            let digest = sha256_bytes(&raw);
            let rel_path = if let Some(existing) = written_digests.get(&digest) {
                existing.clone()
            } else {
                let hash8 = &sha256_hex(&raw)[..8];
                let file_part = res.path.rsplit('/').next().unwrap_or(res.id.as_str());
                let (raw_stem, raw_ext) = file_part.rsplit_once('.').unwrap_or((file_part, ""));
                let safe_stem: String = raw_stem
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let stem = if safe_stem.is_empty() {
                    "media"
                } else {
                    safe_stem.as_str()
                };
                let ext =
                    if !raw_ext.is_empty() && raw_ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                        raw_ext.to_ascii_lowercase()
                    } else if res.media_type.contains("mpeg") || res.media_type.contains("mp3") {
                        "mp3".to_owned()
                    } else if res.media_type.contains("wav") {
                        "wav".to_owned()
                    } else if res.media_type.contains("ogg") {
                        "ogg".to_owned()
                    } else if res.media_type.contains("webm") {
                        "webm".to_owned()
                    } else if res.media_type.contains("flv") {
                        "flv".to_owned()
                    } else if res.media_type.contains("avi") || res.media_type.contains("msvideo") {
                        "avi".to_owned()
                    } else {
                        "mp4".to_owned()
                    };
                let mut candidate = format!("assets/{stem}-{hash8}.{ext}");
                let mut seq = 2usize;
                while used_asset_names.contains(&candidate.to_ascii_lowercase()) {
                    candidate = format!("assets/{stem}-{hash8}-{seq}.{ext}");
                    seq += 1;
                }
                used_asset_names.insert(candidate.to_ascii_lowercase());
                extra_asset_files.push((candidate.clone(), raw));
                written_digests.insert(digest, candidate.clone());
                candidate
            };
            asset_urls.insert(res.path.clone(), rel_path.clone());
            if is_video {
                extracted_video_paths.insert(res.id.clone(), rel_path);
            }
            res.data_base64 = std::sync::Arc::from("");
        }

        if !extracted_video_paths.is_empty() {
            effective_published.format_version = effective_published.format_version.max(10);
            for symbol in &mut effective_published.symbols {
                for layer in &mut symbol.timeline.layers {
                    for key in &mut layer.keyframes {
                        if let Some(video) = &mut key.video
                            && video.deployment == neoflash_core::VideoDeploymentMode::Embedded
                            && let Some(rel) = extracted_video_paths.get(&video.resource)
                        {
                            video.deployment = neoflash_core::VideoDeploymentMode::External;
                            video.external_path = Some(rel.clone());
                        }
                    }
                }
            }
            for entry in &mut effective_video_manifest {
                if let Some(rel) = extracted_video_paths.get(&entry.resource_id) {
                    entry.embedded_in_bundle = false;
                    entry.requires_external_file = true;
                    entry.bundled_byte_len = 0;
                    entry.source.mode = neoflash_core::VideoDeploymentMode::External;
                    entry.source.url_or_path = rel.clone();
                }
            }
        }
    }

    if *level == SizeLevel::Full
        && (sprite_sheet_bundle.is_some()
            || !extra_asset_files.is_empty()
            || !effective_external_videos.is_empty())
    {
        warnings.push(
            "SizeLevel::Full is combined with externalized assets (sprite sheet, assets/ folder or external video); serve the output directory over HTTP(S) rather than moving index.html alone."
                .to_owned(),
        );
    }

    let is_dom_mode = config.is_some_and(|c| c.render_mode == HtmlRenderMode::Dom)
        || (config.is_none() && project.html_render_mode() == HtmlRenderMode::Dom);
    let synthesized_dom_cfg = (config.is_none() && is_dom_mode).then(|| HtmlPublishConfig {
        render_mode: HtmlRenderMode::Dom,
        breakpoints: project.publish_settings().breakpoints,
        ..HtmlPublishConfig::default()
    });
    let effective_config = config.or(synthesized_dom_cfg.as_ref());
    let non_default_config = effective_config.filter(|c| **c != HtmlPublishConfig::default());
    let bundle = Bundle {
        project: &effective_published,
        scenes: prepared.scenes.clone(),
        scripts: effective_scripts.iter().collect(),
        scriptless,
        program,
        wasm: level.wasm_inline().then(|| STANDARD.encode(wasm)),
        wasm_url: match (level, &shared_url) {
            (SizeLevel::Full, _) => None,
            (_, Some(url)) => Some(format!("{}/{WASM_FILE}", url.trim_end_matches('/'))),
            _ => Some(WASM_FILE.to_owned()),
        },
        runtime_id: shared_url.as_ref().map(|_| prepared.runtime_id.clone()),
        player_config: non_default_config,
        video_manifest: &effective_video_manifest,
        sprite_sheet: sprite_sheet_bundle,
        asset_urls,
        resource_aliases: prepared.resource_aliases.clone(),
        publish: PublishInfo {
            level: level.name(),
            omitted_modules: omitted_titles.clone(),
            render_mode: is_dom_mode.then_some("dom"),
        },
        warnings: warnings.clone(),
        load_summary,
        size_report,
        fonts: &prepared.fonts,
    };
    let serialized_bundle = if config.is_some_and(|c| !c.minify) {
        serde_json::to_string_pretty(&bundle)?
    } else {
        serde_json::to_string(&bundle)?
    };
    let bundle_js = format!("globalThis.NeoFlashBundle = {};\n", serialized_bundle);
    let title = html_escape(&project.name);
    let runtime_src = |file: &str| match &shared_url {
        Some(url) => html_escape(&format!("{}/{file}", url.trim_end_matches('/'))),
        None => file.to_owned(),
    };
    let mut scripts = Vec::new();
    if shared_url.is_some() {
        scripts.push(runtime_src(RUNTIME_ID_FILE));
    }
    scripts.push(runtime_src(BINDINGS_FILE));
    scripts.push("bundle.js".to_owned());
    scripts.extend(modules.iter().map(|(file, _)| runtime_src(file)));
    scripts.push(runtime_src("runtime.js"));
    let a11y_overlay_html = html::render_accessibility_overlay_nodes_html(
        &html::collect_project_accessibility_items(&effective_published),
    );
    let html = html_document_with_config(
        &title,
        project.width,
        project.height,
        &prepared.fonts.css_rules,
        &scripts,
        a11y_overlay_html.trim(),
        effective_config,
    );
    let readme = format!(
        "{} — NeoFlash HTML5 export\n\n{}\nThe player and embedded project data need no plugins, SWF or build tools.\nExternal HTTP(S) requests, if used by your scripts, need reachable servers and browser CORS permission. Embedded resources work offline; local filesystem URLs are not a network fallback.\n\nPublishing notes:\n{}\n\nUse scripts in Actions only from authors you trust: JavaScript executes in the browser.\n",
        project.name,
        match level {
            SizeLevel::Full => "Open index.html in a modern browser, or upload this entire folder to static hosting.\nThe shared Rust evaluator and project resources are embedded in bundle.js.".to_owned(),
            SizeLevel::Balanced | SizeLevel::Compact => format!("Open index.html directly from this folder, or upload the entire folder to static hosting.\nOver HTTP(S) the shared Rust evaluator streams from {WASM_FILE}; direct file opening uses the packaged fallback. Project resources are embedded in bundle.js."),
            SizeLevel::Shared(_) => format!("Upload this folder to static hosting. The runtime loads from {}.\nProject resources are embedded in bundle.js.", shared_url.as_deref().unwrap_or_default()),
        },
        warnings.join("\n")
    );

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if shared_url.is_none() {
        files.push((BINDINGS_FILE.into(), bindings_bytes));
        files.push(("runtime.js".into(), HOST.as_bytes().to_vec()));
        for (file, source) in &modules {
            files.push(((*file).into(), source.as_bytes().to_vec()));
        }
        files.push((NOTICES_FILE.into(), ALGORITHM_NOTICES.as_bytes().to_vec()));
        if !level.wasm_inline() {
            files.push((WASM_FILE.into(), wasm.to_vec()));
        }
    }
    if config.is_some_and(|c| c.source_maps) {
        let resolved_map = effective_program_map
            .as_ref()
            .filter(|m| !m.mappings.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                build_fallback_bundle_source_map(&effective_scripts, &effective_program_code)
            });
        let mut map_value = serde_json::to_value(&resolved_map)?;
        if let Some(map) = map_value.as_object_mut() {
            // The authored program is evaluated with `sourceURL=neoflash-project.js`; it is not
            // executable lines in bundle.js, where it is only an escaped JSON string.
            map.insert("file".to_owned(), serde_json::json!("neoflash-project.js"));
            map.insert(
                "x_neoflash_scripts".to_owned(),
                serde_json::json!(
                    effective_scripts
                        .iter()
                        .map(|script| &script.location)
                        .collect::<Vec<_>>()
                ),
            );
        }
        let map_json = if config.is_some_and(|c| !c.minify) {
            serde_json::to_string_pretty(&map_value)?
        } else {
            serde_json::to_string(&map_value)?
        };
        files.push(("bundle.js.map".into(), map_json.into_bytes()));
    }
    let mut module_assets: Vec<String> = extra_asset_files
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    files.extend(extra_asset_files);
    let mut external_paths = Vec::<String>::new();
    for ext_vid in &effective_external_videos {
        neoflash_core::validate_external_video_path(&ext_vid.relative_path)
            .map_err(ExportError::Options)?;
        if reserved_publish_path(&ext_vid.relative_path) {
            return Err(ExportError::Options(format!(
                "external video path '{}' collides with a generated package file",
                ext_vid.relative_path
            )));
        }
        if let Some(previous) = external_paths
            .iter()
            .find(|previous| package_paths_collide(previous, &ext_vid.relative_path))
        {
            return Err(ExportError::Options(format!(
                "external video paths '{}' and '{}' collide by name or file/directory prefix",
                previous, ext_vid.relative_path
            )));
        }
        external_paths.push(ext_vid.relative_path.clone());
        module_assets.push(ext_vid.relative_path.clone());
        if !ext_vid.bytes.is_empty() {
            files.push((ext_vid.relative_path.clone(), ext_vid.bytes.clone()));
        }
    }
    let mut module_runtime_files = Vec::with_capacity(modules.len() + 3);
    if shared_url.is_some() {
        module_runtime_files.push(RUNTIME_ID_FILE);
    }
    module_runtime_files.push(BINDINGS_FILE);
    module_runtime_files.extend(modules.iter().map(|(file, _)| *file));
    module_runtime_files.push("runtime.js");
    let module_wasm_url = match (level, &shared_url) {
        (SizeLevel::Full, _) => None,
        (_, Some(url)) => Some(format!("{}/{WASM_FILE}", url.trim_end_matches('/'))),
        _ => Some(WASM_FILE.to_owned()),
    };
    let module_mjs = render_module_mjs(
        project,
        is_dom_mode,
        level,
        shared_url.as_deref(),
        module_wasm_url.as_deref(),
        &module_assets,
        &module_runtime_files,
        config.is_some_and(|c| !c.minify),
    )?;
    let include_html = config.map(|c| c.include_html).unwrap_or(true);
    if include_html {
        files.push(("bundle.js".into(), bundle_js.into_bytes()));
        files.push((MODULE_FILE.into(), module_mjs.into_bytes()));
        files.push(("README.txt".into(), readme.into_bytes()));
        files.push(("index.html".into(), html.into_bytes()));
    } else {
        files.push(("README.txt".into(), readme.into_bytes()));
        files.push((MODULE_FILE.into(), module_mjs.into_bytes()));
        files.push(("bundle.js".into(), bundle_js.into_bytes()));
    }
    validate_package_file_names(&files)?;

    let resources: String = effective_published
        .resources
        .iter()
        .map(|resource| resource.data_base64.as_ref())
        .collect();
    let project_json = serde_json::to_string(&effective_published)?;
    let content_bytes = project_json.len().saturating_sub(resources.len());
    let script_text: String = ships_program
        .then_some(effective_program_code.as_str())
        .into_iter()
        .chain(effective_scripts.iter().map(|script| script.code.as_str()))
        .collect();
    let fonts_json = serde_json::to_string(&prepared.fonts)?;
    let embedded = vec![
        SizePart {
            kind: "scripts",
            bytes: script_text.len(),
            gzip_bytes: gzip_len(script_text.as_bytes()),
        },
        SizePart {
            kind: "content",
            bytes: content_bytes,
            gzip_bytes: gzip_len(project_json.as_bytes())
                .saturating_sub(gzip_len(resources.as_bytes())),
        },
        SizePart {
            kind: "resources",
            bytes: resources.len(),
            gzip_bytes: gzip_len(resources.as_bytes()),
        },
        SizePart {
            kind: "fonts",
            bytes: fonts_json.len(),
            gzip_bytes: gzip_len(fonts_json.as_bytes()),
        },
    ];
    Ok(Package {
        files,
        script_srcs: scripts,
        level: level.clone(),
        omitted: omitted.keys().copied().collect(),
        wasm_bytes: if shared_url.is_some() {
            0
        } else if level.wasm_inline() {
            wasm.len().div_ceil(3) * 4
        } else {
            wasm.len()
        },
        runtime_js_bytes,
        embedded,
    })
}

/// What a size level gives up, in words for the publish dialog and the package README.
fn level_tradeoffs(level: &SizeLevel, omitted: &[&str]) -> Vec<String> {
    match level {
        SizeLevel::Full => Vec::new(),
        SizeLevel::Balanced => vec![
            "WebAssembly ships as a separate streaming file for HTTP(S), with a packaged fallback for direct file opening."
                .into(),
        ],
        SizeLevel::Compact if omitted.is_empty() => vec![
            "WebAssembly ships as a separate trimmed file for HTTP(S), with a packaged fallback for direct file opening."
                .into(),
        ],
        SizeLevel::Compact => vec![
            "WebAssembly ships as a separate trimmed file for HTTP(S), with a packaged fallback for direct file opening."
                .into(),
            format!(
                "Left out because the project's scripts never name them: {}. A script that builds such a class name at run time cannot reach it; publish at the Balanced level if one does.",
                omitted.join("; ")
            ),
        ],
        SizeLevel::Shared(url) => vec![
            format!(
                "The runtime loads from {url}: publish it there with the same NeoFlash build, and visitors need network access to it. Cross-origin runtime hosts must allow CORS for WebAssembly."
            ),
        ],
    }
}

fn shared_runtime_package_files(bindings: &str, wasm: &[u8], id: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = shared_runtime_files_from(bindings, wasm, id);
    files.push((
        "README.txt".into(),
        format!(
            "NeoFlash shared HTML5 runtime {id}\n\nHost this folder once over HTTP(S) and publish projects at the Shared size level with its URL.\nEvery project checks that it was published against this exact runtime build.\n"
        )
        .into_bytes(),
    ));
    files
}

fn wrap_reusable_wasm_bindings(bindings: &str) -> String {
    const PREFIX: &str = "let NeoFlashWasm = (function(exports) {";
    const SUFFIX: &str = "})({ __proto__: null });";
    let trimmed = bindings.trim_end();
    if let Some(body) = trimmed
        .strip_prefix(PREFIX)
        .and_then(|rest| rest.strip_suffix(SUFFIX))
    {
        format!(
            "globalThis.NeoFlashWasmFactory = globalThis.NeoFlashWasmFactory || function() {{ return (function(exports) {{{body}}})({{ __proto__: null }}); }};\nvar NeoFlashWasm = globalThis.NeoFlashWasm || (globalThis.NeoFlashWasm = globalThis.NeoFlashWasmFactory());\n"
        )
    } else {
        bindings.to_owned()
    }
}

#[allow(clippy::too_many_arguments)]
fn render_module_mjs(
    project: &Project,
    is_dom_mode: bool,
    level: &SizeLevel,
    shared_url: Option<&str>,
    wasm_url: Option<&str>,
    assets: &[String],
    runtime_files: &[&str],
    pretty: bool,
) -> Result<String, ExportError> {
    let manifest_value = serde_json::json!({
        "name": project.name,
        "width": project.width,
        "height": project.height,
        "fps": project.fps,
        "renderMode": if is_dom_mode { "dom" } else { "canvas" },
        "runtimeLevel": level.name(),
        "runtimeUrl": shared_url,
        "bundle": "bundle.js",
        "wasmUrl": wasm_url,
        "assets": assets,
    });
    let manifest_json = if pretty {
        serde_json::to_string_pretty(&manifest_value)?
    } else {
        serde_json::to_string(&manifest_value)?
    };
    let runtime_files_json = serde_json::to_string(runtime_files)?;
    let shared_url_json = serde_json::to_string(&shared_url)?;
    Ok(format!(
        r#"/* NeoFlash HTML5 publication module (`T-016`). */
const MODULE_DIR_URL = new URL('.', import.meta.url).href;
const SHARED_RUNTIME_URL = {shared_url_json};
const RUNTIME_FILES = Object.freeze({runtime_files_json});

export const manifest = Object.freeze({manifest_json});

const bundleTextCache = new Map();

function pageBaseUrl() {{
  if (typeof document !== 'undefined' && typeof document.baseURI === 'string' && document.baseURI) {{
    return document.baseURI;
  }}
  if (typeof location !== 'undefined' && typeof location.href === 'string' && location.href) {{
    return location.href;
  }}
  return MODULE_DIR_URL;
}}

function resolveDirUrl(raw, fallbackBase) {{
  if (typeof raw !== 'string' || !raw.trim()) return null;
  let spec = raw.trim();
  const clean = spec.split('#')[0].split('?')[0];
  const lastSeg = clean.slice(clean.lastIndexOf('/') + 1);
  if (!clean.endsWith('/') && !/\.[A-Za-z0-9]+$/.test(lastSeg)) {{
    spec += '/';
  }}
  const resolved = new URL(spec, fallbackBase || MODULE_DIR_URL);
  if (!resolved.pathname.endsWith('/')) {{
    resolved.pathname = resolved.pathname.slice(0, resolved.pathname.lastIndexOf('/') + 1);
  }}
  resolved.search = '';
  resolved.hash = '';
  return resolved.href;
}}

function resolveBundleUrl(baseUrl) {{
  const raw = typeof baseUrl === 'string' && baseUrl.trim() ? baseUrl.trim() : import.meta.url;
  const clean = raw.split('#')[0].split('?')[0];
  if (clean.endsWith('/bundle.js') || clean === 'bundle.js') {{
    return new URL(raw, MODULE_DIR_URL).href;
  }}
  const lastSeg = clean.slice(clean.lastIndexOf('/') + 1);
  if (!clean.endsWith('/') && lastSeg && !/\.(?:mjs|js|cjs|html|htm|json)$/i.test(lastSeg)) {{
    return new URL('./bundle.js', new URL(raw + '/', MODULE_DIR_URL).href).href;
  }}
  return new URL('./bundle.js', new URL(raw, MODULE_DIR_URL).href).href;
}}

function parseBundleSource(text) {{
  const trimmed = String(text || '').trim();
  if (!trimmed) throw new TypeError('NeoFlash bundle.js is empty.');
  const prefix = /^(?:\/\*[\s\S]*?\*\/\s*)*(?:globalThis|window|self)\.NeoFlashBundle\s*=\s*/.exec(trimmed);
  const jsonText = prefix
    ? trimmed.slice(prefix[0].length).replace(/(?:;\s*)?(?:\/\/#[^\r\n]*\s*)*$/, '').replace(/;\s*$/, '')
    : trimmed;
  const parsed = JSON.parse(jsonText);
  if (!parsed || typeof parsed !== 'object' || !parsed.project || typeof parsed.project !== 'object') {{
    throw new TypeError('Malformed NeoFlash bundle payload.');
  }}
  return parsed;
}}

async function fetchBundleText(url) {{
  if (url.startsWith('file:') && typeof process !== 'undefined' && process.versions?.node) {{
    const fsSpec = 'node:fs/promises';
    const urlSpec = 'node:url';
    const [fsMod, urlMod] = await Promise.all([import(fsSpec), import(urlSpec)]);
    return fsMod.readFile(urlMod.fileURLToPath(url), 'utf8');
  }}
  const response = await fetch(url);
  if (!response.ok) {{
    throw new Error(`Failed to load NeoFlash bundle (${{url}}: ${{response.status}}).`);
  }}
  return response.text();
}}

export async function loadBundle(baseUrl = import.meta.url) {{
  const bundleUrl = resolveBundleUrl(baseUrl);
  let pending = bundleTextCache.get(bundleUrl);
  if (!pending) {{
    pending = fetchBundleText(bundleUrl).catch(err => {{
      bundleTextCache.delete(bundleUrl);
      throw err;
    }});
    bundleTextCache.set(bundleUrl, pending);
  }}
  return parseBundleSource(await pending);
}}

function runtimeRegistry() {{
  let registry = globalThis.__neoflashRuntimeLoader;
  if (!(registry instanceof Map)) {{
    registry = new Map();
    Object.defineProperty(globalThis, '__neoflashRuntimeLoader', {{
      value: registry,
      configurable: true,
      enumerable: false,
      writable: true,
    }});
  }}
  if (!(registry.files instanceof Map)) {{
    registry.files = new Map();
  }}
  return registry;
}}

export async function ensureRuntime(options = {{}}) {{
  const runtimeBase = options?.runtimeUrl
    ? resolveDirUrl(options.runtimeUrl, pageBaseUrl())
    : (SHARED_RUNTIME_URL ? resolveDirUrl(SHARED_RUNTIME_URL, MODULE_DIR_URL) : MODULE_DIR_URL);
  const registry = runtimeRegistry();
  const missingFiles = RUNTIME_FILES.filter(file => {{
    const fileKey = file === 'runtime-id.js' ? `${{runtimeBase}}${{file}}` : file;
    return !registry.files.has(fileKey);
  }});
  if (registry.has(runtimeBase) && missingFiles.length === 0) {{
    return registry.get(runtimeBase);
  }}

  const priorPromise = registry.get(runtimeBase);
  const loadPromise = (async () => {{
    if (typeof document === 'undefined' || !document.head || typeof document.createElement !== 'function') {{
      if (typeof globalThis.NeoFlash?.mount === 'function') return;
      throw new Error('NeoFlash module ensureRuntime requires a DOM document or preloaded NeoFlash runtime.');
    }}
    if (typeof globalThis.NeoFlash?.mount === 'function') {{
      for (const existing of Array.from(document.scripts || [])) {{
        const rawSrc = existing.getAttribute?.('src') || existing.src || '';
        if (!rawSrc) continue;
        const clean = rawSrc.split('#')[0].split('?')[0];
        const fileName = clean.slice(clean.lastIndexOf('/') + 1);
        if (fileName && fileName !== 'runtime-id.js' && !registry.files.has(fileName)) {{
          registry.files.set(fileName, Promise.resolve());
        }}
      }}
    }}
    globalThis.__neoflashManualMountDepth = (globalThis.__neoflashManualMountDepth || 0) + 1;
    globalThis.__NEOFLASH_MANUAL_MOUNT__ = true;
    try {{
      const pending = priorPromise ? [priorPromise] : [];
      for (const file of RUNTIME_FILES) {{
        const fileKey = file === 'runtime-id.js' ? `${{runtimeBase}}${{file}}` : file;
        let filePromise = registry.files.get(fileKey);
        if (!filePromise) {{
          const src = new URL(file, runtimeBase).href;
          filePromise = new Promise((resolve, reject) => {{
            const el = document.createElement('script');
            el.src = src;
            el.async = false;
            el.dataset.neoflashRuntime = file;
            el.onload = () => resolve();
            el.onerror = () => {{
              registry.files.delete(fileKey);
              el.remove?.();
              reject(new Error(`Failed to load NeoFlash runtime script: ${{src}}`));
            }};
            document.head.appendChild(el);
          }});
          registry.files.set(fileKey, filePromise);
        }}
        pending.push(filePromise);
      }}
      await Promise.all(pending);
    }} finally {{
      globalThis.__neoflashManualMountDepth = Math.max(0, (globalThis.__neoflashManualMountDepth || 1) - 1);
      if (globalThis.__neoflashManualMountDepth === 0) {{
        delete globalThis.__NEOFLASH_MANUAL_MOUNT__;
      }}
    }}
  }})().catch(err => {{
    if (registry.get(runtimeBase) === loadPromise) registry.delete(runtimeBase);
    throw err;
  }});

  registry.set(runtimeBase, loadPromise);
  return loadPromise;
}}

export async function mount(container, options = {{}}) {{
  const [, loadedBundle] = await Promise.all([
    ensureRuntime(options),
    options?.bundle ? Promise.resolve(options.bundle) : loadBundle(options?.baseUrl || MODULE_DIR_URL),
  ]);
  if (typeof globalThis.NeoFlash?.mount !== 'function') {{
    throw new Error('NeoFlash runtime did not register globalThis.NeoFlash.mount.');
  }}
  const baseUrl = options?.baseUrl
    ? resolveDirUrl(options.baseUrl, pageBaseUrl())
    : MODULE_DIR_URL;
  const runtimeUrl = options?.runtimeUrl
    ? resolveDirUrl(options.runtimeUrl, pageBaseUrl())
    : (SHARED_RUNTIME_URL ? resolveDirUrl(SHARED_RUNTIME_URL, MODULE_DIR_URL) : baseUrl);
  return globalThis.NeoFlash.mount(container, {{
    ...options,
    bundle: loadedBundle,
    baseUrl,
    runtimeUrl,
  }});
}}

export default mount;
"#
    ))
}

fn shared_runtime_files_from(bindings: &str, wasm: &[u8], id: &str) -> Vec<(String, Vec<u8>)> {
    let reusable = wrap_reusable_wasm_bindings(bindings);
    let bindings_with_fallback = format!(
        "{reusable}\nglobalThis.NeoFlashWasmBase64 = \"{}\";\n",
        STANDARD.encode(wasm)
    );
    let mut files = vec![
        (
            RUNTIME_ID_FILE.to_owned(),
            format!("globalThis.NeoFlashRuntimeId = \"{id}\";\n").into_bytes(),
        ),
        (BINDINGS_FILE.into(), bindings_with_fallback.into_bytes()),
        ("runtime.js".into(), HOST.as_bytes().to_vec()),
        (WASM_FILE.into(), wasm.to_vec()),
        (NOTICES_FILE.into(), ALGORITHM_NOTICES.as_bytes().to_vec()),
    ];
    files.extend(
        HOST_MODULES
            .iter()
            .map(|(file, source)| ((*file).to_owned(), source.as_bytes().to_vec())),
    );
    files
}

fn measure(
    level: SizeLevel,
    package: &Package,
    name: &str,
    shared: Option<(usize, usize)>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<SizeEstimate>, ExportError> {
    let marker = package_marker(&package.files, name, "project")?;
    let mut gzip = Vec::with_capacity(package.files.len());
    for (_, bytes) in &package.files {
        if cancelled() {
            return Ok(None);
        }
        gzip.push(gzip_len(bytes));
    }
    let bytes: usize = package
        .files
        .iter()
        .map(|(_, bytes)| bytes.len())
        .sum::<usize>()
        + marker.len();
    let gzip_bytes: usize = gzip.iter().sum::<usize>() + gzip_len(&marker);
    let file_gzip = |names: &dyn Fn(&str) -> bool| -> usize {
        package
            .files
            .iter()
            .zip(&gzip)
            .filter(|((name, _), _)| names(name))
            .map(|(_, size)| *size)
            .sum()
    };
    let wasm_gzip = if package.level.wasm_inline() {
        package
            .files
            .iter()
            .find(|(name, _)| name == "bundle.js")
            .and_then(|(_, bundle)| {
                let text = std::str::from_utf8(bundle).ok()?;
                let start = text.find("\"wasm\":\"")? + 8;
                let end = start + text[start..].find('"')?;
                Some(gzip_len(&text.as_bytes()[start..end]))
            })
            .unwrap_or_default()
    } else {
        file_gzip(&|name| name == WASM_FILE)
    };
    let runtime_js_gzip =
        file_gzip(&|name| name.ends_with(".js") && name != "bundle.js" && name != RUNTIME_ID_FILE);
    let mut parts = vec![
        SizePart {
            kind: "wasm",
            bytes: package.wasm_bytes,
            gzip_bytes: wasm_gzip,
        },
        SizePart {
            kind: "runtime_js",
            bytes: package.runtime_js_bytes,
            gzip_bytes: runtime_js_gzip,
        },
    ];
    parts.extend(package.embedded.iter().cloned());
    let is_external_resource =
        |n: &str| n.starts_with("assets/") || n == "sprite_sheet.png" || n == "sprite_sheet.json";
    let ext_res_bytes: usize = package
        .files
        .iter()
        .filter(|(n, _)| is_external_resource(n))
        .map(|(_, b)| b.len())
        .sum();
    if ext_res_bytes > 0 {
        let ext_res_gzip = file_gzip(&is_external_resource);
        parts.push(SizePart {
            kind: "resources",
            bytes: ext_res_bytes,
            gzip_bytes: ext_res_gzip,
        });
    }
    let counted: usize = parts.iter().map(|part| part.bytes).sum();
    let counted_gzip: usize = parts.iter().map(|part| part.gzip_bytes).sum();
    parts.push(SizePart {
        kind: "page",
        bytes: bytes.saturating_sub(counted),
        gzip_bytes: gzip_bytes.saturating_sub(counted_gzip),
    });
    let omitted_titles: BTreeSet<&str> = publish::FAMILIES
        .iter()
        .filter(|family| package.omitted.contains(&family.file))
        .map(|family| family.title)
        .collect();
    let omitted_list: Vec<&str> = omitted_titles.iter().copied().collect();
    Ok((!cancelled()).then_some(SizeEstimate {
        tradeoffs: level_tradeoffs(&level, &omitted_list),
        level,
        files: package.files.len() + 1,
        bytes,
        gzip_bytes,
        parts,
        shared_runtime_bytes: shared.map(|(bytes, _)| bytes),
        shared_runtime_gzip_bytes: shared.map(|(_, gzip)| gzip),
        omitted_modules: package
            .omitted
            .iter()
            .map(|file| (*file).to_owned())
            .collect(),
    }))
}

fn package_marker(
    files: &[(String, Vec<u8>)],
    name: &str,
    kind: &str,
) -> Result<Vec<u8>, ExportError> {
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    let has_module = files.iter().any(|(file, _)| file == MODULE_FILE);
    let mut marker = serde_json::json!({
        "generator": "NeoFlash",
        "format_version": 1,
        "kind": kind,
        "project": name,
        "files": names,
    });
    if has_module && let Some(map) = marker.as_object_mut() {
        map.insert("module".to_owned(), serde_json::json!(MODULE_FILE));
    }
    Ok(serde_json::to_string_pretty(&marker)?.into_bytes())
}

pub(crate) fn has_non_ignorable_dir_entries(dir: &Path) -> std::io::Result<bool> {
    if !dir.exists() {
        return Ok(false);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() != ".DS_Store" {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn read_neoflash_marker_files(dir: &Path) -> Option<Vec<String>> {
    let text = fs::read_to_string(dir.join(MARKER)).ok()?;
    let marker = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    if marker["generator"] != "NeoFlash" || marker["format_version"] != 1 {
        return None;
    }
    let names = marker["files"].as_array()?;
    let files: Vec<String> = names
        .iter()
        .map(|name| name.as_str().map(str::to_owned))
        .collect::<Option<_>>()?;
    validate_package_path_set(files.iter().map(String::as_str)).ok()?;
    Some(files)
}

pub(crate) fn is_neoflash_native_banner_html(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() > 16 * 1024 * 1024 {
        return false;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    text.contains("<meta name=\"ad.size\"")
        && text.contains("id=\"nf-ad\"")
        && text.contains("window.clickTag")
}

pub(crate) fn is_neoflash_native_banner_dir(dir: &Path) -> std::io::Result<bool> {
    if !dir.is_dir() {
        return Ok(false);
    }
    let mut found_banner = false;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".DS_Store" {
            continue;
        }
        let Some(name_str) = name.to_str() else {
            return Ok(false);
        };
        if name_str.eq_ignore_ascii_case("index.html")
            && is_neoflash_native_banner_html(&entry.path())
        {
            found_banner = true;
        } else {
            return Ok(false);
        }
    }
    Ok(found_banner)
}

pub(crate) fn cleanup_stale_package_files(
    output_dir: &Path,
    previous_files: &[String],
    keep_lower: &BTreeSet<String>,
) {
    for stale in previous_files {
        // A case-only rename addresses the same file on the default macOS filesystem. Never
        // delete the freshly written spelling while cleaning the marker's previous spelling.
        if validate_package_relative_path(stale).is_ok()
            && !keep_lower.contains(&stale.to_lowercase())
        {
            let _ = fs::remove_file(output_dir.join(stale));
        }
    }
    let _ = fs::remove_dir(output_dir.join("assets"));
}

fn previous_package_files(output_dir: &Path) -> Result<Vec<String>, ExportError> {
    if !has_non_ignorable_dir_entries(output_dir)? {
        return Ok(Vec::new());
    }
    if let Some(marker_files) = read_neoflash_marker_files(output_dir) {
        return Ok(marker_files);
    }
    if is_neoflash_native_banner_dir(output_dir)? {
        return Ok(vec!["index.html".to_owned()]);
    }
    Err(ExportError::Destination(output_dir.display().to_string()))
}

/// Verify that `output_dir` is empty or contains only a previous NeoFlash-managed package.
///
/// This read-only check is shared with headless transactions so they can reject an unowned or
/// malformed destination before recursively snapshotting it.
pub fn preflight_package_output_dir(output_dir: &Path) -> Result<(), ExportError> {
    previous_package_files(output_dir).map(|_| ())
}

/// Writes generated files (index.html last) into a NeoFlash-owned folder and removes files an
/// earlier export there generated that this one no longer has.
pub(crate) fn write_package(
    output_dir: &Path,
    files: &[(String, Vec<u8>)],
    name: &str,
    kind: &str,
) -> Result<PathBuf, ExportError> {
    validate_package_file_names(files)?;
    let previous_files = previous_package_files(output_dir)?;
    fs::create_dir_all(output_dir)?;
    let current_names: BTreeSet<String> =
        files.iter().map(|(name, _)| name.to_lowercase()).collect();
    let marker = package_marker(files, name, kind)?;
    // Stage every generated file in sibling temp files before replacing any existing output file,
    // so an I/O error mid-package never leaves a half-overwritten build behind.
    let (last, rest) = files
        .split_last()
        .expect("a package always has an index file");
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(files.len() + 1);
    let mut created_dirs = Vec::new();
    let stage_result = (|| -> std::io::Result<()> {
        for (file, bytes) in rest {
            let target = output_dir.join(file);
            ensure_export_dir_tracked(
                target.parent().unwrap_or_else(|| Path::new(".")),
                &mut created_dirs,
            )?;
            let temp = reserve_temp_file(&target, bytes)?;
            staged.push((temp, target));
        }
        let marker_target = output_dir.join(MARKER);
        let marker_temp = reserve_temp_file(&marker_target, &marker)?;
        staged.push((marker_temp, marker_target));
        let last_target = output_dir.join(&last.0);
        let last_temp = reserve_temp_file(&last_target, &last.1)?;
        staged.push((last_temp, last_target));
        Ok(())
    })();
    if let Err(err) = stage_result {
        for (temp, _) in staged {
            let _ = fs::remove_file(temp);
        }
        cleanup_created_export_dirs(&mut created_dirs);
        return Err(ExportError::Io(err));
    }
    if let Err(err) = commit_staged_files_atomically(&staged) {
        for (temp, _) in &staged {
            let _ = fs::remove_file(temp);
        }
        cleanup_created_export_dirs(&mut created_dirs);
        return Err(ExportError::Io(err));
    }
    cleanup_stale_package_files(output_dir, &previous_files, &current_names);
    Ok(output_dir.join(&last.0))
}

fn ensure_export_dir_tracked(dir: &Path, created_dirs: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if dir.as_os_str().is_empty() || dir.is_dir() {
        return Ok(());
    }
    let mut missing = Vec::new();
    let mut current = Some(dir);
    while let Some(path) = current {
        if path.as_os_str().is_empty() || path.exists() {
            break;
        }
        missing.push(path.to_path_buf());
        current = path.parent();
    }
    for path in missing.into_iter().rev() {
        match fs::create_dir(&path) {
            Ok(()) => created_dirs.push(path),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => {}
            Err(err) => return Err(err),
        }
    }
    if dir.is_dir() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("export parent '{}' is not a directory", dir.display()),
        ))
    }
}

fn cleanup_created_export_dirs(created_dirs: &mut Vec<PathBuf>) {
    for dir in created_dirs.drain(..).rev() {
        let _ = fs::remove_dir(dir);
    }
}

#[derive(Debug)]
struct PublishedFileState {
    target: PathBuf,
    backup: Option<PathBuf>,
}

/// Commit a set of sibling-staged files as one rollback-capable transaction.
///
/// Renaming a staged file is atomic by itself, but a package contains several files. Backing up
/// each existing target lets a later rename failure restore every file already replaced. Staged
/// files that have not moved remain owned by the caller and may be removed normally.
pub(crate) fn commit_staged_files_atomically(
    staged_files: &[(PathBuf, PathBuf)],
) -> std::io::Result<()> {
    commit_staged_files_atomically_impl(staged_files, true)
}

fn commit_staged_files_atomically_impl(
    staged_files: &[(PathBuf, PathBuf)],
    sync_parent_dirs: bool,
) -> std::io::Result<()> {
    // Keep the short backup/replace window invisible to another exporter in this process. This is
    // especially important on Windows, where replacing an existing destination needs two renames.
    static COMMIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _commit_guard = COMMIT_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    for (_, target) in staged_files {
        match fs::symlink_metadata(target) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::IsADirectory,
                    format!(
                        "cannot replace export file '{}' because it is a directory",
                        target.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }

    // A single output needs no package rollback. Use the platform's one-step replacement so an
    // external reader always sees either the old file or the new file, never an absent target.
    // The backup transaction below is reserved for sets where a later rename may need to restore
    // files already published.
    if let [(temp, target)] = staged_files {
        replace_staged_file_atomically(temp, target, sync_parent_dirs)?;
        if sync_parent_dirs
            && let Some(parent) = target.parent()
            && !parent.as_os_str().is_empty()
            && let Ok(dir_file) = fs::File::open(parent)
        {
            let _ = dir_file.sync_all();
        }
        return Ok(());
    }

    let mut published = Vec::with_capacity(staged_files.len());
    for (temp, target) in staged_files {
        let backup = match fs::symlink_metadata(target) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                let err = std::io::Error::new(
                    std::io::ErrorKind::IsADirectory,
                    format!(
                        "cannot replace export file '{}' because it is a directory",
                        target.display()
                    ),
                );
                return Err(with_rollback_result(err, &mut published));
            }
            Ok(_) => {
                let backup = match reserve_backup_path(target) {
                    Ok(path) => path,
                    Err(err) => return Err(with_rollback_result(err, &mut published)),
                };
                if let Err(err) = fs::rename(target, &backup) {
                    return Err(with_rollback_result(err, &mut published));
                }
                Some(backup)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(with_rollback_result(err, &mut published)),
        };

        published.push(PublishedFileState {
            target: target.clone(),
            backup,
        });
        if let Err(err) = fs::rename(temp, target) {
            return Err(with_rollback_result(err, &mut published));
        }
    }

    let mut touched_parents = BTreeSet::new();
    for state in &published {
        if let Some(backup) = &state.backup {
            let _ = fs::remove_file(backup);
        }
        if let Some(parent) = state.target.parent()
            && !parent.as_os_str().is_empty()
        {
            touched_parents.insert(parent.to_path_buf());
        }
    }
    if sync_parent_dirs {
        for parent in touched_parents {
            if let Ok(dir_file) = fs::File::open(parent) {
                let _ = dir_file.sync_all();
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn replace_staged_file_atomically(
    source: &Path,
    target: &Path,
    write_through: bool,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    // Canonicalizing the existing adjacent parent yields a verbatim (`\\?\`) path on Windows,
    // retaining Unicode and long-path support even when the final target does not exist yet.
    let adjacent_absolute = |path: &Path| -> std::io::Result<PathBuf> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let name = path.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("export path '{}' has no file name", path.display()),
            )
        })?;
        Ok(fs::canonicalize(parent)?.join(name))
    };
    let source = adjacent_absolute(source)?;
    let target = adjacent_absolute(target)?;
    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let target_wide: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let flags = MOVEFILE_REPLACE_EXISTING
        | if write_through {
            MOVEFILE_WRITE_THROUGH
        } else {
            0
        };

    // SAFETY: both UTF-16 buffers are NUL-terminated and remain alive for the call. Staging file
    // handles are closed by `reserve_temp_file` or the caller before this function is entered.
    if unsafe { MoveFileExW(source_wide.as_ptr(), target_wide.as_ptr(), flags) } != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_staged_file_atomically(
    source: &Path,
    target: &Path,
    _write_through: bool,
) -> std::io::Result<()> {
    fs::rename(source, target)
}

fn reserve_backup_path(target: &Path) -> std::io::Result<PathBuf> {
    static BACKUP_ID: AtomicU64 = AtomicU64::new(1);
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    for _ in 0..32 {
        let candidate = parent.join(format!(
            ".{name}.neoflash-{}-{}.backup",
            std::process::id(),
            BACKUP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::symlink_metadata(&candidate) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not reserve a unique export backup file",
    ))
}

fn with_rollback_result(
    original: std::io::Error,
    published: &mut Vec<PublishedFileState>,
) -> std::io::Error {
    let mut rollback_errors = Vec::new();
    for state in published.drain(..).rev() {
        match fs::remove_file(&state.target) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => rollback_errors.push(format!(
                "could not remove replacement '{}': {err}",
                state.target.display()
            )),
        }
        if let Some(backup) = state.backup
            && let Err(err) = fs::rename(&backup, &state.target)
        {
            rollback_errors.push(format!(
                "could not restore '{}' from '{}': {err}",
                state.target.display(),
                backup.display()
            ));
        }
    }
    if rollback_errors.is_empty() {
        original
    } else {
        std::io::Error::new(
            original.kind(),
            format!(
                "{original}; rollback also failed: {}",
                rollback_errors.join("; ")
            ),
        )
    }
}

fn reserve_temp_file(path: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    static TEMP_ID: AtomicU64 = AtomicU64::new(1);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    for _ in 0..32 {
        let temp = parent.join(format!(
            ".{name}.neoflash-{}-{}.tmp",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let write_result = (|| {
            file.write_all(bytes)?;
            file.flush()
        })();
        drop(file);
        if let Err(err) = write_result {
            let _ = fs::remove_file(&temp);
            return Err(err);
        }
        return Ok(temp);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not reserve a unique temporary export file",
    ))
}

/// Write `bytes` to `path` atomically by staging in a sibling temp file and renaming.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = reserve_temp_file(path, bytes)?;
    // Single-file frame/sequence writers may call this thousands of times; the sibling rename is
    // atomic without forcing a directory fsync for each frame.
    let result = commit_staged_files_atomically_impl(&[(temp.clone(), path.to_path_buf())], false);
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub(crate) fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[allow(dead_code)]
fn html_document(title: &str, font_css: &str, scripts: &[String]) -> String {
    let default_overlay = html::render_accessibility_overlay_nodes_html(&[]);
    html_document_with_config(
        title,
        800,
        600,
        font_css,
        scripts,
        default_overlay.trim(),
        None,
    )
}

fn html_document_with_config(
    title: &str,
    width: u32,
    height: u32,
    font_css: &str,
    scripts: &[String],
    a11y_overlay_html: &str,
    config: Option<&HtmlPublishConfig>,
) -> String {
    let script_tags: String = scripts
        .iter()
        .map(|src| format!("  <script defer src=\"{src}\"></script>\n"))
        .collect();
    let font_rules = if font_css.is_empty() {
        String::new()
    } else {
        format!("\n    {font_css}")
    };
    let clean = config.is_some_and(|c| c.chrome.effective_clean());
    let body_attr = if clean { " data-clean=\"true\"" } else { "" };
    let footer_attr = if clean { " hidden" } else { "" };
    let transport_attr = if config.is_some_and(|c| !c.chrome.show_controls) {
        " hidden"
    } else {
        ""
    };
    let status_attr = if config.is_some_and(|c| !c.chrome.show_fps) {
        " hidden"
    } else {
        ""
    };
    let load_attr = if config.is_some_and(|c| !c.chrome.show_load_summary) {
        " hidden"
    } else {
        ""
    };
    let trace_attr = if config.is_some_and(|c| !c.chrome.show_trace) {
        " hidden"
    } else {
        ""
    };
    let loading_attr = if config.is_some_and(|c| !c.preloader) {
        " hidden style=\"display:none\""
    } else {
        ""
    };
    let fullscreen_attr = if config.is_some_and(|c| !c.allow_fullscreen) {
        " hidden"
    } else {
        ""
    };
    let canvas_accel_attr = match config.map(|c| c.acceleration) {
        Some(HtmlAcceleration::GpuWebGl) => " data-acceleration=\"gpu_web_gl\"",
        Some(HtmlAcceleration::Canvas2d) => " data-acceleration=\"canvas2d\"",
        Some(HtmlAcceleration::Software) => " data-acceleration=\"software\"",
        None => "",
    };
    let canvas_output_attr = if config.is_some_and(|c| c.chrome.relay_output_to_host) {
        " data-neoflash-output-endpoint=\"/__neoflash/output\""
    } else {
        ""
    };
    let display_name = if let Some(cfg) = config {
        if cfg.chrome.show_dimensions && !clean {
            format!("{title} · {width}×{height}")
        } else {
            title.to_owned()
        }
    } else {
        title.to_owned()
    };
    let template_css = match config
        .map(|c| c.template)
        .unwrap_or(HtmlTemplate::Responsive)
    {
        HtmlTemplate::Responsive => "",
        HtmlTemplate::FixedStage => "\n    #stage-surface { border: 1px solid #ffffff1a; }",
        HtmlTemplate::Centered => "\n    #stage-surface { border-radius: 6px; overflow: hidden; }",
        HtmlTemplate::Fullscreen => {
            "\n    body { background: #000; }\n    #stage-surface { box-shadow: none; }"
        }
    };
    let clean_css = if config.is_some() {
        "\n    body[data-clean=\"true\"] { grid-template-rows: minmax(0,1fr); }\n    body[data-clean=\"true\"] > footer { display: none !important; }"
    } else {
        ""
    };
    let is_dom_mode = config.is_some_and(|c| c.render_mode == HtmlRenderMode::Dom);
    let body_render_attr = if is_dom_mode {
        " data-render-mode=\"dom\""
    } else {
        ""
    };
    let dom_css = if is_dom_mode {
        "\n    body[data-render-mode=\"dom\"] #stage { position: absolute; inset: 0; pointer-events: none; opacity: 0; visibility: hidden; }\n    #stage-dom-root { position: absolute; inset: 0; display: block; width: 100%; height: 100%; overflow: hidden; transform-origin: 0 0; }\n    .nf-dom-scene { position: absolute; left: 0; top: 0; width: 100%; height: 100%; transform-origin: 0 0; pointer-events: auto; }\n    .nf-dom-container { position: absolute; left: 0; top: 0; transform-origin: 0 0; pointer-events: none; }\n    .nf-dom-button { position: absolute; left: 0; top: 0; transform-origin: 0 0; padding: 0; margin: 0; border: 0; border-radius: 0; background: transparent; color: inherit; min-height: 0; pointer-events: auto; cursor: pointer; text-align: left; }\n    .nf-dom-button:hover, .nf-dom-button:disabled { background: transparent; }\n    .nf-dom-button:focus-visible { outline: 2px solid #7ba9ef; outline-offset: 2px; }\n    .nf-dom-text { position: absolute; left: 0; top: 0; transform-origin: 0 0; margin: 0; padding: 0; line-height: 1.2; }\n    .nf-dom-input { position: absolute; left: 0; top: 0; transform-origin: 0 0; margin: 0; box-sizing: border-box; pointer-events: auto; }\n    .nf-dom-input:focus-visible { outline: 2px solid #7ba9ef; outline-offset: 1px; }\n    .nf-dom-svg { position: absolute; left: 0; top: 0; overflow: visible; transform-origin: 0 0; pointer-events: none; }\n    .nf-dom-bitmap, .nf-dom-video, .nf-dom-canvas-island { position: absolute; left: 0; top: 0; transform-origin: 0 0; pointer-events: none; }"
    } else {
        ""
    };
    let stage_markup = if is_dom_mode {
        let dom_scale = config
            .map(|c| c.scale_mode.as_flash_scale_mode())
            .unwrap_or("showAll");
        let dom_align = config.map(|c| c.stage_align.as_flash_code()).unwrap_or("");
        format!(
            "<canvas id=\"stage\"{canvas_accel_attr}{canvas_output_attr} aria-hidden=\"true\"></canvas><neoflash-stage id=\"stage-dom-root\" data-render-mode=\"dom\" width=\"{width}\" height=\"{height}\" scale-mode=\"{dom_scale}\" align=\"{dom_align}\"></neoflash-stage><div id=\"semantic-layer\"></div>"
        )
    } else {
        format!(
            "<canvas id=\"stage\"{canvas_accel_attr}{canvas_output_attr} aria-label=\"{title}\"></canvas><div id=\"semantic-layer\"></div>{a11y_overlay_html}"
        )
    };
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    :root {{ color-scheme: dark; font: 13px system-ui, sans-serif; }}
    * {{ box-sizing: border-box; }}
    body {{ margin: 0; background: #25272b; color: #d2d6dd; height: 100vh; height: 100dvh; display: grid; grid-template-rows: minmax(0,1fr) auto; overflow: hidden; }}{clean_css}{template_css}
    #viewport {{ position: relative; overflow: auto; min-height: 0; min-width: 0; overscroll-behavior: contain; touch-action: pan-x pan-y; }}
    #viewport:fullscreen, #viewport:-webkit-full-screen {{ background: #000; width: 100vw; height: 100vh; overflow: hidden; }}
    #stage-space {{ position: relative; }}
    #stage-surface {{ position: absolute; box-shadow: 0 12px 48px #0005; }}
    canvas {{ display: block; width: 100%; height: 100%; outline: none; }}
    [data-neoflash-text-input] {{ outline: none; scrollbar-width: none; -ms-overflow-style: none; }}
    [data-neoflash-text-input]::-webkit-scrollbar {{ display: none; width: 0; height: 0; }}
    #semantic-layer {{ position: absolute; inset: 0; pointer-events: none; }}
    #stage-a11y-overlay {{ position: absolute; inset: 0; pointer-events: none; overflow: hidden; }}
    footer {{ display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; padding: 8px 16px; border-top: 1px solid #ffffff12; min-width: 0; }}
    .transport {{ display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }}
    button, select {{ background: #34373e; color: #f3f4f6; border: 1px solid #555a64; border-radius: 4px; font: inherit; padding: 7px 12px; cursor: pointer; min-height: 34px; }}
    button:hover {{ background: #464c58; }}
    button:focus-visible, select:focus-visible {{ outline: 2px solid #7ba9ef; outline-offset: 2px; }}
    button[aria-pressed="true"] {{ background: #405779; }}
    .semantic-target {{ position: absolute; pointer-events: none; padding: 0; margin: 0; border: 0; border-radius: 3px; background: transparent; color: transparent; min-height: 0; font-size: 0; }}
    .semantic-target:hover, .semantic-target:disabled {{ background: transparent; }}
    .semantic-target:focus-visible {{ outline: 3px solid #d4e9ff; outline-offset: 3px; box-shadow: 0 0 0 6px #183d71; }}
    .pan-mode, .pan-mode canvas {{ cursor: grab; }}
    .preview-info {{ display: flex; flex-wrap: wrap; gap: 4px 12px; min-width: 0; flex: 1; }}
    #name {{ overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
    #audio-status {{ color: #c6d8ef; }}
    #status {{ font-variant-numeric: tabular-nums; color: #a5acb7; white-space: nowrap; }}
    #load-info {{ font-variant-numeric: tabular-nums; color: #9db4d0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
    #trace {{ flex-basis: 100%; overflow-wrap: anywhere; min-height: 1em; color: #c6d8ef; }}
    #size-report {{ position: fixed; inset: 24px 24px 72px 24px; max-height: calc(100vh - 96px); overflow: auto; margin: 0; z-index: 2; padding: 20px 24px; border: 1px solid #555a64; border-radius: 6px; background: #1d1f24f2; color: #d8dee9; font: 12px/1.45 ui-monospace, SFMono-Regular, Menlo, monospace; white-space: pre; display: none; box-shadow: 0 16px 48px #0009; }}
    #error {{ position: fixed; inset: 24px; max-height: 60vh; overflow: auto; margin: auto; z-index: 2; padding: 24px; border: 1px solid #f19393; background: #351d22; color: #ffe0e0; white-space: pre-wrap; display: none; }}
    #loading {{ position: fixed; inset: 0; display: grid; place-items: center; pointer-events: none; }}
    @media (max-width: 600px) {{ footer {{ padding: 8px; }} .transport {{ gap: 6px; }} .preview-info {{ flex-basis: 100%; }} #name {{ display: none; }} }}{dom_css}{font_rules}
  </style>
{script_tags}</head>
<body{body_render_attr}{body_attr}>
  <main id="viewport" tabindex="-1" aria-label="Project preview"><div id="stage-space"><div id="stage-surface" role="group" aria-label="{title}">{stage_markup}</div></div></main>
  <footer{footer_attr}><div class="transport" aria-label="Preview controls"{transport_attr}><button id="toggle" type="button">Pause</button><button id="restart" type="button">Restart</button><select id="zoom" aria-label="Preview zoom"><option value="fit">Fit</option><option value="0.25">25%</option><option value="0.5">50%</option><option value="0.75">75%</option><option value="1">100%</option><option value="1.5">150%</option><option value="2">200%</option><option value="4">400%</option></select><button id="pan" type="button" aria-pressed="false" title="Drag the stage to pan. Touch scrolling also works.">Pan</button><button id="fullscreen-toggle" type="button" aria-pressed="false" title="Toggle fullscreen stage"{fullscreen_attr}>Fullscreen</button><button id="audio-toggle" type="button" hidden>Enable audio</button><button id="report-toggle" type="button" aria-pressed="false" title="Show project weight and loaded assets breakdown" hidden>Size report</button></div><div class="preview-info"><span id="name">{display_name}</span><span id="status"{status_attr}></span><span id="load-info"{load_attr}></span><span id="audio-status" role="status" aria-live="polite" aria-atomic="true" hidden></span><span id="trace" role="status" aria-live="polite" aria-atomic="true"{trace_attr}></span></div></footer>
  <div id="loading" role="status"{loading_attr}>Loading project…</div><pre id="size-report" aria-label="Project size and load report"></pre><pre id="error" role="alert"></pre>
</body>
</html>
"##
    )
}

/// Escape XML text / double-quoted attribute characters (`T-260` shared export helper).
pub(crate) fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
#[path = "export_lib_tests.rs"]
mod tests;

#[cfg(test)]
mod skipped_foreign_export_tests {
    use super::*;
    use neoflash_core::{FrameScript, Script, ScriptLanguage, SourceUnit};

    #[test]
    fn strip_skipped_foreign_scripts_clears_inline_class_declarations_and_omits_unused_modules() {
        let mut project = Project::blank();
        project.script_language = Some(ScriptLanguage::JavaScript);
        project.also_compile_compatible_scripts = false;
        project.document_class = Some("app.LegacyMain".into());

        // Filename stem `bundle_classes` does NOT match `LegacyMain` or `EnemyClip`,
        // so `class <short>` matching inside skipped source units is required.
        project.sources.push(SourceUnit {
            id: "as3_bundle".into(),
            path: "src/bundle_classes.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: "package app { public class LegacyMain {} public class EnemyClip {} }".into(),
        });
        // Skipped foreign AS2 frame script referencing XML/ExternalInterface must NOT keep
        // those optional host modules in `SizeLevel::Compact` or count toward `published`.
        project.symbols[0].class_name = Some("app.EnemyClip".into());
        project.symbols[0].timeline.scripts.push(FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "var doc = new XML('<x/>'); ExternalInterface.call('ping');".into(),
            },
        });

        let stripped = strip_skipped_foreign_scripts(&project);
        assert_eq!(stripped.document_class, None);
        assert_eq!(stripped.symbols[0].class_name, None);
        assert!(stripped.sources.is_empty());
        assert!(stripped.symbols[0].timeline.scripts.is_empty());
        assert!(publish::script_corpus(&stripped).trim().is_empty());

        let sources = HOST_MODULES.iter().copied().collect();
        let omitted = publish::omitted_modules(&stripped, &sources);
        assert!(
            omitted.contains_key("xml.js") && omitted.contains_key("external.js"),
            "Compact module pruning on stripped project should omit xml.js and external.js, got: {:?}",
            omitted.keys().collect::<Vec<_>>()
        );
    }

    fn build_synthetic_vp6_flv(vp6a_alpha: bool) -> Vec<u8> {
        fn amf_num(out: &mut Vec<u8>, key: &str, val: f64) {
            out.extend_from_slice(&(key.len() as u16).to_be_bytes());
            out.extend_from_slice(key.as_bytes());
            out.push(0x00);
            out.extend_from_slice(&val.to_be_bytes());
        }
        let mut vp6_raw = vec![0x41_u8, 0x46, 0x00, 0x20, 1, 1, 1, 1];
        vp6_raw.extend_from_slice(&[0_u8; 64]);
        let (tag_hdr, codec_id, pkt_body) = if vp6a_alpha {
            let mut body = vec![0x00_u8, 0x00, 0x00, vp6_raw.len() as u8];
            body.extend_from_slice(&vp6_raw);
            body.extend_from_slice(&vp6_raw);
            (0x15_u8, 5.0, body)
        } else {
            let mut body = vec![0x00_u8];
            body.extend_from_slice(&vp6_raw);
            (0x14_u8, 4.0, body)
        };
        let mut flv = Vec::new();
        flv.extend_from_slice(b"FLV\x01\x01\x00\x00\x00\x09");
        flv.extend_from_slice(&0_u32.to_be_bytes()); // PrevTagSize0 = 0
        let mut meta = Vec::new();
        meta.push(0x02);
        meta.extend_from_slice(&10_u16.to_be_bytes());
        meta.extend_from_slice(b"onMetaData");
        meta.push(0x08);
        meta.extend_from_slice(&5_u32.to_be_bytes());
        amf_num(&mut meta, "duration", 0.04);
        amf_num(&mut meta, "width", 16.0);
        amf_num(&mut meta, "height", 16.0);
        amf_num(&mut meta, "framerate", 25.0);
        amf_num(&mut meta, "videocodecid", codec_id);
        meta.extend_from_slice(&[0x00, 0x00, 0x09]);
        let meta_size = meta.len() as u32;
        flv.push(18);
        flv.extend_from_slice(&meta_size.to_be_bytes()[1..4]);
        flv.extend_from_slice(&[0; 7]);
        flv.extend_from_slice(&meta);
        flv.extend_from_slice(&(11 + meta_size).to_be_bytes());

        let data_size = (1 + pkt_body.len()) as u32;
        flv.push(9); // Video tag
        flv.extend_from_slice(&data_size.to_be_bytes()[1..4]);
        flv.extend_from_slice(&[0, 0, 0, 0]); // Timestamp = 0ms
        flv.extend_from_slice(&[0, 0, 0]); // StreamID = 0
        flv.push(tag_hdr);
        flv.extend_from_slice(&pkt_body);
        flv.extend_from_slice(&(11 + data_size).to_be_bytes());
        flv
    }

    #[test]
    fn flv_video_resources_decode_for_stage_transcode_for_html5_and_export_to_xfl() {
        use neoflash_core::ResourceUnit;

        let vp6_flv = build_synthetic_vp6_flv(false);
        let vp6a_flv = build_synthetic_vp6_flv(true);

        // 1. Native Stage / offline export decode path (`decode_video_stream_bytes`).
        let decoded =
            crate::video::decode_video_stream_bytes("legacy-video-1", "video/x-flv", &vp6_flv)
                .expect("decode FLV stream via ffprobe_ffmpeg");
        assert_eq!(decoded.width, 16);
        assert_eq!(decoded.height, 16);
        assert!(!decoded.frames.is_empty());

        // 2. HTML5 transcode: VP6 -> MP4 (video/mp4), VP6A -> WebM (video/webm with alpha).
        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes(
                "legacy-video-1",
                "legacy/videos/1/clip.flv",
                "video/x-flv",
                &vp6_flv,
            )
            .unwrap(),
        );
        project.resources.push(
            ResourceUnit::from_bytes(
                "legacy-video-2",
                "legacy/videos/2/alpha.flv",
                "video/x-flv",
                &vp6a_flv,
            )
            .unwrap(),
        );
        let mut transcoded = project.clone();
        let (transcode_warnings, _) = transcode_avi_resources_for_html5(&mut transcoded);
        if crate::video::detect_ffmpeg().is_some() {
            assert!(transcode_warnings.is_empty(), "{transcode_warnings:?}");
            let mp4_res = transcoded.resource("legacy-video-1").unwrap();
            assert_eq!(mp4_res.media_type, "video/mp4");
            assert!(mp4_res.path.ends_with(".mp4"));
            let mp4_bytes = mp4_res.decoded_bytes().unwrap();
            assert!(mp4_bytes.len() >= 12 && &mp4_bytes[4..8] == b"ftyp");

            let webm_res = transcoded.resource("legacy-video-2").unwrap();
            assert_eq!(webm_res.media_type, "video/webm");
            assert!(webm_res.path.ends_with(".webm"));
            let webm_bytes = webm_res.decoded_bytes().unwrap();
            assert!(webm_bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]));
        }

        // 3. `fla_preflight::export_fla_bytes_with_profile` succeeds with 0 blocking items
        //    on FLV video + Keyframe.video + linked sound + multi-part `legacy/source.fla.part00`.
        let wav_bytes = [
            b'R', b'I', b'F', b'F', 44, 0, 0, 0, b'W', b'A', b'V', b'E', b'f', b'm', b't', b' ',
            16, 0, 0, 0, 1, 0, 1, 0, 0x44, 0xAC, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, b'd', b'a',
            b't', b'a', 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        project.resources.push(
            ResourceUnit::from_bytes(
                "ClickSound",
                "legacy/sounds/1/click.wav",
                "audio/wav",
                &wav_bytes,
            )
            .unwrap(),
        );
        project.resources.push(
            ResourceUnit::from_bytes(
                "fla-original-0",
                "legacy/source.fla.part00",
                "application/octet-stream",
                b"chunk0",
            )
            .unwrap(),
        );
        project.symbols[0].timeline.layers[0].keyframes[0].video =
            Some(neoflash_core::TimelineVideoSpan {
                resource: "legacy-video-1".into(),
                sync: neoflash_core::VideoSyncMode::Event,
                start_offset_ms: 100,
                ..Default::default()
            });
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(neoflash_core::Element {
                id: "vid-el-1".into(),
                kind: neoflash_core::ElementKind::Video {
                    width: 16.0,
                    height: 16.0,
                },
                ..Default::default()
            });
        let (_, fla_rep) = crate::fla_preflight::export_fla_bytes_with_profile(
            &project,
            crate::fla_preflight::FlaTargetProfile::AnimateCC,
            &crate::fla_preflight::FlaExportOptions::default(),
        )
        .expect("export_fla_bytes_with_profile on FLV + trimmed span + part00");
        assert!(!fla_rep.has_blocking(), "{:?}", fla_rep.items);
    }
}
