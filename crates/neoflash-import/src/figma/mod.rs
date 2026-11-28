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
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use crate::{Diagnostic, ImportError, ImportFormat, ImportReport, Severity, Statistics};
#[cfg(test)]
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use neoflash_core::{
    AuthoredBitmapFill, AuthoredTextFieldOptions, BlendMode, Color, Element, ElementKind,
    ElementVectorEffects, FillRule, FilterSpec, GradientKind, GradientPaint, GradientStop,
    InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, MaskMode, OvalOptions,
    PathCommand, PrimitiveOptions, Project, RectangleOptions, ResourceUnit, StrokeWidthProfile,
    Symbol, SymbolKind, TextAlign, TextFormat, TextFormatRun, Timeline, Transform, VectorMeshPaint,
    VectorMeshTriangle, expand_variable_width_stroke, new_id, oval_path_commands, path_bounds,
    polystar_path_commands, rectangle_path_commands, validate_path_commands,
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

/// How selected Figma pages map into the imported NeoFlash [`Project`] (`T-391`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FigmaPageMapping {
    /// Primary page becomes the root timeline and additional pages become `Project.scenes`.
    #[default]
    Scenes,
    /// Every selected Figma page becomes its own separate top-level NeoFlash `Layer` on the root
    /// symbol's timeline (`1 Figma page = 1 Layer`).
    Layers,
}

/// Fast structural summary of a Figma `.fig` or `.figma.json` document (`T-391`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FigmaDocumentSummary {
    pub name: String,
    pub version_label: String,
    pub pages: Vec<FigmaPageSummary>,
}

/// Per-page metadata returned by [`inspect_figma_bytes`] / [`inspect_figma_path`] (`T-391`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FigmaPageSummary {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub top_frame_count: usize,
    pub node_count: usize,
}

/// Options controlling Figma `.fig` and `.figma.json` import (`T-249`, `T-391`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FigmaImportOptions {
    pub layer_mapping: FigmaLayerMapping,
    pub import_hidden_layers: bool,
    pub preserve_masks: bool,
    pub preserve_blend_modes: bool,
    pub resize_stage_to_artboard: bool,
    /// Optional explicit subset of Figma page IDs to import, in exact requested order.
    /// `None` imports all non-internal pages.
    pub selected_page_ids: Option<Vec<String>>,
    /// How multiple Figma pages map into the resulting project (`Scenes` vs `1 page = 1 Layer`).
    pub page_mapping: FigmaPageMapping,
}

impl Default for FigmaImportOptions {
    fn default() -> Self {
        Self {
            layer_mapping: FigmaLayerMapping::TimelineLayers,
            import_hidden_layers: true,
            preserve_masks: true,
            preserve_blend_modes: true,
            resize_stage_to_artboard: true,
            selected_page_ids: None,
            page_mapping: FigmaPageMapping::Scenes,
        }
    }
}

pub(crate) enum FigmaZipReader<'a> {
    Memory(ZipArchive<Cursor<&'a [u8]>>),
    File(ZipArchive<BufReader<File>>),
}

/// Lazy image provider backed by an opened `.fig` ZIP archive and/or inline base64 images,
/// avoiding eager extraction of hundreds of megabytes of unreferenced raster entries.
pub(crate) struct FigmaImageArchive<'a> {
    zip: Option<FigmaZipReader<'a>>,
    /// Lowercase normalized image hash -> exact ZIP entry name.
    zip_index: HashMap<String, String>,
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
        zip: FigmaZipReader<'a>,
        zip_index: HashMap<String, String>,
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
        let Some(entry_name) = self.zip_index.get(&low) else {
            return Ok(None);
        };
        let Some(zip) = self.zip.as_mut() else {
            return Ok(None);
        };
        match zip {
            FigmaZipReader::Memory(za) => {
                let entry = za.by_name(entry_name).map_err(|e| {
                    format!("Could not open embedded Figma image '{image_ref}': {e}")
                })?;
                read_zip_image_entry(entry, image_ref)
            }
            FigmaZipReader::File(za) => {
                let entry = za.by_name(entry_name).map_err(|e| {
                    format!("Could not open embedded Figma image '{image_ref}': {e}")
                })?;
                read_zip_image_entry(entry, image_ref)
            }
        }
    }
}

fn read_zip_image_entry<R: Read>(
    mut entry: zip::read::ZipFile<'_, R>,
    image_ref: &str,
) -> Result<Option<Vec<u8>>, String> {
    if entry
        .unix_mode()
        .is_some_and(|mode| mode & 0o170000 == 0o120000)
    {
        return Err(format!("Symlink ZIP entry in .fig archive: {image_ref}"));
    }
    if entry.size() > MAX_ZIP_IMAGE_ENTRY_BYTES as u64 {
        return Err(format!(
            "Embedded Figma image '{image_ref}' exceeds 64 MiB single-entry safety limit"
        ));
    }
    let cap = (entry.size() as usize).min(4 * 1024 * 1024);
    let mut buf = Vec::with_capacity(cap);
    entry
        .by_ref()
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
    pub(super) stroke_paths: Vec<(Vec<PathCommand>, FillRule)>,
    pub(super) stroke_paths_are_outlines: bool,
    pub(super) text: Option<FigmaTextData>,
    pub(super) effects: Vec<FigmaEffect>,
    pub(super) component_id: Option<String>,
    pub(super) symbol_overrides: Vec<FigmaSymbolOverride>,
    pub(super) instance_text_layouts: Vec<FigmaInstanceTextLayout>,
    pub(super) background_color: Option<Color>,
    pub(super) auto_layout: FigmaAutoLayout,
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
            stroke_paths_are_outlines: false,
            text: None,
            effects: Vec::new(),
            component_id: None,
            symbol_overrides: Vec::new(),
            instance_text_layouts: Vec::new(),
            background_color: None,
            auto_layout: FigmaAutoLayout::default(),
            children: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum FigmaLayoutMode {
    #[default]
    None,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum FigmaLayoutWrap {
    #[default]
    NoWrap,
    Wrap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum FigmaLayoutSizing {
    #[default]
    Fixed,
    Hug,
    Fill,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum FigmaPrimaryAlign {
    #[default]
    Min,
    Center,
    Max,
    SpaceBetween,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum FigmaCounterAlign {
    #[default]
    Min,
    Center,
    Max,
    Baseline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum FigmaCounterAlignContent {
    #[default]
    Auto,
    SpaceBetween,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum FigmaLayoutPositioning {
    #[default]
    Auto,
    Absolute,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum FigmaConstraintH {
    #[default]
    Left,
    Right,
    LeftAndRight,
    Center,
    Scale,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum FigmaConstraintV {
    #[default]
    Top,
    Bottom,
    TopAndBottom,
    Center,
    Scale,
}

#[derive(Clone, Debug)]
pub(super) struct FigmaAutoLayout {
    pub(super) layout_mode: FigmaLayoutMode,
    pub(super) layout_wrap: FigmaLayoutWrap,
    pub(super) sizing_horizontal: FigmaLayoutSizing,
    pub(super) sizing_vertical: FigmaLayoutSizing,
    pub(super) layout_grow: f64,
    pub(super) layout_align: String,
    pub(super) padding_left: f64,
    pub(super) padding_right: f64,
    pub(super) padding_top: f64,
    pub(super) padding_bottom: f64,
    pub(super) item_spacing: f64,
    pub(super) counter_axis_spacing: Option<f64>,
    pub(super) primary_axis_align_items: FigmaPrimaryAlign,
    pub(super) counter_axis_align_items: FigmaCounterAlign,
    pub(super) counter_axis_align_content: FigmaCounterAlignContent,
    pub(super) layout_positioning: FigmaLayoutPositioning,
    pub(super) constraint_h: FigmaConstraintH,
    pub(super) constraint_v: FigmaConstraintV,
    pub(super) min_width: Option<f64>,
    pub(super) max_width: Option<f64>,
    pub(super) min_height: Option<f64>,
    pub(super) max_height: Option<f64>,
    pub(super) item_reverse_z_index: bool,
    pub(super) strokes_included_in_layout: bool,
    pub(super) stroke_align: String,
    pub(super) has_explicit_position: bool,
    pub(super) has_explicit_size: bool,
}

impl Default for FigmaAutoLayout {
    fn default() -> Self {
        Self {
            layout_mode: FigmaLayoutMode::None,
            layout_wrap: FigmaLayoutWrap::NoWrap,
            sizing_horizontal: FigmaLayoutSizing::Fixed,
            sizing_vertical: FigmaLayoutSizing::Fixed,
            layout_grow: 0.0,
            layout_align: String::from("INHERIT"),
            padding_left: 0.0,
            padding_right: 0.0,
            padding_top: 0.0,
            padding_bottom: 0.0,
            item_spacing: 0.0,
            counter_axis_spacing: None,
            primary_axis_align_items: FigmaPrimaryAlign::Min,
            counter_axis_align_items: FigmaCounterAlign::Min,
            counter_axis_align_content: FigmaCounterAlignContent::Auto,
            layout_positioning: FigmaLayoutPositioning::Auto,
            constraint_h: FigmaConstraintH::Left,
            constraint_v: FigmaConstraintV::Top,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            item_reverse_z_index: false,
            strokes_included_in_layout: false,
            stroke_align: String::from("INSIDE"),
            has_explicit_position: false,
            has_explicit_size: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FigmaArcData {
    pub(super) start_angle_rad: f64,
    pub(super) end_angle_rad: f64,
    pub(super) inner_radius_ratio: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum FigmaGradientKind {
    Linear,
    Radial,
    Angular,
    Diamond,
}

#[derive(Clone, Debug)]
pub(super) enum FigmaPaint {
    Solid {
        color: Color,
    },
    Gradient {
        kind: FigmaGradientKind,
        stops: Vec<GradientStop>,
        uv_transform: Option<[f64; 6]>,
    },
    Image {
        image_ref: String,
        scale_mode: String,
        alpha: f64,
        image_transform: Option<[f64; 6]>,
        scaling_factor: Option<f64>,
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
pub(super) struct FigmaInstanceTextLayout {
    pub(super) id: String,
    pub(super) size: Option<[f64; 2]>,
    pub(super) transform: Option<[f64; 6]>,
}

#[derive(Clone, Debug)]
pub(super) struct FigmaSymbolOverride {
    pub(super) text_auto_resize: Option<String>,
    pub(super) font_size: Option<f64>,
    pub(super) guid_path: Vec<String>,
    pub(super) visible: Option<bool>,
    pub(super) opacity: Option<f64>,
    pub(super) characters: Option<String>,
    pub(super) text_style_ids: Option<Vec<u64>>,
    pub(super) text_style_overrides: Vec<(u64, FigmaTextStyleOverride)>,
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

/// Recover Figma's 2x3 `Paint.transform` (`[m00, m01, m02, m10, m11, m12]`, mapping shape UV
/// `[0, 1]^2` into Figma gradient space `[0, 1]^2`) from REST API `gradientHandlePositions`.
pub(super) fn uv_transform_from_gradient_handles(
    h0: [f64; 2],
    h1: [f64; 2],
    h2: Option<[f64; 2]>,
    radial: bool,
) -> Option<[f64; 6]> {
    if !h0.iter().chain(&h1).all(|v| v.is_finite()) {
        return None;
    }
    let (c1, c2, t) = if radial {
        let p2 = h2
            .filter(|p| p.iter().all(|v| v.is_finite()))
            .unwrap_or([h0[0] - (h1[1] - h0[1]), h0[1] + (h1[0] - h0[0])]);
        let col1 = [2.0 * (h1[0] - h0[0]), 2.0 * (h1[1] - h0[1])];
        let col2 = [2.0 * (p2[0] - h0[0]), 2.0 * (p2[1] - h0[1])];
        let org = [
            h0[0] - 0.5 * col1[0] - 0.5 * col2[0],
            h0[1] - 0.5 * col1[1] - 0.5 * col2[1],
        ];
        (col1, col2, org)
    } else {
        let p2 = h2
            .filter(|p| p.iter().all(|v| v.is_finite()))
            .unwrap_or([h0[0] - (h1[1] - h0[1]), h0[1] + (h1[0] - h0[0])]);
        let col1 = [h1[0] - h0[0], h1[1] - h0[1]];
        let col2 = [p2[0] - h0[0], p2[1] - h0[1]];
        (col1, col2, h0)
    };
    let det = c1[0] * c2[1] - c1[1] * c2[0];
    if !det.is_finite() || det.abs() <= 1e-9 {
        return None;
    }
    let m00 = c2[1] / det;
    let m01 = -c2[0] / det;
    let m02 = -(m00 * t[0] + m01 * t[1]);
    let m10 = -c1[1] / det;
    let m11 = c1[0] / det;
    let m12 = -(m10 * t[0] + m11 * t[1]);
    let mat = [m00, m01, m02, m10, m11, m12];
    mat.iter().all(|v| v.is_finite()).then_some(mat)
}

/// Compose Flash's canonical `[-819.2, 819.2]` gradient box directly into shape-local coordinates
/// via `diag(sw, sh) * inv(Paint.transform) * canonical_flash_to_figma_uv`, preserving anisotropic
/// linear gradient isochrome normals and elliptical/rotated/skewed radial gradients.
pub(super) fn figma_uv_transform_to_flash_gradient_matrix(
    uv_transform: Option<[f64; 6]>,
    w: f64,
    h: f64,
) -> [f64; 6] {
    let sw = if w.is_finite() {
        w.clamp(1.0, 1e6)
    } else {
        100.0
    };
    let sh = if h.is_finite() {
        h.clamp(1.0, 1e6)
    } else {
        100.0
    };
    let fallback = [sw / 1638.4, 0.0, 0.0, sh / 1638.4, sw * 0.5, sh * 0.5];
    let [m00, m01, m02, m10, m11, m12] = uv_transform.unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let det = m00 * m11 - m01 * m10;
    if !det.is_finite() || det.abs() <= 1e-9 {
        return fallback;
    }
    let inv00 = m11 / det;
    let inv01 = -m01 / det;
    let inv02 = -(inv00 * m02 + inv01 * m12);
    let inv10 = -m10 / det;
    let inv11 = m00 / det;
    let inv12 = -(inv10 * m02 + inv11 * m12);
    let mat = [
        sw * inv00 / 1638.4,
        sh * inv10 / 1638.4,
        sw * inv01 / 1638.4,
        sh * inv11 / 1638.4,
        sw * (0.5 * inv00 + 0.5 * inv01 + inv02),
        sh * (0.5 * inv10 + 0.5 * inv11 + inv12),
    ];
    let mat_det = mat[0] * mat[3] - mat[1] * mat[2];
    if mat.iter().all(|v| v.is_finite() && v.abs() <= 1e7)
        && mat_det.is_finite()
        && mat_det.abs() > 1e-12
    {
        mat
    } else {
        fallback
    }
}

/// Returns `true` if `bytes` looks like a `.fig` Kiwi archive (ZIP with `canvas.fig` or `fig-kiwi`
/// / `fig-jam.` magic) or a `.figma.json` document.
pub fn is_figma_bytes(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"fig-kiwi") || bytes.starts_with(b"fig-jam.") {
        return true;
    }
    if bytes.starts_with(b"PK\x03\x04")
        && let Ok(zip) = ZipArchive::new(Cursor::new(bytes))
    {
        for name in zip.file_names().take(128) {
            if name == "canvas.fig"
                || name.ends_with(".fig")
                || name == "document.json"
                || name.ends_with("/document.json")
            {
                return true;
            }
        }
    }
    json_doc::looks_like_figma_json(bytes)
}

pub mod clipboard;
mod convert;
mod json_doc;
mod kiwi;
mod page_layout;
#[cfg(test)]
mod tests;
pub(crate) mod zstd;

pub use clipboard::*;

use convert::build_project_from_figma_doc;
use json_doc::{inspect_figma_json_doc, parse_figma_json_doc};
use kiwi::{inspect_fig_kiwi_canvas, parse_fig_kiwi_canvas_with_options};

struct ExtractedFigmaZipPayload {
    canvas_bytes: Option<Vec<u8>>,
    json_bytes: Option<Vec<u8>>,
    zip_image_index: HashMap<String, String>,
    meta_name: Option<String>,
}

fn ensure_not_zip_symlink<R: Read>(
    entry: &zip::read::ZipFile<'_, R>,
    ename: &str,
) -> Result<(), ImportError> {
    if entry
        .unix_mode()
        .is_some_and(|mode| mode & 0o170000 == 0o120000)
    {
        return Err(ImportError::Container(format!(
            "Symlink ZIP entry in .fig archive: {ename}"
        )));
    }
    Ok(())
}

fn scan_figma_zip_archive<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    index_images: bool,
) -> Result<ExtractedFigmaZipPayload, ImportError> {
    if zip.len() > 16_384 {
        return Err(ImportError::Container(
            ".fig ZIP archive exceeds 16,384 entries".into(),
        ));
    }
    let mut canvas_entry_name: Option<String> = None;
    let mut meta_entry_name: Option<String> = None;
    let mut json_entry_name: Option<String> = None;
    let mut zip_image_index: HashMap<String, String> = HashMap::new();

    {
        for raw_name in zip.file_names() {
            let ename = raw_name.replace('\\', "/");
            crate::source::safe_name(&ename)?;
            if ename.ends_with('/') {
                continue;
            }
            if ename == "canvas.fig" || (ename.ends_with(".fig") && canvas_entry_name.is_none()) {
                canvas_entry_name = Some(raw_name.to_owned());
            } else if ename == "meta.json" || ename.ends_with("/meta.json") {
                meta_entry_name = Some(raw_name.to_owned());
            } else if let Some(rest) = ename
                .strip_prefix("images/")
                .or_else(|| ename.rsplit_once("/images/").map(|(_, r)| r))
            {
                if index_images {
                    let key = rest
                        .split('.')
                        .next()
                        .unwrap_or(rest)
                        .trim()
                        .to_ascii_lowercase();
                    if !key.is_empty() {
                        zip_image_index
                            .entry(key)
                            .or_insert_with(|| raw_name.to_owned());
                    }
                }
            } else if ename.ends_with(".json") && json_entry_name.is_none() {
                json_entry_name = Some(raw_name.to_owned());
            }
        }
    }

    let mut meta_name: Option<String> = None;
    if let Some(ref mname) = meta_entry_name {
        let entry = zip
            .by_name(mname)
            .map_err(|e| ImportError::Container(e.to_string()))?;
        ensure_not_zip_symlink(&entry, mname)?;
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
    }

    let mut canvas_bytes: Option<Vec<u8>> = None;
    if let Some(ref cname) = canvas_entry_name {
        let entry = zip
            .by_name(cname)
            .map_err(|e| ImportError::Container(e.to_string()))?;
        ensure_not_zip_symlink(&entry, cname)?;
        let entry_size = entry.size();
        if entry_size > MAX_FIGMA_DECODE_BYTES as u64 {
            return Err(ImportError::Container(
                "canvas.fig entry exceeds 256 MiB limit".into(),
            ));
        }
        let cap = (entry_size as usize).min(16 * 1024 * 1024);
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
    }

    let mut json_bytes: Option<Vec<u8>> = None;
    if canvas_bytes.is_none()
        && let Some(ref jname) = json_entry_name
    {
        let entry = zip
            .by_name(jname)
            .map_err(|e| ImportError::Container(e.to_string()))?;
        ensure_not_zip_symlink(&entry, jname)?;
        let entry_size = entry.size();
        if entry_size > MAX_FIGMA_DECODE_BYTES as u64 {
            return Err(ImportError::Container(
                ".fig JSON entry exceeds 256 MiB limit".into(),
            ));
        }
        let cap = (entry_size as usize).min(16 * 1024 * 1024);
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

    Ok(ExtractedFigmaZipPayload {
        canvas_bytes,
        json_bytes,
        zip_image_index,
        meta_name,
    })
}

fn inspect_extracted_zip_payload(
    payload: ExtractedFigmaZipPayload,
    source_name: &str,
) -> Result<FigmaDocumentSummary, ImportError> {
    let effective_name = payload.meta_name.as_deref().unwrap_or(source_name);
    if let Some(cb) = payload.canvas_bytes {
        let mut summary = inspect_fig_kiwi_canvas(&cb, effective_name)?;
        if let Some(mtitle) = payload.meta_name {
            summary.name = mtitle;
        }
        return Ok(summary);
    }
    if let Some(jb) = payload.json_bytes {
        let s = std::str::from_utf8(&jb)
            .map_err(|_| ImportError::Container("Figma JSON entry must be UTF-8".into()))?;
        let mut summary = inspect_figma_json_doc(s, effective_name)?;
        if let Some(mtitle) = payload.meta_name
            && (summary.name.is_empty() || summary.name == "Document")
        {
            summary.name = mtitle;
        }
        return Ok(summary);
    }
    Err(ImportError::Container(
        ".fig ZIP archive contains neither canvas.fig nor a Figma JSON document".into(),
    ))
}

/// Quickly inspect a Figma `.fig` archive or `.figma.json` byte slice, returning its document
/// title, format version, and non-internal user pages without decoding geometry blobs or images.
pub fn inspect_figma_bytes(
    bytes: &[u8],
    source_name: &str,
) -> Result<FigmaDocumentSummary, ImportError> {
    if bytes.len() > MAX_FIGMA_FILE_BYTES {
        return Err(ImportError::Container(format!(
            "Figma file exceeds {} MiB input limit",
            MAX_FIGMA_FILE_BYTES / (1024 * 1024)
        )));
    }
    if bytes.starts_with(b"fig-kiwi") || bytes.starts_with(b"fig-jam.") {
        return inspect_fig_kiwi_canvas(bytes, source_name);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        let mut zip = ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ImportError::Container(format!("Invalid .fig ZIP archive: {e}")))?;
        let payload = scan_figma_zip_archive(&mut zip, false)?;
        return inspect_extracted_zip_payload(payload, source_name);
    }
    let json_str = std::str::from_utf8(bytes).map_err(|_| {
        ImportError::Container("Figma file is neither a binary .fig archive nor UTF-8 JSON".into())
    })?;
    inspect_figma_json_doc(json_str, source_name)
}

/// Quickly inspect a `.fig` or `.figma.json` file on disk (`< 100 ms` even on 250+ MiB `.fig`
/// archives by seeking `canvas.fig` directly without reading embedded raster entries).
pub fn inspect_figma_path(path: &Path) -> Result<FigmaDocumentSummary, ImportError> {
    let file_len = std::fs::metadata(path)?.len();
    if file_len > MAX_FIGMA_FILE_BYTES as u64 {
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

    let mut file = File::open(path)?;
    let mut magic = [0u8; 4];
    let read_n = file.read(&mut magic)?;
    file.seek(SeekFrom::Start(0))?;
    if read_n == 4 && &magic == b"PK\x03\x04" {
        let mut zip = ZipArchive::new(BufReader::new(file))
            .map_err(|e| ImportError::Container(format!("Invalid .fig ZIP archive: {e}")))?;
        let payload = scan_figma_zip_archive(&mut zip, false)?;
        return inspect_extracted_zip_payload(payload, stem);
    }

    let mut bytes = Vec::with_capacity((file_len as usize).min(64 * 1024 * 1024));
    file.take(MAX_FIGMA_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FIGMA_FILE_BYTES {
        return Err(ImportError::Container(format!(
            "Figma file exceeds {} MiB input limit",
            MAX_FIGMA_FILE_BYTES / (1024 * 1024)
        )));
    }
    inspect_figma_bytes(&bytes, stem)
}

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

fn import_from_zip_payload(
    payload: ExtractedFigmaZipPayload,
    zip_reader: FigmaZipReader<'_>,
    source_name: &str,
    options: &FigmaImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    let effective_name = payload.meta_name.as_deref().unwrap_or(source_name);
    if let Some(cb) = payload.canvas_bytes {
        let mut doc = parse_fig_kiwi_canvas_with_options(&cb, effective_name, options)?;
        if let Some(mtitle) = payload.meta_name {
            doc.name = mtitle;
        }
        let inline = std::mem::take(&mut doc.inline_images);
        let mut images = FigmaImageArchive::from_zip(zip_reader, payload.zip_image_index, inline);
        return build_project_from_figma_doc(doc, &mut images, options, source_name);
    }
    if let Some(jb) = payload.json_bytes {
        let s = std::str::from_utf8(&jb)
            .map_err(|_| ImportError::Container("Figma JSON entry must be UTF-8".into()))?;
        let mut doc = parse_figma_json_doc(s, effective_name)?;
        if let Some(mtitle) = payload.meta_name
            && (doc.name.is_empty() || doc.name == "Document")
        {
            doc.name = mtitle;
        }
        let inline = std::mem::take(&mut doc.inline_images);
        let mut images = FigmaImageArchive::from_zip(zip_reader, payload.zip_image_index, inline);
        return build_project_from_figma_doc(doc, &mut images, options, source_name);
    }
    Err(ImportError::Container(
        ".fig ZIP archive contains neither canvas.fig nor a Figma JSON document".into(),
    ))
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
        let mut doc = parse_fig_kiwi_canvas_with_options(bytes, source_name, options)?;
        let inline = std::mem::take(&mut doc.inline_images);
        let mut images = FigmaImageArchive::from_inline(inline);
        return build_project_from_figma_doc(doc, &mut images, options, source_name);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        let mut zip = ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ImportError::Container(format!("Invalid .fig ZIP archive: {e}")))?;
        let payload = scan_figma_zip_archive(&mut zip, true)?;
        return import_from_zip_payload(payload, FigmaZipReader::Memory(zip), source_name, options);
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
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.trim_end_matches(".figma"))
        .filter(|s| !s.is_empty())
        .unwrap_or("Figma Import");

    let mut file = File::open(path)?;
    let mut magic = [0u8; 4];
    let read_n = file.read(&mut magic)?;
    file.seek(SeekFrom::Start(0))?;
    if read_n == 4 && &magic == b"PK\x03\x04" {
        let mut zip = ZipArchive::new(BufReader::new(file))
            .map_err(|e| ImportError::Container(format!("Invalid .fig ZIP archive: {e}")))?;
        let payload = scan_figma_zip_archive(&mut zip, true)?;
        let (project, mut report) =
            import_from_zip_payload(payload, FigmaZipReader::File(zip), stem, options)?;
        report.source = path.display().to_string();
        return Ok((project, report));
    }

    let mut bytes = Vec::with_capacity((file_len as usize).min(64 * 1024 * 1024));
    file.take(MAX_FIGMA_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FIGMA_FILE_BYTES {
        return Err(ImportError::Container(format!(
            "Figma file exceeds {} MiB input limit",
            MAX_FIGMA_FILE_BYTES / (1024 * 1024)
        )));
    }
    let (project, mut report) = import_figma_bytes(&bytes, stem, options)?;
    report.source = path.display().to_string();
    Ok((project, report))
}
