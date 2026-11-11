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
struct AeProjectModel {
    name: String,
    fps: f64,
    width: u32,
    height: u32,
    background: [f64; 3],
    footage: Vec<AeFootageItem>,
    compositions: Vec<AeComp>,
    main_comp_id: String,
}

#[derive(Clone, Debug, Serialize)]
struct AeFootageItem {
    id: String,
    name: String,
    relative_path: String,
    media_type: String,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
struct AeComp {
    id: String,
    name: String,
    folder: String,
    width: u32,
    height: u32,
    fps: f64,
    duration_frames: u32,
    background: [f64; 3],
    origin_offset: [f64; 2],
    work_area: Option<[u32; 2]>,
    guides: Vec<AeGuide>,
    markers: Vec<AeMarker>,
    /// Ordered bottom-to-top (matching NeoFlash `timeline.layers` stacking).
    layers: Vec<AeLayer>,
    /// Optional default Time Remap expression applied when this comp is instantiated as a MovieClip.
    script_time_remap_expr: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct AeGuide {
    horizontal: bool,
    position: f64,
}

#[derive(Clone, Debug, Serialize)]
struct AeMarker {
    frame: u32,
    duration_frames: u32,
    label: String,
    comment: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum AeTrackMatte {
    None,
    Alpha,
    AlphaInverted,
    Luma,
    LumaInverted,
}

#[derive(Clone, Debug, Serialize)]
struct AeLayer {
    id: String,
    name: String,
    source_element_id: Option<String>,
    in_frame: u32,
    out_frame: u32,
    visible: bool,
    locked: bool,
    solo: bool,
    is_track_matte_source: bool,
    track_matte: AeTrackMatte,
    blend_mode: &'static str,
    auto_orient_along_path: bool,
    three_d: bool,
    z_depth: f64,
    parent_layer_id: Option<String>,
    anchor_point: [f64; 2],
    position_keys: Vec<AeKey<[f64; 2]>>,
    scale_keys: Vec<AeKey<[f64; 2]>>,
    rotation_keys: Vec<AeKey<f64>>,
    opacity_keys: Vec<AeKey<f64>>,
    skew_keys: Vec<AeKey<f64>>,
    expressions: AeLayerExpressions,
    effects: Vec<AeEffect>,
    markers: Vec<AeMarker>,
    kind: AeLayerKind,
}

#[derive(Clone, Debug, Default, Serialize)]
struct AeLayerExpressions {
    position: Option<String>,
    scale: Option<String>,
    rotation: Option<String>,
    opacity: Option<String>,
    skew: Option<String>,
    time_remap: Option<String>,
    source_text: Option<String>,
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
struct AeEase {
    in_speed: f64,
    in_influence: f64,
    out_speed: f64,
    out_influence: f64,
    hold: bool,
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
struct AeKey<T> {
    frame: u32,
    value: T,
    ease: AeEase,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AeBezierContour {
    pub vertices: Vec<[f64; 2]>,
    pub in_tangents: Vec<[f64; 2]>,
    pub out_tangents: Vec<[f64; 2]>,
    pub closed: bool,
}

#[derive(Clone, Debug, Serialize)]
enum AePaintFill {
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
struct AePaintStroke {
    color: [f64; 3],
    opacity: f64,
    width: f64,
    dashes: Option<[f64; 2]>,
}

#[derive(Clone, Debug, Serialize)]
enum AeVectorModifier {
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
struct AeShapeGroup {
    name: String,
    /// Each contour has one static or animated keyframe track of `AeBezierContour`.
    contour_tracks: Vec<Vec<AeKey<AeBezierContour>>>,
    fills: Vec<AePaintFill>,
    strokes: Vec<AePaintStroke>,
    modifiers: Vec<AeVectorModifier>,
}

#[derive(Clone, Debug, Serialize)]
enum AeLayerKind {
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
enum AeEffect {
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
// Vector Path Conversion (`PathCommand` -> `AeBezierContour`)
// ============================================================================

/// Converts NeoFlash [`PathCommand`] contours into exact After Effects Bezier contours
/// (`vertices` + relative `inTangents` and `outTangents`, elevating quadratics to exact cubics).
pub fn path_commands_to_ae_contours(
    commands: &[PathCommand],
    offset: [f64; 2],
) -> Vec<AeBezierContour> {
    let mut contours = Vec::new();
    let mut cur_vertices: Vec<[f64; 2]> = Vec::new();
    let mut cur_in: Vec<[f64; 2]> = Vec::new();
    let mut cur_out: Vec<[f64; 2]> = Vec::new();
    let mut cur_closed = false;

    let flush = |contours: &mut Vec<AeBezierContour>,
                 verts: &mut Vec<[f64; 2]>,
                 ins: &mut Vec<[f64; 2]>,
                 outs: &mut Vec<[f64; 2]>,
                 closed: &mut bool| {
        if verts.len() >= 2 {
            if *closed && verts.len() > 2 {
                let first = verts[0];
                let last = *verts.last().unwrap();
                if (first[0] - last[0]).hypot(first[1] - last[1]) <= 1e-4 {
                    let last_in = ins.pop().unwrap_or([0.0, 0.0]);
                    verts.pop();
                    outs.pop();
                    ins[0] = last_in;
                }
            }
            contours.push(AeBezierContour {
                vertices: std::mem::take(verts).into_iter().map(finite_pair).collect(),
                in_tangents: std::mem::take(ins).into_iter().map(finite_pair).collect(),
                out_tangents: std::mem::take(outs).into_iter().map(finite_pair).collect(),
                closed: *closed,
            });
        } else {
            verts.clear();
            ins.clear();
            outs.clear();
        }
        *closed = false;
    };

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                flush(
                    &mut contours,
                    &mut cur_vertices,
                    &mut cur_in,
                    &mut cur_out,
                    &mut cur_closed,
                );
                cur_vertices.push([x + offset[0], y + offset[1]]);
                cur_in.push([0.0, 0.0]);
                cur_out.push([0.0, 0.0]);
            }
            PathCommand::LineTo { x, y } => {
                cur_vertices.push([x + offset[0], y + offset[1]]);
                cur_in.push([0.0, 0.0]);
                cur_out.push([0.0, 0.0]);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let p1 = [x + offset[0], y + offset[1]];
                let q = [cx + offset[0], cy + offset[1]];
                if let Some(&p0) = cur_vertices.last() {
                    // Exact quadratic-to-cubic elevation:
                    // C1 = P0 + 2/3 * (Q - P0), C2 = P1 + 2/3 * (Q - P1).
                    let out_rel = [(2.0 / 3.0) * (q[0] - p0[0]), (2.0 / 3.0) * (q[1] - p0[1])];
                    let in_rel = [(2.0 / 3.0) * (q[0] - p1[0]), (2.0 / 3.0) * (q[1] - p1[1])];
                    if let Some(last_out) = cur_out.last_mut() {
                        *last_out = out_rel;
                    }
                    cur_vertices.push(p1);
                    cur_in.push(in_rel);
                    cur_out.push([0.0, 0.0]);
                } else {
                    cur_vertices.push(p1);
                    cur_in.push([0.0, 0.0]);
                    cur_out.push([0.0, 0.0]);
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
                let p1 = [x + offset[0], y + offset[1]];
                let c1 = [cx1 + offset[0], cy1 + offset[1]];
                let c2 = [cx2 + offset[0], cy2 + offset[1]];
                if let Some(&p0) = cur_vertices.last() {
                    if let Some(last_out) = cur_out.last_mut() {
                        *last_out = [c1[0] - p0[0], c1[1] - p0[1]];
                    }
                    cur_vertices.push(p1);
                    cur_in.push([c2[0] - p1[0], c2[1] - p1[1]]);
                    cur_out.push([0.0, 0.0]);
                } else {
                    cur_vertices.push(p1);
                    cur_in.push([0.0, 0.0]);
                    cur_out.push([0.0, 0.0]);
                }
            }
            PathCommand::Close => {
                cur_closed = true;
                flush(
                    &mut contours,
                    &mut cur_vertices,
                    &mut cur_in,
                    &mut cur_out,
                    &mut cur_closed,
                );
            }
        }
    }
    flush(
        &mut contours,
        &mut cur_vertices,
        &mut cur_in,
        &mut cur_out,
        &mut cur_closed,
    );
    contours
}

type ExtractedVectorShape = (
    Vec<PathCommand>,
    FillRule,
    Color,
    Color,
    Option<GradientPaint>,
    Option<GradientPaint>,
    f64,
);

fn element_vector_commands(el: &Element) -> Option<ExtractedVectorShape> {
    match &el.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let cmds = rectangle_path_commands(*width, *height, el.rectangle_options());
            let has_fill = fill[3] > 0;
            let closed_cmds = ensure_filled_closed(cmds, has_fill);
            Some((
                closed_cmds,
                FillRule::NonZero,
                *fill,
                *stroke,
                None,
                None,
                *stroke_width,
            ))
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let cmds = oval_path_commands(*width, *height, el.oval_options());
            let has_fill = fill[3] > 0;
            let closed_cmds = ensure_filled_closed(cmds, has_fill);
            Some((
                closed_cmds,
                FillRule::NonZero,
                *fill,
                *stroke,
                None,
                None,
                *stroke_width,
            ))
        }
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        } => {
            let has_fill = fill[3] > 0;
            Some((
                ensure_filled_closed(commands.clone(), has_fill),
                *fill_rule,
                *fill,
                *stroke,
                None,
                None,
                *stroke_width,
            ))
        }
        ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
        } => {
            let has_fill = fill[3] > 0 || fill_gradient.is_some();
            Some((
                ensure_filled_closed(commands.clone(), has_fill),
                *fill_rule,
                *fill,
                *stroke,
                fill_gradient.clone(),
                stroke_gradient.clone(),
                *stroke_width,
            ))
        }
        _ => None,
    }
}

fn ensure_filled_closed(mut cmds: Vec<PathCommand>, has_fill: bool) -> Vec<PathCommand> {
    if has_fill && !cmds.is_empty() && !matches!(cmds.last(), Some(PathCommand::Close)) {
        cmds.push(PathCommand::Close);
    }
    cmds
}

// ============================================================================
// Script & PropertyExpression -> After Effects JavaScript Expression Compiler
// ============================================================================

/// Translates a NeoFlash [`PropertyExpression`] into a valid After Effects JavaScript expression
/// snippet evaluating the scalar result for `base_var` (e.g. `"value"` or `"value[0]"`).
pub fn compile_property_expression_to_ae_js(expr: &PropertyExpression, base_var: &str) -> String {
    let raw = expr.expression.trim();
    if raw.is_empty() || raw == "value" {
        return base_var.to_owned();
    }
    let mentions_value = contains_ident(raw, "value");
    let rewritten = rewrite_neoflash_expr_tokens(raw, base_var, expr.seed);
    let mut out = String::new();
    let _ = write!(
        out,
        "(() => {{ const amp = {:.6}; const freq = {:.6}; const decay = {:.6}; const phase = {:.6}; \
         const fps = 1.0 / thisComp.frameDuration; const t = Math.max(0, time - inPoint); \
         const frame = timeToFrames(t, fps); const pi = Math.PI; const tau = Math.PI * 2; \
         const lerp = (a, b, u) => a + (b - a) * u; \
         const pingpong = (x) => 1 - Math.abs((((x) % 2) + 2) % 2 - 1); \
         const saw = (x) => (((x) % 1) + 1) % 1; \
         const square = (x) => ((((x) % 1) + 1) % 1) < 0.5 ? 1 : -1; \
         const triangle = (x) => 1 - 4 * Math.abs(((((x) % 1) + 1) % 1) - 0.5); \
         const bounce = (x) => Math.abs(Math.sin(x * Math.PI)) * Math.exp(-decay * Math.max(0, x)); \
         const elastic = (x) => (x <= 0 ? 0 : Math.sin(x * tau * freq) * Math.exp(-decay * x)); \
         const noise = (x) => {{ seedRandom({}, true); return (wiggle(1, 1, 1, 0.5, x) - {base_var}); }}; \
         const _res = ({rewritten}); return {}; }})()",
        expr.amplitude,
        expr.frequency,
        expr.decay,
        expr.phase,
        expr.seed,
        if mentions_value {
            "_res".to_owned()
        } else {
            format!("{base_var} + _res")
        }
    );
    out
}

fn contains_ident(src: &str, needle: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if &src[start..i] == needle {
                return true;
            }
        } else {
            i += 1;
        }
    }
    false
}

fn rewrite_neoflash_expr_tokens(src: &str, base_var: &str, seed: u32) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() + 32);
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            // Peek next non-whitespace char to distinguish function calls from variables
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let is_call = j < chars.len() && chars[j] == '(';
            let lower = ident.to_ascii_lowercase();
            if is_call {
                match lower.as_str() {
                    "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "atan2" | "abs" | "min"
                    | "max" | "floor" | "ceil" | "round" | "sqrt" | "pow" | "exp" => {
                        out.push_str("Math.");
                        out.push_str(&lower);
                    }
                    "wiggle" => {
                        // In AE, `wiggle(freq, amp)` returns `value + delta`. Return delta when
                        // combined with `value + wiggle(...)` or when `mentions_value` is false.
                        let _ = write!(
                            out,
                            "((f, a) => {{ seedRandom({seed}, true); const w = wiggle(f, a); return Array.isArray(w) ? (w[0] - {base_var}) : (w - {base_var}); }})"
                        );
                    }
                    _ => out.push_str(&ident),
                }
            } else {
                match lower.as_str() {
                    "value" => out.push_str(base_var),
                    "time" => out.push('t'),
                    _ => out.push_str(&ident),
                }
            }
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

#[derive(Clone, Debug, Default)]
struct ScriptPropertyDrivers {
    dx_per_frame: f64,
    dy_per_frame: f64,
    x_formula: Option<String>,
    y_formula: Option<String>,
    dsx_pct_per_frame: f64,
    dsy_pct_per_frame: f64,
    sx_formula: Option<String>,
    sy_formula: Option<String>,
    drot_per_frame: f64,
    rot_formula: Option<String>,
    dalpha_pct_per_frame: f64,
    alpha_formula: Option<String>,
    text_formula: Option<String>,
}

impl ScriptPropertyDrivers {
    fn is_empty(&self) -> bool {
        self.dx_per_frame.abs() <= 1e-9
            && self.dy_per_frame.abs() <= 1e-9
            && self.x_formula.is_none()
            && self.y_formula.is_none()
            && self.dsx_pct_per_frame.abs() <= 1e-9
            && self.dsy_pct_per_frame.abs() <= 1e-9
            && self.sx_formula.is_none()
            && self.sy_formula.is_none()
            && self.drot_per_frame.abs() <= 1e-9
            && self.rot_formula.is_none()
            && self.dalpha_pct_per_frame.abs() <= 1e-9
            && self.alpha_formula.is_none()
            && self.text_formula.is_none()
    }

    fn apply_to_expressions(&self, exprs: &mut AeLayerExpressions, origin: [f64; 2]) {
        if self.dx_per_frame.abs() > 1e-9
            || self.dy_per_frame.abs() > 1e-9
            || self.x_formula.is_some()
            || self.y_formula.is_some()
        {
            let x_part = if let Some(fx) = &self.x_formula {
                format!("({fx}) + {:.4}", origin[0])
            } else if self.dx_per_frame.abs() > 1e-9 {
                format!("value[0] + _f * ({:.6})", self.dx_per_frame)
            } else {
                "value[0]".to_owned()
            };
            let y_part = if let Some(fy) = &self.y_formula {
                format!("({fy}) + {:.4}", origin[1])
            } else if self.dy_per_frame.abs() > 1e-9 {
                format!("value[1] + _f * ({:.6})", self.dy_per_frame)
            } else {
                "value[1]".to_owned()
            };
            exprs.position = Some(format!(
                "(() => {{ const _t = Math.max(0, time - inPoint); const _fps = 1.0 / thisComp.frameDuration; const _f = timeToFrames(_t, _fps); return [{x_part}, {y_part}]; }})()"
            ));
        }
        if self.dsx_pct_per_frame.abs() > 1e-9
            || self.dsy_pct_per_frame.abs() > 1e-9
            || self.sx_formula.is_some()
            || self.sy_formula.is_some()
        {
            let sx_part = if let Some(fsx) = &self.sx_formula {
                fsx.clone()
            } else if self.dsx_pct_per_frame.abs() > 1e-9 {
                format!("value[0] + _f * ({:.6})", self.dsx_pct_per_frame)
            } else {
                "value[0]".to_owned()
            };
            let sy_part = if let Some(fsy) = &self.sy_formula {
                fsy.clone()
            } else if self.dsy_pct_per_frame.abs() > 1e-9 {
                format!("value[1] + _f * ({:.6})", self.dsy_pct_per_frame)
            } else {
                "value[1]".to_owned()
            };
            exprs.scale = Some(format!(
                "(() => {{ const _t = Math.max(0, time - inPoint); const _fps = 1.0 / thisComp.frameDuration; const _f = timeToFrames(_t, _fps); return [{sx_part}, {sy_part}]; }})()"
            ));
        }
        if let Some(frot) = &self.rot_formula {
            exprs.rotation = Some(format!(
                "(() => {{ const _t = Math.max(0, time - inPoint); const _fps = 1.0 / thisComp.frameDuration; const _f = timeToFrames(_t, _fps); return ({frot}); }})()"
            ));
        } else if self.drot_per_frame.abs() > 1e-9 {
            exprs.rotation = Some(format!(
                "value + timeToFrames(Math.max(0, time - inPoint), 1.0 / thisComp.frameDuration) * ({:.6})",
                self.drot_per_frame
            ));
        }
        if let Some(fa) = &self.alpha_formula {
            exprs.opacity = Some(format!(
                "(() => {{ const _t = Math.max(0, time - inPoint); const _fps = 1.0 / thisComp.frameDuration; const _f = timeToFrames(_t, _fps); return clamp(({fa}), 0, 100); }})()"
            ));
        } else if self.dalpha_pct_per_frame.abs() > 1e-9 {
            exprs.opacity = Some(format!(
                "clamp(value + timeToFrames(Math.max(0, time - inPoint), 1.0 / thisComp.frameDuration) * ({:.6}), 0, 100)",
                self.dalpha_pct_per_frame
            ));
        }
        if let Some(ft) = &self.text_formula {
            exprs.source_text = Some(format!(
                "(() => {{ const _t = Math.max(0, time - inPoint); const _fps = 1.0 / thisComp.frameDuration; const _f = timeToFrames(_t, _fps); return String({ft}); }})()"
            ));
        }
    }
}

#[derive(Clone, Debug)]
enum TimelineFlowRule {
    Stop { at_frame: u32 },
    GotoAndPlay { at_frame: u32, target_frame: u32 },
    GotoAndStop { at_frame: u32, target_frame: u32 },
}

/// Analyzes timeline frame scripts (`Timeline.scripts`) and instance scripts (`Element.scripts`)
/// to extract After Effects property expressions (`Position`, `Scale`, `Rotation`, `Opacity`,
/// `Source Text`) and `Time Remap` timeline transport expressions (`stop`, `gotoAndPlay`, `gotoAndStop`).
fn analyze_symbol_scripts_for_ae(
    symbol: &Symbol,
    label_map: &BTreeMap<String, u32>,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> (
    BTreeMap<String, ScriptPropertyDrivers>,
    Option<String>,
    Vec<AeMarker>,
) {
    let mut named_drivers: BTreeMap<String, ScriptPropertyDrivers> = BTreeMap::new();
    let mut flow_rules: Vec<TimelineFlowRule> = Vec::new();
    let mut comp_markers: Vec<AeMarker> = Vec::new();

    for fs in &symbol.timeline.scripts {
        let src = fs.script.source.trim();
        if src.is_empty() {
            continue;
        }
        let loc = format!("symbol '{}' frame {}", symbol.name, fs.frame + 1);
        let mut translated_any = false;
        let mut untranslated_lines = Vec::new();

        for stmt in split_script_statements(src) {
            if let Some(rule) = parse_timeline_flow_stmt(&stmt, fs.frame, label_map) {
                flow_rules.push(rule);
                translated_any = true;
            } else if let Some((target, prop, op, rhs)) = parse_property_assignment(&stmt) {
                if !target.is_empty() && target != "this" && target != "_root" {
                    let drv = named_drivers.entry(target.clone()).or_default();
                    if apply_parsed_property_driver(drv, &prop, &op, &rhs, fs.script.language) {
                        translated_any = true;
                    } else {
                        untranslated_lines.push(stmt);
                    }
                } else {
                    untranslated_lines.push(stmt);
                }
            } else if stmt.contains("onEnterFrame") || stmt.contains("ENTER_FRAME") {
                if let Some(body) = extract_brace_body(&stmt) {
                    let mut body_ok = false;
                    for inner in split_script_statements(&body) {
                        if let Some((target, prop, op, rhs)) = parse_property_assignment(&inner)
                            && !target.is_empty()
                            && target != "this"
                            && target != "_root"
                        {
                            let drv = named_drivers.entry(target).or_default();
                            if apply_parsed_property_driver(
                                drv,
                                &prop,
                                &op,
                                &rhs,
                                fs.script.language,
                            ) {
                                body_ok = true;
                            }
                        }
                    }
                    if body_ok {
                        translated_any = true;
                    } else {
                        untranslated_lines.push(stmt);
                    }
                } else {
                    untranslated_lines.push(stmt);
                }
            } else {
                untranslated_lines.push(stmt);
            }
        }

        comp_markers.push(AeMarker {
            frame: fs.frame,
            duration_frames: 0,
            label: format!("Action f{}", fs.frame + 1),
            comment: src.to_owned(),
        });

        if translated_any {
            diagnostics.push(AepDiagnostic {
                code: "AE_SCRIPT_TRANSLATED",
                kind: AepDiagnosticKind::TranslatedExpression,
                location: loc.clone(),
                message: "Translated timeline script actions into After Effects JavaScript Expressions and Comp markers.".into(),
            });
        }
        if !untranslated_lines.is_empty() {
            diagnostics.push(AepDiagnostic {
                code: "AE_SCRIPT_MARKER_ONLY",
                kind: AepDiagnosticKind::MarkerPreserved,
                location: loc,
                message: format!(
                    "Preserved non-declarative script statements in Composition Marker comment ({} statement(s)).",
                    untranslated_lines.len()
                ),
            });
        }
    }

    let time_remap_expr = compile_flow_rules_to_time_remap_expr(&flow_rules);
    (named_drivers, time_remap_expr, comp_markers)
}

fn analyze_element_scripts_for_ae(
    symbol_name: &str,
    frame: u32,
    el: &Element,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> (ScriptPropertyDrivers, Vec<AeMarker>) {
    let mut drivers = ScriptPropertyDrivers::default();
    let mut markers = Vec::new();
    for os in &el.scripts {
        let src = os.script.source.trim();
        if src.is_empty() {
            continue;
        }
        let loc = format!(
            "symbol '{}' frame {} element '{}'",
            symbol_name,
            frame + 1,
            if el.name.is_empty() { &el.id } else { &el.name }
        );
        markers.push(AeMarker {
            frame,
            duration_frames: 0,
            label: format!("{:?} script", os.event),
            comment: src.to_owned(),
        });

        if matches!(os.event, ObjectEvent::EnterFrame | ObjectEvent::Load) {
            let body = extract_enter_frame_or_plain_body(src);
            let mut any_ok = false;
            for stmt in split_script_statements(&body) {
                if let Some((target, prop, op, rhs)) = parse_property_assignment(&stmt)
                    && (target.is_empty() || target == "this")
                    && apply_parsed_property_driver(
                        &mut drivers,
                        &prop,
                        &op,
                        &rhs,
                        os.script.language,
                    )
                {
                    any_ok = true;
                }
            }
            if any_ok {
                diagnostics.push(AepDiagnostic {
                    code: "AE_INSTANCE_EXPR_TRANSLATED",
                    kind: AepDiagnosticKind::TranslatedExpression,
                    location: loc,
                    message: "Translated instance script into After Effects layer JavaScript Expression(s).".into(),
                });
            } else {
                diagnostics.push(AepDiagnostic {
                    code: "AE_INSTANCE_SCRIPT_MARKER",
                    kind: AepDiagnosticKind::MarkerPreserved,
                    location: loc,
                    message: "Preserved instance script on After Effects Layer Marker.".into(),
                });
            }
        } else {
            diagnostics.push(AepDiagnostic {
                code: "AE_INTERACTIVE_HANDLER_MARKER",
                kind: AepDiagnosticKind::MarkerPreserved,
                location: loc,
                message: "Interactive click/button handler preserved on After Effects Layer Marker (AE playback is non-interactive).".into(),
            });
        }
    }
    (drivers, markers)
}

fn compile_flow_rules_to_time_remap_expr(rules: &[TimelineFlowRule]) -> Option<String> {
    if rules.is_empty() {
        return None;
    }
    // Emit a deterministic AE JS expression for `ADBE Time Remapping`
    let mut branches = String::new();
    for rule in rules {
        match *rule {
            TimelineFlowRule::Stop { at_frame } => {
                let _ = write!(branches, "if (f >= {at_frame}) return {at_frame} / fps; ");
            }
            TimelineFlowRule::GotoAndStop {
                at_frame,
                target_frame,
            } => {
                let _ = write!(
                    branches,
                    "if (f >= {at_frame}) return {target_frame} / fps; "
                );
            }
            TimelineFlowRule::GotoAndPlay {
                at_frame,
                target_frame,
            } => {
                if target_frame < at_frame {
                    let span = (at_frame - target_frame).max(1);
                    let _ = write!(
                        branches,
                        "if (f >= {at_frame}) return ({target_frame} + ((f - {target_frame}) % {span})) / fps; "
                    );
                } else {
                    let _ = write!(
                        branches,
                        "if (f >= {at_frame}) return ({target_frame} + (f - {at_frame})) / fps; "
                    );
                }
            }
        }
    }
    Some(format!(
        "(() => {{ const fps = 1.0 / thisComp.frameDuration; const f = timeToFrames(Math.max(0, time - inPoint), fps); {branches}return Math.max(0, time - inPoint); }})()"
    ))
}

fn extract_enter_frame_or_plain_body(src: &str) -> String {
    let trimmed = src.trim();
    if (trimmed.starts_with("onClipEvent")
        || trimmed.contains("onEnterFrame")
        || trimmed.contains("ENTER_FRAME"))
        && let Some(body) = extract_brace_body(trimmed)
    {
        return body;
    }
    trimmed.to_owned()
}

fn extract_brace_body(src: &str) -> Option<String> {
    let open = src.find('{')?;
    let close = src.rfind('}')?;
    (close > open).then(|| src[open + 1..close].trim().to_owned())
}

fn split_script_statements(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_str: Option<char> = None;
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if let Some(q) = in_str {
            cur.push(ch);
            if ch == '\\' && i + 1 < chars.len() {
                i += 1;
                cur.push(chars[i]);
            } else if ch == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        // Skip `//` line comments
        if ch == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                in_str = Some(ch);
                cur.push(ch);
            }
            '{' | '(' | '[' => {
                depth += 1;
                cur.push(ch);
            }
            '}' | ')' | ']' => {
                depth = (depth - 1).max(0);
                cur.push(ch);
            }
            ';' | '\n' if depth == 0 => {
                let s = cur.trim();
                if !s.is_empty() {
                    out.push(s.to_owned());
                }
                cur.clear();
            }
            _ => cur.push(ch),
        }
        i += 1;
    }
    let tail = cur.trim();
    if !tail.is_empty() {
        out.push(tail.to_owned());
    }
    out
}

fn parse_timeline_flow_stmt(
    stmt: &str,
    at_frame: u32,
    label_map: &BTreeMap<String, u32>,
) -> Option<TimelineFlowRule> {
    let s = stmt
        .trim()
        .trim_start_matches("this.")
        .trim_start_matches("_root.")
        .trim();
    if s == "stop()" {
        return Some(TimelineFlowRule::Stop { at_frame });
    }
    for (prefix, is_play) in [("gotoAndPlay(", true), ("gotoAndStop(", false)] {
        if let Some(rest) = s.strip_prefix(prefix)
            && let Some(arg_raw) = rest.strip_suffix(')')
        {
            let arg = arg_raw.trim();
            let target = if let Some(lbl) = arg
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| arg.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            {
                *label_map.get(lbl)?
            } else if let Ok(one_based) = arg.parse::<u32>() {
                one_based.saturating_sub(1)
            } else {
                return None;
            };
            return Some(if is_play {
                TimelineFlowRule::GotoAndPlay {
                    at_frame,
                    target_frame: target,
                }
            } else {
                TimelineFlowRule::GotoAndStop {
                    at_frame,
                    target_frame: target,
                }
            });
        }
    }
    None
}

fn parse_property_assignment(stmt: &str) -> Option<(String, String, String, String)> {
    for op in ["+=", "-=", "="] {
        if let Some(idx) = stmt.find(op) {
            if op == "=" {
                let prev = idx
                    .checked_sub(1)
                    .and_then(|k| stmt.as_bytes().get(k))
                    .copied();
                let next = stmt.as_bytes().get(idx + 1).copied();
                if matches!(
                    prev,
                    Some(b'=' | b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/')
                ) || next == Some(b'=')
                {
                    continue;
                }
            }
            let lhs_raw = stmt[..idx].trim();
            let lhs = lhs_raw
                .strip_prefix("var ")
                .or_else(|| lhs_raw.strip_prefix("let "))
                .or_else(|| lhs_raw.strip_prefix("const "))
                .map(str::trim)
                .unwrap_or(lhs_raw);
            let rhs = stmt[idx + op.len()..].trim();
            if lhs.is_empty() || rhs.is_empty() {
                return None;
            }
            let (target, prop) = if let Some(dot) = lhs.rfind('.') {
                (
                    lhs[..dot].trim().to_owned(),
                    lhs[dot + 1..].trim().to_owned(),
                )
            } else {
                (String::new(), lhs.to_owned())
            };
            if prop.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Some((target, prop, op.to_owned(), rhs.to_owned()));
            }
        }
    }
    None
}

fn apply_parsed_property_driver(
    drv: &mut ScriptPropertyDrivers,
    prop: &str,
    op: &str,
    rhs: &str,
    lang: ScriptLanguage,
) -> bool {
    let is_as2 = matches!(
        lang,
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
    );
    let sign = if op == "-=" { -1.0 } else { 1.0 };

    if matches!(op, "+=" | "-=") {
        let Ok(num) = rhs.parse::<f64>() else {
            return false;
        };
        let delta = sign * num;
        match prop {
            "_x" | "x" => drv.dx_per_frame += delta,
            "_y" | "y" => drv.dy_per_frame += delta,
            "_xscale" => drv.dsx_pct_per_frame += delta,
            "scaleX" => drv.dsx_pct_per_frame += delta * 100.0,
            "_yscale" => drv.dsy_pct_per_frame += delta,
            "scaleY" => drv.dsy_pct_per_frame += delta * 100.0,
            "_rotation" | "rotation" => drv.drot_per_frame += delta,
            "_alpha" => drv.dalpha_pct_per_frame += delta,
            "alpha" => drv.dalpha_pct_per_frame += if is_as2 { delta } else { delta * 100.0 },
            _ => return false,
        }
        return true;
    }

    // Direct `=` formula assignment
    if !is_safe_ae_js_formula(rhs) {
        return false;
    }
    let rewritten = rewrite_script_rhs_for_ae(rhs);
    match prop {
        "_x" | "x" => drv.x_formula = Some(rewritten),
        "_y" | "y" => drv.y_formula = Some(rewritten),
        "_xscale" => drv.sx_formula = Some(rewritten),
        "scaleX" => drv.sx_formula = Some(format!("({rewritten}) * 100.0")),
        "_yscale" => drv.sy_formula = Some(rewritten),
        "scaleY" => drv.sy_formula = Some(format!("({rewritten}) * 100.0")),
        "_rotation" | "rotation" => drv.rot_formula = Some(rewritten),
        "_alpha" => drv.alpha_formula = Some(rewritten),
        "alpha" => {
            drv.alpha_formula = Some(if is_as2 {
                rewritten
            } else {
                format!("({rewritten}) * 100.0")
            });
        }
        "text" => drv.text_formula = Some(rewritten),
        _ => return false,
    }
    true
}

fn is_safe_ae_js_formula(rhs: &str) -> bool {
    if rhs.is_empty() || rhs.len() > 1024 {
        return false;
    }
    // Reject function definitions, loops, assignments or arbitrary host calls
    for forbidden in [
        "function",
        "=>",
        "while",
        "for(",
        "for (",
        "new ",
        "loadMovie",
        "URLRequest",
    ] {
        if rhs.contains(forbidden) {
            return false;
        }
    }
    true
}

fn rewrite_script_rhs_for_ae(rhs: &str) -> String {
    let mut s = rhs
        .replace("getTimer()", "(_t * 1000.0)")
        .replace("this._currentframe", "(_f + 1)")
        .replace("this.currentFrame", "(_f + 1)")
        .replace("Stage.width", "thisComp.width")
        .replace("Stage.height", "thisComp.height")
        .replace("stage.stageWidth", "thisComp.width")
        .replace("stage.stageHeight", "thisComp.height");

    // Identifier-boundary replacement for bare `_currentframe` / `currentFrame`
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 16);
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() || chars[i] == '_' {
            let prev_is_prop = i > 0 && chars[i - 1] == '.';
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            if !prev_is_prop && (ident == "_currentframe" || ident == "currentFrame") {
                out.push_str("(_f + 1)");
            } else {
                out.push_str(&ident);
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    s = out;
    s
}

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

fn collect_reachable_symbols(project: &Project, roots: &[String]) -> BTreeSet<String> {
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

fn compute_symbol_precomp_bounds(project: &Project, sym: &Symbol) -> ([f64; 2], [u32; 2]) {
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

fn local_element_bounds(project: &Project, el: &Element) -> (f64, f64, f64, f64) {
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

#[allow(clippy::too_many_arguments)]
fn build_ae_comp(
    project: &Project,
    sym: &Symbol,
    width: u32,
    height: u32,
    origin: [f64; 2],
    fps: f64,
    background: [f64; 3],
    folder: String,
    options: &AepOptions,
    comp_origins: &BTreeMap<String, ([f64; 2], [u32; 2])>,
    symbol_time_remap_exprs: &BTreeMap<String, String>,
    resource_to_footage: &BTreeMap<String, AeFootageItem>,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> AeComp {
    let duration_frames = sym.timeline.frame_count.max(1);
    let label_map = collect_timeline_labels(&sym.timeline);

    let (named_script_drivers, script_time_remap_expr, mut comp_markers) =
        if options.translate_scripts {
            analyze_symbol_scripts_for_ae(sym, &label_map, diagnostics)
        } else {
            (BTreeMap::new(), None, Vec::new())
        };

    // Frame labels & rich timeline markers -> AE Composition Markers
    for (lbl, frame) in &label_map {
        comp_markers.push(AeMarker {
            frame: *frame,
            duration_frames: 0,
            label: lbl.clone(),
            comment: "Frame label".into(),
        });
    }
    let mut work_area: Option<[u32; 2]> = None;
    for layer in &sym.timeline.layers {
        for kf in &layer.keyframes {
            if work_area.is_none()
                && let Some(wa) = &kf.work_area
                && wa.enabled
            {
                work_area = Some([wa.in_frame, wa.out_frame.max(wa.in_frame + 1)]);
            }
            for tm in &kf.markers {
                comp_markers.push(AeMarker {
                    frame: tm.frame,
                    duration_frames: tm.duration_frames,
                    label: tm.name.clone(),
                    comment: tm.comment.clone(),
                });
            }
        }
    }

    let guides: Vec<AeGuide> = sym
        .timeline
        .guides
        .iter()
        .map(|g| AeGuide {
            horizontal: g.axis == GuideAxis::Horizontal,
            position: g.position
                + if g.axis == GuideAxis::Horizontal {
                    origin[1]
                } else {
                    origin[0]
                },
        })
        .collect();

    // Map mask layers (`LayerRole::Mask`) so target layers (`layer.properties.mask == Some(mask_id)`)
    // get paired with an immediate After Effects Track Matte copy above them.
    let mask_layers: BTreeMap<String, &Layer> = sym
        .timeline
        .layers
        .iter()
        .filter(|l| l.properties.role == LayerRole::Mask)
        .map(|l| (l.id.clone(), l))
        .collect();

    let mut ae_layers: Vec<AeLayer> = Vec::new();
    let mut element_id_to_ae_layer_id: BTreeMap<String, String> = BTreeMap::new();
    let mut pending_parent_links: Vec<(String, String)> = Vec::new();
    let mut camera_keyframes: Vec<(u32, neoflash_core::CameraPose)> = Vec::new();

    for layer in &sym.timeline.layers {
        for kf in &layer.keyframes {
            if let Some(cam) = &kf.camera {
                camera_keyframes.push((kf.frame, *cam));
            }
            for link in &kf.element_parents {
                pending_parent_links.push((link.child_id.clone(), link.parent_id.clone()));
            }
        }

        if matches!(
            layer.properties.role,
            LayerRole::Guide | LayerRole::Folder | LayerRole::Mask
        ) {
            continue;
        }

        let layer_end = layer
            .properties
            .end_frame
            .unwrap_or(duration_frames)
            .min(duration_frames);
        if layer_end == 0 {
            continue;
        }

        // 1. Frame sounds on this layer -> AE Audio Footage layers
        for (k_idx, kf) in layer.keyframes.iter().enumerate() {
            if let Some(snd) = &kf.sound {
                if snd.sync == SoundSync::Stop {
                    continue;
                }
                if let Some(ft) = resource_to_footage.get(&snd.resource) {
                    let next_frame = layer
                        .keyframes
                        .get(k_idx + 1)
                        .map(|n| n.frame.min(layer_end))
                        .unwrap_or(layer_end);
                    let start_sec = snd.in_point.unwrap_or(0.0).max(0.0);
                    let audio_levels_db = build_sound_levels_keys(kf.frame, next_frame, snd);
                    ae_layers.push(AeLayer {
                        id: format!("snd-{}-{}", layer.id, kf.frame),
                        name: format!("{} · Audio", layer.name),
                        source_element_id: None,
                        in_frame: kf.frame,
                        out_frame: next_frame.max(kf.frame + 1),
                        visible: false,
                        locked: layer.locked,
                        solo: false,
                        is_track_matte_source: false,
                        track_matte: AeTrackMatte::None,
                        blend_mode: "NORMAL",
                        auto_orient_along_path: false,
                        three_d: false,
                        z_depth: 0.0,
                        parent_layer_id: None,
                        anchor_point: [0.0, 0.0],
                        position_keys: Vec::new(),
                        scale_keys: Vec::new(),
                        rotation_keys: Vec::new(),
                        opacity_keys: Vec::new(),
                        skew_keys: Vec::new(),
                        expressions: AeLayerExpressions::default(),
                        effects: Vec::new(),
                        markers: Vec::new(),
                        kind: AeLayerKind::Footage {
                            footage_id: ft.id.clone(),
                            is_audio_only: true,
                            is_video: false,
                            start_offset_sec: start_sec,
                            end_offset_sec: snd.out_point,
                            loop_count: if snd.loop_forever {
                                999
                            } else {
                                snd.loops.max(1)
                            },
                            muted: false,
                            audio_levels_db,
                        },
                    });
                }
            }
        }

        // 2. Shape Tween spans (`TweenKind::Shape`) -> Animated Bezier Shape Layer
        let mut handled_spans: BTreeSet<usize> = BTreeSet::new();
        for (k_idx, kf) in layer.keyframes.iter().enumerate() {
            if kf.tween.is_some() && kf.tween_kind == TweenKind::Shape {
                let end_frame = layer
                    .keyframes
                    .get(k_idx + 1)
                    .map(|n| n.frame.min(layer_end))
                    .unwrap_or(layer_end);
                if end_frame > kf.frame
                    && let Some(shape_layer) = build_shape_tween_ae_layer(
                        &sym.timeline,
                        layer,
                        kf,
                        end_frame,
                        origin,
                        fps as u32,
                        options.shape_morph_step.max(1),
                    )
                {
                    handled_spans.insert(k_idx);
                    push_layer_with_optional_track_matte(
                        &mut ae_layers,
                        shape_layer,
                        layer,
                        &mask_layers,
                        project,
                        &sym.timeline,
                        origin,
                        fps as u32,
                    );
                }
            }
        }

        // 3. Collect visual element tracks across non-shape-tween spans on this layer
        let tracks = collect_layer_element_tracks(
            project,
            sym,
            layer,
            layer_end,
            &handled_spans,
            origin,
            fps,
            options,
            &named_script_drivers,
            comp_origins,
            symbol_time_remap_exprs,
            resource_to_footage,
            diagnostics,
        );
        for built in tracks {
            if let Some(eid) = &built.source_element_id {
                element_id_to_ae_layer_id.insert(eid.clone(), built.id.clone());
            }
            push_layer_with_optional_track_matte(
                &mut ae_layers,
                built,
                layer,
                &mask_layers,
                project,
                &sym.timeline,
                origin,
                fps as u32,
            );
        }
    }

    // Wire element parenting (`layer.parent = parentLayer`)
    for (child_eid, parent_eid) in pending_parent_links {
        if let (Some(child_lid), Some(parent_lid)) = (
            element_id_to_ae_layer_id.get(&child_eid).cloned(),
            element_id_to_ae_layer_id.get(&parent_eid).cloned(),
        ) && child_lid != parent_lid
            && let Some(layer) = ae_layers.iter_mut().find(|l| l.id == child_lid)
        {
            layer.parent_layer_id = Some(parent_lid);
        }
    }

    // Optional Timeline Camera -> AE CameraLayer at the top of the composition
    if !camera_keyframes.is_empty() {
        camera_keyframes.sort_by_key(|(f, _)| *f);
        let first_focal = camera_keyframes[0].1.focal_length.max(50.0);
        let cx = f64::from(width) * 0.5;
        let cy = f64::from(height) * 0.5;
        let mut pos_keys = Vec::new();
        let mut rot_keys = Vec::new();
        let mut zoom_keys = Vec::new();
        for (frame, pose) in &camera_keyframes {
            pos_keys.push(AeKey {
                frame: *frame,
                value: [cx + pose.x, cy + pose.y],
                ease: AeEase::default(),
            });
            rot_keys.push(AeKey {
                frame: *frame,
                value: pose.rotation,
                ease: AeEase::default(),
            });
            zoom_keys.push(AeKey {
                frame: *frame,
                value: first_focal * pose.zoom.max(0.05),
                ease: AeEase::default(),
            });
        }
        ae_layers.push(AeLayer {
            id: format!("cam-{}", sym.id),
            name: "NeoFlash Camera".into(),
            source_element_id: None,
            in_frame: 0,
            out_frame: duration_frames,
            visible: true,
            locked: false,
            solo: false,
            is_track_matte_source: false,
            track_matte: AeTrackMatte::None,
            blend_mode: "NORMAL",
            auto_orient_along_path: false,
            three_d: true,
            z_depth: -first_focal,
            parent_layer_id: None,
            anchor_point: [cx, cy],
            position_keys: pos_keys,
            scale_keys: Vec::new(),
            rotation_keys: rot_keys,
            opacity_keys: Vec::new(),
            skew_keys: Vec::new(),
            expressions: AeLayerExpressions::default(),
            effects: Vec::new(),
            markers: Vec::new(),
            kind: AeLayerKind::Camera {
                focal_length: first_focal,
                zoom_keys,
            },
        });
    }

    AeComp {
        id: sym.id.clone(),
        name: if sym.name.trim().is_empty() {
            sym.id.clone()
        } else {
            sym.name.clone()
        },
        folder,
        width,
        height,
        fps,
        duration_frames,
        background,
        origin_offset: origin,
        work_area,
        guides,
        markers: comp_markers,
        layers: ae_layers,
        script_time_remap_expr,
    }
}

#[allow(clippy::too_many_arguments)]
fn push_layer_with_optional_track_matte(
    ae_layers: &mut Vec<AeLayer>,
    mut target_layer: AeLayer,
    authored_layer: &Layer,
    mask_layers: &BTreeMap<String, &Layer>,
    project: &Project,
    timeline: &Timeline,
    origin: [f64; 2],
    fps: u32,
) {
    if let Some(mask_id) = &authored_layer.properties.mask
        && let Some(mask_src) = mask_layers.get(mask_id).copied()
        && let Some(mut matte_layer) = build_mask_matte_layer(
            project,
            timeline,
            mask_src,
            target_layer.in_frame,
            target_layer.out_frame,
            origin,
            fps,
        )
    {
        let matte_mode = match (
            mask_src.properties.mask_mode,
            mask_src.properties.mask_invert,
        ) {
            (MaskMode::Luminance, false) => AeTrackMatte::Luma,
            (MaskMode::Luminance, true) => AeTrackMatte::LumaInverted,
            (_, false) => AeTrackMatte::Alpha,
            (_, true) => AeTrackMatte::AlphaInverted,
        };
        target_layer.track_matte = matte_mode;
        matte_layer.id = format!("matte-for-{}", target_layer.id);
        matte_layer.is_track_matte_source = true;
        matte_layer.visible = false;
        // In bottom-to-top order, `target_layer` is below `matte_layer` (so in AE's 1..N top-down
        // stack, `matte_layer` sits directly above `target_layer`, as required by AE Track Matte!).
        ae_layers.push(target_layer);
        ae_layers.push(matte_layer);
        return;
    }
    ae_layers.push(target_layer);
}

fn build_mask_matte_layer(
    project: &Project,
    timeline: &Timeline,
    mask_layer: &Layer,
    in_frame: u32,
    out_frame: u32,
    origin: [f64; 2],
    fps: u32,
) -> Option<AeLayer> {
    let sampled = sample_layer_in_at_fps(timeline, mask_layer, in_frame, f64::from(fps));
    let mut groups = Vec::new();
    for (idx, el) in sampled.iter().enumerate() {
        if let Some(grp) = build_static_shape_group_for_element(project, el, idx, true) {
            // Shift mask contour into element's local transform at `in_frame`
            groups.push(grp);
        }
    }
    if groups.is_empty() {
        return None;
    }
    let first_t = sampled.first().map(|e| e.transform).unwrap_or_default();
    Some(AeLayer {
        id: format!("mask-{}", mask_layer.id),
        name: format!("{} (Track Matte)", mask_layer.name),
        source_element_id: None,
        in_frame,
        out_frame: out_frame.max(in_frame + 1),
        visible: false,
        locked: true,
        solo: false,
        is_track_matte_source: true,
        track_matte: AeTrackMatte::None,
        blend_mode: "NORMAL",
        auto_orient_along_path: false,
        three_d: false,
        z_depth: 0.0,
        parent_layer_id: None,
        anchor_point: [0.0, 0.0],
        position_keys: vec![AeKey {
            frame: in_frame,
            value: [first_t.x + origin[0], first_t.y + origin[1]],
            ease: AeEase::default(),
        }],
        scale_keys: vec![AeKey {
            frame: in_frame,
            value: [first_t.scale_x * 100.0, first_t.scale_y * 100.0],
            ease: AeEase::default(),
        }],
        rotation_keys: vec![AeKey {
            frame: in_frame,
            value: first_t.rotation,
            ease: AeEase::default(),
        }],
        opacity_keys: vec![AeKey {
            frame: in_frame,
            value: (first_t.alpha * 100.0).clamp(0.0, 100.0),
            ease: AeEase::default(),
        }],
        skew_keys: Vec::new(),
        expressions: AeLayerExpressions::default(),
        effects: Vec::new(),
        markers: Vec::new(),
        kind: AeLayerKind::Shape { groups },
    })
}

fn build_shape_tween_ae_layer(
    timeline: &Timeline,
    layer: &Layer,
    start_kf: &Keyframe,
    end_frame: u32,
    origin: [f64; 2],
    fps: u32,
    morph_step: u32,
) -> Option<AeLayer> {
    let mut frames: Vec<u32> = (start_kf.frame..end_frame)
        .step_by(morph_step as usize)
        .collect();
    if frames.last().copied() != Some(end_frame.saturating_sub(1)) {
        frames.push(end_frame.saturating_sub(1));
    }

    let first_sample = sample_layer_in_at_fps(timeline, layer, start_kf.frame, f64::from(fps));
    let first_el = first_sample.first()?;
    let (_, fill_rule, fill, stroke, fill_grad, _, stroke_width) =
        element_vector_commands(first_el)?;

    let mut contour_tracks: Vec<Vec<AeKey<AeBezierContour>>> = Vec::new();
    for &f in &frames {
        let sampled = sample_layer_in_at_fps(timeline, layer, f, f64::from(fps));
        let mut all_contours = Vec::new();
        for el in &sampled {
            if let Some((cmds, _, _, _, _, _, _)) = element_vector_commands(el) {
                let c = path_commands_to_ae_contours(
                    &cmds,
                    [el.transform.x + origin[0], el.transform.y + origin[1]],
                );
                all_contours.extend(c);
            }
        }
        for (c_idx, contour) in all_contours.into_iter().enumerate() {
            if contour_tracks.len() <= c_idx {
                contour_tracks.push(Vec::new());
            }
            contour_tracks[c_idx].push(AeKey {
                frame: f,
                value: contour,
                ease: AeEase::default(),
            });
        }
    }
    if contour_tracks.is_empty() {
        return None;
    }

    let even_odd = fill_rule == FillRule::EvenOdd;
    let fill_paint = build_ae_fill(fill, fill_grad.as_ref(), even_odd);
    let mut strokes = Vec::new();
    if stroke_width > 0.0 && stroke[3] > 0 {
        strokes.push(AePaintStroke {
            color: color_rgb01(stroke),
            opacity: (f64::from(stroke[3]) / 255.0) * 100.0,
            width: stroke_width,
            dashes: None,
        });
    }

    Some(AeLayer {
        id: format!("morph-{}-{}", layer.id, start_kf.frame),
        name: format!("{} · Shape Morph", layer.name),
        source_element_id: Some(first_el.id.clone()),
        in_frame: start_kf.frame,
        out_frame: end_frame,
        visible: layer.visible,
        locked: layer.locked,
        solo: false,
        is_track_matte_source: false,
        track_matte: AeTrackMatte::None,
        blend_mode: ae_blend_mode(layer.properties.blend_mode),
        auto_orient_along_path: false,
        three_d: layer.properties.z_depth.abs() > 1e-6,
        z_depth: layer.properties.z_depth,
        parent_layer_id: None,
        anchor_point: [0.0, 0.0],
        position_keys: vec![AeKey {
            frame: start_kf.frame,
            value: [0.0, 0.0],
            ease: AeEase::default(),
        }],
        scale_keys: vec![AeKey {
            frame: start_kf.frame,
            value: [100.0, 100.0],
            ease: AeEase::default(),
        }],
        rotation_keys: vec![AeKey {
            frame: start_kf.frame,
            value: 0.0,
            ease: AeEase::default(),
        }],
        opacity_keys: vec![AeKey {
            frame: start_kf.frame,
            value: 100.0,
            ease: AeEase::default(),
        }],
        skew_keys: Vec::new(),
        expressions: AeLayerExpressions::default(),
        effects: convert_filters_to_ae(&layer.properties.filters, ColorTransform::default()),
        markers: Vec::new(),
        kind: AeLayerKind::Shape {
            groups: vec![AeShapeGroup {
                name: "Morph Group".into(),
                contour_tracks,
                fills: if matches!(fill_paint, AePaintFill::None) {
                    Vec::new()
                } else {
                    vec![fill_paint]
                },
                strokes,
                modifiers: Vec::new(),
            }],
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_layer_element_tracks(
    project: &Project,
    sym: &Symbol,
    layer: &Layer,
    layer_end: u32,
    handled_spans: &BTreeSet<usize>,
    origin: [f64; 2],
    fps: f64,
    options: &AepOptions,
    named_script_drivers: &BTreeMap<String, ScriptPropertyDrivers>,
    comp_origins: &BTreeMap<String, ([f64; 2], [u32; 2])>,
    symbol_time_remap_exprs: &BTreeMap<String, String>,
    resource_to_footage: &BTreeMap<String, AeFootageItem>,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> Vec<AeLayer> {
    let mut ordered_ids: Vec<String> = Vec::new();
    let mut by_id: BTreeMap<String, Vec<SpanSample>> = BTreeMap::new();

    for (k_idx, kf) in layer.keyframes.iter().enumerate() {
        if handled_spans.contains(&k_idx) || kf.frame >= layer_end {
            continue;
        }
        let span_end = layer
            .keyframes
            .get(k_idx + 1)
            .map(|n| n.frame.min(layer_end))
            .unwrap_or(layer_end);
        if span_end <= kf.frame {
            continue;
        }

        let has_guide = layer.properties.motion_guide.is_some() || kf.motion_path.is_some();
        for el in &kf.elements {
            if !by_id.contains_key(&el.id) {
                ordered_ids.push(el.id.clone());
            }
            let appearance = kf
                .appearances
                .iter()
                .find(|a| a.element_id == el.id)
                .cloned();
            let is_controller = kf.controller_elements.iter().any(|cid| cid == &el.id);
            by_id.entry(el.id.clone()).or_default().push(SpanSample {
                frame: kf.frame,
                end_frame: span_end,
                element: el.clone(),
                easing: kf.tween,
                orient_to_path: kf.motion_orient_to_path_for(&el.id),
                curved_motion: has_guide
                    || !matches!(
                        kf.motion_path_for(&el.id),
                        neoflash_core::MotionPathSource::None
                    ),
                rotate_turns: kf.classic_rotate_turns,
                property_expressions: kf.property_expressions.clone(),
                video_span: kf.video.clone(),
                is_controller,
                appearance,
                kf_filters: kf.filters.clone(),
            });
        }
    }

    let mut out_layers = Vec::new();
    for eid in ordered_ids {
        let Some(samples) = by_id.remove(&eid) else {
            continue;
        };
        // Split into contiguous active intervals `[in_frame, out_frame)`
        let mut run_start = 0usize;
        while run_start < samples.len() {
            let mut run_end = run_start + 1;
            while run_end < samples.len()
                && samples[run_end].frame == samples[run_end - 1].end_frame
            {
                run_end += 1;
            }
            let slice = &samples[run_start..run_end];
            if let Some(ae_layer) = build_ae_layer_from_run(
                project,
                sym,
                layer,
                slice,
                origin,
                fps,
                options,
                named_script_drivers,
                comp_origins,
                symbol_time_remap_exprs,
                resource_to_footage,
                diagnostics,
            ) {
                out_layers.push(ae_layer);
            }
            run_start = run_end;
        }
    }
    out_layers
}

struct SpanSample {
    frame: u32,
    end_frame: u32,
    element: Element,
    easing: Option<Easing>,
    orient_to_path: bool,
    curved_motion: bool,
    rotate_turns: u32,
    property_expressions: Vec<PropertyExpression>,
    video_span: Option<neoflash_core::TimelineVideoSpan>,
    is_controller: bool,
    appearance: Option<neoflash_core::ElementAppearance>,
    kf_filters: Vec<FilterSpec>,
}

#[allow(clippy::too_many_arguments)]
fn build_ae_layer_from_run(
    project: &Project,
    sym: &Symbol,
    layer: &Layer,
    run: &[SpanSample],
    origin: [f64; 2],
    fps: f64,
    options: &AepOptions,
    named_script_drivers: &BTreeMap<String, ScriptPropertyDrivers>,
    comp_origins: &BTreeMap<String, ([f64; 2], [u32; 2])>,
    symbol_time_remap_exprs: &BTreeMap<String, String>,
    resource_to_footage: &BTreeMap<String, AeFootageItem>,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> Option<AeLayer> {
    let first = &run[0];
    let last = &run[run.len() - 1];
    let in_frame = first.frame;
    let out_frame = last.end_frame.max(in_frame + 1);
    let el = &first.element;
    let pivot = el.transform.pivot.unwrap_or([0.0, 0.0]);

    // Determine pre-comp origin if this element is an Instance
    let precomp_origin = if let ElementKind::Instance { symbol_id, .. } = &el.kind {
        comp_origins
            .get(symbol_id)
            .map(|(o, _)| *o)
            .unwrap_or([0.0, 0.0])
    } else {
        [0.0, 0.0]
    };

    let anchor_point = [precomp_origin[0] + pivot[0], precomp_origin[1] + pivot[1]];

    let mut pos_keys = Vec::new();
    let mut scale_keys = Vec::new();
    let mut rot_keys = Vec::new();
    let mut opacity_keys = Vec::new();
    let mut skew_keys = Vec::new();
    let mut auto_orient = false;
    let mut accum_turns_deg = 0.0_f64;

    for (idx, item) in run.iter().enumerate() {
        auto_orient |= item.orient_to_path;
        let ease = easing_to_ae(item.easing);
        let next_frame = run.get(idx + 1).map(|n| n.frame);

        let needs_dense_sample =
            item.curved_motion || matches!(item.easing, Some(Easing::Bounce | Easing::Elastic));
        if needs_dense_sample
            && item.easing.is_some()
            && let Some(nf) = next_frame
            && nf > item.frame + 1
        {
            for sf in item.frame..nf {
                let sampled = sample_layer_in_at_fps(&sym.timeline, layer, sf, fps);
                if let Some(sel) = sampled.iter().find(|c| c.id == el.id) {
                    push_transform_keys(
                        sf,
                        sel.transform,
                        origin,
                        pivot,
                        accum_turns_deg,
                        AeEase::default(),
                        &mut pos_keys,
                        &mut scale_keys,
                        &mut rot_keys,
                        &mut opacity_keys,
                        &mut skew_keys,
                    );
                }
            }
        } else {
            push_transform_keys(
                item.frame,
                item.element.transform,
                origin,
                pivot,
                accum_turns_deg,
                ease,
                &mut pos_keys,
                &mut scale_keys,
                &mut rot_keys,
                &mut opacity_keys,
                &mut skew_keys,
            );
            if idx + 1 == run.len() && item.easing.is_some() && out_frame > item.frame + 1 {
                let end_f = out_frame - 1;
                let sampled = sample_layer_in_at_fps(&sym.timeline, layer, end_f, fps);
                if let Some(sel) = sampled.iter().find(|c| c.id == el.id) {
                    push_transform_keys(
                        end_f,
                        sel.transform,
                        origin,
                        pivot,
                        accum_turns_deg + f64::from(item.rotate_turns) * 360.0,
                        AeEase::default(),
                        &mut pos_keys,
                        &mut scale_keys,
                        &mut rot_keys,
                        &mut opacity_keys,
                        &mut skew_keys,
                    );
                }
            }
        }
        accum_turns_deg += f64::from(item.rotate_turns) * 360.0;
    }

    let mut expressions = AeLayerExpressions::default();
    let mut markers = Vec::new();

    if options.translate_scripts {
        let mut px_expr: Option<String> = None;
        let mut py_expr: Option<String> = None;
        let mut sx_expr: Option<String> = None;
        let mut sy_expr: Option<String> = None;

        for pe in &first.property_expressions {
            if !pe.enabled {
                continue;
            }
            match pe.property {
                TweenProperty::X => {
                    px_expr = Some(compile_property_expression_to_ae_js(pe, "value[0]"))
                }
                TweenProperty::Y => {
                    py_expr = Some(compile_property_expression_to_ae_js(pe, "value[1]"))
                }
                TweenProperty::ScaleX => {
                    sx_expr = Some(compile_property_expression_to_ae_js(pe, "value[0]"));
                }
                TweenProperty::ScaleY => {
                    sy_expr = Some(compile_property_expression_to_ae_js(pe, "value[1]"));
                }
                TweenProperty::Rotation => {
                    expressions.rotation = Some(compile_property_expression_to_ae_js(pe, "value"));
                }
                TweenProperty::Skew => {
                    expressions.skew = Some(compile_property_expression_to_ae_js(pe, "value"));
                }
                TweenProperty::Alpha => {
                    let compiled = compile_property_expression_to_ae_js(pe, "(value / 100.0)");
                    expressions.opacity = Some(format!("clamp(({compiled}) * 100.0, 0, 100)"));
                }
                TweenProperty::Color => {}
            }
        }
        if px_expr.is_some() || py_expr.is_some() {
            expressions.position = Some(format!(
                "[{}, {}]",
                px_expr.as_deref().unwrap_or("value[0]"),
                py_expr.as_deref().unwrap_or("value[1]")
            ));
        }
        if sx_expr.is_some() || sy_expr.is_some() {
            expressions.scale = Some(format!(
                "[{}, {}]",
                sx_expr.as_deref().unwrap_or("value[0]"),
                sy_expr.as_deref().unwrap_or("value[1]")
            ));
        }

        let (inst_drivers, inst_markers) =
            analyze_element_scripts_for_ae(&sym.name, in_frame, el, diagnostics);
        markers.extend(inst_markers);
        if !inst_drivers.is_empty() {
            inst_drivers.apply_to_expressions(&mut expressions, origin);
        }

        if !el.name.is_empty()
            && let Some(named_drv) = named_script_drivers.get(&el.name)
            && !named_drv.is_empty()
        {
            named_drv.apply_to_expressions(&mut expressions, origin);
        }
    }

    let kind = if first.is_controller {
        AeLayerKind::Null
    } else {
        match &el.kind {
            ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. } => {
                let mut grp = build_static_shape_group_for_element(project, el, 0, false)?;
                if let Some(app) = &first.appearance {
                    apply_element_appearance_to_group(&mut grp, app);
                }
                AeLayerKind::Shape { groups: vec![grp] }
            }
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } => AeLayerKind::Text {
                text: text.clone(),
                font: if font_family.trim().is_empty() {
                    "ArialMT".into()
                } else {
                    font_family.clone()
                },
                font_size: font_size.max(1.0),
                color: color_rgb01(*color),
                bold: false,
                italic: false,
                align: TextAlign::Left,
                leading: font_size * 1.2,
                tracking: 0.0,
                box_size: None,
            },
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                ..
            } => {
                let c = [
                    ((format.color >> 16) & 0xFF) as u8,
                    ((format.color >> 8) & 0xFF) as u8,
                    (format.color & 0xFF) as u8,
                    255,
                ];
                AeLayerKind::Text {
                    text: text.clone(),
                    font: if format.font.trim().is_empty() {
                        "ArialMT".into()
                    } else {
                        format.font.clone()
                    },
                    font_size: format.size.max(1.0),
                    color: color_rgb01(c),
                    bold: format.bold,
                    italic: format.italic,
                    align: format.align,
                    leading: (format.size + format.leading).max(1.0),
                    tracking: format.letter_spacing * 20.0,
                    box_size: Some([width.max(16.0), height.max(16.0)]),
                }
            }
            ElementKind::Bitmap { resource_id, .. } => {
                let ft = resource_to_footage.get(resource_id)?;
                AeLayerKind::Footage {
                    footage_id: ft.id.clone(),
                    is_audio_only: false,
                    is_video: false,
                    start_offset_sec: 0.0,
                    end_offset_sec: None,
                    loop_count: 1,
                    muted: true,
                    audio_levels_db: Vec::new(),
                }
            }
            ElementKind::Video { .. } => {
                let vs = first.video_span.as_ref()?;
                let ft_opt = resource_to_footage.get(&vs.resource).or_else(|| {
                    vs.external_path
                        .as_ref()
                        .and_then(|ext| resource_to_footage.get(&format!("__ext_video__:{ext}")))
                });
                let Some(ft) = ft_opt else {
                    diagnostics.push(AepDiagnostic {
                        code: "AE_EXTERNAL_VIDEO_UNRESOLVED",
                        kind: AepDiagnosticKind::Skipped,
                        location: format!("symbol '{}' layer '{}'", sym.name, layer.name),
                        message: format!(
                            "Skipped video layer because external/embedded video '{}' could not be resolved into Footage.",
                            vs.external_path.as_deref().unwrap_or(vs.resource.as_str())
                        ),
                    });
                    return None;
                };
                AeLayerKind::Footage {
                    footage_id: ft.id.clone(),
                    is_audio_only: false,
                    is_video: true,
                    start_offset_sec: f64::from(vs.start_offset_ms) / 1000.0,
                    end_offset_sec: vs.end_offset_ms.map(|ms| f64::from(ms) / 1000.0),
                    loop_count: if vs.loop_video { 999 } else { 1 },
                    muted: vs.muted,
                    audio_levels_db: Vec::new(),
                }
            }
            ElementKind::Instance {
                symbol_id,
                playback,
            } => {
                let child_frames = project
                    .symbol(symbol_id)
                    .map(|s| s.timeline.frame_count.max(1))
                    .unwrap_or(1);
                let (tr_keys, tr_expr) = build_instance_time_remap(
                    *playback,
                    layer.properties.sync_timeline,
                    in_frame,
                    out_frame,
                    child_frames,
                    fps,
                    options
                        .translate_scripts
                        .then(|| symbol_time_remap_exprs.get(symbol_id).cloned())
                        .flatten(),
                );
                if expressions.time_remap.is_none() {
                    expressions.time_remap = tr_expr;
                }
                AeLayerKind::PreComp {
                    comp_id: symbol_id.clone(),
                    comp_origin: precomp_origin,
                    time_remap_keys: tr_keys,
                }
            }
        }
    };

    let effective_blend = if el.blend_mode == BlendMode::Normal {
        layer.properties.blend_mode
    } else {
        el.blend_mode
    };
    let mut all_filters = layer.properties.filters.clone();
    all_filters.extend(first.kf_filters.iter().cloned());
    all_filters.extend(el.filters.iter().cloned());
    let effects = convert_filters_to_ae(&all_filters, el.transform.color_transform);

    let display_name = if !el.name.trim().is_empty() {
        format!("{} · {}", layer.name, el.name)
    } else {
        layer.name.clone()
    };

    Some(AeLayer {
        id: format!("lyr-{}-{}-{}", layer.id, el.id, in_frame),
        name: display_name,
        source_element_id: Some(el.id.clone()),
        in_frame,
        out_frame,
        visible: layer.visible && el.visible,
        locked: layer.locked,
        solo: false,
        is_track_matte_source: false,
        track_matte: AeTrackMatte::None,
        blend_mode: ae_blend_mode(effective_blend),
        auto_orient_along_path: auto_orient,
        three_d: layer.properties.z_depth.abs() > 1e-6,
        z_depth: layer.properties.z_depth,
        parent_layer_id: None,
        anchor_point,
        position_keys: dedup_constant_keys(pos_keys),
        scale_keys: dedup_constant_keys(scale_keys),
        rotation_keys: dedup_constant_keys(rot_keys),
        opacity_keys: dedup_constant_keys(opacity_keys),
        skew_keys: dedup_constant_zero_keys(skew_keys),
        expressions,
        effects,
        markers,
        kind,
    })
}

// ============================================================================
// Shape, Paint, Filter, Time-Remap & Footage Builders
// ============================================================================

fn build_static_shape_group_for_element(
    _project: &Project,
    el: &Element,
    idx: usize,
    force_opaque_matte: bool,
) -> Option<AeShapeGroup> {
    let (cmds, fill_rule, mut fill, stroke, fill_grad, _stroke_grad, stroke_width) =
        element_vector_commands(el)?;
    if force_opaque_matte && fill[3] == 0 && fill_grad.is_none() {
        fill = [255, 255, 255, 255];
    }
    let contours = path_commands_to_ae_contours(&cmds, [0.0, 0.0]);
    if contours.is_empty() {
        return None;
    }
    let contour_tracks: Vec<Vec<AeKey<AeBezierContour>>> = contours
        .into_iter()
        .map(|c| {
            vec![AeKey {
                frame: 0,
                value: c,
                ease: AeEase::default(),
            }]
        })
        .collect();

    let even_odd = fill_rule == FillRule::EvenOdd;
    let mut fills = Vec::new();
    let main_fill = build_ae_fill(fill, fill_grad.as_ref(), even_odd);
    if !matches!(main_fill, AePaintFill::None) {
        fills.push(main_fill);
    }

    let mut strokes = Vec::new();
    if stroke_width > 0.0 && stroke[3] > 0 {
        strokes.push(AePaintStroke {
            color: color_rgb01(stroke),
            opacity: (f64::from(stroke[3]) / 255.0) * 100.0,
            width: stroke_width,
            dashes: None,
        });
    }

    Some(AeShapeGroup {
        name: if el.name.trim().is_empty() {
            format!("Vector Shape {}", idx + 1)
        } else {
            el.name.clone()
        },
        contour_tracks,
        fills,
        strokes,
        modifiers: Vec::new(),
    })
}

fn build_ae_fill(fill: Color, gradient: Option<&GradientPaint>, even_odd: bool) -> AePaintFill {
    if let Some(grad) = gradient
        && !grad.stops.is_empty()
    {
        let [a, b, c, d, tx, ty] = grad.matrix;
        // Canonical Flash gradient box spans `-819.2..+819.2`.
        let start = [a * -819.2 + tx, b * -819.2 + ty];
        let end = match grad.kind {
            GradientKind::Linear => [a * 819.2 + tx, b * 819.2 + ty],
            GradientKind::Radial => {
                let rx = (a * 819.2).hypot(b * 819.2);
                let ry = (c * 819.2).hypot(d * 819.2);
                let r = rx.max(ry).max(1.0);
                [tx + r, ty]
            }
        };
        let stops: Vec<(f64, [f64; 4])> = grad
            .stops
            .iter()
            .map(|s| {
                let pos = (f64::from(s.ratio) / 255.0).clamp(0.0, 1.0);
                let rgb = color_rgb01(s.color);
                let alpha = (f64::from(s.color[3]) / 255.0).clamp(0.0, 1.0);
                (pos, [rgb[0], rgb[1], rgb[2], alpha])
            })
            .collect();
        return AePaintFill::Gradient {
            radial: grad.kind == GradientKind::Radial,
            start: if grad.kind == GradientKind::Radial {
                [tx, ty]
            } else {
                start
            },
            end,
            highlight_length: (grad.focal_point_ratio * 100.0).clamp(-99.0, 99.0),
            stops,
            opacity: 100.0,
            even_odd,
        };
    }

    if fill[3] == 0 {
        return AePaintFill::None;
    }
    AePaintFill::Solid {
        color: color_rgb01(fill),
        opacity: (f64::from(fill[3]) / 255.0) * 100.0,
        even_odd,
    }
}

fn apply_element_appearance_to_group(
    grp: &mut AeShapeGroup,
    app: &neoflash_core::ElementAppearance,
) {
    if !app.entries.is_empty() {
        grp.fills.clear();
        grp.strokes.clear();
        for entry in &app.entries {
            match entry {
                AppearancePaintEntry::Fill {
                    color,
                    gradient,
                    opacity,
                    ..
                } => {
                    let mut f = build_ae_fill(*color, gradient.as_ref(), false);
                    match &mut f {
                        AePaintFill::Solid { opacity: op, .. }
                        | AePaintFill::Gradient { opacity: op, .. } => {
                            *op = (*op * opacity.clamp(0.0, 1.0)).clamp(0.0, 100.0);
                        }
                        AePaintFill::None => {}
                    }
                    if !matches!(f, AePaintFill::None) {
                        grp.fills.push(f);
                    }
                }
                AppearancePaintEntry::Stroke {
                    color,
                    width,
                    pattern_brush,
                    opacity,
                    ..
                } => {
                    if *width > 0.0 && color[3] > 0 {
                        let dashes = match pattern_brush {
                            Some(neoflash_core::PatternBrushKind::DashPattern { dash, gap }) => {
                                Some([dash.max(0.5), gap.max(0.5)])
                            }
                            _ => None,
                        };
                        grp.strokes.push(AePaintStroke {
                            color: color_rgb01(*color),
                            opacity: ((f64::from(color[3]) / 255.0)
                                * opacity.clamp(0.0, 1.0)
                                * 100.0)
                                .clamp(0.0, 100.0),
                            width: *width,
                            dashes,
                        });
                    }
                }
            }
        }
    }

    for modifier in &app.modifiers {
        match *modifier {
            VectorModifier::Repeater {
                copies,
                offset_x,
                offset_y,
                scale_step,
                rotation_step,
            } => {
                grp.modifiers.push(AeVectorModifier::Repeater {
                    copies: copies.max(1),
                    offset_x,
                    offset_y,
                    scale_pct: (scale_step * 100.0).clamp(1.0, 1000.0),
                    rotation: rotation_step,
                });
            }
            VectorModifier::OffsetContour { delta } => {
                grp.modifiers
                    .push(AeVectorModifier::OffsetPaths { amount: delta });
            }
            VectorModifier::LiveCorners { radius } => {
                grp.modifiers.push(AeVectorModifier::RoundCorners {
                    radius: radius.max(0.0),
                });
            }
            VectorModifier::Mirror { .. } => {}
        }
    }
}

fn convert_filters_to_ae(filters: &[FilterSpec], ct: ColorTransform) -> Vec<AeEffect> {
    let mut out = Vec::new();
    for f in filters {
        match f {
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                ..
            } => {
                out.push(AeEffect::DropShadow {
                    color: u32_rgb01(*color),
                    opacity_pct: (alpha * 100.0).clamp(0.0, 100.0),
                    direction_deg: (*angle + 90.0).rem_euclid(360.0),
                    distance: distance.max(0.0),
                    softness: ((blur_x + blur_y) * 0.5).max(0.0),
                });
            }
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                ..
            } => {
                out.push(AeEffect::Glow {
                    color: u32_rgb01(*color),
                    radius: ((blur_x + blur_y) * 0.5).max(0.0),
                    intensity: (strength * alpha).clamp(0.0, 10.0),
                });
            }
            FilterSpec::Blur { blur_x, blur_y, .. } => {
                let dims = if (blur_x - blur_y).abs() <= 0.5 {
                    1 // Horizontal and Vertical
                } else if blur_x > blur_y {
                    2 // Horizontal
                } else {
                    3 // Vertical
                };
                out.push(AeEffect::GaussianBlur {
                    blurriness: blur_x.max(*blur_y).max(0.0),
                    dimensions: dims,
                });
            }
            FilterSpec::Bevel {
                distance,
                angle,
                strength,
                ..
            } => {
                out.push(AeEffect::BevelAlpha {
                    edge_thickness: distance.max(1.0),
                    light_angle: *angle,
                    light_intensity: strength.clamp(0.0, 2.0),
                });
            }
            FilterSpec::GradientGlow {
                colors,
                blur_x,
                blur_y,
                strength,
                ..
            } => {
                let col = colors.first().copied().unwrap_or(0xFFFFFF);
                out.push(AeEffect::Glow {
                    color: u32_rgb01(col),
                    radius: ((blur_x + blur_y) * 0.5).max(0.0),
                    intensity: strength.clamp(0.0, 10.0),
                });
            }
            FilterSpec::GradientBevel {
                distance,
                angle,
                strength,
                ..
            } => {
                out.push(AeEffect::BevelAlpha {
                    edge_thickness: distance.max(1.0),
                    light_angle: *angle,
                    light_intensity: strength.clamp(0.0, 2.0),
                });
            }
            FilterSpec::ColorMatrix { .. } | FilterSpec::Convolution { .. } => {}
        }
    }

    // Non-identity RGB tint -> AE Tint effect
    if !ct.is_identity() {
        let tint_strength = (1.0
            - (ct.multipliers[0] + ct.multipliers[1] + ct.multipliers[2]) / 3.0)
            .clamp(0.0, 1.0);
        if tint_strength > 0.02
            || ct.offsets[0].abs() > 2.0
            || ct.offsets[1].abs() > 2.0
            || ct.offsets[2].abs() > 2.0
        {
            let col = [
                (ct.offsets[0] / 255.0).clamp(0.0, 1.0),
                (ct.offsets[1] / 255.0).clamp(0.0, 1.0),
                (ct.offsets[2] / 255.0).clamp(0.0, 1.0),
            ];
            out.push(AeEffect::Tint {
                color: col,
                amount_pct: (tint_strength * 100.0).clamp(5.0, 100.0),
            });
        }
    }
    out
}

fn build_instance_time_remap(
    playback: InstancePlayback,
    sync_timeline: bool,
    in_frame: u32,
    out_frame: u32,
    child_frames: u32,
    fps: f64,
    script_expr: Option<String>,
) -> (Vec<AeKey<f64>>, Option<String>) {
    let cf = child_frames.max(1);
    if cf <= 1 && script_expr.is_none() {
        return (Vec::new(), None);
    }
    match playback {
        InstancePlayback::SingleFrame(f) => {
            let t = f64::from(f.min(cf - 1)) / fps;
            let ease = AeEase {
                hold: true,
                ..AeEase::default()
            };
            (
                vec![AeKey {
                    frame: in_frame,
                    value: t,
                    ease,
                }],
                None,
            )
        }
        InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_) => {
            let start_f = match playback {
                InstancePlayback::PlayOnceFrom(s) => s.min(cf - 1),
                _ => 0,
            };
            let max_t = f64::from(cf - 1) / fps;
            let start_t = f64::from(start_f) / fps;
            let expr = format!("Math.min({max_t:.6}, {start_t:.6} + Math.max(0, time - inPoint))");
            (
                vec![AeKey {
                    frame: in_frame,
                    value: start_t,
                    ease: AeEase::default(),
                }],
                Some(expr),
            )
        }
        InstancePlayback::Loop | InstancePlayback::LoopFrom(_) => {
            let start_f = match playback {
                InstancePlayback::LoopFrom(s) => s % cf,
                _ => 0,
            };
            let base_ref = if sync_timeline {
                "time"
            } else {
                "(time - inPoint)"
            };
            let expr = format!(
                "(() => {{ const fps = 1.0 / thisComp.frameDuration; const f = ({start_f} + Math.max(0, timeToFrames({base_ref}, fps))) % {cf}; return f / fps; }})()"
            );
            (
                vec![AeKey {
                    frame: in_frame,
                    value: f64::from(start_f) / fps,
                    ease: AeEase::default(),
                }],
                Some(expr),
            )
        }
        InstancePlayback::Independent => {
            if let Some(se) = script_expr {
                return (
                    vec![AeKey {
                        frame: in_frame,
                        value: 0.0,
                        ease: AeEase::default(),
                    }],
                    Some(se),
                );
            }
            if out_frame - in_frame > cf && cf > 1 {
                // Independent MovieClips loop continuously in Flash unless stopped by script
                let expr = format!(
                    "(() => {{ const fps = 1.0 / thisComp.frameDuration; const f = Math.max(0, timeToFrames(time - inPoint, fps)) % {cf}; return f / fps; }})()"
                );
                (
                    vec![AeKey {
                        frame: in_frame,
                        value: 0.0,
                        ease: AeEase::default(),
                    }],
                    Some(expr),
                )
            } else {
                (Vec::new(), None)
            }
        }
    }
}

fn build_sound_levels_keys(
    start_frame: u32,
    end_frame: u32,
    snd: &neoflash_core::FrameSound,
) -> Vec<AeKey<[f64; 2]>> {
    let amp_to_db = |amp: f64| -> f64 {
        if amp <= 0.001 {
            -96.0
        } else {
            (20.0 * amp.log10()).clamp(-96.0, 12.0)
        }
    };
    let end_f = end_frame.max(start_frame + 1);
    if let Some(env) = &snd.envelope
        && !env.is_empty()
    {
        let span = f64::from(end_f - start_frame);
        return env
            .iter()
            .map(|&[pos, l, r]| AeKey {
                frame: start_frame + (pos.clamp(0.0, 1.0) * span).round() as u32,
                value: [amp_to_db(l), amp_to_db(r)],
                ease: AeEase::default(),
            })
            .collect();
    }
    match snd.effect {
        SoundEffect::None | SoundEffect::Custom => Vec::new(),
        SoundEffect::LeftChannel => vec![AeKey {
            frame: start_frame,
            value: [0.0, -96.0],
            ease: AeEase::default(),
        }],
        SoundEffect::RightChannel => vec![AeKey {
            frame: start_frame,
            value: [-96.0, 0.0],
            ease: AeEase::default(),
        }],
        SoundEffect::FadeIn => vec![
            AeKey {
                frame: start_frame,
                value: [-48.0, -48.0],
                ease: AeEase::default(),
            },
            AeKey {
                frame: end_f,
                value: [0.0, 0.0],
                ease: AeEase::default(),
            },
        ],
        SoundEffect::FadeOut => vec![
            AeKey {
                frame: start_frame,
                value: [0.0, 0.0],
                ease: AeEase::default(),
            },
            AeKey {
                frame: end_f,
                value: [-48.0, -48.0],
                ease: AeEase::default(),
            },
        ],
        SoundEffect::FadeLeftToRight => vec![
            AeKey {
                frame: start_frame,
                value: [0.0, -48.0],
                ease: AeEase::default(),
            },
            AeKey {
                frame: end_f,
                value: [-48.0, 0.0],
                ease: AeEase::default(),
            },
        ],
        SoundEffect::FadeRightToLeft => vec![
            AeKey {
                frame: start_frame,
                value: [-48.0, 0.0],
                ease: AeEase::default(),
            },
            AeKey {
                frame: end_f,
                value: [0.0, -48.0],
                ease: AeEase::default(),
            },
        ],
    }
}

#[allow(clippy::too_many_arguments)]
fn push_transform_keys(
    frame: u32,
    t: Transform,
    origin: [f64; 2],
    pivot: [f64; 2],
    extra_rot_deg: f64,
    ease: AeEase,
    pos_keys: &mut Vec<AeKey<[f64; 2]>>,
    scale_keys: &mut Vec<AeKey<[f64; 2]>>,
    rot_keys: &mut Vec<AeKey<f64>>,
    opacity_keys: &mut Vec<AeKey<f64>>,
    skew_keys: &mut Vec<AeKey<f64>>,
) {
    // Account for custom pivot when Anchor Point is shifted by `pivot`
    let rot = finite_or(t.rotation, 0.0);
    let skew = finite_or(t.skew, 0.0);
    let sx = finite_or(t.scale_x, 1.0);
    let sy_scale = finite_or(t.scale_y, 1.0);
    let (s, c) = rot.to_radians().sin_cos();
    let (sy, cy) = (rot + skew).to_radians().sin_cos();
    let px = finite_or(
        finite_or(t.x, 0.0) + origin[0] + c * sx * pivot[0] - sy * sy_scale * pivot[1],
        0.0,
    );
    let py = finite_or(
        finite_or(t.y, 0.0) + origin[1] + s * sx * pivot[0] + cy * sy_scale * pivot[1],
        0.0,
    );

    pos_keys.push(AeKey {
        frame,
        value: [px, py],
        ease,
    });
    scale_keys.push(AeKey {
        frame,
        value: [
            finite_or(sx * 100.0, 100.0),
            finite_or(sy_scale * 100.0, 100.0),
        ],
        ease,
    });
    rot_keys.push(AeKey {
        frame,
        value: finite_or(rot + extra_rot_deg, 0.0),
        ease,
    });
    let alpha_mul = finite_or(t.color_transform.multipliers[3], 1.0);
    let alpha_off = finite_or(t.color_transform.offsets[3], 0.0) / 255.0;
    let op = finite_or(
        ((finite_or(t.alpha, 1.0) * alpha_mul + alpha_off) * 100.0).clamp(0.0, 100.0),
        100.0,
    );
    opacity_keys.push(AeKey {
        frame,
        value: op,
        ease,
    });
    skew_keys.push(AeKey {
        frame,
        value: skew,
        ease,
    });
}

fn dedup_constant_keys<T: PartialEq>(keys: Vec<AeKey<T>>) -> Vec<AeKey<T>> {
    if keys.len() <= 1 {
        return keys;
    }
    let first_val = &keys[0].value;
    if keys.iter().all(|k| &k.value == first_val) {
        vec![keys.into_iter().next().unwrap()]
    } else {
        keys
    }
}

fn dedup_constant_zero_keys(keys: Vec<AeKey<f64>>) -> Vec<AeKey<f64>> {
    if keys.iter().all(|k| k.value.abs() <= 1e-6) {
        Vec::new()
    } else {
        dedup_constant_keys(keys)
    }
}

fn easing_to_ae(ease: Option<Easing>) -> AeEase {
    let Some(ease) = ease else {
        return AeEase {
            in_speed: 0.0,
            in_influence: 16.6667,
            out_speed: 0.0,
            out_influence: 16.6667,
            hold: true,
        };
    };
    match ease {
        Easing::Linear => AeEase::default(),
        Easing::EaseIn => AeEase {
            in_speed: 0.0,
            in_influence: 33.33,
            out_speed: 0.0,
            out_influence: 75.0,
            hold: false,
        },
        Easing::EaseOut => AeEase {
            in_speed: 0.0,
            in_influence: 75.0,
            out_speed: 0.0,
            out_influence: 33.33,
            hold: false,
        },
        Easing::EaseInOut => AeEase {
            in_speed: 0.0,
            in_influence: 66.67,
            out_speed: 0.0,
            out_influence: 66.67,
            hold: false,
        },
        Easing::Flash { acceleration } => {
            let amt = (acceleration / 100.0).clamp(-1.0, 1.0);
            if amt < 0.0 {
                // Ease In
                AeEase {
                    in_speed: 0.0,
                    in_influence: 20.0,
                    out_speed: 0.0,
                    out_influence: (16.67 + (-amt) * 75.0).clamp(16.67, 95.0),
                    hold: false,
                }
            } else {
                // Ease Out
                AeEase {
                    in_speed: 0.0,
                    in_influence: (16.67 + amt * 75.0).clamp(16.67, 95.0),
                    out_speed: 0.0,
                    out_influence: 20.0,
                    hold: false,
                }
            }
        }
        Easing::CubicBezier { x1, x2, .. } => AeEase {
            out_influence: (x1.clamp(0.05, 0.99) * 100.0),
            in_influence: ((1.0 - x2.clamp(0.01, 0.95)) * 100.0),
            in_speed: 0.0,
            out_speed: 0.0,
            hold: false,
        },
        Easing::Bounce | Easing::Elastic => AeEase::default(),
    }
}

fn ae_blend_mode(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal | BlendMode::Layer => "NORMAL",
        BlendMode::Multiply => "MULTIPLY",
        BlendMode::Screen => "SCREEN",
        BlendMode::Lighten => "LIGHTEN",
        BlendMode::Darken => "DARKEN",
        BlendMode::Difference | BlendMode::Invert => "DIFFERENCE",
        BlendMode::Add => "ADD",
        BlendMode::Subtract => "SUBTRACT",
        BlendMode::Alpha => "STENCIL_ALPHA",
        BlendMode::Erase => "SILHOUETTE_ALPHA",
        BlendMode::Overlay => "OVERLAY",
        BlendMode::Hardlight => "HARD_LIGHT",
    }
}

fn color_rgb01(c: Color) -> [f64; 3] {
    [
        f64::from(c[0]) / 255.0,
        f64::from(c[1]) / 255.0,
        f64::from(c[2]) / 255.0,
    ]
}

fn u32_rgb01(rgb: u32) -> [f64; 3] {
    [
        f64::from((rgb >> 16) & 0xFF) / 255.0,
        f64::from((rgb >> 8) & 0xFF) / 255.0,
        f64::from(rgb & 0xFF) / 255.0,
    ]
}

fn collect_timeline_labels(timeline: &Timeline) -> BTreeMap<String, u32> {
    let mut map = BTreeMap::new();
    for layer in &timeline.layers {
        for kf in &layer.keyframes {
            let lbl = kf.label.trim();
            if !lbl.is_empty() && !map.contains_key(lbl) {
                map.insert(lbl.to_owned(), kf.frame);
            }
        }
    }
    map
}

type ExtractedAepFootage = (
    Vec<AeFootageItem>,
    Vec<PathBuf>,
    BTreeMap<String, AeFootageItem>,
);

fn extract_aep_footage(
    project: &Project,
    footage_dir: &Path,
    footage_dir_name: &str,
    base_dir: Option<&Path>,
    diagnostics: &mut Vec<AepDiagnostic>,
) -> Result<ExtractedAepFootage, ExportError> {
    let mut items = Vec::new();
    let mut written_files = Vec::new();
    let mut map = BTreeMap::new();
    let mut used_filenames_lower = BTreeSet::new();

    let alloc_safe_filename =
        |raw_stem_src: &str, ext: &str, used_lower: &mut BTreeSet<String>| -> (String, String) {
            let filtered: String = raw_stem_src
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let trimmed = filtered.trim_matches(|c| c == '_' || c == '-');
            let safe_stem = if trimmed.is_empty() {
                "asset".to_owned()
            } else {
                trimmed.to_owned()
            };
            let mut candidate = format!("{safe_stem}.{ext}");
            let mut seq = 2usize;
            while !used_lower.insert(candidate.to_ascii_lowercase()) {
                candidate = format!("{safe_stem}_{seq}.{ext}");
                seq += 1;
            }
            (safe_stem, candidate)
        };

    let mut ensured_dir = false;
    for (idx, res) in project.resources.iter().enumerate() {
        if res.data_base64.is_empty() {
            continue;
        }
        let Ok(mut bytes) = STANDARD.decode(res.data_base64.as_ref()) else {
            diagnostics.push(AepDiagnostic {
                code: "AE_INVALID_RESOURCE",
                kind: AepDiagnosticKind::Skipped,
                location: format!("resource '{}'", res.id),
                message: "Skipped resource with invalid base64 payload.".into(),
            });
            continue;
        };
        if bytes.is_empty() {
            continue;
        }

        let mut effective_media_type = res.media_type.clone();
        let explicit_webp = res
            .bitmap_export
            .is_some_and(|s| matches!(s.compression, neoflash_core::BitmapCompression::Webp));
        if !explicit_webp
            && effective_media_type.eq_ignore_ascii_case("image/webp")
            && let Ok(decoded_webp) = image::load_from_memory(&bytes)
        {
            let rgba = decoded_webp.to_rgba8();
            let mut png_buf = Vec::new();
            let enc = image::codecs::png::PngEncoder::new(&mut png_buf);
            if image::ImageEncoder::write_image(
                enc,
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
            )
            .is_ok()
            {
                bytes = png_buf;
                effective_media_type = "image/png".to_owned();
                diagnostics.push(AepDiagnostic {
                    code: "AE_WEBP_TRANSCODED_PNG",
                    kind: AepDiagnosticKind::Approximated,
                    location: format!("resource '{}'", res.id),
                    message: "Transcoded WebP resource to lossless PNG footage for universal After Effects import.".into(),
                });
            }
        }

        let ext = extension_for_media_type(&effective_media_type, &res.path);
        let raw_stem = Path::new(&res.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&res.id);
        let (safe_stem, file_name) = alloc_safe_filename(raw_stem, &ext, &mut used_filenames_lower);

        if !ensured_dir {
            fs::create_dir_all(footage_dir)?;
            ensured_dir = true;
        }
        let dest = footage_dir.join(&file_name);
        atomic_write(&dest, &bytes)?;
        written_files.push(dest);

        let (w, h) = if effective_media_type.starts_with("image/") {
            image::load_from_memory(&bytes)
                .ok()
                .map(|img| (Some(img.width()), Some(img.height())))
                .unwrap_or((None, None))
        } else {
            (None, None)
        };

        let ft_item = AeFootageItem {
            id: format!("ft_{idx}_{safe_stem}"),
            name: file_name.clone(),
            relative_path: format!("{footage_dir_name}/{file_name}"),
            media_type: effective_media_type,
            width: w,
            height: h,
        };
        map.insert(res.id.clone(), ft_item.clone());
        items.push(ft_item);
    }

    // Also stage any external timeline videos (`VideoDeploymentMode::External`) resolvable from `base_dir`.
    if let Some(base) = base_dir {
        let mut seen_ext = BTreeSet::new();
        for sym in &project.symbols {
            for layer in &sym.timeline.layers {
                for kf in &layer.keyframes {
                    if let Some(vs) = &kf.video
                        && let Some(ext_rel) = &vs.external_path
                        && neoflash_core::validate_external_video_path(ext_rel).is_ok()
                        && seen_ext.insert(ext_rel.clone())
                    {
                        let src_path = base.join(ext_rel);
                        if let Ok(bytes) = fs::read(&src_path)
                            && !bytes.is_empty()
                        {
                            let ext = extension_for_media_type("", ext_rel);
                            let raw_stem = Path::new(ext_rel)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("video");
                            let (safe_stem, file_name) =
                                alloc_safe_filename(raw_stem, &ext, &mut used_filenames_lower);
                            if !ensured_dir {
                                fs::create_dir_all(footage_dir)?;
                                ensured_dir = true;
                            }
                            let dest = footage_dir.join(&file_name);
                            atomic_write(&dest, &bytes)?;
                            written_files.push(dest);
                            let idx = items.len();
                            let ft_item = AeFootageItem {
                                id: format!("ft_ext_{idx}_{safe_stem}"),
                                name: file_name.clone(),
                                relative_path: format!("{footage_dir_name}/{file_name}"),
                                media_type: format!("video/{ext}"),
                                width: None,
                                height: None,
                            };
                            map.insert(format!("__ext_video__:{ext_rel}"), ft_item.clone());
                            if !vs.resource.is_empty() && !map.contains_key(&vs.resource) {
                                map.insert(vs.resource.clone(), ft_item.clone());
                            }
                            items.push(ft_item);
                        }
                    }
                }
            }
        }
    }

    Ok((items, written_files, map))
}

fn extension_for_media_type(media_type: &str, fallback_path: &str) -> String {
    match media_type.to_ascii_lowercase().as_str() {
        "image/png" => "png".to_owned(),
        "image/jpeg" | "image/jpg" => "jpg".to_owned(),
        "image/webp" => "webp".to_owned(),
        "image/gif" => "gif".to_owned(),
        "image/bmp" => "bmp".to_owned(),
        "image/tiff" => "tif".to_owned(),
        "audio/wav" | "audio/x-wav" => "wav".to_owned(),
        "audio/mpeg" | "audio/mp3" => "mp3".to_owned(),
        "audio/ogg" => "ogg".to_owned(),
        "video/mp4" => "mp4".to_owned(),
        "video/quicktime" => "mov".to_owned(),
        "video/webm" => "webm".to_owned(),
        "video/x-msvideo" | "video/avi" => "avi".to_owned(),
        _ => Path::new(fallback_path)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| {
                !e.is_empty() && e.len() <= 8 && e.chars().all(|c| c.is_ascii_alphanumeric())
            })
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_else(|| "bin".to_owned()),
    }
}

// ============================================================================
// 1. RIFX / Egg! Binary `.aep` Container Writer
// ============================================================================

fn sanitize_riff_marker_field(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '|' => '/',
            '\r' | '\n' => ' ',
            '\0' => ' ',
            _ => c,
        })
        .collect()
}

fn push_riff_chunk(buf: &mut Vec<u8>, fourcc: &[u8; 4], payload: &[u8]) -> Result<(), ExportError> {
    let len_u32 = u32::try_from(payload.len()).map_err(|_| {
        ExportError::Options("After Effects RIFX chunk exceeds the 32-bit container limit.".into())
    })?;
    buf.extend_from_slice(fourcc);
    buf.extend_from_slice(&len_u32.to_be_bytes());
    buf.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        buf.push(0);
    }
    Ok(())
}

fn push_riff_list(
    buf: &mut Vec<u8>,
    form_type: &[u8; 4],
    children: &[u8],
) -> Result<(), ExportError> {
    let total = children
        .len()
        .checked_add(4)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| {
            ExportError::Options(
                "After Effects RIFX LIST exceeds the 32-bit container limit.".into(),
            )
        })?;
    buf.extend_from_slice(b"LIST");
    buf.extend_from_slice(&total.to_be_bytes());
    buf.extend_from_slice(form_type);
    buf.extend_from_slice(children);
    if children.len() % 2 == 1 {
        buf.push(0);
    }
    Ok(())
}

fn encode_rifx_aep_container(
    model: &AeProjectModel,
    jsx_source: &str,
) -> Result<Vec<u8>, ExportError> {
    let mut egg_body = Vec::with_capacity(8192);

    // `head` chunk: AE version header (CC 2024 / 24.0 compatible interchange descriptor)
    let mut head = Vec::with_capacity(20);
    head.extend_from_slice(&0x0058_0000_u32.to_be_bytes());
    head.extend_from_slice(&24_u32.to_be_bytes());
    head.extend_from_slice(&(model.compositions.len() as u32).to_be_bytes());
    head.extend_from_slice(&(model.footage.len() as u32).to_be_bytes());
    head.extend_from_slice(&8_u32.to_be_bytes()); // 8 bpc
    push_riff_chunk(&mut egg_body, b"head", &head)?;

    let mut fold_body = Vec::new();
    let next_id = &mut 1_u32;

    // Footage `Item` chunks (`Pin ` -> `Als2` relative path)
    for ft in &model.footage {
        let mut item_body = Vec::new();
        let mut idta = Vec::with_capacity(16);
        idta.extend_from_slice(&2_u16.to_be_bytes()); // type = Footage
        idta.extend_from_slice(&(*next_id).to_be_bytes());
        *next_id += 1;
        idta.extend_from_slice(&ft.width.unwrap_or(0).to_be_bytes());
        idta.extend_from_slice(&ft.height.unwrap_or(0).to_be_bytes());
        push_riff_chunk(&mut item_body, b"idta", &idta)?;
        push_riff_chunk(&mut item_body, b"Utf8", ft.name.as_bytes())?;

        let mut pin_body = Vec::new();
        push_riff_chunk(&mut pin_body, b"Als2", ft.relative_path.as_bytes())?;
        push_riff_list(&mut item_body, b"Pin ", &pin_body)?;
        push_riff_list(&mut fold_body, b"Item", &item_body)?;
    }

    // Composition `Item` chunks (`Cdta` header + top-to-bottom `Layr` lists with `shap` & `expr`)
    for comp in &model.compositions {
        let mut item_body = Vec::new();
        let mut idta = Vec::with_capacity(16);
        idta.extend_from_slice(&1_u16.to_be_bytes()); // type = Composition
        idta.extend_from_slice(&(*next_id).to_be_bytes());
        *next_id += 1;
        idta.extend_from_slice(&comp.width.to_be_bytes());
        idta.extend_from_slice(&comp.height.to_be_bytes());
        push_riff_chunk(&mut item_body, b"idta", &idta)?;
        push_riff_chunk(&mut item_body, b"Utf8", comp.name.as_bytes())?;

        // `Cdta` composition metrics
        let mut cdta = Vec::with_capacity(48);
        cdta.extend_from_slice(&comp.width.to_be_bytes());
        cdta.extend_from_slice(&comp.height.to_be_bytes());
        cdta.extend_from_slice(&comp.fps.to_be_bytes());
        cdta.extend_from_slice(&comp.duration_frames.to_be_bytes());
        cdta.push((comp.background[0].clamp(0.0, 1.0) * 255.0).round() as u8);
        cdta.push((comp.background[1].clamp(0.0, 1.0) * 255.0).round() as u8);
        cdta.push((comp.background[2].clamp(0.0, 1.0) * 255.0).round() as u8);
        cdta.push(255);
        push_riff_chunk(&mut item_body, b"Cdta", &cdta)?;

        // Comp markers
        for m in &comp.markers {
            let payload = format!(
                "{}|{}|{}|{}",
                m.frame,
                m.duration_frames,
                sanitize_riff_marker_field(&m.label),
                sanitize_riff_marker_field(&m.comment)
            );
            push_riff_chunk(&mut item_body, b"mrkr", payload.as_bytes())?;
        }

        // Layers in AE top-to-bottom order (reverse of NeoFlash bottom-to-top)
        for layer in comp.layers.iter().rev() {
            let mut layr_body = Vec::new();
            let mut ldta = Vec::with_capacity(32);
            ldta.extend_from_slice(&layer.in_frame.to_be_bytes());
            ldta.extend_from_slice(&layer.out_frame.to_be_bytes());
            let mut flags = 0_u16;
            if layer.visible {
                flags |= 0x0001;
            }
            if layer.locked {
                flags |= 0x0002;
            }
            if layer.three_d {
                flags |= 0x0004;
            }
            if layer.auto_orient_along_path {
                flags |= 0x0008;
            }
            if layer.is_track_matte_source {
                flags |= 0x0010;
            }
            ldta.extend_from_slice(&flags.to_be_bytes());
            ldta.extend_from_slice(&(finite_or(layer.anchor_point[0], 0.0) as f32).to_be_bytes());
            ldta.extend_from_slice(&(finite_or(layer.anchor_point[1], 0.0) as f32).to_be_bytes());
            ldta.extend_from_slice(&(finite_or(layer.z_depth, 0.0) as f32).to_be_bytes());
            push_riff_chunk(&mut layr_body, b"ldta", &ldta)?;
            push_riff_chunk(&mut layr_body, b"Utf8", layer.name.as_bytes())?;

            // Transform keyframes (`tdgp`)
            let tdgp_json = serde_json::to_vec(&serde_json::json!({
                "position": layer.position_keys,
                "scale": layer.scale_keys,
                "rotation": layer.rotation_keys,
                "opacity": layer.opacity_keys,
                "skew": layer.skew_keys,
                "blend": layer.blend_mode,
                "matte": layer.track_matte,
            }))?;
            push_riff_chunk(&mut layr_body, b"tdgp", &tdgp_json)?;

            // Vector Bezier contours (`shap`: count + closed flag + IEEE-754 float32 vertices/in/out)
            if let AeLayerKind::Shape { groups } = &layer.kind {
                for grp in groups {
                    for track in &grp.contour_tracks {
                        for key in track {
                            let c = &key.value;
                            let mut shap = Vec::with_capacity(12 + c.vertices.len() * 24);
                            shap.extend_from_slice(&key.frame.to_be_bytes());
                            shap.extend_from_slice(&(c.vertices.len() as u32).to_be_bytes());
                            shap.push(u8::from(c.closed));
                            shap.extend_from_slice(&[0, 0, 0]);
                            for idx in 0..c.vertices.len() {
                                let v = c.vertices[idx];
                                let tin = c.in_tangents.get(idx).copied().unwrap_or([0.0, 0.0]);
                                let tout = c.out_tangents.get(idx).copied().unwrap_or([0.0, 0.0]);
                                for val in [v[0], v[1], tin[0], tin[1], tout[0], tout[1]] {
                                    shap.extend_from_slice(
                                        &(finite_or(val, 0.0) as f32).to_be_bytes(),
                                    );
                                }
                            }
                            push_riff_chunk(&mut layr_body, b"shap", &shap)?;
                        }
                    }
                }
            }

            // AE JavaScript Expressions (`expr` UTF-8 chunks)
            for (prop_name, expr_opt) in [
                ("ADBE Position", &layer.expressions.position),
                ("ADBE Scale", &layer.expressions.scale),
                ("ADBE Rotate Z", &layer.expressions.rotation),
                ("ADBE Opacity", &layer.expressions.opacity),
                ("ADBE Geometry2-0006", &layer.expressions.skew),
                ("ADBE Time Remapping", &layer.expressions.time_remap),
                ("ADBE Text Document", &layer.expressions.source_text),
            ] {
                if let Some(code) = expr_opt {
                    let payload = format!("{prop_name}\0{code}");
                    push_riff_chunk(&mut layr_body, b"expr", payload.as_bytes())?;
                }
            }

            for m in &layer.markers {
                let payload = format!(
                    "{}|{}|{}|{}",
                    m.frame,
                    m.duration_frames,
                    sanitize_riff_marker_field(&m.label),
                    sanitize_riff_marker_field(&m.comment)
                );
                push_riff_chunk(&mut layr_body, b"mrkr", payload.as_bytes())?;
            }

            push_riff_list(&mut item_body, b"Layr", &layr_body)?;
        }

        push_riff_list(&mut fold_body, b"Item", &item_body)?;
    }

    push_riff_list(&mut egg_body, b"Fold", &fold_body)?;

    // Embed full structured interchange model + ExtendScript builder inside `NEX1` & `ESCR` chunks
    let model_json = serde_json::to_vec(model)?;
    push_riff_chunk(&mut egg_body, b"NEX1", &model_json)?;
    push_riff_chunk(&mut egg_body, b"ESCR", jsx_source.as_bytes())?;

    let total_riff = egg_body
        .len()
        .checked_add(4)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| {
            ExportError::Options(
                "After Effects RIFX container exceeds the 32-bit length limit.".into(),
            )
        })?;
    let mut rifx = Vec::with_capacity(12 + egg_body.len());
    rifx.extend_from_slice(b"RIFX");
    rifx.extend_from_slice(&total_riff.to_be_bytes());
    rifx.extend_from_slice(b"Egg!");
    rifx.extend_from_slice(&egg_body);
    Ok(rifx)
}

// ============================================================================
// 2. After Effects XML Project (`.aepx`) Writer
// ============================================================================

fn xml_esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && c != '\n' && c != '\r' && c != '\t' => {}
            _ => out.push(ch),
        }
    }
    out
}

fn render_aepx_xml(model: &AeProjectModel, rifx_bytes: &[u8]) -> String {
    let mut xml = String::with_capacity(16_384);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        xml,
        "<AfterEffectsProject xmlns=\"http://ns.adobe.com/aep/1.0/\" version=\"24.0\" name=\"{}\" mainCompId=\"{}\">",
        xml_esc(&model.name),
        xml_esc(&model.main_comp_id)
    );

    xml.push_str("  <FootageItems>\n");
    for ft in &model.footage {
        let _ = writeln!(
            xml,
            "    <Footage id=\"{}\" name=\"{}\" path=\"{}\" mediaType=\"{}\"/>",
            xml_esc(&ft.id),
            xml_esc(&ft.name),
            xml_esc(&ft.relative_path),
            xml_esc(&ft.media_type)
        );
    }
    xml.push_str("  </FootageItems>\n");

    xml.push_str("  <Compositions>\n");
    for comp in &model.compositions {
        let _ = writeln!(
            xml,
            "    <Composition id=\"{}\" name=\"{}\" folder=\"{}\" width=\"{}\" height=\"{}\" fps=\"{:.4}\" durationFrames=\"{}\" bgColor=\"#{:02x}{:02x}{:02x}\">",
            xml_esc(&comp.id),
            xml_esc(&comp.name),
            xml_esc(&comp.folder),
            comp.width,
            comp.height,
            comp.fps,
            comp.duration_frames,
            (comp.background[0] * 255.0).round() as u8,
            (comp.background[1] * 255.0).round() as u8,
            (comp.background[2] * 255.0).round() as u8,
        );
        for m in &comp.markers {
            let _ = writeln!(
                xml,
                "      <Marker frame=\"{}\" duration=\"{}\" label=\"{}\">{}</Marker>",
                m.frame,
                m.duration_frames,
                xml_esc(&m.label),
                xml_esc(&m.comment)
            );
        }
        // Top-to-bottom AE stacking
        for layer in comp.layers.iter().rev() {
            let kind_tag = match &layer.kind {
                AeLayerKind::Shape { .. } => "ShapeLayer",
                AeLayerKind::Text { .. } => "TextLayer",
                AeLayerKind::PreComp { .. } => "PreCompLayer",
                AeLayerKind::Footage { .. } => "FootageLayer",
                AeLayerKind::Null => "NullLayer",
                AeLayerKind::Camera { .. } => "CameraLayer",
            };
            let _ = writeln!(
                xml,
                "      <{kind_tag} id=\"{}\" name=\"{}\" inFrame=\"{}\" outFrame=\"{}\" blendMode=\"{}\" threeD=\"{}\">",
                xml_esc(&layer.id),
                xml_esc(&layer.name),
                layer.in_frame,
                layer.out_frame,
                layer.blend_mode,
                layer.three_d
            );
            for (prop, expr_opt) in [
                ("Position", &layer.expressions.position),
                ("Scale", &layer.expressions.scale),
                ("Rotation", &layer.expressions.rotation),
                ("Opacity", &layer.expressions.opacity),
                ("Skew", &layer.expressions.skew),
                ("TimeRemap", &layer.expressions.time_remap),
                ("SourceText", &layer.expressions.source_text),
            ] {
                if let Some(expr) = expr_opt {
                    let _ = writeln!(
                        xml,
                        "        <Expression property=\"{prop}\">{}</Expression>",
                        xml_esc(expr)
                    );
                }
            }
            if let AeLayerKind::Shape { groups } = &layer.kind {
                for grp in groups {
                    let _ = writeln!(xml, "        <VectorGroup name=\"{}\">", xml_esc(&grp.name));
                    for track in &grp.contour_tracks {
                        let _ =
                            writeln!(xml, "          <BezierPath keyframes=\"{}\">", track.len());
                        for key in track {
                            let verts_str: Vec<String> = key
                                .value
                                .vertices
                                .iter()
                                .enumerate()
                                .map(|(vi, v)| {
                                    let ti = key
                                        .value
                                        .in_tangents
                                        .get(vi)
                                        .copied()
                                        .unwrap_or([0.0, 0.0]);
                                    let to = key
                                        .value
                                        .out_tangents
                                        .get(vi)
                                        .copied()
                                        .unwrap_or([0.0, 0.0]);
                                    format!(
                                        "{:.3},{:.3}({:.3},{:.3}|{:.3},{:.3})",
                                        finite_or(v[0], 0.0),
                                        finite_or(v[1], 0.0),
                                        finite_or(ti[0], 0.0),
                                        finite_or(ti[1], 0.0),
                                        finite_or(to[0], 0.0),
                                        finite_or(to[1], 0.0)
                                    )
                                })
                                .collect();
                            let _ = writeln!(
                                xml,
                                "            <Key frame=\"{}\" closed=\"{}\">{}</Key>",
                                key.frame,
                                key.value.closed,
                                verts_str.join(" ")
                            );
                        }
                        xml.push_str("          </BezierPath>\n");
                    }
                    xml.push_str("        </VectorGroup>\n");
                }
            }
            let _ = writeln!(xml, "      </{kind_tag}>");
        }
        xml.push_str("    </Composition>\n");
    }
    xml.push_str("  </Compositions>\n");

    let _ = writeln!(
        xml,
        "  <RifxPayload encoding=\"base64\">{}</RifxPayload>",
        STANDARD.encode(rifx_bytes)
    );
    xml.push_str("</AfterEffectsProject>\n");
    xml
}

// ============================================================================
// 3. Self-Building Adobe After Effects ExtendScript (`.jsx`) Constructor
// ============================================================================

fn js_str(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap_or_else(|_| "\"\"".into())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn render_extendscript_jsx(model: &AeProjectModel, aep_path: &Path) -> String {
    let target_aep_name = aep_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project.aep");

    let mut js = String::with_capacity(32_768);
    js.push_str("/* NeoFlash -> Adobe After Effects Interchange Builder (T-246)\n");
    js.push_str(" * Run in After Effects via: File > Scripts > Run Script File...\n");
    js.push_str(
        " * Reconstructs native editable Bezier Shape Layers, Gradients, Strokes, Pre-Comps,\n",
    );
    js.push_str(
        " * Time Remapping, Track Mattes, Filters, Text, Footage trims, and AE JS Expressions.\n",
    );
    js.push_str(" */\n");
    js.push_str("(function buildNeoFlashAfterEffectsProject() {\n");
    js.push_str("  if (typeof app === 'undefined' || !app.project) { return; }\n");
    js.push_str("  app.beginUndoGroup('Import NeoFlash Project');\n");
    js.push_str("  try {\n");
    js.push_str("    if (app.project.expressionEngine !== undefined) { try { app.project.expressionEngine = 'javascript-1.0'; } catch (_) {} }\n");
    js.push_str("    var scriptFile = new File($.fileName);\n");
    js.push_str("    var baseFolder = scriptFile.parent;\n");
    js.push_str(
        "    function hasOwn(m, k) { return Object.prototype.hasOwnProperty.call(m, k); }\n",
    );
    js.push_str("    var folders = {};\n");
    js.push_str("    function ensureFolder(path) {\n");
    js.push_str("      if (!path) return app.project.rootFolder;\n");
    js.push_str("      if (hasOwn(folders, path)) return folders[path];\n");
    js.push_str("      var parts = path.split('/');\n");
    js.push_str("      var cur = app.project.rootFolder;\n");
    js.push_str("      var acc = '';\n");
    js.push_str("      for (var i = 0; i < parts.length; i++) {\n");
    js.push_str("        if (!parts[i]) continue;\n");
    js.push_str("        acc = acc ? (acc + '/' + parts[i]) : parts[i];\n");
    js.push_str("        if (!hasOwn(folders, acc)) {\n");
    js.push_str("          var f = app.project.items.addFolder(parts[i]);\n");
    js.push_str("          f.parentFolder = cur;\n");
    js.push_str("          folders[acc] = f;\n");
    js.push_str("        }\n");
    js.push_str("        cur = folders[acc];\n");
    js.push_str("      }\n");
    js.push_str("      return cur;\n");
    js.push_str("    }\n");
    js.push_str("    function setKeyed1D(prop, keys, fps) {\n");
    js.push_str("      if (!keys || keys.length === 0) return;\n");
    js.push_str("      if (keys.length === 1) { prop.setValue(keys[0][1]); return; }\n");
    js.push_str("      for (var i = 0; i < keys.length; i++) {\n");
    js.push_str("        var k = keys[i];\n");
    js.push_str("        prop.setValueAtTime(k[0] / fps, k[1]);\n");
    js.push_str("      }\n");
    js.push_str("      for (var j = 0; j < keys.length; j++) {\n");
    js.push_str("        var ke = keys[j];\n");
    js.push_str("        var idx = j + 1;\n");
    js.push_str("        if (ke[4]) {\n");
    js.push_str("          prop.setInterpolationTypeAtKey(idx, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.HOLD);\n");
    js.push_str("        } else {\n");
    js.push_str(
        "          var easeIn = new KeyframeEase(0, Math.max(0.1, Math.min(100, ke[2])));\n",
    );
    js.push_str(
        "          var easeOut = new KeyframeEase(0, Math.max(0.1, Math.min(100, ke[3])));\n",
    );
    js.push_str(
        "          try { prop.setTemporalEaseAtKey(idx, [easeIn], [easeOut]); } catch (_) {}\n",
    );
    js.push_str("        }\n");
    js.push_str("      }\n");
    js.push_str("    }\n");
    js.push_str("    function setKeyed2D(prop, keys, fps, is3D, zVal) {\n");
    js.push_str("      if (!keys || keys.length === 0) return;\n");
    js.push_str(
        "      function vec(v) { return is3D ? [v[0], v[1], zVal || 0] : [v[0], v[1]]; }\n",
    );
    js.push_str("      if (keys.length === 1) { prop.setValue(vec(keys[0][1])); return; }\n");
    js.push_str("      for (var i = 0; i < keys.length; i++) {\n");
    js.push_str("        prop.setValueAtTime(keys[i][0] / fps, vec(keys[i][1]));\n");
    js.push_str("      }\n");
    js.push_str("    }\n");
    js.push_str("    function makeShape(verts, ins, outs, closed) {\n");
    js.push_str("      var s = new Shape();\n");
    js.push_str("      s.vertices = verts;\n");
    js.push_str("      s.inTangents = ins;\n");
    js.push_str("      s.outTangents = outs;\n");
    js.push_str("      s.closed = closed;\n");
    js.push_str("      return s;\n");
    js.push_str("    }\n");

    // 1. Import Footage items
    js.push_str("    var footageMap = {};\n");
    if !model.footage.is_empty() {
        js.push_str("    var footageFolder = ensureFolder('Footage');\n");
        for ft in &model.footage {
            let _ = writeln!(
                js,
                "    (function() {{ var f = new File(baseFolder.fsName + '/' + {}); if (f.exists) {{ var io = new ImportOptions(f); var it = app.project.importFile(io); it.parentFolder = footageFolder; footageMap[{}] = it; }} }})();",
                js_str(&ft.relative_path),
                js_str(&ft.id)
            );
        }
    }

    // 2. Create Compositions first so nested PreComps can resolve any symbol comp
    js.push_str("    var compMap = {};\n");
    for comp in &model.compositions {
        let dur_sec = (f64::from(comp.duration_frames.max(1)) / comp.fps).max(1.0 / comp.fps);
        let _ = writeln!(
            js,
            "    (function() {{ var c = app.project.items.addComp({}, {}, {}, 1.0, {:.6}, {:.4}); c.bgColor = [{:.4}, {:.4}, {:.4}]; c.parentFolder = ensureFolder({}); compMap[{}] = c; }})();",
            js_str(&comp.name),
            comp.width,
            comp.height,
            dur_sec,
            comp.fps,
            comp.background[0],
            comp.background[1],
            comp.background[2],
            js_str(&comp.folder),
            js_str(&comp.id),
        );
    }

    // 3. Populate each Composition (bottom-to-top, since `comp.layers.add*()` inserts at top index 1!)
    for comp in &model.compositions {
        let _ = writeln!(js, "    (function(comp) {{");
        let _ = writeln!(js, "      if (!comp) return;");
        let _ = writeln!(js, "      var fps = {:.4};", comp.fps);
        let _ = writeln!(js, "      var layerMap = {{}};");

        if let Some([ws, we]) = comp.work_area {
            let _ = writeln!(
                js,
                "      try {{ comp.workAreaStart = {:.6}; comp.workAreaDuration = {:.6}; }} catch (_) {{}}",
                f64::from(ws) / comp.fps,
                f64::from(we.saturating_sub(ws).max(1)) / comp.fps
            );
        }
        for g in &comp.guides {
            let ori = if g.horizontal {
                " Horizontal "
            } else {
                " Vertical "
            };
            let _ = ori;
            let _ = writeln!(
                js,
                "      try {{ comp.addGuide({}, {:.3}); }} catch (_) {{}}",
                if g.horizontal { 0 } else { 1 },
                g.position
            );
        }
        for m in &comp.markers {
            let _ = writeln!(
                js,
                "      try {{ var mv = new MarkerValue({}); mv.chapter = {}; mv.duration = {:.6}; comp.markerProperty.setValueAtTime({:.6}, mv); }} catch (_) {{}}",
                js_str(&m.comment),
                js_str(&m.label),
                f64::from(m.duration_frames) / comp.fps,
                f64::from(m.frame) / comp.fps,
            );
        }

        for layer in &comp.layers {
            js.push_str("      (function() {\n");
            js.push_str("        var lyr = null;\n");
            match &layer.kind {
                AeLayerKind::Shape { groups } => {
                    js.push_str("        lyr = comp.layers.addShape();\n");
                    js.push_str("        var rootVec = lyr.property('ADBE Root Vectors Group');\n");
                    for grp in groups {
                        let _ = writeln!(
                            js,
                            "        var vg = rootVec.addProperty('ADBE Vector Group'); vg.name = {};",
                            js_str(&grp.name)
                        );
                        js.push_str("        var vc = vg.property('ADBE Vectors Group');\n");
                        for (ci, track) in grp.contour_tracks.iter().enumerate() {
                            let _ = writeln!(
                                js,
                                "        var sp_{ci} = vc.addProperty('ADBE Vector Shape - Group'); var pathProp_{ci} = sp_{ci}.property('ADBE Vector Shape');"
                            );
                            if track.len() == 1 {
                                let c = &track[0].value;
                                let _ = writeln!(
                                    js,
                                    "        pathProp_{ci}.setValue(makeShape({}, {}, {}, {}));",
                                    serde_json::to_string(&c.vertices).unwrap_or_default(),
                                    serde_json::to_string(&c.in_tangents).unwrap_or_default(),
                                    serde_json::to_string(&c.out_tangents).unwrap_or_default(),
                                    c.closed
                                );
                            } else {
                                for key in track {
                                    let c = &key.value;
                                    let _ = writeln!(
                                        js,
                                        "        pathProp_{ci}.setValueAtTime({} / fps, makeShape({}, {}, {}, {}));",
                                        key.frame,
                                        serde_json::to_string(&c.vertices).unwrap_or_default(),
                                        serde_json::to_string(&c.in_tangents).unwrap_or_default(),
                                        serde_json::to_string(&c.out_tangents).unwrap_or_default(),
                                        c.closed
                                    );
                                }
                            }
                        }

                        // Strokes
                        for st in &grp.strokes {
                            let _ = writeln!(
                                js,
                                "        (function() {{ var st = vc.addProperty('ADBE Vector Graphic - Stroke'); st.property('ADBE Vector Stroke Color').setValue([{:.4}, {:.4}, {:.4}, 1]); st.property('ADBE Vector Stroke Opacity').setValue({:.3}); st.property('ADBE Vector Stroke Width').setValue({:.3}); try {{ st.property('ADBE Vector Stroke Line Cap').setValue(2); st.property('ADBE Vector Stroke Line Join').setValue(2); }} catch (_) {{}}",
                                st.color[0], st.color[1], st.color[2], st.opacity, st.width
                            );
                            if let Some([dash, gap]) = st.dashes {
                                let _ = writeln!(
                                    js,
                                    "          try {{ var ds = st.property('ADBE Vector Stroke Dashes'); var d1 = ds.addProperty('ADBE Vector Stroke Dash 1'); d1.setValue({dash:.3}); var g1 = ds.addProperty('ADBE Vector Stroke Gap 1'); g1.setValue({gap:.3}); }} catch (_) {{}}"
                                );
                            }
                            js.push_str("        })();\n");
                        }

                        // Fills
                        for fl in &grp.fills {
                            match fl {
                                AePaintFill::None => {}
                                AePaintFill::Solid {
                                    color,
                                    opacity,
                                    even_odd,
                                } => {
                                    let _ = writeln!(
                                        js,
                                        "        (function() {{ var fl = vc.addProperty('ADBE Vector Graphic - Fill'); fl.property('ADBE Vector Fill Color').setValue([{:.4}, {:.4}, {:.4}, 1]); fl.property('ADBE Vector Fill Opacity').setValue({:.3}); try {{ fl.property('ADBE Vector Fill Rule').setValue({}); }} catch (_) {{}} }})();",
                                        color[0],
                                        color[1],
                                        color[2],
                                        opacity,
                                        if *even_odd { 2 } else { 1 }
                                    );
                                }
                                AePaintFill::Gradient {
                                    radial,
                                    start,
                                    end,
                                    highlight_length,
                                    stops,
                                    opacity,
                                    even_odd,
                                } => {
                                    let fallback_rgb = stops
                                        .first()
                                        .map(|(_, c)| [c[0], c[1], c[2]])
                                        .unwrap_or([0.5, 0.5, 0.5]);
                                    let _ = writeln!(
                                        js,
                                        "        (function() {{ try {{ var gf = vc.addProperty('ADBE Vector Graphic - G-Fill'); gf.property('ADBE Vector Grad Type').setValue({}); gf.property('ADBE Vector Grad Start Pt').setValue([{:.3}, {:.3}]); gf.property('ADBE Vector Grad End Pt').setValue([{:.3}, {:.3}]); if ({}) {{ gf.property('ADBE Vector Grad HiLite Length').setValue({:.3}); }} gf.property('ADBE Vector Fill Opacity').setValue({:.3}); gf.property('ADBE Vector Fill Rule').setValue({}); }} catch (_) {{ var sf = vc.addProperty('ADBE Vector Graphic - Fill'); sf.property('ADBE Vector Fill Color').setValue([{:.4}, {:.4}, {:.4}, 1]); }} }})();",
                                        if *radial { 2 } else { 1 },
                                        start[0],
                                        start[1],
                                        end[0],
                                        end[1],
                                        radial,
                                        highlight_length,
                                        opacity,
                                        if *even_odd { 2 } else { 1 },
                                        fallback_rgb[0],
                                        fallback_rgb[1],
                                        fallback_rgb[2],
                                    );
                                }
                            }
                        }

                        // Vector Modifiers (`Repeater`, `Offset Paths`, `Round Corners`)
                        for m in &grp.modifiers {
                            match *m {
                                AeVectorModifier::Repeater {
                                    copies,
                                    offset_x,
                                    offset_y,
                                    scale_pct,
                                    rotation,
                                } => {
                                    let _ = writeln!(
                                        js,
                                        "        try {{ var rp = vc.addProperty('ADBE Vector Filter - Repeater'); rp.property('ADBE Vector Repeater Copies').setValue({copies}); var rpt = rp.property('ADBE Vector Repeater Transform'); rpt.property('ADBE Vector Repeater Position').setValue([{offset_x:.3}, {offset_y:.3}]); rpt.property('ADBE Vector Repeater Scale').setValue([{scale_pct:.3}, {scale_pct:.3}]); rpt.property('ADBE Vector Repeater Rotation').setValue({rotation:.3}); }} catch (_) {{}}"
                                    );
                                }
                                AeVectorModifier::OffsetPaths { amount } => {
                                    let _ = writeln!(
                                        js,
                                        "        try {{ var op = vc.addProperty('ADBE Vector Filter - Offset'); op.property('ADBE Vector Offset Amount').setValue({amount:.3}); }} catch (_) {{}}"
                                    );
                                }
                                AeVectorModifier::RoundCorners { radius } => {
                                    let _ = writeln!(
                                        js,
                                        "        try {{ var rc = vc.addProperty('ADBE Vector Filter - RC'); rc.property('ADBE Vector RoundCorner Radius').setValue({radius:.3}); }} catch (_) {{}}"
                                    );
                                }
                            }
                        }
                    }
                }
                AeLayerKind::Text {
                    text,
                    font,
                    font_size,
                    color,
                    bold,
                    italic,
                    align,
                    leading,
                    tracking,
                    box_size,
                } => {
                    if let Some([bw, bh]) = box_size {
                        let _ = writeln!(
                            js,
                            "        lyr = comp.layers.addBoxText([{bw:.2}, {bh:.2}]);"
                        );
                    } else {
                        let _ =
                            writeln!(js, "        lyr = comp.layers.addText({});", js_str(text));
                    }
                    let just = match align {
                        TextAlign::Left => "ParagraphJustification.LEFT_JUSTIFY",
                        TextAlign::Center => "ParagraphJustification.CENTER_JUSTIFY",
                        TextAlign::Right => "ParagraphJustification.RIGHT_JUSTIFY",
                    };
                    let _ = writeln!(
                        js,
                        "        (function() {{ var tp = lyr.property('ADBE Text Properties').property('ADBE Text Document'); var td = tp.value; td.text = {}; td.fontSize = {:.2}; td.fillColor = [{:.4}, {:.4}, {:.4}]; td.applyFill = true; try {{ td.font = {}; td.fauxBold = {}; td.fauxItalic = {}; td.leading = {:.2}; td.tracking = {:.2}; td.justification = {just}; }} catch (_) {{}} tp.setValue(td); }})();",
                        js_str(text),
                        font_size,
                        color[0],
                        color[1],
                        color[2],
                        js_str(font),
                        bold,
                        italic,
                        leading,
                        tracking
                    );
                }
                AeLayerKind::PreComp {
                    comp_id,
                    time_remap_keys,
                    ..
                } => {
                    let _ = writeln!(
                        js,
                        "        var subComp = hasOwn(compMap, {}) ? compMap[{}] : null; if (!subComp) return; lyr = comp.layers.add(subComp);",
                        js_str(comp_id),
                        js_str(comp_id)
                    );
                    if !time_remap_keys.is_empty() || layer.expressions.time_remap.is_some() {
                        js.push_str("        try { lyr.timeRemapEnabled = true; } catch (_) {}\n");
                        if !time_remap_keys.is_empty() {
                            let keys_arr = format_keys_1d(time_remap_keys);
                            let _ = writeln!(
                                js,
                                "        try {{ setKeyed1D(lyr.property('ADBE Time Remapping'), {keys_arr}, fps); }} catch (_) {{}}"
                            );
                        }
                    }
                }
                AeLayerKind::Footage {
                    footage_id,
                    is_audio_only,
                    start_offset_sec,
                    end_offset_sec,
                    loop_count,
                    muted,
                    audio_levels_db,
                    ..
                } => {
                    let _ = writeln!(
                        js,
                        "        var ft = hasOwn(footageMap, {}) ? footageMap[{}] : null; if (!ft) return; if ({loop_count} > 1) {{ try {{ ft.mainSource.loop = {loop_count}; }} catch (_) {{}} }} lyr = comp.layers.add(ft);",
                        js_str(footage_id),
                        js_str(footage_id)
                    );
                    if *is_audio_only {
                        js.push_str("        try { lyr.audioEnabled = true; } catch (_) {}\n");
                    } else if *muted {
                        js.push_str("        try { lyr.audioEnabled = false; } catch (_) {}\n");
                    }
                    if *start_offset_sec > 0.0 {
                        let _ = writeln!(
                            js,
                            "        lyr.startTime = ({} / fps) - {:.6};",
                            layer.in_frame, start_offset_sec
                        );
                    }
                    if let Some(end_sec) = end_offset_sec {
                        let trim_dur = (*end_sec - *start_offset_sec).max(1.0 / comp.fps);
                        let _ = writeln!(
                            js,
                            "        lyr.outPoint = Math.min(lyr.outPoint, ({} / fps) + {:.6});",
                            layer.in_frame, trim_dur
                        );
                    }
                    if !audio_levels_db.is_empty() {
                        let lv_arr = format_keys_2d(audio_levels_db);
                        let _ = writeln!(
                            js,
                            "        try {{ setKeyed2D(lyr.property('ADBE Audio Group').property('ADBE Audio Levels'), {lv_arr}, fps, false, 0); }} catch (_) {{}}"
                        );
                    }
                }
                AeLayerKind::Null => {
                    js.push_str("        lyr = comp.layers.addNull();\n");
                }
                AeLayerKind::Camera {
                    focal_length,
                    zoom_keys,
                } => {
                    let _ = writeln!(
                        js,
                        "        lyr = comp.layers.addCamera({}, [{:.2}, {:.2}]);",
                        js_str(&layer.name),
                        f64::from(comp.width) * 0.5,
                        f64::from(comp.height) * 0.5
                    );
                    let _ = focal_length;
                    if !zoom_keys.is_empty() {
                        let z_arr = format_keys_1d(zoom_keys);
                        let _ = writeln!(
                            js,
                            "        try {{ setKeyed1D(lyr.property('ADBE Camera Options Group').property('ADBE Camera Zoom'), {z_arr}, fps); }} catch (_) {{}}"
                        );
                    }
                }
            }

            // Common Layer setup: name, in/out points, 3D, blend mode, track matte, transforms, effects, expressions, markers
            let _ = writeln!(
                js,
                "        if (!lyr) return; lyr.name = {}; layerMap[{}] = lyr;",
                js_str(&layer.name),
                js_str(&layer.id)
            );
            if !matches!(layer.kind, AeLayerKind::Footage { start_offset_sec, .. } if start_offset_sec > 0.0)
            {
                let _ = writeln!(js, "        lyr.startTime = {} / fps;", layer.in_frame);
            }
            let _ = writeln!(
                js,
                "        lyr.inPoint = {} / fps; lyr.outPoint = Math.max(({} + 1) / fps, {} / fps);",
                layer.in_frame, layer.in_frame, layer.out_frame
            );
            if layer.three_d {
                js.push_str("        try { lyr.threeDLayer = true; } catch (_) {}\n");
            }
            if layer.auto_orient_along_path {
                js.push_str(
                    "        try { lyr.autoOrient = AutoOrientType.ALONG_PATH; } catch (_) {}\n",
                );
            }
            if layer.blend_mode != "NORMAL" {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.blendingMode = BlendingMode.{}; }} catch (_) {{}}",
                    layer.blend_mode
                );
            }
            if layer.track_matte != AeTrackMatte::None {
                let tm = match layer.track_matte {
                    AeTrackMatte::None => "NO_TRACK_MATTE",
                    AeTrackMatte::Alpha => "ALPHA",
                    AeTrackMatte::AlphaInverted => "ALPHA_INVERTED",
                    AeTrackMatte::Luma => "LUMA",
                    AeTrackMatte::LumaInverted => "LUMA_INVERTED",
                };
                let _ = writeln!(
                    js,
                    "        try {{ lyr.trackMatteType = TrackMatteType.{tm}; }} catch (_) {{}}"
                );
            }

            // Transforms
            if !matches!(
                layer.kind,
                AeLayerKind::Footage {
                    is_audio_only: true,
                    ..
                }
            ) {
                js.push_str("        var xf = lyr.property('ADBE Transform Group');\n");
                let _ = writeln!(
                    js,
                    "        if (xf) {{ try {{ xf.property('ADBE Anchor Point').setValue({}); }} catch (_) {{}}",
                    if layer.three_d {
                        format!(
                            "[{:.3}, {:.3}, 0]",
                            layer.anchor_point[0], layer.anchor_point[1]
                        )
                    } else {
                        format!(
                            "[{:.3}, {:.3}]",
                            layer.anchor_point[0], layer.anchor_point[1]
                        )
                    }
                );
                if !layer.position_keys.is_empty() {
                    let _ = writeln!(
                        js,
                        "          setKeyed2D(xf.property('ADBE Position'), {}, fps, {}, {:.3});",
                        format_keys_2d(&layer.position_keys),
                        layer.three_d,
                        layer.z_depth
                    );
                }
                if !layer.scale_keys.is_empty() {
                    let _ = writeln!(
                        js,
                        "          setKeyed2D(xf.property('ADBE Scale'), {}, fps, {}, 100);",
                        format_keys_2d(&layer.scale_keys),
                        layer.three_d
                    );
                }
                if !layer.rotation_keys.is_empty() {
                    let _ = writeln!(
                        js,
                        "          setKeyed1D(xf.property('ADBE Rotate Z'), {}, fps);",
                        format_keys_1d(&layer.rotation_keys)
                    );
                }
                if !layer.opacity_keys.is_empty() {
                    let _ = writeln!(
                        js,
                        "          setKeyed1D(xf.property('ADBE Opacity'), {}, fps);",
                        format_keys_1d(&layer.opacity_keys)
                    );
                }
                js.push_str("        }\n");
            }

            // Skew via `ADBE Geometry2` Transform effect when needed
            if !layer.skew_keys.is_empty() || layer.expressions.skew.is_some() {
                js.push_str("        try { var fxSkew = lyr.property('ADBE Effect Parade').addProperty('ADBE Geometry2'); var skProp = fxSkew.property('ADBE Geometry2-0006');\n");
                if !layer.skew_keys.is_empty() {
                    let _ = writeln!(
                        js,
                        "          setKeyed1D(skProp, {}, fps);",
                        format_keys_1d(&layer.skew_keys)
                    );
                }
                if let Some(sk_expr) = &layer.expressions.skew {
                    let _ = writeln!(js, "          skProp.expression = {};", js_str(sk_expr));
                }
                js.push_str("        } catch (_) {}\n");
            }

            // Native AE Effects (`Drop Shadow`, `Glow`, `Gaussian Blur`, `Bevel Alpha`, `Tint`)
            if !layer.effects.is_empty() {
                js.push_str("        var fxParade = lyr.property('ADBE Effect Parade');\n");
                for fx in &layer.effects {
                    match fx {
                        AeEffect::DropShadow {
                            color,
                            opacity_pct,
                            direction_deg,
                            distance,
                            softness,
                        } => {
                            let _ = writeln!(
                                js,
                                "        try {{ var ds = fxParade.addProperty('ADBE Drop Shadow'); ds.property('ADBE Drop Shadow-0001').setValue([{:.4}, {:.4}, {:.4}, 1]); ds.property('ADBE Drop Shadow-0002').setValue({:.2}); ds.property('ADBE Drop Shadow-0003').setValue({:.2}); ds.property('ADBE Drop Shadow-0004').setValue({:.2}); ds.property('ADBE Drop Shadow-0005').setValue({:.2}); }} catch (_) {{}}",
                                color[0],
                                color[1],
                                color[2],
                                opacity_pct * 2.55,
                                direction_deg,
                                distance,
                                softness
                            );
                        }
                        AeEffect::Glow {
                            color,
                            radius,
                            intensity,
                        } => {
                            let _ = writeln!(
                                js,
                                "        try {{ var gl = fxParade.addProperty('ADBE Glo2'); gl.property('ADBE Glo2-0003').setValue({radius:.2}); gl.property('ADBE Glo2-0004').setValue({intensity:.2}); gl.property('ADBE Glo2-0008').setValue([{:.4}, {:.4}, {:.4}, 1]); }} catch (_) {{}}",
                                color[0], color[1], color[2]
                            );
                        }
                        AeEffect::GaussianBlur {
                            blurriness,
                            dimensions,
                        } => {
                            let _ = writeln!(
                                js,
                                "        try {{ var gb = fxParade.addProperty('ADBE Gaussian Blur 2'); gb.property('ADBE Gaussian Blur 2-0001').setValue({blurriness:.2}); gb.property('ADBE Gaussian Blur 2-0002').setValue({dimensions}); }} catch (_) {{}}"
                            );
                        }
                        AeEffect::BevelAlpha {
                            edge_thickness,
                            light_angle,
                            light_intensity,
                        } => {
                            let _ = writeln!(
                                js,
                                "        try {{ var bv = fxParade.addProperty('ADBE Bevel Alpha'); bv.property('ADBE Bevel Alpha-0001').setValue({edge_thickness:.2}); bv.property('ADBE Bevel Alpha-0002').setValue({light_angle:.2}); bv.property('ADBE Bevel Alpha-0004').setValue({light_intensity:.2}); }} catch (_) {{}}"
                            );
                        }
                        AeEffect::Tint { color, amount_pct } => {
                            let _ = writeln!(
                                js,
                                "        try {{ var tn = fxParade.addProperty('ADBE Tint'); tn.property('ADBE Tint-0001').setValue([{:.4}, {:.4}, {:.4}, 1]); tn.property('ADBE Tint-0002').setValue([{:.4}, {:.4}, {:.4}, 1]); tn.property('ADBE Tint-0003').setValue({amount_pct:.2}); }} catch (_) {{}}",
                                color[0], color[1], color[2], color[0], color[1], color[2]
                            );
                        }
                    }
                }
            }

            // Apply After Effects JavaScript Expressions!
            if let Some(pe) = &layer.expressions.position {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.property('ADBE Transform Group').property('ADBE Position').expression = {}; }} catch (_) {{}}",
                    js_str(pe)
                );
            }
            if let Some(se) = &layer.expressions.scale {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.property('ADBE Transform Group').property('ADBE Scale').expression = {}; }} catch (_) {{}}",
                    js_str(se)
                );
            }
            if let Some(re) = &layer.expressions.rotation {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.property('ADBE Transform Group').property('ADBE Rotate Z').expression = {}; }} catch (_) {{}}",
                    js_str(re)
                );
            }
            if let Some(oe) = &layer.expressions.opacity {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.property('ADBE Transform Group').property('ADBE Opacity').expression = {}; }} catch (_) {{}}",
                    js_str(oe)
                );
            }
            if let Some(tre) = &layer.expressions.time_remap {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.timeRemapEnabled = true; lyr.property('ADBE Time Remapping').expression = {}; }} catch (_) {{}}",
                    js_str(tre)
                );
            }
            if let Some(ste) = &layer.expressions.source_text {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.property('ADBE Text Properties').property('ADBE Text Document').expression = {}; }} catch (_) {{}}",
                    js_str(ste)
                );
            }

            // Layer Markers (preserving scripts & cue notes)
            for m in &layer.markers {
                let _ = writeln!(
                    js,
                    "        try {{ var lmv = new MarkerValue({}); lmv.chapter = {}; lyr.property('ADBE Marker').setValueAtTime({:.6}, lmv); }} catch (_) {{}}",
                    js_str(&m.comment),
                    js_str(&m.label),
                    f64::from(m.frame) / comp.fps
                );
            }

            let _ = writeln!(js, "        lyr.enabled = {};", layer.visible);
            if layer.locked {
                js.push_str("        lyr.locked = true;\n");
            }
            js.push_str("      })();\n");
        }

        // Wire Layer Parenting after all layers in this composition exist
        for layer in &comp.layers {
            if let Some(pid) = &layer.parent_layer_id {
                let _ = writeln!(
                    js,
                    "      if (hasOwn(layerMap, {}) && hasOwn(layerMap, {})) {{ try {{ layerMap[{}].parent = layerMap[{}]; }} catch (_) {{}} }}",
                    js_str(&layer.id),
                    js_str(pid),
                    js_str(&layer.id),
                    js_str(pid)
                );
            }
        }

        let _ = writeln!(
            js,
            "    }})(hasOwn(compMap, {}) ? compMap[{}] : null);",
            js_str(&comp.id),
            js_str(&comp.id)
        );
    }

    let _ = writeln!(
        js,
        "    if (hasOwn(compMap, {})) {{ try {{ compMap[{}].openInViewer(); }} catch (_) {{}} }}",
        js_str(&model.main_comp_id),
        js_str(&model.main_comp_id)
    );
    let _ = writeln!(
        js,
        "    try {{ app.project.save(new File(baseFolder.fsName + '/' + {})); }} catch (_) {{}}",
        js_str(target_aep_name)
    );
    js.push_str("  } finally {\n");
    js.push_str("    app.endUndoGroup();\n");
    js.push_str("  }\n");
    js.push_str("})();\n");
    js
}

fn format_keys_1d(keys: &[AeKey<f64>]) -> String {
    let mut out = String::from("[");
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "[{},{:.4},{:.2},{:.2},{}]",
            k.frame,
            finite_or(k.value, 0.0),
            finite_or(k.ease.in_influence, 16.67),
            finite_or(k.ease.out_influence, 16.67),
            k.ease.hold
        );
    }
    out.push(']');
    out
}

fn format_keys_2d(keys: &[AeKey<[f64; 2]>]) -> String {
    let mut out = String::from("[");
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "[{},[{:.4},{:.4}],{:.2},{:.2},{}]",
            k.frame,
            finite_or(k.value[0], 0.0),
            finite_or(k.value[1], 0.0),
            finite_or(k.ease.in_influence, 16.67),
            finite_or(k.ease.out_influence, 16.67),
            k.ease.hold
        );
    }
    out.push(']');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{
        BitmapCompression, BitmapExportSettings, FrameScript, GradientStop, ObjectScript,
        RectangleOptions, ResourceUnit, Script, SymbolKind,
    };
    use std::process::Command;

    #[test]
    fn bezier_path_conversion_elevates_quadratics_and_preserves_cubics_and_rounded_rects() {
        let cmds = vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::QuadraticTo {
                cx: 30.0,
                cy: -60.0,
                x: 60.0,
                y: 0.0,
            },
            PathCommand::CubicTo {
                cx1: 60.0,
                cy1: 40.0,
                cx2: 0.0,
                cy2: 40.0,
                x: 0.0,
                y: 0.0,
            },
            PathCommand::Close,
        ];
        let contours = path_commands_to_ae_contours(&cmds, [10.0, 20.0]);
        assert_eq!(contours.len(), 1);
        let c = &contours[0];
        assert!(c.closed);
        // Duplicate closing vertex merged onto vertex 0 while keeping incoming tangent!
        assert_eq!(c.vertices.len(), 2);
        assert_eq!(c.vertices[0], [10.0, 20.0]);
        assert_eq!(c.vertices[1], [70.0, 20.0]);
        // Outgoing tangent from v0 for quadratic `(0,0) -> (30,-60) -> (60,0)` is `2/3 * (30, -60) = (20, -40)`
        assert!((c.out_tangents[0][0] - 20.0).abs() < 1e-6);
        assert!((c.out_tangents[0][1] - (-40.0)).abs() < 1e-6);
        // Incoming tangent at v0 from cubic closing segment `(cx2=0, cy2=40)` -> `(0, 40)`
        assert!((c.in_tangents[0][0] - 0.0).abs() < 1e-6);
        assert!((c.in_tangents[0][1] - 40.0).abs() < 1e-6);

        let mut rect_el = Element {
            id: "r1".into(),
            name: "Card".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 120.0,
                height: 80.0,
                fill: [40, 120, 240, 255],
                stroke: [255, 255, 255, 255],
                stroke_width: 2.0,
            },
            ..Element::default()
        };
        rect_el.set_rectangle_options(RectangleOptions {
            corner_radii: [12.0, 12.0, 12.0, 12.0],
            corners_locked: true,
        });
        let (rcmds, _, _, _, _, _, _) = element_vector_commands(&rect_el).unwrap();
        let rcontours = path_commands_to_ae_contours(&rcmds, [0.0, 0.0]);
        assert_eq!(rcontours.len(), 1);
        assert!(rcontours[0].closed);
        assert_eq!(rcontours[0].vertices.len(), 8);
    }

    #[test]
    fn aep_export_preserves_vectors_gradients_precomps_footage_and_compiles_scripts_to_ae_expressions()
     {
        let mut project = Project::blank();
        project.name = "AE Interchange Suite".into();
        project.width = 800;
        project.height = 600;
        project.fps = 24;

        // Add a PNG bitmap resource with custom WebP publish compression (`T-251`)
        let mut img = image::RgbaImage::new(16, 16);
        for px in img.pixels_mut() {
            *px = image::Rgba([255, 80, 40, 255]);
        }
        let mut png_bytes = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        let mut res =
            ResourceUnit::from_bytes("res-bmp", "sprites/badge.png", "image/png", &png_bytes)
                .unwrap();
        res.bitmap_export = Some(BitmapExportSettings {
            compression: BitmapCompression::Webp,
            use_imported_jpeg_data: true,
            jpeg_quality: 85,
            allow_smoothing: true,
        });
        project.resources.push(res);

        // Add a nested Symbol (`sym-wheel`) with negative coordinates around (0, 0) + loop script `gotoAndPlay("spin")`
        let wheel_sym = Symbol {
            id: "sym-wheel".into(),
            name: "Wheel".into(),
            folder: "Rigs".into(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 12,
                layers: vec![Layer {
                    id: "wl-1".into(),
                    name: "Spokes".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        label: "spin".into(),
                        elements: vec![Element {
                            id: "spoke".into(),
                            name: "spoke_shape".into(),
                            transform: Transform {
                                x: -40.0,
                                y: -40.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 80.0,
                                height: 80.0,
                                fill: [240, 200, 40, 255],
                                stroke: [20, 20, 30, 255],
                                stroke_width: 3.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                    properties: Default::default(),
                }],
                scripts: vec![FrameScript {
                    frame: 11,
                    script: Script {
                        language: ScriptLanguage::ActionScript2,
                        source: "gotoAndPlay(\"spin\");".into(),
                    },
                }],
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        };
        project.symbols.push(wheel_sym);

        // Configure Root scene with:
        // 1. Gradient PaintedPath vector shape + PropertyExpression (`sin(time * tau * freq) * amp`)
        // 2. Shape Tween morph layer (`TweenKind::Shape`)
        // 3. Nested `sym-wheel` instance with AS2 `onClipEvent(enterFrame) { _rotation += 6; _x += 3; }`
        // 4. Dynamic Text layer driven by frame script `hud_txt.text = "Frame " + _currentframe;`
        // 5. Bitmap layer referencing `res-bmp`
        let root_id = project.root.clone();
        let root_sym = project.symbol_mut(&root_id).unwrap();
        root_sym.timeline.frame_count = 24;
        root_sym.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "hud_txt.text = \"Frame \" + _currentframe;".into(),
            },
        });
        root_sym.timeline.layers = vec![
            Layer {
                id: "l-grad".into(),
                name: "Aurora Vector".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "el-grad".into(),
                        name: "aurora".into(),
                        transform: Transform {
                            x: 100.0,
                            y: 120.0,
                            ..Transform::default()
                        },
                        filters: vec![FilterSpec::Glow {
                            color: 0x00FFCC,
                            alpha: 0.8,
                            blur_x: 12.0,
                            blur_y: 12.0,
                            strength: 1.5,
                            quality: 2,
                            inner: false,
                            knockout: false,
                        }],
                        kind: ElementKind::PaintedPath {
                            commands: vec![
                                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                PathCommand::CubicTo {
                                    cx1: 80.0,
                                    cy1: -40.0,
                                    cx2: 160.0,
                                    cy2: 40.0,
                                    x: 240.0,
                                    y: 0.0,
                                },
                                PathCommand::LineTo { x: 240.0, y: 90.0 },
                                PathCommand::LineTo { x: 0.0, y: 90.0 },
                                PathCommand::Close,
                            ],
                            fill_rule: FillRule::NonZero,
                            fill: [0; 4],
                            stroke: [255, 255, 255, 200],
                            fill_gradient: Some(GradientPaint {
                                kind: GradientKind::Linear,
                                matrix: [0.15, 0.0, 0.0, 0.15, 120.0, 45.0],
                                stops: vec![
                                    GradientStop {
                                        ratio: 0,
                                        color: [20, 220, 180, 255],
                                    },
                                    GradientStop {
                                        ratio: 255,
                                        color: [40, 80, 255, 255],
                                    },
                                ],
                                focal_point_ratio: 0.0,
                                ..GradientPaint::default()
                            }),
                            stroke_gradient: None,
                            stroke_width: 2.0,
                        },
                        ..Element::default()
                    }],
                    property_expressions: vec![PropertyExpression {
                        property: TweenProperty::Y,
                        expression: "sin(time * tau * freq) * amp".into(),
                        enabled: true,
                        amplitude: 18.0,
                        frequency: 1.5,
                        ..PropertyExpression::default()
                    }],
                    ..Keyframe::default()
                }],
                properties: Default::default(),
            },
            Layer {
                id: "l-morph".into(),
                name: "Star Morph".into(),
                visible: true,
                locked: false,
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        tween: Some(Easing::EaseInOut),
                        tween_kind: TweenKind::Shape,
                        elements: vec![Element {
                            id: "m1".into(),
                            name: String::new(),
                            transform: Transform {
                                x: 300.0,
                                y: 200.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 60.0,
                                height: 60.0,
                                fill: [255, 90, 120, 255],
                                stroke: [0; 4],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                    Keyframe {
                        frame: 12,
                        elements: vec![Element {
                            id: "m2".into(),
                            name: String::new(),
                            transform: Transform {
                                x: 360.0,
                                y: 220.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Ellipse {
                                width: 90.0,
                                height: 90.0,
                                fill: [255, 90, 120, 255],
                                stroke: [0; 4],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    },
                ],
                properties: Default::default(),
            },
            Layer {
                id: "l-wheel".into(),
                name: "Wheel Rig".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "inst-wheel".into(),
                        name: "wheel_mc".into(),
                        transform: Transform {
                            x: 200.0,
                            y: 400.0,
                            ..Transform::default()
                        },
                        scripts: vec![ObjectScript {
                            event: ObjectEvent::EnterFrame,
                            script: Script {
                                language: ScriptLanguage::ActionScript2,
                                source: "onClipEvent(enterFrame) { _rotation += 6; _x += 3; }"
                                    .into(),
                            },
                        }],
                        kind: ElementKind::Instance {
                            symbol_id: "sym-wheel".into(),
                            playback: InstancePlayback::Independent,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
                properties: Default::default(),
            },
            Layer {
                id: "l-hud".into(),
                name: "HUD".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![
                        Element {
                            id: "el-bmp".into(),
                            name: "badge".into(),
                            transform: Transform {
                                x: 40.0,
                                y: 40.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Bitmap {
                                resource_id: "res-bmp".into(),
                                width: 16,
                                height: 16,
                                smoothing: true,
                            },
                            ..Element::default()
                        },
                        Element {
                            id: "el-txt".into(),
                            name: "hud_txt".into(),
                            transform: Transform {
                                x: 70.0,
                                y: 56.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Text {
                                text: "Frame 1".into(),
                                font_family: "Helvetica".into(),
                                font_size: 20.0,
                                color: [255, 255, 255, 255],
                            },
                            ..Element::default()
                        },
                    ],
                    ..Keyframe::default()
                }],
                properties: Default::default(),
            },
        ];

        project.upgrade_format_for_features();
        project.validate().expect("valid test project");

        let dir = std::env::temp_dir().join(format!("nf-aep-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let aep_out = dir.join("aurora_interchange.aep");

        let report = export_project_aep(&project, &aep_out, &AepOptions::default())
            .expect("export_project_aep succeeds");

        assert!(report.aep_path.exists());
        assert!(report.aepx_path.as_ref().unwrap().exists());
        assert!(report.jsx_path.as_ref().unwrap().exists());
        assert_eq!(report.composition_count, 2);
        assert!(report.vector_shape_layer_count >= 3);
        assert_eq!(report.text_layer_count, 1);
        assert_eq!(report.precomp_layer_count, 1);
        assert_eq!(report.footage_layer_count, 1);
        assert!(
            report.expression_count >= 5,
            "expected Y PropertyExpression + wheel Pos/Rot/TimeRemap + HUD SourceText expressions, got {}",
            report.expression_count
        );

        // Verify per-bitmap WebP export into `<stem>_Footage/` (`T-251` + `T-246`)
        assert_eq!(report.footage_files.len(), 1);
        assert_eq!(
            report.footage_files[0].extension().and_then(|e| e.to_str()),
            Some("webp")
        );

        // Verify RIFX / Egg! binary container magic & chunks
        let aep_bytes = fs::read(&report.aep_path).unwrap();
        assert_eq!(&aep_bytes[0..4], b"RIFX");
        assert_eq!(&aep_bytes[8..12], b"Egg!");
        assert!(aep_bytes.windows(4).any(|w| w == b"Cdta"));
        assert!(aep_bytes.windows(4).any(|w| w == b"shap"));
        assert!(aep_bytes.windows(4).any(|w| w == b"expr"));

        // Verify `.aepx` XML contains VectorGroup Bezier paths & translated Expressions
        let xml = fs::read_to_string(report.aepx_path.as_ref().unwrap()).unwrap();
        assert!(xml.contains("<AfterEffectsProject"));
        assert!(xml.contains("<BezierPath"));
        assert!(xml.contains("<Expression property=\"Rotation\">"));
        assert!(xml.contains("<Expression property=\"TimeRemap\">"));
        assert!(xml.contains("<Expression property=\"SourceText\">"));

        // Verify `.jsx` ExtendScript parses AND executes cleanly against an AE ExtendScript DOM mock,
        // and verify every generated AE JavaScript Expression evaluates to exact runtime values!
        let jsx_path = report.jsx_path.as_ref().unwrap();
        let jsx_text = fs::read_to_string(jsx_path).unwrap();
        assert!(jsx_text.contains("ADBE Vector Shape - Group"));
        assert!(jsx_text.contains("ADBE Vector Graphic - G-Fill"));
        assert!(jsx_text.contains("ADBE Time Remapping"));

        let harness_path = dir.join("verify_ae_dom_and_expressions.cjs");
        let harness = r#"
const fs = require('fs');
const vm = require('vm');
const jsxFile = process.argv[2];
const code = fs.readFileSync(jsxFile, 'utf8');

const expressions = [];
function makeProp(name) {
  const children = {};
  return {
    name,
    value: { text: '' },
    numKeys: 0,
    expression: '',
    property(childName) {
      if (!children[childName]) children[childName] = makeProp(childName);
      return children[childName];
    },
    addProperty(childName) {
      const p = makeProp(childName);
      children[childName + '_' + Object.keys(children).length] = p;
      return p;
    },
    setValue(v) { this.value = v; },
    setValueAtTime(_t, v) { this.numKeys += 1; this.value = v; },
    setInterpolationTypeAtKey() {},
    setTemporalEaseAtKey() {},
    children,
  };
}

const createdComps = [];
const sandbox = {
  Math, Number, String, Boolean, Array, Object, parseFloat, parseInt, isNaN, isFinite,
  $: { fileName: jsxFile },
  Folder: function(p) { this.fsName = p; this.exists = true; },
  File: function(p) { this.fsName = p; this.parent = { fsName: require('path').dirname(p) }; this.exists = fs.existsSync(p); },
  ImportOptions: function(f) { this.file = f; },
  Shape: function() { this.vertices = []; this.inTangents = []; this.outTangents = []; this.closed = false; },
  MarkerValue: function(c) { this.comment = c; this.chapter = ''; this.duration = 0; },
  KeyframeEase: function(s, i) { this.speed = s; this.influence = i; },
  KeyframeInterpolationType: { HOLD: 'HOLD', BEZIER: 'BEZIER', LINEAR: 'LINEAR' },
  ParagraphJustification: { LEFT_JUSTIFY: 0, CENTER_JUSTIFY: 1, RIGHT_JUSTIFY: 2 },
  AutoOrientType: { ALONG_PATH: 1 },
  BlendingMode: new Proxy({}, { get: (_t, k) => k }),
  TrackMatteType: new Proxy({}, { get: (_t, k) => k }),
  app: {
    beginUndoGroup() {},
    endUndoGroup() {},
    newProject() {},
    project: {
      file: null,
      save() {},
      importFile(io) { return { file: io.file, mainSource: { loop: 1 }, parentFolder: null }; },
      items: {
        addFolder(name) { return { name, parentFolder: null }; },
        addComp(name, width, height, aspect, duration, frameRate) {
          const layers = [];
          const comp = {
            name, width, height, aspect, duration, frameRate, bgColor: [0,0,0],
            markerProperty: makeProp('Marker'),
            addGuide() {},
            openInViewer() {},
            layers: {
              items: layers,
              _mk(kind) {
                const root = makeProp('LayerRoot');
                const lyr = {
                  kind, name: '', startTime: 0, inPoint: 0, outPoint: duration,
                  enabled: true, locked: false, threeDLayer: false, timeRemapEnabled: false,
                  property: (n) => root.property(n),
                  _root: root,
                };
                layers.unshift(lyr);
                return lyr;
              },
              addShape() { return this._mk('shape'); },
              addText(t) { const l = this._mk('text'); l.property('ADBE Text Properties').property('ADBE Text Document').value = { text: t }; return l; },
              addBoxText() { return this._mk('boxText'); },
              add(item) { return this._mk('av:' + (item.name || 'footage')); },
              addNull() { return this._mk('null'); },
              addCamera(n) { const l = this._mk('camera'); l.name = n; return l; },
            },
          };
          createdComps.push(comp);
          return comp;
        },
      },
    },
  },
};

vm.runInNewContext(code, sandbox, { filename: 'aurora_interchange.jsx' });
if (createdComps.length !== 2) throw new Error('Expected 2 comps, got ' + createdComps.length);

// Collect all non-empty expressions across all layers and evaluate them at frame 0 and frame 6!
function collectExprs(comp, layer, prop, path) {
  if (prop.expression && prop.expression.trim().length > 0) {
    expressions.push({ comp: comp.name, layer: layer.name, prop: prop.name, value: prop.value, code: prop.expression });
  }
  for (const k of Object.keys(prop.children)) {
    collectExprs(comp, layer, prop.children[k], path + '/' + k);
  }
}
for (const c of createdComps) {
  for (const l of c.layers.items) {
    collectExprs(c, l, l._root, '');
  }
}
if (expressions.length < 5) throw new Error('Expected >= 5 expressions, got ' + expressions.length);

function evalAeExpr(exprObj, frame, fps) {
  const time = frame / fps;
  const baseVal = Array.isArray(exprObj.value) ? exprObj.value.slice() : (typeof exprObj.value === 'number' ? exprObj.value : [200, 400]);
  const ctx = {
    Math, Number, String, Boolean, Array, Object,
    time,
    inPoint: 0,
    outPoint: 1,
    value: baseVal,
    thisComp: { frameDuration: 1 / fps, duration: 24 / fps, width: 800, height: 600 },
    thisLayer: { index: 1, inPoint: 0, outPoint: 1 },
    timeToFrames: (t) => Math.floor((t === undefined ? time : t) * fps + 1e-6),
    framesToTime: (f) => f / fps,
    clamp: (v, a, b) => Math.min(Math.max(v, a), b),
    linear: (t, t1, t2, v1, v2) => v1 + ((t - t1) / Math.max(1e-6, t2 - t1)) * (v2 - v1),
    ease: (t, t1, t2, v1, v2) => v1 + ((t - t1) / Math.max(1e-6, t2 - t1)) * (v2 - v1),
    wiggle: () => baseVal,
  };
  return vm.runInNewContext(exprObj.code, ctx);
}

// 1. Verify wheel_mc Rotate Z expression (`_rotation += 6` => +36 deg at frame 6)
const wheelRot = expressions.find(e => e.layer.includes('wheel_mc') && e.prop === 'ADBE Rotate Z');
if (!wheelRot) throw new Error('Missing wheel_mc rotation expression in ' + JSON.stringify(expressions.map(e => e.layer + ':' + e.prop)));
wheelRot.value = 0;
const rot6 = evalAeExpr(wheelRot, 6, 24);
if (Math.abs(rot6 - 36) > 1e-3) throw new Error('Expected wheel_mc rot@6 = 36, got ' + rot6);

// 2. Verify wheel_mc Time Remap loop (`gotoAndPlay("spin")` at frame 11 -> wraps frame 14 back into [0..11])
const wheelTr = expressions.find(e => e.layer.includes('wheel_mc') && e.prop === 'ADBE Time Remapping');
if (!wheelTr) throw new Error('Missing wheel_mc Time Remapping expression');
const tr14 = evalAeExpr(wheelTr, 14, 24);
if (!(tr14 >= 0 && tr14 < 12 / 24)) throw new Error('Expected looped TimeRemap in [0, 0.5), got ' + tr14);

// 3. Verify hud_txt SourceText expression (`"Frame " + _currentframe` -> "Frame 7" at frame index 6)
const hudTxt = expressions.find(e => e.layer.includes('hud_txt') && e.prop === 'ADBE Text Document');
if (!hudTxt) throw new Error('Missing hud_txt SourceText expression');
const txt6 = evalAeExpr(hudTxt, 6, 24);
if (txt6 !== 'Frame 7') throw new Error('Expected hud_txt@6 = "Frame 7", got ' + JSON.stringify(txt6));

// 4. Verify Aurora Vector Y PropertyExpression (`sin(time * tau * freq) * amp`)
const auroraPos = expressions.find(e => e.layer.includes('Aurora Vector') && e.prop === 'ADBE Position');
if (!auroraPos) throw new Error('Missing Aurora Vector Position expression');
auroraPos.value = [100, 120];
const pos4 = evalAeExpr(auroraPos, 4, 24);
if (!Array.isArray(pos4) || Math.abs(pos4[0] - 100) > 1e-3 || Math.abs(pos4[1] - 120) < 1.0) {
  throw new Error('Expected oscillating Y on Aurora Vector, got ' + JSON.stringify(pos4));
}
"#;
        fs::write(&harness_path, harness).unwrap();
        let output = Command::new("node")
            .arg(&harness_path)
            .arg(jsx_path)
            .output()
            .expect("node must be available to run AE ExtendScript + Expression verification");
        assert!(
            output.status.success(),
            "AE ExtendScript & JS Expression runtime verification failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn t259_export_hardening_smoothing_false_banner_clicktag_and_aep_edge_cases() {
        // 1. `apply_bitmap_export_settings`: `allow_smoothing == false` must turn smoothing OFF even when element defaulted to `true`!
        let mut proj = Project::blank();
        let rgba = image::RgbaImage::from_pixel(2, 2, image::Rgba([240, 120, 40, 128]));
        let mut png_bytes = Vec::new();
        rgba.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        let mut res =
            ResourceUnit::from_bytes("bmp_sharp", "icons/---.png", "image/png", &png_bytes)
                .unwrap();
        res.bitmap_export = Some(BitmapExportSettings {
            compression: BitmapCompression::LosslessPng,
            allow_smoothing: false,
            ..BitmapExportSettings::default()
        });
        proj.resources.push(res);
        let mut webp_bytes = Vec::new();
        rgba.write_to(
            &mut std::io::Cursor::new(&mut webp_bytes),
            image::ImageFormat::WebP,
        )
        .unwrap();
        proj.resources.push(
            ResourceUnit::from_bytes("bmp_webp_raw", "icons/raw.webp", "image/webp", &webp_bytes)
                .unwrap(),
        );
        proj.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "bmp_el".into(),
                name: "constructor".into(),
                kind: ElementKind::Bitmap {
                    resource_id: "bmp_sharp".into(),
                    width: 2,
                    height: 2,
                    smoothing: true,
                },
                ..Element::default()
            });

        let mut derived = proj.clone();
        let (_, _) = crate::apply_bitmap_export_settings(&mut derived, 80);
        let ElementKind::Bitmap { smoothing, .. } =
            &derived.symbols[0].timeline.layers[0].keyframes[0].elements[0].kind
        else {
            panic!("expected bitmap");
        };
        assert!(
            !*smoothing,
            "allow_smoothing: false must turn bitmap smoothing OFF"
        );

        // 2. Banner `click_tag_url` rejects `javascript:` and escapes `</script>` in valid `https://` URLs
        let mut banner_proj = Project::blank();
        banner_proj.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "cta".into(),
                kind: ElementKind::Rectangle {
                    width: 100.0,
                    height: 40.0,
                    fill: [255, 80, 40, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            });
        let dir = std::env::temp_dir().join(format!("nf_aep_t259_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let js_err = crate::export_native_banner_html(
            &banner_proj,
            dir.join("bad_banner.html"),
            &crate::BannerOptions {
                click_tag_url: Some("javascript:alert(1)".into()),
                ..crate::BannerOptions::default()
            },
        );
        assert!(js_err.is_err(), "javascript: clickTag must be rejected");

        let safe_rep = crate::export_native_banner_html(
            &banner_proj,
            dir.join("safe_banner.html"),
            &crate::BannerOptions {
                click_tag_url: Some("https://example.com/?q=</script>".into()),
                ..crate::BannerOptions::default()
            },
        )
        .unwrap();
        let safe_html = fs::read_to_string(&safe_rep.html_path).unwrap();
        assert!(
            !safe_html.contains("?q=</script>"),
            "inline script literal must escape </script>"
        );

        // 3. AEP export handles WebP->PNG footage transcode, Object.prototype-colliding names (`constructor`),
        //    `var` / identifier-boundary `_currentframe`, and pipe/newline marker sanitization.
        proj.symbols[0].folder = "constructor/toString".into();
        proj.symbols[0].timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "var my_currentframe = 10;\n// pipe | comment\n".into(),
            },
        });
        let report = export_project_aep(&proj, dir.join("hardened.aep"), &AepOptions::default())
            .expect("export hardened AEP");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "AE_WEBP_TRANSCODED_PNG"),
            "WebP bitmap should transcode to PNG footage: {:?}",
            report.diagnostics
        );
        let jsx = fs::read_to_string(report.jsx_path.unwrap()).unwrap();
        assert!(jsx.contains("Object.prototype.hasOwnProperty.call"));
        let _ = fs::remove_dir_all(&dir);
    }
}
