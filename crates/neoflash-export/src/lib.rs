//! Self-contained HTML5 publication. Authored evaluation remains in the Rust WASM runtime.

pub mod atlas;
pub mod audio;
pub mod fla_preflight;
pub mod fonts;
pub mod image_export;
pub mod legacy_fla;
pub mod lottie;
pub mod profiles;
pub mod publish;
pub mod svg;
pub mod swf;
pub mod video;
pub use atlas::*;
pub use audio::*;
pub use fla_preflight::*;
pub use fonts::*;
pub use image_export::*;
pub use legacy_fla::*;
pub use lottie::*;
pub use profiles::*;
pub use publish::{
    HtmlAcceleration, HtmlPublishConfig, HtmlScaleMode, HtmlStageAlign, HtmlTemplate,
    PlayerChromeConfig, PublishOptions, SizeEstimate, SizeLevel, SizePart, gzip_len,
    validate_runtime_url,
};
pub use svg::*;
pub use swf::*;
pub use video::*;

use base64::{Engine, engine::general_purpose::STANDARD};
use neoflash_core::{ElementKind, Project, SceneRange, Script, ScriptLanguage};
use serde::Serialize;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ResourceLoadItem {
    pub path: String,
    pub category: String,
    pub media_type: String,
    pub raw_bytes: usize,
    pub b64_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct LoadSummary {
    pub project_name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub scenes_count: usize,
    pub frames_count: u32,
    pub symbols_count: usize,
    pub movie_clips_count: usize,
    pub graphics_count: usize,
    pub buttons_count: usize,
    pub symbols_bytes: usize,
    pub resources_count: usize,
    pub audio_count: usize,
    pub audio_bytes: usize,
    pub bitmap_count: usize,
    pub bitmap_bytes: usize,
    pub data_count: usize,
    pub data_bytes: usize,
    pub resources_raw_bytes: usize,
    pub resources_b64_bytes: usize,
    pub resources: Vec<ResourceLoadItem>,
    pub fonts: Vec<String>,
    pub source_files_count: usize,
    pub handler_scripts_count: usize,
    pub scripts_source_bytes: usize,
    pub scripts_compiled_bytes: usize,
    pub published_project_bytes: usize,
    pub bundle_payload_bytes: usize,
    pub runtime_js_bytes: usize,
    pub wasm_bytes: usize,
    pub total_package_bytes: usize,
}

impl LoadSummary {
    pub fn summary_line(&self) -> String {
        let weight = if self.total_package_bytes > 0 {
            format!(
                "Project weight: {} (total HTML5 package: {}, WASM: {}, JS runtime: {})",
                format_bytes(self.bundle_payload_bytes),
                human_unit(self.total_package_bytes),
                human_unit(self.wasm_bytes),
                human_unit(self.runtime_js_bytes),
            )
        } else {
            format!(
                "Project weight: {} (JS runtime: {})",
                format_bytes(self.bundle_payload_bytes),
                human_unit(self.runtime_js_bytes),
            )
        };
        let res_detail = if self.resources_count == 0 {
            "0 resources".to_string()
        } else {
            format!(
                "{} resource(s) [Audio: {} ({}), Bitmap: {} ({}), Data: {} ({})]",
                self.resources_count,
                self.audio_count,
                human_unit(self.audio_bytes),
                self.bitmap_count,
                human_unit(self.bitmap_bytes),
                self.data_count,
                human_unit(self.data_bytes),
            )
        };
        let fonts_detail = if self.fonts.is_empty() {
            "0 fonts".to_string()
        } else {
            format!("{} font(s) ({})", self.fonts.len(), self.fonts.join(", "))
        };
        format!(
            "{weight} | Loads: {} scene(s) ({} frame(s)), {} symbol(s) ({} MC / {} Graphic / {} Button, {}), {}, {}, {} script(s) ({})",
            self.scenes_count,
            self.frames_count,
            self.symbols_count,
            self.movie_clips_count,
            self.graphics_count,
            self.buttons_count,
            human_unit(self.symbols_bytes),
            res_detail,
            fonts_detail,
            self.source_files_count + self.handler_scripts_count,
            human_unit(self.scripts_compiled_bytes),
        )
    }
}

fn human_unit(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{} ({bytes} B)", human_unit(bytes))
    }
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

pub(crate) fn static_runtime_js_bytes() -> usize {
    HOST.len()
        + HOST_MODULES
            .iter()
            .map(|(_, source)| source.len())
            .sum::<usize>()
}

#[derive(Serialize)]
struct Bundle<'a> {
    /// Single main timeline: authored scenes are concatenated before publishing.
    project: &'a Project,
    /// Scene names and frame ranges inside the flattened main timeline.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scenes: Vec<SceneRange>,
    scripts: Vec<&'a CompiledScript>,
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
}

#[derive(Serialize)]
struct ProgramBundle<'a> {
    code: &'a str,
    class_names: &'a [String],
    source_url: &'static str,
    source_map: Option<&'a neoflash_script::SourceMap>,
    source_locations: Vec<neoflash_script::RuntimeSourceLocation>,
}

/// Reports current publish limitations before the user exports their work.
pub fn export_warnings(project: &Project) -> Vec<String> {
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
        .filter(|res| !is_font_resource(res))
        .count();
    if non_font_resources > 0 {
        warnings.push(format!("{} project resource(s) are embedded and available by their relative paths. Embedding bytes alone does not create display placements or install fonts.", project.resources.len()));
    }
    warnings
}

fn decoded_base64_len(encoded: &str) -> usize {
    STANDARD.decode(encoded).map_or_else(
        |_| {
            let trimmed = encoded.trim();
            let pad = trimmed
                .bytes()
                .rev()
                .take_while(|b| *b == b'=')
                .count()
                .min(2);
            (trimmed.len() / 4) * 3 - pad.min((trimmed.len() / 4) * 3)
        },
        |bytes| bytes.len(),
    )
}

/// Compute a structured project weight and asset load breakdown.
pub fn compute_load_summary(project: &Project) -> Result<LoadSummary, ExportError> {
    let (_, summary) = build_size_report_and_summary(project, None)?;
    Ok(summary)
}

/// Generate a human-readable classic Flash `Size Report` text breakdown by scenes/frames,
/// library symbols, referenced fonts, embedded audio/bitmap/data resources, and scripts.
pub fn format_size_report(project: &Project) -> Result<String, ExportError> {
    let (report, _) = build_size_report_and_summary(project, None)?;
    Ok(report)
}

/// Runtime weight a size report quotes: the WebAssembly binary, how it ships and the runtime
/// JavaScript of the package.
#[derive(Clone, Copy)]
pub(crate) struct RuntimeFigures {
    pub(crate) wasm_bytes: usize,
    pub(crate) wasm_inline: bool,
    pub(crate) runtime_js_bytes: usize,
}

fn build_size_report_and_summary(
    project: &Project,
    runtime: Option<RuntimeFigures>,
) -> Result<(String, LoadSummary), ExportError> {
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
    let compiled_scripts = compile_scripts(project, &program)?;
    size_report_from(project, &program, &compiled_scripts, runtime)
}

pub(crate) fn size_report_from(
    project: &Project,
    program: &neoflash_script::ProjectProgram,
    compiled_scripts: &[CompiledScript],
    runtime: Option<RuntimeFigures>,
) -> Result<(String, LoadSummary), ExportError> {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    let (published, flattened_ranges) = neoflash_core::flatten_scenes_with_ranges(project);
    let published = publishable(&published).unwrap_or(published);
    let published_project_bytes = serde_json::to_string(&published)?.len();

    let scene_ids: Vec<&str> = if project.scenes.is_empty() {
        vec![project.root.as_str()]
    } else {
        project.scenes.iter().map(String::as_str).collect()
    };
    let scene_id_set: BTreeSet<&str> = scene_ids.iter().copied().collect();

    let total_frames: u32 = if flattened_ranges.is_empty() {
        project
            .symbol(&project.root)
            .map_or(0, |s| s.timeline.frame_count)
    } else {
        flattened_ranges.iter().map(|r| r.frames).sum()
    };
    let library_symbols: Vec<_> = project
        .symbols
        .iter()
        .filter(|s| !scene_id_set.contains(s.id.as_str()))
        .collect();

    let mut movie_clips_count = 0usize;
    let mut graphics_count = 0usize;
    let mut buttons_count = 0usize;
    let mut symbol_rows = Vec::with_capacity(library_symbols.len());
    let mut symbol_total = 0usize;
    for sym in &library_symbols {
        let elems: usize = sym
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .map(|k| k.elements.len())
            .sum();
        let bytes = serde_json::to_string(sym)?.len();
        symbol_total += bytes;
        let kind = match sym.kind {
            neoflash_core::SymbolKind::MovieClip => {
                movie_clips_count += 1;
                "MovieClip"
            }
            neoflash_core::SymbolKind::Graphic => {
                graphics_count += 1;
                "Graphic"
            }
            neoflash_core::SymbolKind::Button => {
                buttons_count += 1;
                "Button"
            }
        };
        symbol_rows.push((
            sym.name.as_str(),
            kind,
            sym.timeline.frame_count,
            sym.timeline.layers.len(),
            elems,
            bytes,
        ));
    }

    let mut font_stats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    match &element.kind {
                        ElementKind::Text {
                            font_family, text, ..
                        } => {
                            let entry = font_stats.entry(font_family.clone()).or_default();
                            entry.0 += 1;
                            entry.1 += text.chars().count();
                        }
                        ElementKind::TextBox { format, text, .. } => {
                            let entry = font_stats.entry(format.font.clone()).or_default();
                            entry.0 += 1;
                            entry.1 += text.chars().count();
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let (mut audio_count, mut bitmap_count, mut data_count) = (0usize, 0usize, 0usize);
    let (mut audio_raw, mut bitmap_raw, mut other_raw, mut total_b64) =
        (0usize, 0usize, 0usize, 0usize);
    let mut resource_items = Vec::with_capacity(project.resources.len());
    for res in &project.resources {
        let raw = decoded_base64_len(&res.data_base64);
        let b64 = res.data_base64.len();
        total_b64 += b64;
        let category = if res.is_audio() {
            audio_count += 1;
            audio_raw += raw;
            "Audio"
        } else if res.media_type.starts_with("image/") {
            bitmap_count += 1;
            bitmap_raw += raw;
            "Bitmap"
        } else {
            data_count += 1;
            other_raw += raw;
            "Data"
        };
        resource_items.push(ResourceLoadItem {
            path: res.path.clone(),
            category: category.to_string(),
            media_type: res.media_type.clone(),
            raw_bytes: raw,
            b64_bytes: b64,
        });
    }

    let source_unit_bytes: usize = project.sources.iter().map(|u| u.source.len()).sum();
    let handler_source_bytes: usize = compiled_scripts.iter().map(|s| s.source.len()).sum();
    let handler_compiled_bytes: usize = compiled_scripts.iter().map(|s| s.code.len()).sum();
    let program_compiled_bytes = program.code.len();
    let bundle_payload_bytes =
        published_project_bytes + program_compiled_bytes + handler_compiled_bytes;
    let runtime_js_bytes = runtime.map_or_else(static_runtime_js_bytes, |r| r.runtime_js_bytes);
    let wasm_bytes = runtime.map_or(0, |r| r.wasm_bytes);
    let shipped_wasm_bytes = match runtime {
        Some(r) if r.wasm_inline => wasm_bytes.div_ceil(3) * 4,
        _ => wasm_bytes,
    };
    let total_package_bytes = if wasm_bytes > 0 {
        bundle_payload_bytes + shipped_wasm_bytes + runtime_js_bytes
    } else {
        0
    };

    let summary = LoadSummary {
        project_name: project.name.clone(),
        width: project.width,
        height: project.height,
        fps: project.fps,
        scenes_count: scene_ids.len(),
        frames_count: total_frames,
        symbols_count: library_symbols.len(),
        movie_clips_count,
        graphics_count,
        buttons_count,
        symbols_bytes: symbol_total,
        resources_count: project.resources.len(),
        audio_count,
        audio_bytes: audio_raw,
        bitmap_count,
        bitmap_bytes: bitmap_raw,
        data_count,
        data_bytes: other_raw,
        resources_raw_bytes: audio_raw + bitmap_raw + other_raw,
        resources_b64_bytes: total_b64,
        resources: resource_items,
        fonts: font_stats.keys().cloned().collect(),
        source_files_count: project.sources.len(),
        handler_scripts_count: compiled_scripts.len(),
        scripts_source_bytes: source_unit_bytes + handler_source_bytes,
        scripts_compiled_bytes: program_compiled_bytes + handler_compiled_bytes,
        published_project_bytes,
        bundle_payload_bytes,
        runtime_js_bytes,
        wasm_bytes,
        total_package_bytes,
    };

    let mut out = String::new();
    let _ = writeln!(out, "NeoFlash Size Report — {}", project.name);
    let _ = writeln!(
        out,
        "================================================================"
    );
    let _ = writeln!(
        out,
        "Dimensions:      {} x {} px @ {} fps (format v{})",
        project.width, project.height, project.fps, project.format_version
    );
    let _ = writeln!(
        out,
        "Scenes / Frames: {} scene(s), {} total main-timeline frame(s)",
        scene_ids.len(),
        total_frames
    );
    let _ = writeln!(
        out,
        "Library / Assets: {} library symbol(s), {} resource(s), {} source file(s), {} handler script(s)",
        library_symbols.len(),
        project.resources.len(),
        project.sources.len(),
        compiled_scripts.len()
    );
    let _ = writeln!(
        out,
        "Project Weight:  {}",
        format_bytes(bundle_payload_bytes)
    );
    if wasm_bytes > 0 {
        let _ = writeln!(
            out,
            "HTML5 Package:   {} total (Project {} + WASM {} + JS Runtime {})",
            format_bytes(total_package_bytes),
            format_bytes(bundle_payload_bytes),
            format_bytes(wasm_bytes),
            format_bytes(runtime_js_bytes)
        );
    } else {
        let _ = writeln!(out, "JS Runtime Host: {}", format_bytes(runtime_js_bytes));
    }
    let _ = writeln!(out);

    // 1. Scenes & Frames breakdown.
    let _ = writeln!(out, "Scenes & Frames");
    let _ = writeln!(
        out,
        "----------------------------------------------------------------"
    );
    let _ = writeln!(
        out,
        "{:<22} {:>7} {:>8} {:>7} {:>12} {:>12}",
        "Scene / Frame", "Elements", "Scripts", "Sounds", "Frame Bytes", "Cumulative"
    );
    let mut cumulative_frame_bytes = 0usize;
    for scene_id in &scene_ids {
        let Some(scene) = project.symbol(scene_id) else {
            continue;
        };
        let scene_bytes = serde_json::to_string(scene)?.len();
        let _ = writeln!(
            out,
            "[{}] ({} frames, {} layers, {} B total)",
            scene.name,
            scene.timeline.frame_count,
            scene.timeline.layers.len(),
            scene_bytes
        );
        let mut frame_indices = BTreeSet::from([0u32]);
        for layer in &scene.timeline.layers {
            for key in &layer.keyframes {
                frame_indices.insert(key.frame);
            }
        }
        for fs in &scene.timeline.scripts {
            frame_indices.insert(fs.frame);
        }
        for frame_idx in frame_indices {
            let mut elem_count = 0usize;
            let mut sound_count = 0usize;
            let mut frame_bytes = 0usize;
            for layer in &scene.timeline.layers {
                if let Some(key) = layer.keyframes.iter().find(|k| k.frame == frame_idx) {
                    elem_count += key.elements.len();
                    if key.sound.is_some() {
                        sound_count += 1;
                    }
                    frame_bytes += serde_json::to_string(key)?.len();
                }
            }
            let mut script_count = 0usize;
            for fs in scene
                .timeline
                .scripts
                .iter()
                .filter(|fs| fs.frame == frame_idx)
            {
                script_count += 1;
                frame_bytes += serde_json::to_string(fs)?.len();
            }
            cumulative_frame_bytes += frame_bytes;
            let label = format!("  Frame {}", frame_idx + 1);
            let _ = writeln!(
                out,
                "{:<22} {:>7} {:>8} {:>7} {:>12} {:>12}",
                label, elem_count, script_count, sound_count, frame_bytes, cumulative_frame_bytes
            );
        }
    }
    let _ = writeln!(out);

    // 2. Symbols breakdown.
    let _ = writeln!(out, "Symbols");
    let _ = writeln!(
        out,
        "----------------------------------------------------------------"
    );
    if symbol_rows.is_empty() {
        let _ = writeln!(out, "(no library symbols)");
    } else {
        let _ = writeln!(
            out,
            "{:<24} {:<12} {:>7} {:>7} {:>9} {:>12}",
            "Symbol", "Kind", "Frames", "Layers", "Elements", "Bytes"
        );
        for (name, kind, frames, layers, elems, bytes) in &symbol_rows {
            let _ = writeln!(
                out,
                "{:<24} {:<12} {:>7} {:>7} {:>9} {:>12}",
                name, kind, frames, layers, elems, bytes
            );
        }
        let _ = writeln!(
            out,
            "Library Symbols Subtotal: {} B ({})",
            symbol_total,
            human_unit(symbol_total)
        );
    }
    let _ = writeln!(out);

    // 3. Fonts breakdown.
    let _ = writeln!(out, "Fonts");
    let _ = writeln!(
        out,
        "----------------------------------------------------------------"
    );
    if font_stats.is_empty() {
        let _ = writeln!(out, "(no text fonts referenced)");
    } else {
        let embedded = embedded_font_families(project);
        let _ = writeln!(
            out,
            "{:<28} {:>12} {:>12} {:<16}",
            "Font Family", "Placements", "Characters", "Delivery"
        );
        for (font, (placements, chars)) in &font_stats {
            let delivery = if embedded.contains(&font.to_ascii_lowercase()) {
                "embedded (@font-face)"
            } else {
                "system reference"
            };
            let _ = writeln!(
                out,
                "{:<28} {:>12} {:>12} {:<16}",
                font, placements, chars, delivery
            );
        }
    }
    let _ = writeln!(out);

    // 4. Embedded Resources (Audio / Bitmaps / Other).
    let _ = writeln!(out, "Embedded Resources (Audio / Bitmaps / Data)");
    let _ = writeln!(
        out,
        "----------------------------------------------------------------"
    );
    if summary.resources.is_empty() {
        let _ = writeln!(out, "(no embedded resources)");
    } else {
        let _ = writeln!(
            out,
            "{:<24} {:<10} {:<20} {:>10} {:>12}",
            "Path", "Category", "Media Type", "Raw Bytes", "Bundle B64"
        );
        for item in &summary.resources {
            let _ = writeln!(
                out,
                "{:<24} {:<10} {:<20} {:>10} {:>12}",
                item.path, item.category, item.media_type, item.raw_bytes, item.b64_bytes
            );
        }
        let _ = writeln!(
            out,
            "Resources Subtotal: Audio {} B, Bitmap {} B, Data {} B (Raw {} B / Base64 {} B)",
            audio_raw,
            bitmap_raw,
            other_raw,
            audio_raw + bitmap_raw + other_raw,
            total_b64
        );
    }
    let _ = writeln!(out);

    // 5. Scripts breakdown.
    let _ = writeln!(out, "Scripts");
    let _ = writeln!(
        out,
        "----------------------------------------------------------------"
    );
    if project.sources.is_empty() && compiled_scripts.is_empty() {
        let _ = writeln!(out, "(no authored scripts)");
    } else {
        let _ = writeln!(
            out,
            "{:<34} {:<16} {:>12} {:>12}",
            "Location", "Language", "Source Bytes", "Compiled JS"
        );
        for unit in &project.sources {
            let _ = writeln!(
                out,
                "{:<34} {:<16} {:>12} {:>12}",
                unit.path,
                format!("{:?}", unit.language),
                unit.source.len(),
                "-"
            );
        }
        if !project.sources.is_empty() || program_compiled_bytes > 0 {
            let _ = writeln!(
                out,
                "{:<34} {:<16} {:>12} {:>12}",
                "[Compiled Project Program]",
                format!("{} class(es)", program.class_names.len()),
                source_unit_bytes,
                program_compiled_bytes
            );
        }
        for script in compiled_scripts {
            let _ = writeln!(
                out,
                "{:<34} {:<16} {:>12} {:>12}",
                script.location,
                format!("{:?}", script.language),
                script.source.len(),
                script.code.len()
            );
        }
        let _ = writeln!(
            out,
            "Scripts Subtotal: Source {} B, Compiled JS {} B",
            source_unit_bytes + handler_source_bytes,
            program_compiled_bytes + handler_compiled_bytes
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Published Bundle Payload (excluding WASM engine): {} B ({})",
        bundle_payload_bytes,
        human_unit(bundle_payload_bytes)
    );
    Ok((out, summary))
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

/// Authoring-only data that never reaches the published bundle: ruler guides. Returns a stripped
/// copy only when there is something to strip, so ordinary exports do not clone the project.
fn publishable(project: &Project) -> Option<Project> {
    let has_guides = project
        .symbols
        .iter()
        .any(|symbol| !symbol.timeline.guides.is_empty());
    has_guides.then(|| {
        let mut copy = project.clone();
        for symbol in &mut copy.symbols {
            symbol.timeline.guides.clear();
        }
        copy
    })
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
    let options = options.validated().map_err(ExportError::Options)?;
    let prepared = prepare(project, runtime_dir, options.fonts, options.audio)?;
    let package = assemble(project, &prepared, &options.size)?;
    write_package(output_dir, &package.files, &project.name, "project")
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
    let prepared = prepare_with_config(
        project,
        runtime_dir,
        options.fonts,
        options.audio,
        Some(config),
    )?;
    let package = assemble_with_config(project, &prepared, &options.size, Some(config))?;
    write_package(output_dir, &package.files, &project.name, "project")
}

/// Export a self-contained Desktop application bundle (`.app` on macOS plus a portable Windows
/// `.bat` launcher) wrapping the Clean HTML5 package in `output_dir`.
pub fn export_desktop_app(
    project: &Project,
    output_dir: &Path,
    runtime_dir: &Path,
    options: &PublishOptions,
    config: &HtmlPublishConfig,
) -> Result<PathBuf, ExportError> {
    let mut app_config = config.clone();
    app_config.include_html = true;
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
    fs::create_dir_all(&macos_dir)?;
    fs::create_dir_all(&web_dir)?;
    export_project_with_config(project, &web_dir, runtime_dir, options, &app_config)?;
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
        "sprite_sheet.png",
        "sprite_sheet.json",
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

fn validate_package_file_names(files: &[(String, Vec<u8>)]) -> Result<(), ExportError> {
    let mut names: Vec<&str> = Vec::with_capacity(files.len());
    for (name, _) in files {
        if name.eq_ignore_ascii_case(MARKER) {
            return Err(ExportError::Options(format!(
                "package file '{name}' collides with the NeoFlash ownership marker"
            )));
        }
        if let Some(previous) = names
            .iter()
            .find(|previous| package_paths_collide(name, previous))
        {
            return Err(ExportError::Options(format!(
                "package paths '{previous}' and '{name}' collide by name or file/directory prefix"
            )));
        }
        names.push(name);
    }
    Ok(())
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
    let video_manifest = neoflash_core::video_manifest_entries(&published);
    let external_videos = neoflash_core::strip_non_embedded_video_bytes(&mut published);
    let fonts = apply_font_policy(&mut published, font_policy);
    apply_audio_policy(&mut published, audio_policy)?;
    if let Some(cfg) = config {
        apply_bitmap_jpeg_quality(&mut published, cfg.jpeg_quality);
    }
    let runtime_id = shared_runtime_id(&bindings, &wasm);
    let mut warnings = export_warnings(project);
    if font_policy == FontExportPolicy::Subset
        && fonts.subset_stats.iter().any(|stats| !stats.subset_applied)
    {
        warnings.push(
            "Font subset requested: NeoFlash preserved the complete browser-loadable font because true SFNT/WOFF subsetting is not implemented yet."
                .to_owned(),
        );
    }
    Ok(Prepared {
        program,
        scripts,
        published,
        scenes,
        fonts,
        external_videos,
        video_manifest,
        warnings,
        bindings,
        wasm,
        runtime_id,
    })
}

fn apply_bitmap_jpeg_quality(project: &mut Project, default_quality: u8) {
    let quality = default_quality.clamp(1, 100);
    for resource in &mut project.resources {
        if !resource.media_type.starts_with("image/") {
            continue;
        }
        let target_q = match resource.bitmap_export {
            Some(settings) => {
                if settings.compression != neoflash_core::BitmapCompression::PhotoJpeg {
                    continue;
                }
                if !settings.use_imported_jpeg_data {
                    Some(settings.jpeg_quality.clamp(1, 100))
                } else if quality != 80 {
                    Some(quality)
                } else {
                    None
                }
            }
            None => (quality != 80 && resource.media_type == "image/jpeg").then_some(quality),
        };
        let Some(target_q) = target_q else {
            continue;
        };
        if let Ok(raw) = STANDARD.decode(resource.data_base64.as_ref())
            && let Ok(img) = image::load_from_memory(&raw)
        {
            let rgb = img.to_rgb8();
            let mut out = Vec::new();
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, target_q);
            if encoder
                .encode(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .is_ok()
                && out.len() <= neoflash_core::MAX_RESOURCE_BYTES
            {
                resource.media_type = "image/jpeg".to_owned();
                resource.data_base64 = STANDARD.encode(&out).into();
            }
        }
    }
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
        publish::omitted_modules(project, &sources)
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
    let bindings_bytes = if shared_url.is_none() && !level.wasm_inline() {
        format!(
            "{}\nglobalThis.NeoFlashWasmBase64 = \"{}\";\n",
            prepared.bindings,
            STANDARD.encode(wasm)
        )
        .into_bytes()
    } else {
        prepared.bindings.clone().into_bytes()
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
        project,
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
    let scriptless = publish::script_corpus(project).trim().is_empty()
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
    let non_default_config = config.filter(|c| **c != HtmlPublishConfig::default());
    let bundle = Bundle {
        project: &prepared.published,
        scenes: prepared.scenes.clone(),
        scripts: effective_scripts.iter().collect(),
        program,
        wasm: level.wasm_inline().then(|| STANDARD.encode(wasm)),
        wasm_url: match (level, &shared_url) {
            (SizeLevel::Full, _) => None,
            (_, Some(url)) => Some(format!("{}/{WASM_FILE}", url.trim_end_matches('/'))),
            _ => Some(WASM_FILE.to_owned()),
        },
        runtime_id: shared_url.as_ref().map(|_| prepared.runtime_id.clone()),
        player_config: non_default_config,
        video_manifest: &prepared.video_manifest,
        publish: PublishInfo {
            level: level.name(),
            omitted_modules: omitted_titles.clone(),
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
    let html = html_document_with_config(
        &title,
        project.width,
        project.height,
        &prepared.fonts.css_rules,
        &scripts,
        config,
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
    if config.is_some_and(|c| c.sprite_sheets)
        && let Some((png_bytes, sheet_manifest)) =
            atlas::pack_project_bitmap_resources_sprite_sheet(
                &prepared.published,
                config.is_some_and(|c| !c.minify),
            )?
    {
        files.push(("sprite_sheet.png".into(), png_bytes));
        files.push(("sprite_sheet.json".into(), sheet_manifest.into_bytes()));
    }
    let mut external_paths = Vec::<String>::new();
    for ext_vid in &prepared.external_videos {
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
        if !ext_vid.bytes.is_empty() {
            files.push((ext_vid.relative_path.clone(), ext_vid.bytes.clone()));
        }
    }
    let include_html = config.map(|c| c.include_html).unwrap_or(true);
    if include_html {
        files.push(("bundle.js".into(), bundle_js.into_bytes()));
        files.push(("README.txt".into(), readme.into_bytes()));
        files.push(("index.html".into(), html.into_bytes()));
    } else {
        files.push(("README.txt".into(), readme.into_bytes()));
        files.push(("bundle.js".into(), bundle_js.into_bytes()));
    }
    validate_package_file_names(&files)?;

    let resources: String = prepared
        .published
        .resources
        .iter()
        .map(|resource| resource.data_base64.as_ref())
        .collect();
    let project_json = serde_json::to_string(&prepared.published)?;
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PublishSourceMapShift {
    generated_lines: isize,
    first_line_columns: isize,
}

fn begins_with_use_strict_directive(code: &str) -> bool {
    let trimmed = code.trim_start();
    ["\"use strict\"", "'use strict'"]
        .into_iter()
        .any(|directive| {
            trimmed.strip_prefix(directive).is_some_and(|tail| {
                tail.is_empty()
                    || tail.starts_with(';')
                    || tail.starts_with('\n')
                    || tail.starts_with('\r')
            })
        })
}

fn strip_generated_use_strict(code: &str) -> Option<(String, PublishSourceMapShift)> {
    for directive in ["\"use strict\";", "'use strict';"] {
        if let Some(rest) = code.strip_prefix(&format!("{directive}\r\n")) {
            // Preserve the line break so existing generated line positions remain exact.
            return Some((format!("\r\n{rest}"), PublishSourceMapShift::default()));
        }
        if let Some(rest) = code.strip_prefix(&format!("{directive}\n")) {
            // Preserve the line break so existing generated line positions remain exact.
            return Some((format!("\n{rest}"), PublishSourceMapShift::default()));
        }
        if let Some(rest) = code.strip_prefix(&format!("{directive} ")) {
            return Some((
                rest.to_owned(),
                PublishSourceMapShift {
                    generated_lines: 0,
                    first_line_columns: -(directive.encode_utf16().count() as isize + 1),
                },
            ));
        }
        if let Some(rest) = code.strip_prefix(directive) {
            return Some((
                rest.to_owned(),
                PublishSourceMapShift {
                    generated_lines: 0,
                    first_line_columns: -(directive.encode_utf16().count() as isize),
                },
            ));
        }
    }
    None
}

/// Apply only transformations that are safe without reparsing executable JavaScript.
///
/// `minify` still compacts the surrounding JSON bundle, but executable program and handler code
/// is preserved byte-for-byte. The previous line-oriented whitespace/comment remover could change
/// token boundaries (`x + +y` -> `x++y`) and interpreted `//` inside multiline templates as a
/// comment. Strict mode is inserted on its own generated line so Source Map v3 positions can be
/// rebased exactly.
fn apply_js_publish_transforms(
    code: &str,
    config: Option<&HtmlPublishConfig>,
) -> (String, PublishSourceMapShift) {
    let Some(cfg) = config else {
        return (code.to_owned(), PublishSourceMapShift::default());
    };

    if cfg.strict_mode {
        if begins_with_use_strict_directive(code) {
            (code.to_owned(), PublishSourceMapShift::default())
        } else {
            (
                format!("\"use strict\";\n{code}"),
                PublishSourceMapShift {
                    generated_lines: 1,
                    first_line_columns: 0,
                },
            )
        }
    } else {
        strip_generated_use_strict(code)
            .unwrap_or_else(|| (code.to_owned(), PublishSourceMapShift::default()))
    }
}

fn apply_publish_source_map_shift(
    map: &neoflash_script::SourceMap,
    shift: PublishSourceMapShift,
) -> neoflash_script::SourceMap {
    if shift == PublishSourceMapShift::default() {
        return map.clone();
    }

    let removed_lines = shift.generated_lines.unsigned_abs();
    let segments = map
        .decode_segments()
        .into_iter()
        .filter_map(|mut segment| {
            let original_generated_line = segment.generated_line;
            if shift.generated_lines >= 0 {
                segment.generated_line = segment
                    .generated_line
                    .saturating_add(shift.generated_lines as usize);
            } else if segment.generated_line < removed_lines {
                return None;
            } else {
                segment.generated_line -= removed_lines;
            }

            if original_generated_line == 0 && shift.first_line_columns != 0 {
                if shift.first_line_columns >= 0 {
                    segment.generated_column = segment
                        .generated_column
                        .saturating_add(shift.first_line_columns as usize);
                } else {
                    segment.generated_column = segment
                        .generated_column
                        .saturating_sub(shift.first_line_columns.unsigned_abs());
                }
            }
            Some(segment)
        })
        .collect();
    neoflash_script::SourceMap::from_segments(
        map.file.clone(),
        map.sources.clone(),
        map.sources_content.clone(),
        map.names.clone(),
        segments,
    )
}

fn build_fallback_bundle_source_map(
    scripts: &[CompiledScript],
    program_code: &str,
) -> neoflash_script::SourceMap {
    if !scripts.is_empty() {
        let modules: Vec<(String, String, String)> = scripts
            .iter()
            .map(|s| (s.location.clone(), s.source.clone(), s.code.clone()))
            .collect();
        let combined_code = scripts
            .iter()
            .map(|s| s.code.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        neoflash_script::build_bundle_source_map("neoflash-project.js", &modules, &combined_code)
    } else {
        neoflash_script::build_source_map("neoflash-project.js", program_code, program_code)
    }
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

fn shared_runtime_files_from(bindings: &str, wasm: &[u8], id: &str) -> Vec<(String, Vec<u8>)> {
    let bindings_with_fallback = format!(
        "{bindings}\nglobalThis.NeoFlashWasmBase64 = \"{}\";\n",
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
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "generator": "NeoFlash",
        "format_version": 1,
        "kind": kind,
        "project": name,
        "files": names,
    }))?
    .into_bytes())
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
    let mut previous_files = Vec::new();
    if output_dir.exists() && fs::read_dir(output_dir)?.next().is_some() {
        let marker = fs::read_to_string(output_dir.join(MARKER))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .filter(|marker| marker["generator"] == "NeoFlash" && marker["format_version"] == 1)
            .ok_or_else(|| ExportError::Destination(output_dir.display().to_string()))?;
        previous_files = marker["files"]
            .as_array()
            .map(|names| {
                names
                    .iter()
                    .filter_map(|name| name.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
    }
    fs::create_dir_all(output_dir)?;
    let current_names: BTreeSet<String> =
        files.iter().map(|(name, _)| name.to_lowercase()).collect();
    let marker = package_marker(files, name, kind)?;
    // Prepare all data before writing. Replace only generated files, with index written last.
    let (last, rest) = files
        .split_last()
        .expect("a package always has an index file");
    for (file, bytes) in rest {
        atomic_write(&output_dir.join(file), bytes)?;
    }
    atomic_write(&output_dir.join(MARKER), &marker)?;
    atomic_write(&output_dir.join(&last.0), &last.1)?;
    for stale in previous_files {
        let plain = !stale.contains(['/', '\\']) && stale != MARKER && !stale.starts_with('.');
        let valid_rel = neoflash_core::validate_external_video_path(&stale).is_ok();
        // A case-only rename addresses the same file on the default macOS filesystem. Never
        // delete the freshly written spelling while cleaning the marker's previous spelling.
        if (plain || valid_rel) && !current_names.contains(&stale.to_lowercase()) {
            let _ = fs::remove_file(output_dir.join(stale));
        }
    }
    Ok(output_dir.join(&last.0))
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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
        let result = write_result.and_then(|()| fs::rename(&temp, path));
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        return result;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not reserve a unique temporary export file",
    ))
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
    html_document_with_config(title, 800, 600, font_css, scripts, None)
}

fn html_document_with_config(
    title: &str,
    width: u32,
    height: u32,
    font_css: &str,
    scripts: &[String],
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
    @media (max-width: 600px) {{ footer {{ padding: 8px; }} .transport {{ gap: 6px; }} .preview-info {{ flex-basis: 100%; }} #name {{ display: none; }} }}{font_rules}
  </style>
{script_tags}</head>
<body{body_attr}>
  <main id="viewport" tabindex="-1" aria-label="Project preview"><div id="stage-space"><div id="stage-surface" role="group" aria-label="{title}"><canvas id="stage" aria-label="{title}"></canvas><div id="semantic-layer"></div></div></div></main>
  <footer{footer_attr}><div class="transport" aria-label="Preview controls"{transport_attr}><button id="toggle" type="button">Pause</button><button id="restart" type="button">Restart</button><select id="zoom" aria-label="Preview zoom"><option value="fit">Fit</option><option value="0.25">25%</option><option value="0.5">50%</option><option value="0.75">75%</option><option value="1">100%</option><option value="1.5">150%</option><option value="2">200%</option><option value="4">400%</option></select><button id="pan" type="button" aria-pressed="false" title="Drag the stage to pan. Touch scrolling also works.">Pan</button><button id="fullscreen-toggle" type="button" aria-pressed="false" title="Toggle fullscreen stage"{fullscreen_attr}>Fullscreen</button><button id="audio-toggle" type="button" hidden>Enable audio</button><button id="report-toggle" type="button" aria-pressed="false" title="Show project weight and loaded assets breakdown" hidden>Size report</button></div><div class="preview-info"><span id="name">{display_name}</span><span id="status"{status_attr}></span><span id="load-info"{load_attr}></span><span id="audio-status" role="status" aria-live="polite" aria-atomic="true" hidden></span><span id="trace" role="status" aria-live="polite" aria-atomic="true"{trace_attr}></span></div></footer>
  <div id="loading" role="status"{loading_attr}>Loading project…</div><pre id="size-report" aria-label="Project size and load report"></pre><pre id="error" role="alert"></pre>
</body>
</html>
"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    fn temp() -> PathBuf {
        static ID: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-export-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn runtime(path: &Path) {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("neoflash_web.js"), "var NeoFlashWasm;").unwrap();
        fs::write(path.join("neoflash_web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
    }
    #[test]
    fn atomic_write_uses_distinct_temps_and_cleans_them_after_parallel_replacement() {
        let dir = temp();
        let png = dir.join("sheet.png");
        let json = dir.join("sheet.json");
        let mut workers = Vec::new();
        for index in 0..16u8 {
            let path = if index % 2 == 0 {
                png.clone()
            } else {
                json.clone()
            };
            workers.push(std::thread::spawn(move || {
                let bytes = vec![index; 1024];
                atomic_write(&path, &bytes).expect("parallel atomic write");
            }));
        }
        for worker in workers {
            worker.join().expect("atomic writer thread");
        }
        for path in [&png, &json] {
            let bytes = fs::read(path).expect("final atomic output");
            assert_eq!(bytes.len(), 1024);
            assert!(bytes.iter().all(|byte| *byte == bytes[0]));
        }
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".neoflash-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files leaked: {leftovers:?}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn package_paths_reject_generated_name_collisions_before_writing() {
        assert!(reserved_publish_path("index.html"));
        assert!(reserved_publish_path("RUNTIME.JS"));
        assert!(reserved_publish_path("neoflash-export.json"));
        assert!(reserved_publish_path("runtime.js/clip.mp4"));
        assert!(!reserved_publish_path("media/clip.mp4"));

        let duplicate = vec![
            ("media/Clip.mp4".to_owned(), vec![1]),
            ("media/clip.mp4".to_owned(), vec![2]),
        ];
        assert!(validate_package_file_names(&duplicate).is_err());
        let marker = vec![(MARKER.to_owned(), vec![1])];
        assert!(validate_package_file_names(&marker).is_err());
        let file_directory_collision = vec![
            ("runtime.js".to_owned(), vec![1]),
            ("RUNTIME.JS/clip.mp4".to_owned(), vec![2]),
        ];
        assert!(validate_package_file_names(&file_directory_collision).is_err());
        assert!(package_paths_collide("media", "Media/clip.mp4"));

        let dir = temp();
        let sentinel = dir.join("keep.txt");
        fs::write(&sentinel, b"keep").unwrap();
        assert!(write_package(&dir, &file_directory_collision, "collision", "project").is_err());
        assert_eq!(fs::read(&sentinel).unwrap(), b"keep");
        assert!(!dir.join("media").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_external_video_still_rejects_generated_package_path_collision() {
        let dir = temp();
        let runtime_dir = dir.join("runtime");
        runtime(&runtime_dir);
        let output_dir = dir.join("output");

        let mut project = Project::blank();
        project.format_version = 11;
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes("video", "index.html", "video/mp4", &[])
                .unwrap(),
        );
        project.symbols[0].timeline.layers[0].keyframes[0].video = Some(
            neoflash_core::TimelineVideoSpan::external("video", "index.html"),
        );
        project.validate().unwrap();

        let error = export_project_with_config(
            &project,
            &output_dir,
            &runtime_dir,
            &PublishOptions::default(),
            &HtmlPublishConfig::clean_publish_default(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("collides with a generated package file"),
            "got: {error}"
        );
        assert!(
            !output_dir.exists(),
            "failed preflight must not write output"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn package_case_only_asset_rename_never_deletes_the_new_file() {
        let dir = temp();
        let first = vec![
            ("Media/clip.mp4".to_owned(), b"old".to_vec()),
            ("index.html".to_owned(), b"first".to_vec()),
        ];
        write_package(&dir, &first, "case rename", "project").unwrap();

        let second = vec![
            ("media/clip.mp4".to_owned(), b"new".to_vec()),
            ("index.html".to_owned(), b"second".to_vec()),
        ];
        write_package(&dir, &second, "case rename", "project").unwrap();
        assert_eq!(fs::read(dir.join("media/clip.mp4")).unwrap(), b"new");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn export_escapes_title_and_keeps_sources_external() {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);
        let mut project = Project::blank();
        project.name = "</title><script>alert('x')</script>".into();
        project.format_version = 3;
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes(
                "sample",
                "data/sample.txt",
                "text/plain",
                b"</script>portable data",
            )
            .unwrap(),
        );
        let output = dir.join("output");
        export_project(&project, &output, &rt).unwrap();
        let html = fs::read_to_string(output.join("index.html")).unwrap();
        assert!(html.contains("&lt;/title&gt;&lt;script&gt;"));
        assert!(!html.contains("<script>alert"));
        assert!(
            fs::read_to_string(output.join("bundle.js"))
                .unwrap()
                .contains("AGFzbQEAAAA=")
        );
        assert!(!html.contains("https://"));
        assert!(html.find("src=\"path.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
        assert!(html.find("src=\"net.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
        assert!(
            html.find("src=\"external.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap()
        );
        assert!(html.find("src=\"bitmap.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
        assert!(html.find("src=\"text.js\"").unwrap() < html.find("src=\"display.js\"").unwrap());
        assert!(
            html.find("src=\"text-input.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap()
        );
        assert!(
            html.find("src=\"filters.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap()
        );
        assert!(
            html.find("src=\"audio-formats.js\"").unwrap() < html.find("src=\"audio.js\"").unwrap()
        );
        assert!(html.find("src=\"audio.js\"").unwrap() < html.find("src=\"video.js\"").unwrap());
        assert!(html.find("src=\"video.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
        assert!(html.find("src=\"xml.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
        assert!(html.find("src=\"loader.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
        assert!(html.find("src=\"amf.js\"").unwrap() < html.find("src=\"storage.js\"").unwrap());
        assert!(
            html.find("src=\"storage.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap()
        );
        assert!(html.find("src=\"air.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap());
        assert!(
            html.find("src=\"as3-net-sys.js\"").unwrap() < html.find("src=\"runtime.js\"").unwrap()
        );
        assert!(
            html.find("src=\"as3-media-events.js\"").unwrap()
                < html.find("src=\"runtime.js\"").unwrap()
        );
        assert!(
            fs::read_to_string(output.join("air.js"))
                .unwrap()
                .contains("NeoFlashAIR")
        );
        assert!(
            fs::read_to_string(output.join("as3-media-events.js"))
                .unwrap()
                .contains("NeoFlashAS3MediaEvents")
        );
        let bundle = fs::read_to_string(output.join("bundle.js")).unwrap();
        assert!(bundle.contains("PC9zY3JpcHQ+cG9ydGFibGUgZGF0YQ=="));
        assert!(
            !output.join("data/sample.txt").exists(),
            "resource bytes stay in the portable bundle, without filesystem path writes"
        );
        assert!(
            fs::read_to_string(output.join("net.js"))
                .unwrap()
                .contains("NeoFlashNet")
        );
        assert!(
            fs::read_to_string(output.join("external.js"))
                .unwrap()
                .contains("NeoFlashExternal")
        );
        assert!(
            fs::read_to_string(output.join("path.js"))
                .unwrap()
                .contains("isPointInStroke")
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn size_levels_change_delivery_and_leave_out_only_unnamed_families() {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);
        let banner = Project::blank();
        let out = dir.join("out");
        let publish = |project: &Project, size: SizeLevel| {
            let options = PublishOptions {
                fonts: FontExportPolicy::Embed,
                size,
                audio: AudioExportPolicy::Original,
            };
            export_project_with_options(project, &out, &rt, &options).unwrap();
            let bundle = fs::read_to_string(out.join("bundle.js")).unwrap();
            let html = fs::read_to_string(out.join("index.html")).unwrap();
            (bundle, html)
        };

        let (bundle, _) = publish(&banner, SizeLevel::Full);
        assert!(bundle.contains("\"wasm\":\"AGFzbQEAAAA=\"") && !out.join(WASM_FILE).exists());

        let (bundle, _) = publish(&banner, SizeLevel::Balanced);
        assert!(
            bundle.contains("\"wasm_url\":\"neoflash_web_bg.wasm\"")
                && !bundle.contains("\"wasm\":")
        );
        assert!(out.join(WASM_FILE).exists() && out.join("air.js").exists());

        // A banner without scripts drops every script family but keeps sound and video transport.
        let (bundle, html) = publish(&banner, SizeLevel::Compact);
        for gone in [
            "air.js",
            "as3-fte-3d.js",
            "as2-object.js",
            "xml.js",
            "timers.js",
        ] {
            assert!(
                !out.join(gone).exists() && !html.contains(gone),
                "{gone} left out"
            );
        }
        for kept in [
            "net.js",
            "byte-array.js",
            "audio.js",
            "display.js",
            "runtime.js",
            WASM_FILE,
        ] {
            assert!(out.join(kept).exists(), "{kept} kept");
        }
        assert!(
            !bundle.contains("\"program\":"),
            "a project without scripts ships no prelude"
        );
        assert!(bundle.contains("\"level\":\"compact\"") && bundle.contains("AIR desktop APIs"));

        let mut fte = Project::blank();
        let root = fte.root.clone();
        fte.symbol_mut(&root)
            .unwrap()
            .timeline
            .scripts
            .push(neoflash_core::FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source:
                    "import flash.text.engine.ElementFormat;\ntrace(new ElementFormat().fontSize);"
                        .into(),
            },
        });
        publish(&fte, SizeLevel::Compact);
        assert!(out.join("as3-fte-3d.js").exists() && out.join("xml.js").exists());
        assert!(!out.join("air.js").exists() && !out.join("as3-net-sys.js").exists());

        let (bundle, html) = publish(
            &banner,
            SizeLevel::Shared("https://cdn.example.com/nf/".into()),
        );
        let mut files: Vec<_> = fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(
            files,
            ["README.txt", "bundle.js", "index.html", MARKER],
            "stale runtime files are removed"
        );
        assert!(html.contains("src=\"https://cdn.example.com/nf/runtime-id.js\""));
        assert!(
            html.contains("src=\"bundle.js\"")
                && html.contains("src=\"https://cdn.example.com/nf/air.js\"")
        );
        assert!(
            bundle.contains("\"wasm_url\":\"https://cdn.example.com/nf/neoflash_web_bg.wasm\"")
        );
        assert!(bundle.contains("\"runtime_id\":\""));

        let shared = dir.join("shared");
        export_shared_runtime(&shared, &rt).unwrap();
        let id = fs::read_to_string(shared.join(RUNTIME_ID_FILE)).unwrap();
        let bundle_id = bundle
            .split("\"runtime_id\":\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert!(
            id.contains(bundle_id)
                && shared.join(WASM_FILE).exists()
                && shared.join("air.js").exists()
        );

        let estimates = estimate_publish_sizes(
            &banner,
            &rt,
            FontExportPolicy::Embed,
            "https://cdn.example.com/nf",
        )
        .unwrap();
        let names: Vec<_> = estimates.iter().map(|e| e.level.name()).collect();
        assert_eq!(names, ["full", "balanced", "compact", "shared"]);
        assert!(estimates[2].bytes < estimates[1].bytes && estimates[3].bytes < estimates[2].bytes);
        assert!(estimates[3].shared_runtime_bytes.unwrap() > estimates[1].bytes / 2);
        let totals = |folder: &Path| {
            fs::read_dir(folder)
                .unwrap()
                .map(|entry| {
                    let data = fs::read(entry.unwrap().path()).unwrap();
                    (1usize, data.len(), gzip_len(&data))
                })
                .fold((0, 0, 0), |sum, item| {
                    (sum.0 + item.0, sum.1 + item.1, sum.2 + item.2)
                })
        };
        let shared_totals = totals(&shared);
        assert_eq!(estimates[3].shared_runtime_bytes, Some(shared_totals.1));
        assert_eq!(
            estimates[3].shared_runtime_gzip_bytes,
            Some(shared_totals.2)
        );
        for estimate in &estimates {
            publish(&banner, estimate.level.clone());
            assert_eq!(
                totals(&out),
                (estimate.files, estimate.bytes, estimate.gzip_bytes)
            );

            assert_eq!(
                estimate.parts.iter().map(|p| p.bytes).sum::<usize>(),
                estimate.bytes
            );
            assert!(estimate.gzip_bytes < estimate.bytes);
        }
        assert!(estimates[2].omitted_modules.contains(&"air.js".to_owned()));
        assert!(estimates[0].tradeoffs.is_empty() && !estimates[1].tradeoffs.is_empty());
        assert_eq!(
            estimate_publish_options(
                &banner,
                &rt,
                &PublishOptions {
                    fonts: FontExportPolicy::Embed,
                    size: SizeLevel::Shared(" https://cdn.example.com/nf/ ".into()),
                    audio: AudioExportPolicy::Original,
                }
            )
            .unwrap(),
            estimates[3],
            "selected normalized options measure exactly what publication writes"
        );
        let (root_bundle, root_html) = publish(&banner, SizeLevel::Shared("/".into()));
        assert!(root_html.contains("src=\"/runtime-id.js\""));
        assert!(root_bundle.contains("\"wasm_url\":\"/neoflash_web_bg.wasm\""));
        // Rebuilding the runtime must change the identity expected by Shared publications.
        fs::write(rt.join(BINDINGS_FILE), "var NeoFlashWasm; // rebuilt").unwrap();
        let (rebuilt, _) = publish(&banner, SizeLevel::Shared("../runtime".into()));
        assert!(!rebuilt.contains(&format!("\"runtime_id\":\"{bundle_id}\"")));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn estimates_cancel_and_invalid_options_fail_before_runtime_io() {
        let dir = temp();
        let rt = dir.join("runtime");
        let project = Project::blank();
        let invalid = PublishOptions {
            fonts: FontExportPolicy::Embed,
            size: SizeLevel::Shared("https://".into()),
            audio: AudioExportPolicy::Original,
        };
        assert!(matches!(
            export_project_with_options(&project, &dir.join("out"), &rt, &invalid),
            Err(ExportError::Options(_))
        ));
        assert!(matches!(
            estimate_publish_options(&project, &rt, &invalid),
            Err(ExportError::Options(_))
        ));
        assert!(!dir.join("out").exists());
        assert!(
            estimate_publish_levels_cancellable(
                &project,
                &rt,
                FontExportPolicy::Embed,
                &[SizeLevel::Full],
                || true
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(
            estimate_publish_levels_cancellable(
                &project,
                &rt,
                FontExportPolicy::Embed,
                &[],
                || false
            )
            .unwrap(),
            Some(vec![])
        );
        runtime(&rt);
        let mut checks = 0;
        assert!(
            estimate_publish_levels_cancellable(
                &project,
                &rt,
                FontExportPolicy::Embed,
                &[SizeLevel::Full, SizeLevel::Compact],
                || {
                    checks += 1;
                    checks >= 6
                }
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(
            checks, 6,
            "cancellation stops compression without publishing a partial result"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn multiple_scenes_publish_one_main_timeline_with_scene_ranges() {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);
        let mut project = Project::blank();
        project.format_version = 6;
        let mut second = project.symbols[0].clone();
        second.id = "scene-two".into();
        second.name = "Scene 2".into();
        second.timeline.frame_count = 4;
        second.timeline.layers[0].id = "scene-two-layer".into();
        second.timeline.scripts.push(neoflash_core::FrameScript {
            frame: 1,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "gotoAndStop(1, \"Scene 1\");".into(),
            },
        });
        project.scenes = vec![project.root.clone(), second.id.clone()];
        project.symbols.push(second);
        let output = dir.join("output");
        export_project(&project, &output, &rt).unwrap();
        let bundle = fs::read_to_string(output.join("bundle.js")).unwrap();
        let json = bundle
            .strip_prefix("globalThis.NeoFlashBundle = ")
            .and_then(|rest| rest.strip_suffix(";\n"))
            .unwrap();
        let bundle: serde_json::Value = serde_json::from_str(json).unwrap();
        let published: Project = serde_json::from_value(bundle["project"].clone()).unwrap();
        published.validate().unwrap();
        assert!(published.scenes.is_empty());
        assert_eq!(published.symbols.len(), 1);
        let root = published.symbol(&published.root).unwrap();
        assert_eq!(root.timeline.frame_count, 124);
        assert_eq!(root.timeline.scripts[0].frame, 121);
        assert_eq!(
            bundle["scenes"],
            serde_json::json!([
                {"name": "Scene 1", "start": 0, "frames": 120},
                {"name": "Scene 2", "start": 120, "frames": 4},
            ])
        );
        assert!(
            bundle["scripts"][0]["location"]
                .as_str()
                .unwrap()
                .starts_with("Scene 2 / frame 2"),
            "diagnostics keep the authored scene frame"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn export_strips_authoring_guides() {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);
        let mut project = Project::blank();
        project.format_version = neoflash_core::Guide::FORMAT_VERSION;
        project.symbols[0]
            .timeline
            .guides
            .push(neoflash_core::Guide {
                axis: neoflash_core::GuideAxis::Vertical,
                position: 123.,
            });
        let output = dir.join("output");
        export_project(&project, &output, &rt).unwrap();
        let bundle = fs::read_to_string(output.join("bundle.js")).unwrap();
        assert!(bundle.contains("NeoFlashBundle"));
        assert!(!bundle.contains("\"guides\""), "guides are authoring-only");
        assert_eq!(
            project.symbols[0].timeline.guides.len(),
            1,
            "source untouched"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn refuses_unrelated_output_without_modifying_it() {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);
        let out = dir.join("output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("index.html"), "existing work").unwrap();
        assert!(matches!(
            export_project(&Project::blank(), &out, &rt),
            Err(ExportError::Destination(_))
        ));
        assert_eq!(
            fs::read_to_string(out.join("index.html")).unwrap(),
            "existing work"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn missing_runtime_does_not_create_output() {
        let dir = temp();
        let out = dir.join("out");
        assert!(matches!(
            export_project(&Project::blank(), &out, &dir.join("missing")),
            Err(ExportError::MissingRuntime(_))
        ));
        assert!(!out.exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn publication_maps_handlers_and_groups_identical_authored_origins() {
        let mut project = Project::blank();
        project.symbols[0].timeline.frame_count = 2;
        let source = "// ё\ntrace('mapped');\nthrow new Error('boom');";
        project.symbols[0].timeline.scripts = (0..2)
            .map(|frame| neoflash_core::FrameScript {
                frame,
                script: Script {
                    language: ScriptLanguage::ActionScript3,
                    source: source.into(),
                },
            })
            .collect();
        let program = neoflash_script::compile_project(&project).unwrap();
        let scripts = compile_scripts(&project, &program).unwrap();
        assert_eq!(scripts.len(), 1);
        let script = &scripts[0];
        assert_eq!(script.source_url, "neoflash-script-0.js");
        assert_eq!(script.location, "Shared script 1");
        assert_eq!(script.locations.len(), 2);
        assert!(script.locations[0].ends_with("frame 1"));
        assert!(script.locations[1].ends_with("frame 2"));
        assert_eq!(script.source_map.sources, ["Shared script 1"]);
        assert!(
            script
                .source_locations
                .iter()
                .any(|location| location.line == 3 && location.source == "Shared script 1")
        );
        // A duplicate invalid script must report errors, never index an absent compiled entry.
        for frame in &mut project.symbols[0].timeline.scripts {
            frame.script.source = "var = ;".into();
        }
        assert!(matches!(
            compile_scripts(&project, &program),
            Err(ExportError::Scripts(_))
        ));
    }

    #[test]
    fn unsupported_script_blocks_export_before_writing() {
        let dir = temp();
        let out = dir.join("out");
        let mut project = Project::blank();
        project.symbols[0]
            .timeline
            .scripts
            .push(neoflash_core::FrameScript {
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript3,
                    source: "package example { public class Missing {} }".into(),
                },
            });
        assert!(matches!(
            export_project(&project, &out, &dir.join("missing")),
            Err(ExportError::Scripts(_))
        ));
        assert!(!out.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn size_report_breaks_down_scenes_frames_symbols_fonts_resources_and_scripts() {
        let mut project = Project::demo();
        project.format_version = 7;
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes(
                "sfx-beep",
                "audio/beep.wav",
                "audio/wav",
                &[0u8; 44],
            )
            .unwrap(),
        );
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes(
                "img-dot",
                "images/dot.png",
                "image/png",
                b"\x89PNG\r\n\x1a\n",
            )
            .unwrap(),
        );
        project.sources.push(neoflash_core::SourceUnit {
            id: "src-util".into(),
            path: "AppHelper.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: "package { public class AppHelper { public static function tag():String { return \"ok\"; } } }".into(),
        });
        project.symbols[0]
            .timeline
            .scripts
            .push(neoflash_core::FrameScript {
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript3,
                    source: "trace(AppHelper.tag());".into(),
                },
            });
        let report = format_size_report(&project).expect("size report succeeds");
        assert!(report.contains("NeoFlash Size Report —"));
        assert!(report.contains("Project Weight:"));
        assert!(report.contains("Scenes & Frames"));
        assert!(report.contains("Frame 1"));
        assert!(report.contains("Symbols"));
        assert!(report.contains("Fonts"));
        assert!(report.contains("Embedded Resources (Audio / Bitmaps / Data)"));
        assert!(report.contains("audio/beep.wav"));
        assert!(report.contains("Audio"));
        assert!(report.contains("images/dot.png"));
        assert!(report.contains("Bitmap"));
        assert!(report.contains("Scripts"));
        assert!(report.contains("AppHelper.as"));
        assert!(report.contains("Scripts Subtotal:"));
        let summary = compute_load_summary(&project).expect("load summary succeeds");
        assert_eq!(summary.scenes_count, 1);
        assert_eq!(summary.resources_count, 2);
        assert_eq!(summary.audio_count, 1);
        assert_eq!(summary.bitmap_count, 1);
        assert!(summary.summary_line().contains("Project weight:"));
        assert!(summary.summary_line().contains("Loads:"));
    }

    #[test]
    fn font_policies_embed_subset_and_convert_static_text_to_outlines() {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);

        let mut project = Project::demo();
        project.format_version = 7;
        let ttf_bytes = include_bytes!("../../neoflash-ui/assets/fonts/Inter-Regular.ttf");
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes(
                "font-neo",
                "fonts/NeoFont.ttf",
                "font/ttf",
                ttf_bytes,
            )
            .unwrap(),
        );
        // Set the demo text element to use NeoFont
        if let Some(elem) = project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .iter_mut()
            .find(|e| matches!(e.kind, ElementKind::Text { .. }))
            && let ElementKind::Text { font_family, .. } = &mut elem.kind
        {
            *font_family = "NeoFont".into();
        }

        // 1. Embed policy injects @font-face CSS and suppresses unembedded font warning
        let warnings = export_warnings(&project);
        assert!(
            !warnings.iter().any(|w| w.contains("NeoFont")),
            "embedded font resource suppresses missing-font warning: {warnings:?}"
        );
        let out_embed = dir.join("out-embed");
        export_project_with_policy(&project, &out_embed, &rt, FontExportPolicy::Embed).unwrap();
        let html_embed = fs::read_to_string(out_embed.join("index.html")).unwrap();
        assert!(html_embed.contains("@font-face"));
        assert!(html_embed.contains("font-family: \"NeoFont\""));
        assert!(html_embed.contains("src=\"dom-layout.js\""));
        assert!(
            fs::read_to_string(out_embed.join("dom-layout.js"))
                .unwrap()
                .contains("NeoFlashDomLayout")
        );

        // 2. Subset policy subsets embedded font resource to used characters
        let out_subset = dir.join("out-subset");
        export_project_with_policy(&project, &out_subset, &rt, FontExportPolicy::Subset).unwrap();
        let bundle_subset: serde_json::Value = serde_json::from_str(
            fs::read_to_string(out_subset.join("bundle.js"))
                .unwrap()
                .strip_prefix("globalThis.NeoFlashBundle = ")
                .unwrap()
                .strip_suffix(";\n")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(bundle_subset["fonts"]["policy"], "subset");
        assert_eq!(
            bundle_subset["fonts"]["subset_stats"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            bundle_subset["fonts"]["subset_stats"][0]["codepoint_count"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(
            bundle_subset["fonts"]["subset_stats"][0]["subset_applied"], true,
            "subset policy must emit a real SFNT/TTF subset for TrueType fonts"
        );
        assert!(
            bundle_subset["fonts"]["subset_stats"][0]["subset_bytes"]
                .as_u64()
                .unwrap()
                < bundle_subset["fonts"]["subset_stats"][0]["original_bytes"]
                    .as_u64()
                    .unwrap()
        );
        assert!(
            !bundle_subset["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning
                    .as_str()
                    .is_some_and(|warning| warning.contains("complete browser-loadable font")))
        );

        // 3. Outlines policy converts static text placements into vector Path elements
        let out_outlines = dir.join("out-outlines");
        export_project_with_policy(&project, &out_outlines, &rt, FontExportPolicy::Outlines)
            .unwrap();
        let bundle_outlines: serde_json::Value = serde_json::from_str(
            fs::read_to_string(out_outlines.join("bundle.js"))
                .unwrap()
                .strip_prefix("globalThis.NeoFlashBundle = ")
                .unwrap()
                .strip_suffix(";\n")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(bundle_outlines["fonts"]["policy"], "outlines");
        assert!(
            bundle_outlines["fonts"]["outlined_elements"]
                .as_u64()
                .unwrap()
                >= 1
        );
        let published: Project =
            serde_json::from_value(bundle_outlines["project"].clone()).unwrap();
        assert!(
            !published.symbols[0].timeline.layers[0].keyframes[0]
                .elements
                .iter()
                .any(|e| matches!(e.kind, ElementKind::Text { .. })),
            "static text was converted to vector path outlines"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn selected_text_embedding_uses_authored_family_style_ranges_and_field_state() {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);

        let mut project = Project::blank();
        project.format_version = 7;
        let font_id = "font-book-bold";
        let font_bytes = include_bytes!("../../neoflash-ui/assets/fonts/Inter-Regular.ttf");
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes(
                font_id,
                "fonts/Book Antiqua Bold.ttf",
                "font/ttf",
                font_bytes,
            )
            .unwrap(),
        );
        let metadata = serde_json::json!([{
            "id": font_id,
            "name": "Book Antiqua Bold",
            "family": "Book Antiqua",
            "style": "Bold",
            "ranges": [true, false, false, false, false, false, false, false],
            "extra_chars": "Ω",
            "export_for_actionscript": false,
            "class_name": ""
        }]);
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes(
                "font-embedding-metadata",
                "fonts/embedding.json",
                "application/json",
                &serde_json::to_vec(&metadata).unwrap(),
            )
            .unwrap(),
        );
        let mut format = neoflash_core::TextFormat {
            font: "Book Antiqua".to_owned(),
            size: 18.0,
            bold: true,
            ..neoflash_core::TextFormat::default()
        };
        format.set_field_options(Some(neoflash_core::AuthoredTextFieldOptions {
            embed_fonts: true,
            ..Default::default()
        }));
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(neoflash_core::Element {
                id: "embedded-field".to_owned(),
                name: "Headline".to_owned(),
                kind: ElementKind::TextBox {
                    text: "AΩ".to_owned(),
                    width: 180.0,
                    height: 40.0,
                    format,
                    word_wrap: false,
                    multiline: false,
                },
                ..Default::default()
            });
        project.validate().unwrap();
        assert!(has_embedded_font_face(
            &project,
            "Book Antiqua",
            true,
            false
        ));
        assert!(
            !export_warnings(&project)
                .iter()
                .any(|warning| warning.contains("requires embedded font"))
        );

        let mut subset_project = project.clone();
        let subset = apply_font_policy(&mut subset_project, FontExportPolicy::Subset);
        assert_eq!(subset.subset_stats.len(), 1);
        assert!(subset.subset_stats[0].subset_applied);
        assert!(subset.subset_stats[0].subset_bytes < subset.subset_stats[0].original_bytes);
        let subset_decoded = subset_project.resources[0].decoded_bytes().unwrap();
        assert_eq!(subset_decoded.len(), subset.subset_stats[0].subset_bytes);
        let parsed_subset = ab_glyph::FontRef::try_from_slice(&subset_decoded).unwrap();
        let parsed_orig = ab_glyph::FontRef::try_from_slice(font_bytes).unwrap();
        use ab_glyph::Font as _;
        assert_eq!(parsed_subset.glyph_id('A'), parsed_orig.glyph_id('A'));
        assert_eq!(parsed_subset.glyph_id('Ω'), parsed_orig.glyph_id('Ω'));
        assert_eq!(
            subset.subset_stats[0].codepoint_count, 28,
            "space + A-Z + the explicit custom Omega"
        );

        let mut non_sfnt_project = project.clone();
        non_sfnt_project.resources[0] = neoflash_core::ResourceUnit::from_bytes(
            font_id,
            "fonts/Book Antiqua Bold.woff2",
            "font/woff2",
            b"wOF2-mock-font-payload-that-is-not-sfnt-ttf",
        )
        .unwrap();
        let non_sfnt_subset = apply_font_policy(&mut non_sfnt_project, FontExportPolicy::Subset);
        assert!(!non_sfnt_subset.subset_stats[0].subset_applied);
        assert_eq!(
            non_sfnt_subset.subset_stats[0].original_bytes,
            non_sfnt_subset.subset_stats[0].subset_bytes
        );

        let out = dir.join("out-selected-font");
        export_project_with_policy(&project, &out, &rt, FontExportPolicy::Embed).unwrap();
        let html = fs::read_to_string(out.join("index.html")).unwrap();
        assert!(html.contains("font-family: \"Book Antiqua\""));
        assert!(html.contains("font-style: normal"));
        assert!(html.contains("font-weight: 700"));
        assert!(!html.contains("font-family: \"Book Antiqua Bold\""));
        let bundle: serde_json::Value = serde_json::from_str(
            fs::read_to_string(out.join("bundle.js"))
                .unwrap()
                .strip_prefix("globalThis.NeoFlashBundle = ")
                .unwrap()
                .strip_suffix(";\n")
                .unwrap(),
        )
        .unwrap();
        let published: Project = serde_json::from_value(bundle["project"].clone()).unwrap();
        let ElementKind::TextBox { format, .. } = &published.symbols[0].timeline.layers[0]
            .keyframes[0]
            .elements
            .iter()
            .find(|element| element.id == "embedded-field")
            .unwrap()
            .kind
        else {
            panic!("published embedded field missing");
        };
        assert!(
            format
                .field_options()
                .is_some_and(|options| options.embed_fonts)
        );

        let ElementKind::TextBox { format, .. } = &mut project.symbols[0].timeline.layers[0]
            .keyframes[0]
            .elements
            .iter_mut()
            .find(|element| element.id == "embedded-field")
            .unwrap()
            .kind
        else {
            unreachable!();
        };
        format.italic = true;
        assert!(export_warnings(&project).iter().any(|warning| {
            warning.contains("Book Antiqua")
                && warning.contains("Bold Italic")
                && warning.contains("Properties ▸ Embed font ▸ Embedding…")
        }));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn executable_publish_code_preserves_token_boundaries_templates_and_comments_when_minified() {
        let source = "const plus = x + +y;\nconst minus = x - -y;\nconst template = `first\n// this line is template data\n${value /* expression comment */}\nlast`;\nconst matcher = /\\/\\//;";
        let config = HtmlPublishConfig {
            minify: true,
            strict_mode: true,
            ..HtmlPublishConfig::clean_publish_default()
        };

        let (published, shift) = apply_js_publish_transforms(source, Some(&config));

        assert_eq!(shift.generated_lines, 1);
        assert_eq!(shift.first_line_columns, 0);
        assert_eq!(published, format!("\"use strict\";\n{source}"));
        assert!(published.contains("x + +y"));
        assert!(published.contains("x - -y"));
        assert!(published.contains("// this line is template data"));
        assert!(published.contains("/* expression comment */"));
    }

    #[test]
    fn executable_publish_source_maps_follow_strict_prefix_and_same_line_removal() {
        let source = "const answer = value + 1;";
        let base_map = neoflash_script::build_source_map("author.js", source, source);
        let strict_config = HtmlPublishConfig {
            minify: true,
            strict_mode: true,
            ..HtmlPublishConfig::clean_publish_default()
        };
        let (strict_code, strict_shift) = apply_js_publish_transforms(source, Some(&strict_config));
        let strict_map = apply_publish_source_map_shift(&base_map, strict_shift);

        assert_eq!(strict_code, format!("\"use strict\";\n{source}"));
        assert!(
            strict_map
                .decode_segments()
                .iter()
                .all(|segment| segment.generated_line >= 1)
        );
        let mapped = strict_map
            .lookup_original_position(2, 1)
            .expect("first authored token remains mapped after strict prefix");
        assert_eq!(mapped.source, "author.js");
        assert_eq!((mapped.line, mapped.column), (1, 1));

        let same_line = "\"use strict\"; const answer = value + 1;";
        let same_line_map = neoflash_script::SourceMap::from_segments(
            None,
            vec!["author.js".into()],
            vec![source.into()],
            Vec::new(),
            vec![neoflash_script::SourceMappingSegment {
                generated_line: 0,
                generated_column: "\"use strict\"; ".encode_utf16().count(),
                source_index: 0,
                original_line: 0,
                original_column: 0,
                name_index: None,
            }],
        );
        let non_strict_config = HtmlPublishConfig {
            strict_mode: false,
            ..strict_config
        };
        let (non_strict_code, non_strict_shift) =
            apply_js_publish_transforms(same_line, Some(&non_strict_config));
        let non_strict_map = apply_publish_source_map_shift(&same_line_map, non_strict_shift);
        assert_eq!(non_strict_code, source);
        assert_eq!(non_strict_map.decode_segments()[0].generated_column, 0);
    }

    #[test]
    fn html_publish_config_drives_clean_publish_preview_chrome_templates_scale_jpeg_maps_sprites_and_minify()
     {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);

        let mut project = Project::demo();
        project.format_version = 3;
        let mut rgb = image::RgbImage::new(8, 8);
        for (x, y, px) in rgb.enumerate_pixels_mut() {
            *px = image::Rgb([(x * 28) as u8, (y * 28) as u8, ((x + y) * 16) as u8]);
        }
        let mut jpeg_high = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_high, 98);
        enc.encode_image(&image::DynamicImage::ImageRgb8(rgb))
            .unwrap();
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes(
                "photo-jpg",
                "images/photo.jpg",
                "image/jpeg",
                &jpeg_high,
            )
            .unwrap(),
        );

        let options = PublishOptions {
            fonts: FontExportPolicy::Embed,
            size: SizeLevel::Full,
            audio: AudioExportPolicy::Original,
        };

        // 1. Clean publish vs Preview chrome + individual HUD toggles
        let out_clean = dir.join("out-clean");
        let clean_cfg = HtmlPublishConfig::clean_publish_default();
        export_project_with_config(&project, &out_clean, &rt, &options, &clean_cfg).unwrap();
        let html_clean = fs::read_to_string(out_clean.join("index.html")).unwrap();
        assert!(html_clean.contains("<body data-clean=\"true\">"));
        assert!(html_clean.contains("<footer hidden>"));

        let out_hud = dir.join("out-hud");
        let hud_cfg = HtmlPublishConfig {
            chrome: PlayerChromeConfig {
                clean: false,
                show_fps: false,
                show_dimensions: true,
                show_controls: false,
                show_load_summary: false,
                show_trace: true,
            },
            template: HtmlTemplate::Centered,
            scale_mode: HtmlScaleMode::NoScale,
            stage_align: HtmlStageAlign::BottomRight,
            allow_fullscreen: true,
            acceleration: HtmlAcceleration::GpuWebGl,
            loop_timeline: false,
            autoplay: false,
            preloader: false,
            source_maps: true,
            sprite_sheets: true,
            include_trace: false,
            minify: false,
            jpeg_quality: 30,
            strict_mode: true,
            assets_folder: false,
            include_html: true,
        };
        export_project_with_config(&project, &out_hud, &rt, &options, &hud_cfg).unwrap();
        let html_hud = fs::read_to_string(out_hud.join("index.html")).unwrap();
        assert!(!html_hud.contains("<body data-clean=\"true\">"));
        assert!(html_hud.contains("border-radius: 6px; overflow: hidden;"));
        assert!(html_hud.contains("Ideas in motion · 960×600"));
        assert!(html_hud.contains("id=\"fullscreen-toggle\""));
        assert!(html_hud.contains("<span id=\"status\" hidden></span>"));
        assert!(
            html_hud.contains("<div class=\"transport\" aria-label=\"Preview controls\" hidden>")
        );
        assert!(html_hud.contains("<span id=\"load-info\" hidden></span>"));
        assert!(!html_hud.contains(
            "id=\"trace\" role=\"status\" aria-live=\"polite\" aria-atomic=\"true\" hidden"
        ));
        assert!(html_hud.contains("id=\"loading\" role=\"status\" hidden style=\"display:none\""));

        let bundle_hud_raw = fs::read_to_string(out_hud.join("bundle.js")).unwrap();
        assert!(bundle_hud_raw.contains("\"stage_align\": \"bottom_right\""));
        assert!(bundle_hud_raw.contains("\"allow_fullscreen\": true"));
        assert!(bundle_hud_raw.ends_with(";\n"));
        assert!(
            bundle_hud_raw.lines().count() > 20,
            "minify: false pretty-prints bundle.js"
        );
        assert!(out_hud.join("bundle.js.map").is_file());
        assert!(out_hud.join("sprite_sheet.json").is_file());

        let bundle_json = bundle_hud_raw
            .strip_prefix("globalThis.NeoFlashBundle = ")
            .unwrap()
            .strip_suffix(";\n")
            .unwrap();
        let bundle_val: serde_json::Value = serde_json::from_str(bundle_json).unwrap();
        assert!(
            bundle_val["program"]["code"]
                .as_str()
                .unwrap()
                .ends_with("//# sourceMappingURL=bundle.js.map"),
            "the map belongs to the dynamically evaluated neoflash-project.js program"
        );
        let map: serde_json::Value =
            serde_json::from_slice(&fs::read(out_hud.join("bundle.js.map")).unwrap()).unwrap();
        assert_eq!(map["file"], "neoflash-project.js");
        assert_eq!(bundle_val["player_config"]["template"], "centered");
        assert_eq!(bundle_val["player_config"]["scale_mode"], "no_scale");
        assert_eq!(bundle_val["player_config"]["acceleration"], "gpu_web_gl");
        assert_eq!(bundle_val["player_config"]["loop_timeline"], false);
        assert_eq!(bundle_val["player_config"]["autoplay"], false);
        assert_eq!(bundle_val["player_config"]["preloader"], false);
        assert_eq!(bundle_val["player_config"]["include_trace"], false);
        assert_eq!(bundle_val["player_config"]["strict_mode"], true);
        assert_eq!(bundle_val["player_config"]["chrome"]["show_fps"], false);
        assert_eq!(
            bundle_val["player_config"]["chrome"]["show_dimensions"],
            true
        );

        let reencoded_b64 = bundle_val["project"]["resources"][0]["data_base64"]
            .as_str()
            .unwrap();
        let reencoded_bytes = STANDARD.decode(reencoded_b64).unwrap();
        assert!(
            reencoded_bytes.len() < jpeg_high.len(),
            "jpeg_quality=30 re-encodes JPEG to fewer bytes than quality=98 ({} < {})",
            reencoded_bytes.len(),
            jpeg_high.len()
        );

        // Verify background size estimate matches exact on-disk bytes for custom HtmlPublishConfig
        let est_hud = estimate_publish_options_cancellable_with_config(
            &project,
            &rt,
            &options,
            &hud_cfg,
            || false,
        )
        .unwrap()
        .unwrap();
        let totals = |folder: &Path| {
            fs::read_dir(folder)
                .unwrap()
                .map(|entry| {
                    let data = fs::read(entry.unwrap().path()).unwrap();
                    (1usize, data.len(), gzip_len(&data))
                })
                .fold((0, 0, 0), |sum, item| {
                    (sum.0 + item.0, sum.1 + item.1, sum.2 + item.2)
                })
        };
        assert_eq!(
            totals(&out_hud),
            (est_hud.files, est_hud.bytes, est_hud.gzip_bytes)
        );

        // 2. include_html: false omits index.html and matches size estimate
        let mut no_html_cfg = clean_cfg.clone();
        no_html_cfg.include_html = false;
        let out_no_html = dir.join("out-no-html");
        export_project_with_config(&project, &out_no_html, &rt, &options, &no_html_cfg).unwrap();
        assert!(!out_no_html.join("index.html").exists());
        assert!(out_no_html.join("bundle.js").is_file());
        let est_no_html = estimate_publish_options_cancellable_with_config(
            &project,
            &rt,
            &options,
            &no_html_cfg,
            || false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            totals(&out_no_html),
            (est_no_html.files, est_no_html.bytes, est_no_html.gzip_bytes)
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn compact_and_balanced_packages_embed_file_protocol_wasm_fallback_for_authored_and_classic_fla_scenes()
     {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);

        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir.join("../..");
        let mut scenes: Vec<(String, Project)> = vec![("demo".into(), Project::demo())];
        for rel in [
            "examples/fla-corpus/authored/nested-playback-as2/nested-playback-as2.fla",
            "examples/fla-corpus/authored/frame-scripts-as3/frame-scripts-as3.fla",
            "examples/Macromedia Flash MX ActionScript_program files/Superclass.fla",
        ] {
            let path = workspace_root.join(rel);
            if path.is_file()
                && let Ok(proj) = neoflash_import::import_project(&path)
            {
                scenes.push((rel.to_owned(), proj));
            }
        }
        assert!(
            scenes.len() >= 3,
            "expected demo plus at least two imported FLA scenes"
        );

        let totals = |folder: &Path| {
            fs::read_dir(folder)
                .unwrap()
                .map(|entry| {
                    let data = fs::read(entry.unwrap().path()).unwrap();
                    (1usize, data.len(), gzip_len(&data))
                })
                .fold((0, 0, 0), |sum, item| {
                    (sum.0 + item.0, sum.1 + item.1, sum.2 + item.2)
                })
        };

        for (idx, (label, scene)) in scenes.iter().enumerate() {
            for level in [SizeLevel::Full, SizeLevel::Balanced, SizeLevel::Compact] {
                let out = dir.join(format!("scene-{idx}-{}", level.name()));
                let options = PublishOptions {
                    fonts: FontExportPolicy::Embed,
                    size: level.clone(),
                    audio: AudioExportPolicy::Original,
                };
                let cfg = HtmlPublishConfig::clean_publish_default();
                export_project_with_config(scene, &out, &rt, &options, &cfg).unwrap_or_else(|e| {
                    panic!("failed exporting {label} at {}: {e}", level.name())
                });
                let bindings = fs::read_to_string(out.join(BINDINGS_FILE)).unwrap();
                if level.wasm_inline() {
                    assert!(
                        !bindings.contains("NeoFlashWasmBase64"),
                        "Full level inlines wasm in bundle.js, not neoflash_web.js ({label})"
                    );
                } else {
                    let expected_b64 =
                        STANDARD.encode(fs::read(out.join(WASM_FILE)).expect("wasm file"));
                    assert!(
                        bindings.contains(&format!(
                            "globalThis.NeoFlashWasmBase64 = \"{expected_b64}\";"
                        )),
                        "{} level must embed NeoFlashWasmBase64 fallback in neoflash_web.js for direct file:// opening ({label})",
                        level.name()
                    );
                }
                let est = estimate_publish_options_cancellable_with_config(
                    scene,
                    &rt,
                    &options,
                    &cfg,
                    || false,
                )
                .unwrap()
                .unwrap();
                assert_eq!(
                    totals(&out),
                    (est.files, est.bytes, est.gzip_bytes),
                    "size estimate matches published disk bytes for {label} ({})",
                    level.name()
                );
            }
        }

        let shared_dir = dir.join("shared-runtime");
        export_shared_runtime(&shared_dir, &rt).unwrap();
        let shared_bindings = fs::read_to_string(shared_dir.join(BINDINGS_FILE)).unwrap();
        assert!(
            shared_bindings.contains("globalThis.NeoFlashWasmBase64 = \"AGFzbQEAAAA=\";"),
            "shared runtime also embeds NeoFlashWasmBase64 for local relative-path file:// setups"
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn export_desktop_app_creates_macos_app_bundle_and_windows_launcher() {
        let dir = temp();
        let rt = dir.join("runtime");
        runtime(&rt);

        let project = Project::demo();
        let out_desktop = dir.join("desktop-out");
        let options = PublishOptions {
            fonts: FontExportPolicy::Embed,
            size: SizeLevel::Full,
            audio: AudioExportPolicy::Original,
        };
        let app_bundle = export_desktop_app(
            &project,
            &out_desktop,
            &rt,
            &options,
            &HtmlPublishConfig::clean_publish_default(),
        )
        .unwrap();
        assert!(app_bundle.ends_with("Ideas in motion.app"));
        assert!(app_bundle.join("Contents/Info.plist").is_file());
        assert!(app_bundle.join("Contents/MacOS/Ideas in motion").is_file());
        assert!(
            app_bundle
                .join("Contents/Resources/web/index.html")
                .is_file()
        );
        assert!(out_desktop.join("Launch Ideas in motion.bat").is_file());

        let html =
            fs::read_to_string(app_bundle.join("Contents/Resources/web/index.html")).unwrap();
        assert!(html.contains("<body data-clean=\"true\">"));

        fs::remove_dir_all(dir).unwrap();
    }
}
