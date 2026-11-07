//! Self-contained HTML5 publication. Authored evaluation remains in the Rust WASM runtime.

pub mod atlas;
pub mod audio;
pub mod fonts;
pub mod image_export;
pub mod lottie;
pub mod profiles;
pub mod publish;
pub mod svg;
pub mod video;
pub use atlas::*;
pub use audio::*;
pub use fonts::*;
pub use image_export::*;
pub use lottie::*;
pub use profiles::*;
pub use publish::{
    PublishOptions, SizeEstimate, SizeLevel, SizePart, gzip_len, validate_runtime_url,
};
pub use svg::*;
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

#[derive(Serialize)]
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
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    match &element.kind {
                        ElementKind::Text { font_family, .. } => {
                            fonts.insert(font_family.clone());
                        }
                        ElementKind::TextBox { format, .. } => {
                            fonts.insert(format.font.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let mut warnings: Vec<_> = fonts
        .into_iter()
        .filter(|font| !embedded.contains(&font.to_ascii_lowercase()))
        .map(|font| format!(
            "Font '{font}' is referenced by name, not embedded. Visitors need this font installed; otherwise the browser uses a fallback."
        ))
        .collect();
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
    let fonts = apply_font_policy(&mut published, font_policy);
    apply_audio_policy(&mut published, audio_policy)?;
    let runtime_id = shared_runtime_id(&bindings, &wasm);
    Ok(Prepared {
        program,
        scripts,
        published,
        scenes,
        fonts,
        warnings: export_warnings(project),
        bindings,
        wasm,
        runtime_id,
    })
}

pub(crate) fn assemble(
    project: &Project,
    prepared: &Prepared,
    level: &SizeLevel,
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
    let runtime_js_bytes = if shared_url.is_some() {
        0
    } else {
        prepared.bindings.len()
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
    let (size_report, load_summary) =
        size_report_from(project, &prepared.program, &prepared.scripts, Some(figures))?;
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
    let program = ships_program.then(|| ProgramBundle {
        code: &prepared.program.code,
        class_names: &prepared.program.class_names,
        source_url: "neoflash-project.js",
        source_map: prepared.program.bundle_source_map.as_ref(),
        source_locations: prepared
            .program
            .bundle_source_map
            .as_ref()
            .map(|map| map.runtime_locations())
            .unwrap_or_default(),
    });
    let bundle = Bundle {
        project: &prepared.published,
        scenes: prepared.scenes.clone(),
        scripts: prepared.scripts.iter().collect(),
        program,
        wasm: level.wasm_inline().then(|| STANDARD.encode(wasm)),
        wasm_url: match (level, &shared_url) {
            (SizeLevel::Full, _) => None,
            (_, Some(url)) => Some(format!("{}/{WASM_FILE}", url.trim_end_matches('/'))),
            _ => Some(WASM_FILE.to_owned()),
        },
        runtime_id: shared_url.as_ref().map(|_| prepared.runtime_id.clone()),
        publish: PublishInfo {
            level: level.name(),
            omitted_modules: omitted_titles.clone(),
        },
        warnings: warnings.clone(),
        load_summary,
        size_report,
        fonts: &prepared.fonts,
    };
    let bundle_js = format!(
        "globalThis.NeoFlashBundle = {};\n",
        serde_json::to_string(&bundle)?
    );
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
    let html = html_document(&title, &prepared.fonts.css_rules, &scripts);
    let readme = format!(
        "{} — NeoFlash HTML5 export\n\n{}\nThe player and embedded project data need no plugins, SWF or build tools.\nExternal HTTP(S) requests, if used by your scripts, need reachable servers and browser CORS permission. Embedded resources work offline; local filesystem URLs are not a network fallback.\n\nPublishing notes:\n{}\n\nUse scripts in Actions only from authors you trust: JavaScript executes in the browser.\n",
        project.name,
        match level {
            SizeLevel::Full => "Open index.html in a modern browser, or upload this entire folder to static hosting.\nThe shared Rust evaluator and project resources are embedded in bundle.js.".to_owned(),
            SizeLevel::Balanced | SizeLevel::Compact => format!("Upload this entire folder to static hosting and open index.html over HTTP(S).\nThe shared Rust evaluator is {WASM_FILE}; project resources are embedded in bundle.js."),
            SizeLevel::Shared(_) => format!("Upload this folder to static hosting. The runtime loads from {}.\nProject resources are embedded in bundle.js.", shared_url.as_deref().unwrap_or_default()),
        },
        warnings.join("\n")
    );

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if shared_url.is_none() {
        files.push((BINDINGS_FILE.into(), prepared.bindings.clone().into_bytes()));
        files.push(("runtime.js".into(), HOST.as_bytes().to_vec()));
        for (file, source) in &modules {
            files.push(((*file).into(), source.as_bytes().to_vec()));
        }
        files.push((NOTICES_FILE.into(), ALGORITHM_NOTICES.as_bytes().to_vec()));
        if !level.wasm_inline() {
            files.push((WASM_FILE.into(), wasm.to_vec()));
        }
    }
    files.push(("bundle.js".into(), bundle_js.into_bytes()));
    files.push(("README.txt".into(), readme.into_bytes()));
    files.push(("index.html".into(), html.into_bytes()));

    let resources: String = prepared
        .published
        .resources
        .iter()
        .map(|resource| resource.data_base64.as_ref())
        .collect();
    let project_json = serde_json::to_string(&prepared.published)?;
    let content_bytes = project_json.len().saturating_sub(resources.len());
    let script_text: String = ships_program
        .then_some(prepared.program.code.as_str())
        .into_iter()
        .chain(prepared.scripts.iter().map(|script| script.code.as_str()))
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
    let serve = "Serve the folder over HTTP(S): browsers do not load a separate WebAssembly file from a page opened from disk.";
    match level {
        SizeLevel::Full => Vec::new(),
        SizeLevel::Balanced => vec![serve.into()],
        SizeLevel::Compact if omitted.is_empty() => vec![serve.into()],
        SizeLevel::Compact => vec![
            serve.into(),
            format!(
                "Left out because the project's scripts never name them: {}. A script that builds such a class name at run time cannot reach it; publish at the Balanced level if one does.",
                omitted.join("; ")
            ),
        ],
        SizeLevel::Shared(url) => vec![
            serve.into(),
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
    let mut files = vec![
        (
            RUNTIME_ID_FILE.to_owned(),
            format!("globalThis.NeoFlashRuntimeId = \"{id}\";\n").into_bytes(),
        ),
        (BINDINGS_FILE.into(), bindings.as_bytes().to_vec()),
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
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
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
        if plain && !names.contains(&stale.as_str()) {
            let _ = fs::remove_file(output_dir.join(stale));
        }
    }
    Ok(output_dir.join(&last.0))
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static TEMP_ID: AtomicU64 = AtomicU64::new(1);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
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

fn html_document(title: &str, font_css: &str, scripts: &[String]) -> String {
    let script_tags: String = scripts
        .iter()
        .map(|src| format!("  <script defer src=\"{src}\"></script>\n"))
        .collect();
    let font_rules = if font_css.is_empty() {
        String::new()
    } else {
        format!("\n    {font_css}")
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
    body {{ margin: 0; background: #25272b; color: #d2d6dd; height: 100vh; height: 100dvh; display: grid; grid-template-rows: minmax(0,1fr) auto; overflow: hidden; }}
    #viewport {{ position: relative; overflow: auto; min-height: 0; min-width: 0; overscroll-behavior: contain; touch-action: pan-x pan-y; }}
    #stage-space {{ position: relative; }}
    #stage-surface {{ position: absolute; box-shadow: 0 12px 48px #0005; }}
    canvas {{ display: block; width: 100%; height: 100%; outline: none; }}
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
<body>
  <main id="viewport" tabindex="-1" aria-label="Project preview"><div id="stage-space"><div id="stage-surface" role="group" aria-label="{title}"><canvas id="stage" aria-label="{title}"></canvas><div id="semantic-layer"></div></div></div></main>
  <footer><div class="transport" aria-label="Preview controls"><button id="toggle" type="button">Pause</button><button id="restart" type="button">Restart</button><select id="zoom" aria-label="Preview zoom"><option value="fit">Fit</option><option value="0.25">25%</option><option value="0.5">50%</option><option value="0.75">75%</option><option value="1">100%</option><option value="1.5">150%</option><option value="2">200%</option><option value="4">400%</option></select><button id="pan" type="button" aria-pressed="false" title="Drag the stage to pan. Touch scrolling also works.">Pan</button><button id="audio-toggle" type="button" hidden>Enable audio</button><button id="report-toggle" type="button" aria-pressed="false" title="Show project weight and loaded assets breakdown" hidden>Size report</button></div><div class="preview-info"><span id="name">{title}</span><span id="status"></span><span id="load-info"></span><span id="audio-status" role="status" aria-live="polite" aria-atomic="true" hidden></span><span id="trace" role="status" aria-live="polite" aria-atomic="true"></span></div></footer>
  <div id="loading" role="status">Loading project…</div><pre id="size-report" aria-label="Project size and load report"></pre><pre id="error" role="alert"></pre>
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
        let codepoints: std::collections::BTreeSet<u32> =
            "NeoFlash HTML5!".chars().map(|c| c as u32).collect();
        let ttf_bytes = neoflash_core::TextFormat::subset_font_bytes(
            b"not-an-sfnt-use-portable-metrics",
            &codepoints,
        )
        .unwrap();
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes(
                "font-neo",
                "fonts/NeoFont.ttf",
                "font/ttf",
                &ttf_bytes,
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
}
