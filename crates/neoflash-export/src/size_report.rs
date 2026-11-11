use super::{
    CompiledScript, ExportError, compile_scripts, embedded_font_families, publishable,
    static_runtime_js_bytes,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use neoflash_core::{ElementKind, Project};
use serde::Serialize;
use std::collections::BTreeSet;

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

pub(crate) fn decoded_base64_len(encoded: &str) -> usize {
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
