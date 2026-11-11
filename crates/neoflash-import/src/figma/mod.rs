//! Layered Figma (`.fig` Kiwi archives & `.figma.json` REST/plugin exports) composition importer (`T-249`).
//!
//! Decomposes Figma pages, artboard frames, groups, reusable components/instances, clipping & sibling
//! masks, blend modes, vector networks/paths, solid/linear/radial/image paints, strokes, rounded
//! corners, drop-shadow/blur effects and editable text into multi-layer NeoFlash compositions and
//! Library `MovieClip` symbols with located approximation diagnostics.

use crate::{
    Diagnostic, ImportError, ImportFormat, ImportReport, Severity, Statistics,
    vector_fidelity::parse_svg_path_data,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::read::DeflateDecoder;
use neoflash_core::{
    BlendMode, Color, Element, ElementKind, FillRule, FilterSpec, GradientKind, GradientPaint,
    GradientStop, InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, MaskMode,
    OvalOptions, PathCommand, PrimitiveOptions, Project, RectangleOptions, ResourceUnit, Symbol,
    SymbolKind, TextAlign, TextFormat, Timeline, Transform, new_id, oval_path_commands,
    polystar_path_commands, rectangle_path_commands, validate_path_commands,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

const MAX_FIGMA_FILE_BYTES: usize = 256 * 1024 * 1024;
const MAX_FIGMA_DECODE_BYTES: usize = 128 * 1024 * 1024;
const MAX_FIGMA_IMAGE_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_FIGMA_NODES: usize = 50_000;
const MAX_KIWI_VALUES: usize = 2_000_000;

/// How top-level Figma artboard/frame children map into the NeoFlash timeline (`T-249`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FigmaLayerMapping {
    /// Each child in the composition artboard becomes its own NeoFlash `Layer` (with `Folder` and
    /// `Mask` hierarchy preserved), matching After Effects / Animate layered composition import.
    #[default]
    TimelineLayers,
    /// Wrap each top-level child in its own `SymbolKind::MovieClip` symbol and place on its layer.
    MovieClipSymbols,
    /// Place all converted elements onto a single timeline `Layer`.
    FlattenSingleLayer,
}

/// Options controlling Figma `.fig` and `.figma.json` import (`T-249`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FigmaImportOptions {
    pub layer_mapping: FigmaLayerMapping,
    pub import_hidden_layers: bool,
    pub preserve_masks: bool,
    pub preserve_blend_modes: bool,
    pub resize_stage_to_artboard: bool,
}

impl Default for FigmaImportOptions {
    fn default() -> Self {
        Self {
            layer_mapping: FigmaLayerMapping::TimelineLayers,
            import_hidden_layers: true,
            preserve_masks: true,
            preserve_blend_modes: true,
            resize_stage_to_artboard: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct FigmaDoc {
    pub(super) name: String,
    pub(super) pages: Vec<FigmaNode>,
    pub(super) images: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug)]
pub(super) struct FigmaNode {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) node_type: String,
    pub(super) visible: bool,
    pub(super) locked: bool,
    pub(super) opacity: f64,
    pub(super) blend_mode: String,
    pub(super) is_mask: bool,
    pub(super) mask_type: String,
    pub(super) clips_content: bool,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) rotation_deg: f64,
    pub(super) relative_transform: Option<[f64; 6]>,
    pub(super) corner_radii: [f64; 4],
    pub(super) arc_data: Option<(f64, f64, f64)>, // (startingAngle_rad, endingAngle_rad, innerRadius_ratio)
    pub(super) point_count: Option<u32>,
    pub(super) star_inner_radius: Option<f64>,
    pub(super) fills: Vec<FigmaPaint>,
    pub(super) strokes: Vec<FigmaPaint>,
    pub(super) stroke_weight: f64,
    pub(super) fill_paths: Vec<(Vec<PathCommand>, FillRule)>,
    pub(super) stroke_paths: Vec<Vec<PathCommand>>,
    pub(super) text: Option<FigmaTextData>,
    pub(super) effects: Vec<FigmaEffect>,
    pub(super) component_id: Option<String>,
    pub(super) background_color: Option<Color>,
    pub(super) children: Vec<FigmaNode>,
}

impl Default for FigmaNode {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::from("Layer"),
            node_type: String::from("GROUP"),
            visible: true,
            locked: false,
            opacity: 1.0,
            blend_mode: String::from("PASS_THROUGH"),
            is_mask: false,
            mask_type: String::from("ALPHA"),
            clips_content: false,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            rotation_deg: 0.0,
            relative_transform: None,
            corner_radii: [0.0; 4],
            arc_data: None,
            point_count: None,
            star_inner_radius: None,
            fills: Vec::new(),
            strokes: Vec::new(),
            stroke_weight: 0.0,
            fill_paths: Vec::new(),
            stroke_paths: Vec::new(),
            text: None,
            effects: Vec::new(),
            component_id: None,
            background_color: None,
            children: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum FigmaPaint {
    Solid {
        color: Color,
    },
    Gradient {
        kind: GradientKind,
        stops: Vec<GradientStop>,
        start: [f64; 2],
        end: [f64; 2],
    },
    Image {
        image_ref: String,
        alpha: f64,
    },
}

#[derive(Clone, Debug)]
pub(super) struct FigmaTextData {
    pub(super) characters: String,
    pub(super) font_family: String,
    pub(super) font_size: f64,
    pub(super) bold: bool,
    pub(super) italic: bool,
    pub(super) align: TextAlign,
    pub(super) letter_spacing: f64,
    pub(super) line_height: Option<f64>,
}

#[derive(Clone, Debug)]
pub(super) enum FigmaEffect {
    DropShadow {
        color: Color,
        offset_x: f64,
        offset_y: f64,
        radius: f64,
        inner: bool,
    },
    LayerBlur {
        radius: f64,
    },
    BackgroundBlur {
        radius: f64,
    },
}

/// Returns `true` if `bytes` looks like a `.fig` Kiwi archive (ZIP with `canvas.fig` or `fig-kiwi` magic)
/// or a `.figma.json` document.
pub fn is_figma_bytes(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"fig-kiwi") || bytes.starts_with(b"fig-jam.") {
        return true;
    }
    if bytes.starts_with(b"PK\x03\x04")
        && let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes))
    {
        for i in 0..zip.len().min(64) {
            if let Ok(entry) = zip.by_index(i)
                && (entry.name() == "canvas.fig" || entry.name().ends_with(".fig"))
            {
                return true;
            }
        }
    }
    let trimmed = std::str::from_utf8(&bytes[..bytes.len().min(4096)])
        .ok()
        .map(str::trim_start);
    if let Some(s) = trimmed
        && s.starts_with('{')
        && (s.contains("\"schemaVersion\"")
            || (s.contains("\"document\"") && s.contains("\"CANVAS\""))
            || (s.contains("\"nodes\"") && s.contains("\"DOCUMENT\"")))
    {
        return true;
    }
    false
}

mod convert;
mod json_doc;
mod kiwi;
#[cfg(test)]
mod tests;

use convert::build_project_from_figma_doc;
use json_doc::parse_figma_json_doc;
use kiwi::parse_fig_kiwi_canvas;

/// Import a `.figma.json` UTF-8 string into a multi-layer NeoFlash [`Project`].
pub fn import_figma_json_str(
    json_str: &str,
    source_name: &str,
    options: &FigmaImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    let doc = parse_figma_json_doc(json_str, source_name)?;
    build_project_from_figma_doc(doc, options, source_name)
}

/// Import a Figma `.fig` archive (ZIP with `canvas.fig` + `images/*` or standalone `fig-kiwi`) or `.figma.json` byte slice.
pub fn import_figma_bytes(
    bytes: &[u8],
    source_name: &str,
    options: &FigmaImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    if bytes.len() > MAX_FIGMA_FILE_BYTES {
        return Err(ImportError::Container(format!(
            "Figma file exceeds {} MiB input limit",
            MAX_FIGMA_FILE_BYTES / (1024 * 1024)
        )));
    }
    if bytes.starts_with(b"fig-kiwi") || bytes.starts_with(b"fig-jam.") {
        let doc = parse_fig_kiwi_canvas(bytes, source_name, BTreeMap::new())?;
        return build_project_from_figma_doc(doc, options, source_name);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ImportError::Container(format!("Invalid .fig ZIP archive: {e}")))?;
        if zip.len() > 4096 {
            return Err(ImportError::Container(
                ".fig ZIP archive exceeds 4096 entries".into(),
            ));
        }
        let mut canvas_bytes: Option<Vec<u8>> = None;
        let mut json_bytes: Option<Vec<u8>> = None;
        let mut images = BTreeMap::new();
        let mut total_image_bytes = 0usize;
        let mut meta_name: Option<String> = None;

        for i in 0..zip.len() {
            let entry = zip
                .by_index(i)
                .map_err(|e| ImportError::Container(e.to_string()))?;
            let ename = entry.name().to_owned();
            crate::source::safe_name(&ename)?;
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(ImportError::Container(format!(
                    "Symlink ZIP entry in .fig archive: {ename}"
                )));
            }
            if entry.is_dir() {
                continue;
            }
            if ename == "canvas.fig" || ename.ends_with(".fig") {
                let mut raw = Vec::new();
                entry
                    .take(MAX_FIGMA_DECODE_BYTES as u64 + 1)
                    .read_to_end(&mut raw)?;
                if raw.len() > MAX_FIGMA_DECODE_BYTES {
                    return Err(ImportError::Container(
                        "canvas.fig entry exceeds 128 MiB limit".into(),
                    ));
                }
                canvas_bytes = Some(raw);
            } else if ename == "meta.json" {
                let mut raw = String::new();
                let _ = entry.take(1024 * 1024).read_to_string(&mut raw);
                if let Ok(v) = serde_json::from_str::<Value>(&raw)
                    && let Some(fname) = v.get("file_name").and_then(Value::as_str)
                {
                    meta_name = Some(fname.to_owned());
                }
            } else if ename.starts_with("images/") {
                let key = ename
                    .trim_start_matches("images/")
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_owned();
                if !key.is_empty() {
                    let mut raw = Vec::new();
                    entry
                        .take(neoflash_core::MAX_RESOURCE_BYTES as u64 + 1)
                        .read_to_end(&mut raw)?;
                    if raw.len() > neoflash_core::MAX_RESOURCE_BYTES {
                        return Err(ImportError::Container(format!(
                            ".fig image '{ename}' exceeds resource size budget"
                        )));
                    }
                    total_image_bytes = total_image_bytes.saturating_add(raw.len());
                    if total_image_bytes > MAX_FIGMA_IMAGE_TOTAL_BYTES {
                        return Err(ImportError::Container(
                            ".fig archive images exceed 64 MiB aggregate budget".into(),
                        ));
                    }
                    images.insert(key, raw);
                }
            } else if ename.ends_with(".json") && json_bytes.is_none() {
                let mut raw = Vec::new();
                entry
                    .take(MAX_FIGMA_DECODE_BYTES as u64 + 1)
                    .read_to_end(&mut raw)?;
                if raw.len() > MAX_FIGMA_DECODE_BYTES {
                    return Err(ImportError::Container(
                        ".fig JSON entry exceeds 128 MiB limit".into(),
                    ));
                }
                json_bytes = Some(raw);
            }
        }

        let effective_name = meta_name.as_deref().unwrap_or(source_name);
        if let Some(cb) = canvas_bytes {
            let doc = parse_fig_kiwi_canvas(&cb, effective_name, images)?;
            return build_project_from_figma_doc(doc, options, source_name);
        }
        if let Some(jb) = json_bytes {
            let s = std::str::from_utf8(&jb)
                .map_err(|_| ImportError::Container("Figma JSON entry must be UTF-8".into()))?;
            let mut doc = parse_figma_json_doc(s, effective_name)?;
            doc.images.extend(images);
            return build_project_from_figma_doc(doc, options, source_name);
        }
        return Err(ImportError::Container(
            ".fig ZIP archive contains neither canvas.fig nor a Figma JSON document".into(),
        ));
    }

    let json_str = std::str::from_utf8(bytes).map_err(|_| {
        ImportError::Container("Figma file is neither a binary .fig archive nor UTF-8 JSON".into())
    })?;
    import_figma_json_str(json_str, source_name, options)
}

/// Import a `.fig` or `.figma.json` file from disk into a fresh [`Project`].
pub fn import_figma_path(
    path: &Path,
    options: &FigmaImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    if std::fs::metadata(path)?.len() > MAX_FIGMA_FILE_BYTES as u64 {
        return Err(ImportError::Container(format!(
            "Figma file exceeds {} MiB input limit",
            MAX_FIGMA_FILE_BYTES / (1024 * 1024)
        )));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_FIGMA_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.trim_end_matches(".figma"))
        .filter(|s| !s.is_empty())
        .unwrap_or("Figma Import");
    let (project, mut report) = import_figma_bytes(&bytes, stem, options)?;
    report.source = path.display().to_string();
    Ok((project, report))
}
