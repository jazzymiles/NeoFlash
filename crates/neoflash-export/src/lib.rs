//! Self-contained HTML5 publication. Authored evaluation remains in the Rust WASM runtime.

use base64::{Engine, engine::general_purpose::STANDARD};
use neoflash_core::{ElementKind, Project, SceneRange, Script, ScriptLanguage};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

const HOST: &str = include_str!("../../../web/runtime.js");
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
}

#[derive(Serialize)]
struct CompiledScript {
    language: ScriptLanguage,
    source: String,
    code: String,
    location: String,
}

#[derive(Serialize)]
struct Bundle<'a> {
    /// Single main timeline: authored scenes are concatenated before publishing.
    project: &'a Project,
    /// Scene names and frame ranges inside the flattened main timeline.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scenes: Vec<SceneRange>,
    scripts: Vec<CompiledScript>,
    program: ProgramBundle<'a>,
    wasm: String,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct ProgramBundle<'a> {
    code: &'a str,
    class_names: &'a [String],
}

/// Reports current publish limitations before the user exports their work.
pub fn export_warnings(project: &Project) -> Vec<String> {
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
    let mut warnings: Vec<_> = fonts.into_iter().map(|font| format!(
        "Font '{font}' is referenced by name, not embedded. Visitors need this font installed; otherwise the browser uses a fallback."
    )).collect();
    if !project.resources.is_empty() {
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

/// Generate a human-readable classic Flash `Size Report` text breakdown by scenes/frames,
/// library symbols, referenced fonts, embedded audio/bitmap/data resources, and scripts.
pub fn format_size_report(project: &Project) -> Result<String, ExportError> {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

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
    let (published, flattened_ranges) = neoflash_core::flatten_scenes_with_ranges(project);
    let published = publishable(&published).unwrap_or(published);
    let published_project_bytes = serde_json::to_string(&published)?.len();

    let scene_ids: Vec<&str> = if project.scenes.is_empty() {
        vec![project.root.as_str()]
    } else {
        project.scenes.iter().map(String::as_str).collect()
    };
    let scene_id_set: BTreeSet<&str> = scene_ids.iter().copied().collect();

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
    if library_symbols.is_empty() {
        let _ = writeln!(out, "(no library symbols)");
    } else {
        let _ = writeln!(
            out,
            "{:<24} {:<12} {:>7} {:>7} {:>9} {:>12}",
            "Symbol", "Kind", "Frames", "Layers", "Elements", "Bytes"
        );
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
                neoflash_core::SymbolKind::MovieClip => "MovieClip",
                neoflash_core::SymbolKind::Graphic => "Graphic",
                neoflash_core::SymbolKind::Button => "Button",
            };
            let _ = writeln!(
                out,
                "{:<24} {:<12} {:>7} {:>7} {:>9} {:>12}",
                sym.name,
                kind,
                sym.timeline.frame_count,
                sym.timeline.layers.len(),
                elems,
                bytes
            );
        }
        let _ = writeln!(out, "Library Symbols Subtotal: {} B", symbol_total);
    }
    let _ = writeln!(out);

    // 3. Fonts breakdown.
    let _ = writeln!(out, "Fonts");
    let _ = writeln!(
        out,
        "----------------------------------------------------------------"
    );
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
    if font_stats.is_empty() {
        let _ = writeln!(out, "(no text fonts referenced)");
    } else {
        let _ = writeln!(
            out,
            "{:<28} {:>12} {:>12} {:<16}",
            "Font Family", "Placements", "Characters", "Delivery"
        );
        for (font, (placements, chars)) in &font_stats {
            let _ = writeln!(
                out,
                "{:<28} {:>12} {:>12} {:<16}",
                font, placements, chars, "system reference"
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
    if project.resources.is_empty() {
        let _ = writeln!(out, "(no embedded resources)");
    } else {
        let _ = writeln!(
            out,
            "{:<24} {:<10} {:<20} {:>10} {:>12}",
            "Path", "Category", "Media Type", "Raw Bytes", "Bundle B64"
        );
        let (mut audio_raw, mut bitmap_raw, mut other_raw, mut total_b64) =
            (0usize, 0usize, 0usize, 0usize);
        for res in &project.resources {
            let raw = decoded_base64_len(&res.data_base64);
            let b64 = res.data_base64.len();
            total_b64 += b64;
            let category = if res.is_audio() {
                audio_raw += raw;
                "Audio"
            } else if res.media_type.starts_with("image/") {
                bitmap_raw += raw;
                "Bitmap"
            } else {
                other_raw += raw;
                "Data"
            };
            let _ = writeln!(
                out,
                "{:<24} {:<10} {:<20} {:>10} {:>12}",
                res.path, category, res.media_type, raw, b64
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
    let source_unit_bytes: usize = project.sources.iter().map(|u| u.source.len()).sum();
    let handler_source_bytes: usize = compiled_scripts.iter().map(|s| s.source.len()).sum();
    let handler_compiled_bytes: usize = compiled_scripts.iter().map(|s| s.code.len()).sum();
    let program_compiled_bytes = program.code.len();
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
        for script in &compiled_scripts {
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
        "Published Bundle Payload (excluding WASM engine): {} B",
        published_project_bytes + program_compiled_bytes + handler_compiled_bytes
    );
    Ok(out)
}

fn compile_scripts(
    project: &Project,
    program: &neoflash_script::ProjectProgram,
) -> Result<Vec<CompiledScript>, ExportError> {
    let mut scripts = Vec::new();
    let mut failures = Vec::new();
    let mut seen = BTreeSet::new();
    let mut add = |script: &Script, location: String| {
        if script.source.trim().is_empty() {
            return;
        }
        let key = serde_json::to_string(script).expect("Script contains only serializable strings");
        if !seen.insert(key) {
            return;
        }
        match neoflash_script::compile_with_program(script, program) {
            Ok(code) => scripts.push(CompiledScript {
                language: script.language,
                source: script.source.clone(),
                code,
                location,
            }),
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
    let bindings_path = runtime_dir.join("neoflash_web.js");
    let wasm_path = runtime_dir.join("neoflash_web_bg.wasm");
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
    let warnings = export_warnings(project);
    let published = publishable(&published).unwrap_or(published);
    let bundle = Bundle {
        project: &published,
        scenes,
        scripts,
        program: ProgramBundle {
            code: &program.code,
            class_names: &program.class_names,
        },
        wasm: STANDARD.encode(wasm),
        warnings: warnings.clone(),
    };
    let bundle_js = format!(
        "globalThis.NeoFlashBundle = {};\n",
        serde_json::to_string(&bundle)?
    );
    let title = html_escape(&project.name);
    let html = html_document(&title);
    let marker =
        serde_json::json!({"generator":"NeoFlash", "format_version":1, "project":project.name});
    let readme = format!(
        "{} — NeoFlash HTML5 export\n\nOpen index.html in a modern browser, or upload this entire folder to static hosting.\nThe player and embedded project data need no plugins, CDN, SWF or build tools.\nExternal HTTP(S) requests, if used by your scripts, need reachable servers and browser CORS permission. Embedded resources work offline; local filesystem URLs are not a network fallback.\nThe shared Rust evaluator and project resources are embedded in bundle.js.\n\nPublishing notes:\n{}\n\nUse scripts in Actions only from authors you trust: JavaScript executes in the browser.\n",
        project.name,
        if warnings.is_empty() {
            "No publishing warnings.".into()
        } else {
            warnings.join("\n")
        }
    );

    if output_dir.exists() && fs::read_dir(output_dir)?.next().is_some() {
        let owned = fs::read_to_string(output_dir.join(MARKER))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|marker| {
                marker["generator"] == "NeoFlash" && marker["format_version"] == 1
            });
        if !owned {
            return Err(ExportError::Destination(output_dir.display().to_string()));
        }
    }
    fs::create_dir_all(output_dir)?;
    // Prepare all data before writing. Replace only generated files, with index written last.
    for (name, contents) in [
        ("neoflash_web.js", bindings.as_str()),
        ("runtime.js", HOST),
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
        ("THIRD_PARTY_NOTICES.txt", ALGORITHM_NOTICES),
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
        ("bundle.js", bundle_js.as_str()),
        ("README.txt", readme.as_str()),
    ] {
        atomic_write(&output_dir.join(name), contents.as_bytes())?;
    }
    atomic_write(
        &output_dir.join(MARKER),
        serde_json::to_string_pretty(&marker)?.as_bytes(),
    )?;
    atomic_write(&output_dir.join("index.html"), html.as_bytes())?;
    Ok(output_dir.join("index.html"))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension(format!("neoflash-{}.tmp", std::process::id()));
    fs::write(&temp, bytes)?;
    fs::rename(temp, path)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn html_document(title: &str) -> String {
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
    #trace {{ flex-basis: 100%; overflow-wrap: anywhere; min-height: 1em; color: #c6d8ef; }}
    #error {{ position: fixed; inset: 24px; max-height: 60vh; overflow: auto; margin: auto; z-index: 2; padding: 24px; border: 1px solid #f19393; background: #351d22; color: #ffe0e0; white-space: pre-wrap; display: none; }}
    #loading {{ position: fixed; inset: 0; display: grid; place-items: center; pointer-events: none; }}
    @media (max-width: 600px) {{ footer {{ padding: 8px; }} .transport {{ gap: 6px; }} .preview-info {{ flex-basis: 100%; }} #name {{ display: none; }} }}
  </style>
  <script defer src="neoflash_web.js"></script>
  <script defer src="bundle.js"></script>
  <script defer src="path.js"></script>
  <script defer src="accessibility.js"></script>
  <script defer src="preview.js"></script>
  <script defer src="events.js"></script>
  <script defer src="stage.js"></script>
  <script defer src="legacy-input.js"></script>
  <script defer src="geom.js"></script>
  <script defer src="geom3d.js"></script>
  <script defer src="as2-strings.js"></script>
  <script defer src="as2-platform.js"></script>
  <script defer src="as2-object.js"></script>
  <script defer src="as2-transitions.js"></script>
  <script defer src="as2-depth-manager.js"></script>
  <script defer src="as2-print.js"></script>
  <script defer src="as2-boxing.js"></script>
  <script defer src="as2-capture.js"></script>
  <script defer src="as2-connections.js"></script>
  <script defer src="as2-stage.js"></script>
  <script defer src="as2-media.js"></script>
  <script defer src="as2-services.js"></script>
  <script defer src="as2-locale.js"></script>
  <script defer src="as2-files.js"></script>
  <script defer src="as2-network.js"></script>
  <script defer src="as2-text.js"></script>
  <script defer src="as2-data.js"></script>
  <script defer src="as2-binding.js"></script>
  <script defer src="as3-strings.js"></script>
  <script defer src="as3-fte-3d.js"></script>
  <script defer src="easing.js"></script>
  <script defer src="math.js"></script>
  <script defer src="date.js"></script>
  <script defer src="number-format.js"></script>
  <script defer src="broadcaster.js"></script>
  <script defer src="timers.js"></script>
  <script defer src="byte-array.js"></script>
  <script defer src="net.js"></script>
  <script defer src="external.js"></script>
  <script defer src="bitmap.js"></script>
  <script defer src="text.js"></script>
  <script defer src="text-input.js"></script>
  <script defer src="filters.js"></script>
  <script defer src="audio-formats.js"></script>
  <script defer src="audio.js"></script>
  <script defer src="video.js"></script>
  <script defer src="xml.js"></script>
  <script defer src="loader.js"></script>
  <script defer src="amf.js"></script>
  <script defer src="storage.js"></script>
  <script defer src="air.js"></script>
  <script defer src="as3-net-sys.js"></script>
  <script defer src="as3-media-events.js"></script>
  <script defer src="display.js"></script>
  <script defer src="runtime.js"></script>
</head>
<body>
  <main id="viewport" tabindex="-1" aria-label="Project preview"><div id="stage-space"><div id="stage-surface" role="group" aria-label="{title}"><canvas id="stage" aria-label="{title}"></canvas><div id="semantic-layer"></div></div></div></main>
  <footer><div class="transport" aria-label="Preview controls"><button id="toggle" type="button">Pause</button><button id="restart" type="button">Restart</button><select id="zoom" aria-label="Preview zoom"><option value="fit">Fit</option><option value="0.25">25%</option><option value="0.5">50%</option><option value="0.75">75%</option><option value="1">100%</option><option value="1.5">150%</option><option value="2">200%</option><option value="4">400%</option></select><button id="pan" type="button" aria-pressed="false" title="Drag the stage to pan. Touch scrolling also works.">Pan</button><button id="audio-toggle" type="button" hidden>Enable audio</button></div><div class="preview-info"><span id="name">{title}</span><span id="status"></span><span id="audio-status" role="status" aria-live="polite" aria-atomic="true" hidden></span><span id="trace" role="status" aria-live="polite" aria-atomic="true"></span></div></footer>
  <div id="loading" role="status">Loading project…</div><pre id="error" role="alert"></pre>
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
    }
}
