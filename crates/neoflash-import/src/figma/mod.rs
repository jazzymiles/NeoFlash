#![forbid(unsafe_code)]
//! Layered Figma (`.fig` Kiwi archives with Deflate or RFC 8878 Zstandard compression,
//! `fig-jam.` whiteboards, and `.figma.json` REST/plugin exports) composition importer
//! (`T-249`, `T-312`).
//!
//! Decomposes Figma pages, artboard frames, groups, reusable components/symbols & overridden
//! instances (including `Internal Only Canvas` master symbols), clipping & sibling masks, blend
//! modes, `commandsBlob` & `vectorNetworkBlob` vector networks/paths, solid/linear/radial/image
//! paints, independent strokes, asymmetric rounded corners, drop-shadow/blur effects, and
//! multi-style editable text into NeoFlash compositions, scenes, and Library `MovieClip` symbols
//! with located approximation diagnostics.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use crate::{Diagnostic, ImportError, ImportFormat, ImportReport, Severity, Statistics};
#[cfg(test)]
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use neoflash_core::{
    AuthoredBitmapFill, AuthoredTextFieldOptions, BlendMode, Color, Element, ElementKind, FillRule,
    FilterSpec, GradientKind, GradientPaint, GradientStop, InstancePlayback, Keyframe, Layer,
    LayerProperties, LayerRole, MaskMode, OvalOptions, PathCommand, PrimitiveOptions, Project,
    RectangleOptions, ResourceUnit, Symbol, SymbolKind, TextAlign, TextFormat, TextFormatRun,
    Timeline, Transform, new_id, oval_path_commands, polystar_path_commands,
    rectangle_path_commands, validate_path_commands,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::ZipArchive;

/// Maximum `.fig` / `.figma.json` container size accepted by the importer (512 MiB, matching FLA).
pub const MAX_FIGMA_FILE_BYTES: usize = 512 * 1024 * 1024;
/// Maximum decompressed Kiwi stream or JSON payload size (256 MiB).
pub(crate) const MAX_FIGMA_DECODE_BYTES: usize = 256 * 1024 * 1024;
/// Maximum raw bytes read for a single embedded ZIP image entry before downscaling (64 MiB).
pub(crate) const MAX_ZIP_IMAGE_ENTRY_BYTES: usize = 64 * 1024 * 1024;
/// Maximum number of Figma nodes materialized per document.
pub(crate) const MAX_FIGMA_NODES: usize = 300_000;
/// Maximum Kiwi field/scalar decode steps per stream.
pub(crate) const MAX_KIWI_VALUES: usize = 100_000_000;

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

/// Lazy image provider backed by an opened `.fig` ZIP archive and/or inline base64 images,
/// avoiding eager extraction of hundreds of megabytes of unreferenced raster entries.
pub(crate) struct FigmaImageArchive<'a> {
    zip: Option<ZipArchive<Cursor<&'a [u8]>>>,
    /// Lowercase normalized image hash -> ZIP entry index.
    zip_index: HashMap<String, usize>,
    inline: BTreeMap<String, Vec<u8>>,
}

impl<'a> FigmaImageArchive<'a> {
    pub(crate) fn from_inline(inline: BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            zip: None,
            zip_index: HashMap::new(),
            inline,
        }
    }

    pub(crate) fn from_zip(
        zip: ZipArchive<Cursor<&'a [u8]>>,
        zip_index: HashMap<String, usize>,
        inline: BTreeMap<String, Vec<u8>>,
    ) -> Self {
        Self {
            zip: Some(zip),
            zip_index,
            inline,
        }
    }

    pub(crate) fn has_any(&self) -> bool {
        !self.zip_index.is_empty() || !self.inline.is_empty()
    }

    pub(crate) fn has_image(&self, image_ref: &str) -> bool {
        let low = image_ref.to_ascii_lowercase();
        self.inline.contains_key(image_ref)
            || self.inline.contains_key(&low)
            || self.zip_index.contains_key(&low)
    }

    /// Read a single referenced image entry on demand, bounded by `MAX_ZIP_IMAGE_ENTRY_BYTES`.
    pub(crate) fn load_image(&mut self, image_ref: &str) -> Result<Option<Vec<u8>>, String> {
        if let Some(bytes) = self.inline.get(image_ref) {
            return Ok(Some(bytes.clone()));
        }
        let low = image_ref.to_ascii_lowercase();
        if let Some(bytes) = self.inline.get(&low) {
            return Ok(Some(bytes.clone()));
        }
        let Some(&entry_idx) = self.zip_index.get(&low) else {
            return Ok(None);
        };
        let Some(zip) = self.zip.as_mut() else {
            return Ok(None);
        };
        let entry = zip
            .by_index(entry_idx)
            .map_err(|e| format!("Could not open embedded Figma image '{image_ref}': {e}"))?;
        if entry.size() > MAX_ZIP_IMAGE_ENTRY_BYTES as u64 {
            return Err(format!(
                "Embedded Figma image '{image_ref}' exceeds 64 MiB single-entry safety limit"
            ));
        }
        let cap = (entry.size() as usize).min(4 * 1024 * 1024);
        let mut buf = Vec::with_capacity(cap);
        entry
            .take(MAX_ZIP_IMAGE_ENTRY_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("Failed reading embedded Figma image '{image_ref}': {e}"))?;
        if buf.len() > MAX_ZIP_IMAGE_ENTRY_BYTES {
            return Err(format!(
                "Embedded Figma image '{image_ref}' exceeds 64 MiB single-entry safety limit"
            ));
        }
        if buf.is_empty() {
            return Ok(None);
        }
        Ok(Some(buf))
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct FigmaDoc {
    pub(super) name: String,
    pub(super) version_label: String,
    pub(super) pages: Vec<FigmaNode>,
    pub(super) inline_images: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug)]
pub(super) struct FigmaNode {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) node_type: String,
    pub(super) visible: bool,
    pub(super) locked: bool,
    pub(super) internal_only: bool,
    pub(super) is_state_group: bool,
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
    /// Optional 2x3 local-to-parent affine matrix `[a, b, c, d, tx, ty]`
    /// (`x' = a*x + c*y + tx`, `y' = b*x + d*y + ty`).
    pub(super) relative_transform: Option<[f64; 6]>,
    /// Corner radii in Figma `[top_left, top_right, bottom_right, bottom_left]` order.
    pub(super) corner_radii: [f64; 4],
    pub(super) arc_data: Option<FigmaArcData>,
    pub(super) point_count: Option<u32>,
    pub(super) star_inner_radius: Option<f64>,
    pub(super) fills: Vec<FigmaPaint>,
    pub(super) strokes: Vec<FigmaPaint>,
    pub(super) stroke_weight: f64,
    /// Optional independent rectangular border weights `[top, right, bottom, left]`.
    pub(super) border_weights: Option<[f64; 4]>,
    pub(super) fill_paths: Vec<(Vec<PathCommand>, FillRule)>,
    pub(super) stroke_paths: Vec<Vec<PathCommand>>,
    pub(super) text: Option<FigmaTextData>,
    pub(super) effects: Vec<FigmaEffect>,
    pub(super) component_id: Option<String>,
    pub(super) symbol_overrides: Vec<FigmaSymbolOverride>,
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
            internal_only: false,
            is_state_group: false,
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
            border_weights: None,
            fill_paths: Vec::new(),
            stroke_paths: Vec::new(),
            text: None,
            effects: Vec::new(),
            component_id: None,
            symbol_overrides: Vec::new(),
            background_color: None,
            children: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FigmaArcData {
    pub(super) start_angle_rad: f64,
    pub(super) end_angle_rad: f64,
    pub(super) inner_radius_ratio: f64,
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
        approximated_from: Option<&'static str>,
    },
    Image {
        image_ref: String,
        scale_mode: String,
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
    pub(super) underline: bool,
    pub(super) strikethrough: bool,
    pub(super) align: TextAlign,
    pub(super) justify: bool,
    pub(super) align_v: String,
    pub(super) letter_spacing: f64,
    pub(super) line_height: Option<f64>,
    pub(super) auto_resize: String,
    pub(super) style_runs: Vec<FigmaTextStyleRun>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct FigmaTextStyleOverride {
    pub(super) font_family: Option<String>,
    pub(super) font_size: Option<f64>,
    pub(super) bold: Option<bool>,
    pub(super) italic: Option<bool>,
    pub(super) underline: Option<bool>,
    pub(super) strikethrough: Option<bool>,
    pub(super) fill_color: Option<Color>,
    pub(super) letter_spacing: Option<f64>,
    pub(super) line_height: Option<f64>,
}

#[derive(Clone, Debug)]
pub(super) struct FigmaTextStyleRun {
    pub(super) start_utf16: u32,
    pub(super) end_utf16: u32,
    pub(super) style: FigmaTextStyleOverride,
}

#[derive(Clone, Debug)]
pub(super) struct FigmaSymbolOverride {
    pub(super) guid_path: Vec<String>,
    pub(super) visible: Option<bool>,
    pub(super) opacity: Option<f64>,
    pub(super) characters: Option<String>,
    pub(super) fills: Option<Vec<FigmaPaint>>,
    pub(super) strokes: Option<Vec<FigmaPaint>>,
    pub(super) overridden_symbol_id: Option<String>,
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

/// Invert Figma's 2x3 `Paint.transform` (`[m00, m01, m02, m10, m11, m12]`, mapping shape UV
/// `[0,1]^2` into gradient space) to recover shape-local `(start, end)` handles.
pub(super) fn gradient_handles_from_paint_matrix(
    m: [f64; 6],
    w: f64,
    h: f64,
    radial: bool,
) -> ([f64; 2], [f64; 2]) {
    let [m00, m01, m02, m10, m11, m12] = m;
    let det = m00 * m11 - m01 * m10;
    let sw = w.max(1.0);
    let sh = h.max(1.0);
    if !det.is_finite() || det.abs() <= 1e-9 {
        return if radial {
            ([sw * 0.5, sh * 0.5], [sw, sh * 0.5])
        } else {
            ([0.0, sh * 0.5], [sw, sh * 0.5])
        };
    }
    let inv_point = |gx: f64, gy: f64| -> [f64; 2] {
        let dx = gx - m02;
        let dy = gy - m12;
        let u = (m11 * dx - m01 * dy) / det;
        let v = (-m10 * dx + m00 * dy) / det;
        [u * sw, v * sh]
    };
    if radial {
        (inv_point(0.5, 0.5), inv_point(1.0, 0.5))
    } else {
        (inv_point(0.0, 0.5), inv_point(1.0, 0.5))
    }
}

/// Returns `true` if `bytes` looks like a `.fig` Kiwi archive (ZIP with `canvas.fig` or `fig-kiwi`
/// / `fig-jam.` magic) or a `.figma.json` document.
pub fn is_figma_bytes(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"fig-kiwi") || bytes.starts_with(b"fig-jam.") {
        return true;
    }
    if bytes.starts_with(b"PK\x03\x04")
        && let Ok(mut zip) = ZipArchive::new(Cursor::new(bytes))
    {
        for i in 0..zip.len().min(128) {
            if let Ok(entry) = zip.by_index(i) {
                let name = entry.name();
                if name == "canvas.fig"
                    || name.ends_with(".fig")
                    || name == "document.json"
                    || name.ends_with("/document.json")
                {
                    return true;
                }
            }
        }
    }
    json_doc::looks_like_figma_json(bytes)
}

mod convert;
mod json_doc;
mod kiwi;
#[cfg(test)]
mod tests;
pub(crate) mod zstd;

use convert::build_project_from_figma_doc;
use json_doc::parse_figma_json_doc;
use kiwi::parse_fig_kiwi_canvas;

/// Import a `.figma.json` UTF-8 string into a multi-layer NeoFlash [`Project`].
pub fn import_figma_json_str(
    json_str: &str,
    source_name: &str,
    options: &FigmaImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    let mut doc = parse_figma_json_doc(json_str, source_name)?;
    let inline = std::mem::take(&mut doc.inline_images);
    let mut images = FigmaImageArchive::from_inline(inline);
    build_project_from_figma_doc(doc, &mut images, options, source_name)
}

/// Import a Figma `.fig` archive (ZIP with `canvas.fig` + `images/*` or standalone `fig-kiwi` /
/// `fig-jam.`) or `.figma.json` byte slice.
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
        let doc = parse_fig_kiwi_canvas(bytes, source_name)?;
        let mut images = FigmaImageArchive::from_inline(BTreeMap::new());
        return build_project_from_figma_doc(doc, &mut images, options, source_name);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        let mut zip = ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ImportError::Container(format!("Invalid .fig ZIP archive: {e}")))?;
        if zip.len() > 16_384 {
            return Err(ImportError::Container(
                ".fig ZIP archive exceeds 16,384 entries".into(),
            ));
        }
        let mut canvas_bytes: Option<Vec<u8>> = None;
        let mut json_bytes: Option<Vec<u8>> = None;
        let mut zip_image_index: HashMap<String, usize> = HashMap::new();
        let mut meta_name: Option<String> = None;

        for i in 0..zip.len() {
            let entry = zip
                .by_index(i)
                .map_err(|e| ImportError::Container(e.to_string()))?;
            let ename = entry.name().replace('\\', "/");
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
                if entry.size() > MAX_FIGMA_DECODE_BYTES as u64 {
                    return Err(ImportError::Container(
                        "canvas.fig entry exceeds 256 MiB limit".into(),
                    ));
                }
                let cap = (entry.size() as usize).min(16 * 1024 * 1024);
                let mut raw = Vec::with_capacity(cap);
                entry
                    .take(MAX_FIGMA_DECODE_BYTES as u64 + 1)
                    .read_to_end(&mut raw)?;
                if raw.len() > MAX_FIGMA_DECODE_BYTES {
                    return Err(ImportError::Container(
                        "canvas.fig entry exceeds 256 MiB limit".into(),
                    ));
                }
                canvas_bytes = Some(raw);
            } else if ename == "meta.json" || ename.ends_with("/meta.json") {
                let mut raw = String::new();
                let _ = entry.take(1024 * 1024).read_to_string(&mut raw);
                if let Ok(v) = serde_json::from_str::<Value>(&raw)
                    && let Some(fname) = v
                        .get("file_name")
                        .or_else(|| v.get("name"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                {
                    meta_name = Some(fname.to_owned());
                }
            } else if let Some(rest) = ename
                .strip_prefix("images/")
                .or_else(|| ename.rsplit_once("/images/").map(|(_, r)| r))
            {
                if entry.size() > 0 {
                    let key = rest
                        .split('.')
                        .next()
                        .unwrap_or(rest)
                        .trim()
                        .to_ascii_lowercase();
                    if !key.is_empty() {
                        zip_image_index.entry(key).or_insert(i);
                    }
                }
            } else if ename.ends_with(".json") && json_bytes.is_none() {
                if entry.size() > MAX_FIGMA_DECODE_BYTES as u64 {
                    return Err(ImportError::Container(
                        ".fig JSON entry exceeds 256 MiB limit".into(),
                    ));
                }
                let cap = (entry.size() as usize).min(16 * 1024 * 1024);
                let mut raw = Vec::with_capacity(cap);
                entry
                    .take(MAX_FIGMA_DECODE_BYTES as u64 + 1)
                    .read_to_end(&mut raw)?;
                if raw.len() > MAX_FIGMA_DECODE_BYTES {
                    return Err(ImportError::Container(
                        ".fig JSON entry exceeds 256 MiB limit".into(),
                    ));
                }
                json_bytes = Some(raw);
            }
        }

        let effective_name = meta_name.as_deref().unwrap_or(source_name);
        if let Some(cb) = canvas_bytes {
            let mut doc = parse_fig_kiwi_canvas(&cb, effective_name)?;
            if let Some(mtitle) = meta_name {
                doc.name = mtitle;
            }
            let mut images = FigmaImageArchive::from_zip(zip, zip_image_index, BTreeMap::new());
            return build_project_from_figma_doc(doc, &mut images, options, source_name);
        }
        if let Some(jb) = json_bytes {
            let s = std::str::from_utf8(&jb)
                .map_err(|_| ImportError::Container("Figma JSON entry must be UTF-8".into()))?;
            let mut doc = parse_figma_json_doc(s, effective_name)?;
            if let Some(mtitle) = meta_name
                && (doc.name.is_empty() || doc.name == "Document")
            {
                doc.name = mtitle;
            }
            let inline = std::mem::take(&mut doc.inline_images);
            let mut images = FigmaImageArchive::from_zip(zip, zip_image_index, inline);
            return build_project_from_figma_doc(doc, &mut images, options, source_name);
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
    let file_len = std::fs::metadata(path)?.len();
    if file_len > MAX_FIGMA_FILE_BYTES as u64 {
        return Err(ImportError::Container(format!(
            "Figma file exceeds {} MiB input limit",
            MAX_FIGMA_FILE_BYTES / (1024 * 1024)
        )));
    }
    let mut bytes = Vec::with_capacity((file_len as usize).min(64 * 1024 * 1024));
    File::open(path)?
        .take(MAX_FIGMA_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FIGMA_FILE_BYTES {
        return Err(ImportError::Container(format!(
            "Figma file exceeds {} MiB input limit",
            MAX_FIGMA_FILE_BYTES / (1024 * 1024)
        )));
    }
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
