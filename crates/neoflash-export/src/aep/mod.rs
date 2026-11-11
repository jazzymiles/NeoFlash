//! Adobe After Effects (`.aep` / `.aepx` + ExtendScript `.jsx` + `Footage/`) interchange exporter (`T-246`).
//!
//! Preserves NeoFlash projects across environments with maximum structural fidelity:
//! - **Vectors stay vectors:** `Rectangle` (including per-corner radii), `Ellipse` (including arcs and
//!   inner holes), `Path`, `PaintedPath`, `Appearance` multi-fill/stroke stacks, `Repeater` / `Offset` /
//!   `LiveCorners` modifiers, and `Shape` tweens become true editable Bezier `ShapeLayer`s
//!   (`ADBE Vector Shape - Group` with exact `vertices`, relative `inTangents` and `outTangents`,
//!   `ADBE Vector Graphic - Fill` / `G-Fill` / `Stroke`, and animated morph keyframes).
//! - **Scripts & expressions become After Effects JavaScript Expressions:**
//!   1. `Keyframe.property_expressions` (`PropertyExpression` on `X`, `Y`, `ScaleX`, `ScaleY`,
//!      `Rotation`, `Skew`, `Alpha`) compile into After Effects JavaScript Expression Engine code
//!      (`Position`, `Scale`, `Rotation`, `Opacity`, `Transform` Skew).
//!   2. AS1 / AS2 / AS3 / JS / TS `EnterFrame` handlers and frame-script property drivers
//!      (`_x`, `x`, `_y`, `y`, `_xscale`, `scaleX`, `_yscale`, `scaleY`, `_rotation`, `rotation`,
//!      `_alpha`, `alpha`, dynamic `.text`) translate into closed-form time/frame AE JavaScript
//!      Expressions (`timeToFrames(time - inPoint)`, `Math.*`, `clamp`, `wiggle`).
//!   3. Timeline flow actions (`stop()`, `gotoAndPlay(frameOrLabel)`, `gotoAndStop(frameOrLabel)`)
//!      compile into `Time Remap` JavaScript Expressions on symbol pre-comps plus Composition markers.
//!   4. Interactive or imperative handlers (`Click`, `Load`, sound/network calls) are preserved
//!      verbatim on After Effects Layer/Composition `MarkerValue`s with located diagnostics.
//! - **Compositions, masks, filters, 3D/Camera & Footage:** Nested `Symbol` pre-comps (with negative
//!   registration-point bounds preservation), multi-scene sequencing, `Graphic` `Loop` / `PlayOnce` /
//!   `SingleFrame` Time Remapping, `KeyframeEase` temporal curves, `TrackMatte` (`Geometry` / `Alpha` /
//!   `Luminance` + `Invert`), `BlendingMode`, native AE `Effects` (`Drop Shadow`, `Glow`, `Gaussian Blur`,
//!   `Bevel Alpha`, `Tint`), editable `TextLayer`s, 3D `z_depth` + `CameraLayer`, ruler guides, and
//!   extracted `Footage/` bitmaps/audio/video (honoring per-bitmap Library `BitmapExportSettings`).

use crate::{ExportError, apply_bitmap_export_settings, atomic_write};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use neoflash_core::{
    AppearancePaintEntry, BlendMode, Color, ColorTransform, Easing, Element, ElementKind, FillRule,
    FilterSpec, GradientKind, GradientPaint, GuideAxis, InstancePlayback, Keyframe, Layer,
    LayerRole, MaskMode, ObjectEvent, PathCommand, Project, PropertyExpression, ScriptLanguage,
    SoundEffect, SoundSync, Symbol, TextAlign, Timeline, Transform, TweenKind, TweenProperty,
    VectorModifier, oval_path_commands, rectangle_path_commands, sample_layer_in_at_fps,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

/// Options for Adobe After Effects (`.aep` / `.aepx` / `.jsx`) export (`T-246`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AepOptions {
    /// Optional symbol ID to export; `None` exports the full project (all scenes and used symbols).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Write companion `.aepx` After Effects XML project alongside `.aep`.
    #[serde(default = "bool_true")]
    pub include_aepx: bool,
    /// Write companion `.jsx` self-building After Effects ExtendScript alongside `.aep`.
    #[serde(default = "bool_true")]
    pub include_extendscript: bool,
    /// Translate NeoFlash `PropertyExpression`s and AS1/AS2/AS3/JS/TS scripts into AE JS Expressions.
    #[serde(default = "bool_true")]
    pub translate_scripts: bool,
    /// Fallback JPEG quality (`1..=100`) for bitmaps using document-default publish quality.
    #[serde(default = "default_jpeg_quality")]
    pub jpeg_quality: u8,
    /// Frame step when sampling topology-changing shape tweens into Bezier keyframes (`>= 1`).
    #[serde(default = "default_morph_step")]
    pub shape_morph_step: u32,
    /// Optional project base directory used to resolve external video files into `<stem>_Footage/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<PathBuf>,
}

const fn bool_true() -> bool {
    true
}
const fn default_jpeg_quality() -> u8 {
    80
}
const fn default_morph_step() -> u32 {
    1
}

impl Default for AepOptions {
    fn default() -> Self {
        Self {
            symbol: None,
            include_aepx: true,
            include_extendscript: true,
            translate_scripts: true,
            jpeg_quality: 80,
            shape_morph_step: 1,
            base_dir: None,
        }
    }
}

#[inline]
fn finite_or(v: f64, fallback: f64) -> f64 {
    if v.is_finite() { v } else { fallback }
}

#[inline]
fn finite_pair(p: [f64; 2]) -> [f64; 2] {
    [finite_or(p[0], 0.0), finite_or(p[1], 0.0)]
}

/// Classification of an After Effects interchange note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AepDiagnosticKind {
    TranslatedExpression,
    MarkerPreserved,
    Approximated,
    Skipped,
}

/// Located diagnostic emitted during After Effects export.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AepDiagnostic {
    pub code: &'static str,
    pub kind: AepDiagnosticKind,
    pub location: String,
    pub message: String,
}

/// Summary of a completed After Effects export.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AepExportReport {
    pub aep_path: PathBuf,
    pub aepx_path: Option<PathBuf>,
    pub jsx_path: Option<PathBuf>,
    pub footage_dir: PathBuf,
    pub footage_files: Vec<PathBuf>,
    pub composition_count: usize,
    pub vector_shape_layer_count: usize,
    pub text_layer_count: usize,
    pub precomp_layer_count: usize,
    pub footage_layer_count: usize,
    pub expression_count: usize,
    pub marker_count: usize,
    pub diagnostics: Vec<AepDiagnostic>,
}

// ============================================================================
// Intermediate After Effects Interchange Model
// ============================================================================

#[derive(Clone, Debug, Serialize)]
pub(super) struct AeProjectModel {
    pub(super) name: String,
    pub(super) fps: f64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) background: [f64; 3],
    pub(super) footage: Vec<AeFootageItem>,
    pub(super) compositions: Vec<AeComp>,
    pub(super) main_comp_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AeFootageItem {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) relative_path: String,
    pub(super) media_type: String,
    pub(super) width: Option<u32>,
    pub(super) height: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AeComp {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) folder: String,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) fps: f64,
    pub(super) duration_frames: u32,
    pub(super) background: [f64; 3],
    pub(super) origin_offset: [f64; 2],
    pub(super) work_area: Option<[u32; 2]>,
    pub(super) guides: Vec<AeGuide>,
    pub(super) markers: Vec<AeMarker>,
    /// Ordered bottom-to-top (matching NeoFlash `timeline.layers` stacking).
    pub(super) layers: Vec<AeLayer>,
    /// Optional default Time Remap expression applied when this comp is instantiated as a MovieClip.
    pub(super) script_time_remap_expr: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct AeGuide {
    pub(super) horizontal: bool,
    pub(super) position: f64,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AeMarker {
    pub(super) frame: u32,
    pub(super) duration_frames: u32,
    pub(super) label: String,
    pub(super) comment: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) enum AeTrackMatte {
    None,
    Alpha,
    AlphaInverted,
    Luma,
    LumaInverted,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AeLayer {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) source_element_id: Option<String>,
    pub(super) in_frame: u32,
    pub(super) out_frame: u32,
    pub(super) visible: bool,
    pub(super) locked: bool,
    pub(super) solo: bool,
    pub(super) is_track_matte_source: bool,
    pub(super) track_matte: AeTrackMatte,
    pub(super) blend_mode: &'static str,
    pub(super) auto_orient_along_path: bool,
    pub(super) three_d: bool,
    pub(super) z_depth: f64,
    pub(super) parent_layer_id: Option<String>,
    pub(super) anchor_point: [f64; 2],
    pub(super) position_keys: Vec<AeKey<[f64; 2]>>,
    pub(super) scale_keys: Vec<AeKey<[f64; 2]>>,
    pub(super) rotation_keys: Vec<AeKey<f64>>,
    pub(super) opacity_keys: Vec<AeKey<f64>>,
    pub(super) skew_keys: Vec<AeKey<f64>>,
    pub(super) expressions: AeLayerExpressions,
    pub(super) effects: Vec<AeEffect>,
    pub(super) markers: Vec<AeMarker>,
    pub(super) kind: AeLayerKind,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct AeLayerExpressions {
    pub(super) position: Option<String>,
    pub(super) scale: Option<String>,
    pub(super) rotation: Option<String>,
    pub(super) opacity: Option<String>,
    pub(super) skew: Option<String>,
    pub(super) time_remap: Option<String>,
    pub(super) source_text: Option<String>,
}

impl AeLayerExpressions {
    fn count(&self) -> usize {
        usize::from(self.position.is_some())
            + usize::from(self.scale.is_some())
            + usize::from(self.rotation.is_some())
            + usize::from(self.opacity.is_some())
            + usize::from(self.skew.is_some())
            + usize::from(self.time_remap.is_some())
            + usize::from(self.source_text.is_some())
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct AeEase {
    pub(super) in_speed: f64,
    pub(super) in_influence: f64,
    pub(super) out_speed: f64,
    pub(super) out_influence: f64,
    pub(super) hold: bool,
}

impl Default for AeEase {
    fn default() -> Self {
        Self {
            in_speed: 0.0,
            in_influence: 16.6667,
            out_speed: 0.0,
            out_influence: 16.6667,
            hold: false,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AeKey<T> {
    pub(super) frame: u32,
    pub(super) value: T,
    pub(super) ease: AeEase,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AeBezierContour {
    pub vertices: Vec<[f64; 2]>,
    pub in_tangents: Vec<[f64; 2]>,
    pub out_tangents: Vec<[f64; 2]>,
    pub closed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(super) enum AePaintFill {
    None,
    Solid {
        color: [f64; 3],
        opacity: f64,
        even_odd: bool,
    },
    Gradient {
        radial: bool,
        start: [f64; 2],
        end: [f64; 2],
        highlight_length: f64,
        stops: Vec<(f64, [f64; 4])>,
        opacity: f64,
        even_odd: bool,
    },
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AePaintStroke {
    pub(super) color: [f64; 3],
    pub(super) opacity: f64,
    pub(super) width: f64,
    pub(super) dashes: Option<[f64; 2]>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) enum AeVectorModifier {
    Repeater {
        copies: u32,
        offset_x: f64,
        offset_y: f64,
        scale_pct: f64,
        rotation: f64,
    },
    OffsetPaths {
        amount: f64,
    },
    RoundCorners {
        radius: f64,
    },
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AeShapeGroup {
    pub(super) name: String,
    /// Each contour has one static or animated keyframe track of `AeBezierContour`.
    pub(super) contour_tracks: Vec<Vec<AeKey<AeBezierContour>>>,
    pub(super) fills: Vec<AePaintFill>,
    pub(super) strokes: Vec<AePaintStroke>,
    pub(super) modifiers: Vec<AeVectorModifier>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) enum AeLayerKind {
    Shape {
        groups: Vec<AeShapeGroup>,
    },
    Text {
        text: String,
        font: String,
        font_size: f64,
        color: [f64; 3],
        bold: bool,
        italic: bool,
        align: TextAlign,
        leading: f64,
        tracking: f64,
        box_size: Option<[f64; 2]>,
    },
    PreComp {
        comp_id: String,
        comp_origin: [f64; 2],
        time_remap_keys: Vec<AeKey<f64>>,
    },
    Footage {
        footage_id: String,
        is_audio_only: bool,
        is_video: bool,
        start_offset_sec: f64,
        end_offset_sec: Option<f64>,
        loop_count: u32,
        muted: bool,
        audio_levels_db: Vec<AeKey<[f64; 2]>>,
    },
    Null,
    Camera {
        focal_length: f64,
        zoom_keys: Vec<AeKey<f64>>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub(super) enum AeEffect {
    DropShadow {
        color: [f64; 3],
        opacity_pct: f64,
        direction_deg: f64,
        distance: f64,
        softness: f64,
    },
    Glow {
        color: [f64; 3],
        radius: f64,
        intensity: f64,
    },
    GaussianBlur {
        blurriness: f64,
        dimensions: u8,
    },
    BevelAlpha {
        edge_thickness: f64,
        light_angle: f64,
        light_intensity: f64,
    },
    Tint {
        color: [f64; 3],
        amount_pct: f64,
    },
}

// ============================================================================

mod aepx;
mod comp_builder;
mod expressions;
mod footage;
mod jsx;
mod rifx;
mod shapes;
#[cfg(test)]
mod tests;

use aepx::render_aepx_xml;
use comp_builder::{build_ae_comp, color_rgb01};
pub use expressions::compile_property_expression_to_ae_js;
use expressions::{
    ScriptPropertyDrivers, analyze_element_scripts_for_ae, analyze_symbol_scripts_for_ae,
};
use footage::extract_aep_footage;
use jsx::render_extendscript_jsx;
use rifx::encode_rifx_aep_container;
pub use shapes::path_commands_to_ae_contours;
use shapes::{
    apply_element_appearance_to_group, build_ae_fill, build_static_shape_group_for_element,
    element_vector_commands,
};

// ============================================================================
// Project -> AeProjectModel Builder
// ============================================================================

/// Exports a NeoFlash [`Project`] to an Adobe After Effects interchange package:
/// - `<stem>.aep` (`RIFX` / `Egg!` binary project container with `Cdta`, `Layr`, `shap` & `expr` chunks)
/// - `<stem>.aepx` (After Effects XML project, when `options.include_aepx` is `true`)
/// - `<stem>.jsx` (Self-building After Effects ExtendScript constructor, when `options.include_extendscript` is `true`)
/// - `<stem>_Footage/` (Extracted bitmaps, audio and video assets honoring per-bitmap publish settings)
pub fn export_project_aep(
    project: &Project,
    path: impl AsRef<Path>,
    options: &AepOptions,
) -> Result<AepExportReport, ExportError> {
    let out_path = path.as_ref();
    let aep_path = if out_path.extension().is_some() {
        out_path.to_path_buf()
    } else {
        out_path.with_extension("aep")
    };
    let parent_dir = aep_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent_dir)?;

    let stem = aep_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("project");
    let footage_dir_name = format!("{stem}_Footage");
    let footage_dir = parent_dir.join(&footage_dir_name);

    // Apply personal Library bitmap export settings (`T-251`) on a derived copy so authored data stays untouched.
    let mut working = project.clone();
    let (_, bitmap_diags) = apply_bitmap_export_settings(&mut working, options.jpeg_quality);

    let mut diagnostics = Vec::new();
    for msg in bitmap_diags {
        diagnostics.push(AepDiagnostic {
            code: "AE_BITMAP_APPROX",
            kind: AepDiagnosticKind::Approximated,
            location: "Library".into(),
            message: msg,
        });
    }

    // 1. Extract used Footage resources (`image/*`, `audio/*`, `video/*`, plus external video spans)
    let (footage_items, footage_files, resource_to_footage) = extract_aep_footage(
        &working,
        &footage_dir,
        &footage_dir_name,
        options.base_dir.as_deref(),
        &mut diagnostics,
    )?;

    // 2. Build compositions (symbols + scenes + optional multi-scene master composition)
    let raw_fps = f64::from(working.fps.max(1));
    let fps = raw_fps.clamp(1.0, 99.0);
    if (fps - raw_fps).abs() > 1e-6 {
        diagnostics.push(AepDiagnostic {
            code: "AE_FPS_CLAMPED",
            kind: AepDiagnosticKind::Approximated,
            location: "Project".into(),
            message: format!(
                "Clamped composition frame rate from {raw_fps:.2} fps to After Effects' {fps:.2} fps limit."
            ),
        });
    }
    let stage_w = working.width.clamp(4, 30_000);
    let stage_h = working.height.clamp(4, 30_000);
    if stage_w != working.width.max(1) || stage_h != working.height.max(1) {
        diagnostics.push(AepDiagnostic {
            code: "AE_COMP_BOUNDS_CLAMPED",
            kind: AepDiagnosticKind::Approximated,
            location: "Project".into(),
            message: format!(
                "Clamped composition dimensions {}x{} into After Effects' 4..=30000 px range ({stage_w}x{stage_h}).",
                working.width, working.height
            ),
        });
    }
    let bg = color_rgb01(working.background);

    let scene_ids: Vec<String> = if let Some(sym_id) = &options.symbol {
        if working.symbol(sym_id).is_none() {
            return Err(ExportError::MissingRootSymbol(sym_id.clone()));
        }
        vec![sym_id.clone()]
    } else if working.scenes.is_empty() {
        vec![working.root.clone()]
    } else {
        working.scenes.clone()
    };

    let scene_set: BTreeSet<String> = scene_ids.iter().cloned().collect();
    let reachable_symbols = collect_reachable_symbols(&working, &scene_ids);

    // Pre-compute symbol pre-comp bounds so negative coordinates around (0, 0) are never clipped
    let mut comp_origins: BTreeMap<String, ([f64; 2], [u32; 2])> = BTreeMap::new();
    for sym in &working.symbols {
        if scene_set.contains(&sym.id) {
            comp_origins.insert(sym.id.clone(), ([0.0, 0.0], [stage_w, stage_h]));
        } else if reachable_symbols.contains(&sym.id) {
            let (origin, dims) = compute_symbol_precomp_bounds(&working, sym);
            comp_origins.insert(
                sym.id.clone(),
                (origin, [dims[0].clamp(4, 30_000), dims[1].clamp(4, 30_000)]),
            );
        }
    }

    let mut compositions: Vec<AeComp> = Vec::new();
    let mut symbol_time_remap_exprs: BTreeMap<String, String> = BTreeMap::new();

    // First build Library symbol pre-comps so scene comps can reference their `script_time_remap_expr`
    for sym in &working.symbols {
        if !scene_set.contains(&sym.id) && reachable_symbols.contains(&sym.id) {
            let &(origin, [cw, ch]) = comp_origins
                .get(&sym.id)
                .unwrap_or(&([0.0, 0.0], [stage_w, stage_h]));
            let comp = build_ae_comp(
                &working,
                sym,
                cw,
                ch,
                origin,
                fps,
                bg,
                if sym.folder.is_empty() {
                    "Symbols".to_owned()
                } else {
                    format!("Symbols/{}", sym.folder)
                },
                options,
                &comp_origins,
                &symbol_time_remap_exprs,
                &resource_to_footage,
                &mut diagnostics,
            );
            if let Some(expr) = &comp.script_time_remap_expr {
                symbol_time_remap_exprs.insert(sym.id.clone(), expr.clone());
            }
            compositions.push(comp);
        }
    }

    // Next build Scene compositions
    let mut built_scene_comps: Vec<(String, u32)> = Vec::new();
    for sid in &scene_ids {
        if let Some(sym) = working.symbol(sid) {
            let comp = build_ae_comp(
                &working,
                sym,
                stage_w,
                stage_h,
                [0.0, 0.0],
                fps,
                bg,
                String::new(),
                options,
                &comp_origins,
                &symbol_time_remap_exprs,
                &resource_to_footage,
                &mut diagnostics,
            );
            built_scene_comps.push((comp.id.clone(), comp.duration_frames.max(1)));
            compositions.push(comp);
        }
    }

    // If multiple scenes exist, synthesize a sequenced Master Composition playing Scene 1, Scene 2, ...
    let main_comp_id = if built_scene_comps.len() > 1 {
        let mut cursor = 0u32;
        let mut master_layers = Vec::new();
        let mut master_markers = Vec::new();
        for (idx, (sc_id, dur)) in built_scene_comps.iter().enumerate() {
            let scene_name = working
                .symbol(sc_id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| format!("Scene {}", idx + 1));
            master_markers.push(AeMarker {
                frame: cursor,
                duration_frames: *dur,
                label: scene_name.clone(),
                comment: format!("Scene {}", idx + 1),
            });
            master_layers.push(AeLayer {
                id: format!("master-scene-{idx}"),
                name: scene_name,
                source_element_id: None,
                in_frame: cursor,
                out_frame: cursor + *dur,
                visible: true,
                locked: false,
                solo: false,
                is_track_matte_source: false,
                track_matte: AeTrackMatte::None,
                blend_mode: "NORMAL",
                auto_orient_along_path: false,
                three_d: false,
                z_depth: 0.0,
                parent_layer_id: None,
                anchor_point: [0.0, 0.0],
                position_keys: vec![AeKey {
                    frame: cursor,
                    value: [0.0, 0.0],
                    ease: AeEase::default(),
                }],
                scale_keys: vec![AeKey {
                    frame: cursor,
                    value: [100.0, 100.0],
                    ease: AeEase::default(),
                }],
                rotation_keys: vec![AeKey {
                    frame: cursor,
                    value: 0.0,
                    ease: AeEase::default(),
                }],
                opacity_keys: vec![AeKey {
                    frame: cursor,
                    value: 100.0,
                    ease: AeEase::default(),
                }],
                skew_keys: Vec::new(),
                expressions: AeLayerExpressions::default(),
                effects: Vec::new(),
                markers: Vec::new(),
                kind: AeLayerKind::PreComp {
                    comp_id: sc_id.clone(),
                    comp_origin: [0.0, 0.0],
                    time_remap_keys: Vec::new(),
                },
            });
            cursor += *dur;
        }
        let master_id = "__nf_master_comp".to_owned();
        compositions.push(AeComp {
            id: master_id.clone(),
            name: if working.name.trim().is_empty() {
                "Main Composition".into()
            } else {
                working.name.clone()
            },
            folder: String::new(),
            width: stage_w,
            height: stage_h,
            fps,
            duration_frames: cursor.max(1),
            background: bg,
            origin_offset: [0.0, 0.0],
            work_area: None,
            guides: Vec::new(),
            markers: master_markers,
            layers: master_layers,
            script_time_remap_expr: None,
        });
        master_id
    } else {
        built_scene_comps
            .first()
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| working.root.clone())
    };

    let model = AeProjectModel {
        name: if working.name.trim().is_empty() {
            stem.to_owned()
        } else {
            working.name.clone()
        },
        fps,
        width: stage_w,
        height: stage_h,
        background: bg,
        footage: footage_items,
        compositions,
        main_comp_id,
    };

    // Count summary metrics
    let mut vector_shape_layer_count = 0usize;
    let mut text_layer_count = 0usize;
    let mut precomp_layer_count = 0usize;
    let mut footage_layer_count = 0usize;
    let mut expression_count = 0usize;
    let mut marker_count = 0usize;

    for comp in &model.compositions {
        marker_count += comp.markers.len();
        for layer in &comp.layers {
            expression_count += layer.expressions.count();
            marker_count += layer.markers.len();
            match &layer.kind {
                AeLayerKind::Shape { .. } => vector_shape_layer_count += 1,
                AeLayerKind::Text { .. } => text_layer_count += 1,
                AeLayerKind::PreComp { .. } => precomp_layer_count += 1,
                AeLayerKind::Footage { .. } => footage_layer_count += 1,
                AeLayerKind::Null | AeLayerKind::Camera { .. } => {}
            }
        }
    }

    // 3. Serialize RIFX/Egg! `.aep`, `.aepx` XML, and self-building ExtendScript `.jsx`
    let jsx_source = render_extendscript_jsx(&model, &aep_path);
    let aep_bytes = encode_rifx_aep_container(&model, &jsx_source)?;
    atomic_write(&aep_path, &aep_bytes)?;

    let aepx_path = if options.include_aepx {
        let p = aep_path.with_extension("aepx");
        let xml = render_aepx_xml(&model, &aep_bytes);
        atomic_write(&p, xml.as_bytes())?;
        Some(p)
    } else {
        None
    };

    let jsx_path = if options.include_extendscript {
        let p = aep_path.with_extension("jsx");
        atomic_write(&p, jsx_source.as_bytes())?;
        Some(p)
    } else {
        None
    };

    Ok(AepExportReport {
        aep_path,
        aepx_path,
        jsx_path,
        footage_dir,
        footage_files,
        composition_count: model.compositions.len(),
        vector_shape_layer_count,
        text_layer_count,
        precomp_layer_count,
        footage_layer_count,
        expression_count,
        marker_count,
        diagnostics,
    })
}

pub(super) fn collect_reachable_symbols(project: &Project, roots: &[String]) -> BTreeSet<String> {
    let mut visited = BTreeSet::new();
    let mut stack: Vec<String> = roots.to_vec();
    while let Some(id) = stack.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        if let Some(sym) = project.symbol(&id) {
            for layer in &sym.timeline.layers {
                for kf in &layer.keyframes {
                    for el in &kf.elements {
                        if let ElementKind::Instance { symbol_id, .. } = &el.kind
                            && !visited.contains(symbol_id)
                        {
                            stack.push(symbol_id.clone());
                        }
                    }
                }
            }
        }
    }
    visited
}

pub(super) fn compute_symbol_precomp_bounds(
    project: &Project,
    sym: &Symbol,
) -> ([f64; 2], [u32; 2]) {
    let mut min_x = 0.0_f64;
    let mut min_y = 0.0_f64;
    let mut max_x = f64::from(project.width.max(64));
    let mut max_y = f64::from(project.height.max(64));
    let mut any = false;

    for layer in &sym.timeline.layers {
        if matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder) {
            continue;
        }
        for kf in &layer.keyframes {
            for el in &kf.elements {
                let (lx0, ly0, lx1, ly1) = local_element_bounds(project, el);
                let sx = el.transform.scale_x.abs().max(0.01);
                let sy = el.transform.scale_y.abs().max(0.01);
                let pad = 32.0;
                let x0 = el.transform.x + lx0 * sx - pad;
                let y0 = el.transform.y + ly0 * sy - pad;
                let x1 = el.transform.x + lx1 * sx + pad;
                let y1 = el.transform.y + ly1 * sy + pad;
                if !any {
                    min_x = x0.min(0.0);
                    min_y = y0.min(0.0);
                    max_x = x1.max(64.0);
                    max_y = y1.max(64.0);
                    any = true;
                } else {
                    min_x = min_x.min(x0);
                    min_y = min_y.min(y0);
                    max_x = max_x.max(x1);
                    max_y = max_y.max(y1);
                }
            }
        }
    }
    if !any {
        return ([0.0, 0.0], [project.width.max(64), project.height.max(64)]);
    }
    let ox = (-min_x).max(0.0).ceil();
    let oy = (-min_y).max(0.0).ceil();
    let w = ((max_x + ox).ceil() as u32).clamp(16, 8192);
    let h = ((max_y + oy).ceil() as u32).clamp(16, 8192);
    ([ox, oy], [w, h])
}

pub(super) fn local_element_bounds(project: &Project, el: &Element) -> (f64, f64, f64, f64) {
    match &el.kind {
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. }
        | ElementKind::Video { width, height }
        | ElementKind::TextBox { width, height, .. } => (0.0, 0.0, *width, *height),
        ElementKind::Bitmap { width, height, .. } => {
            (0.0, 0.0, f64::from(*width), f64::from(*height))
        }
        ElementKind::Text {
            text, font_size, ..
        } => (
            0.0,
            -*font_size,
            (text.chars().count() as f64 * *font_size * 0.65).max(32.0),
            *font_size * 0.5,
        ),
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            let mut x0 = 0.0_f64;
            let mut y0 = 0.0_f64;
            let mut x1 = 16.0_f64;
            let mut y1 = 16.0_f64;
            let mut first = true;
            for cmd in commands {
                let pts: &[[f64; 2]] = match cmd {
                    PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => &[[*x, *y]],
                    PathCommand::QuadraticTo { cx, cy, x, y } => &[[*cx, *cy], [*x, *y]],
                    PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x,
                        y,
                    } => &[[*cx1, *cy1], [*cx2, *cy2], [*x, *y]],
                    PathCommand::Close => &[],
                };
                for &[px, py] in pts {
                    if first {
                        x0 = px;
                        y0 = py;
                        x1 = px;
                        y1 = py;
                        first = false;
                    } else {
                        x0 = x0.min(px);
                        y0 = y0.min(py);
                        x1 = x1.max(px);
                        y1 = y1.max(py);
                    }
                }
            }
            (x0, y0, x1, y1)
        }
        ElementKind::Instance { symbol_id, .. } => {
            if let Some(child) = project.symbol(symbol_id)
                && child.id != symbol_id.as_str()
            {
                let _ = child;
            }
            (-128.0, -128.0, 256.0, 256.0)
        }
    }
}
