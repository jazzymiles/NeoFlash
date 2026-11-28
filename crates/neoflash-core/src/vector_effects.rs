//! Advanced vector effects and mesh/perspective authoring (`T-120`):
//! - Freeform (point & line) gradients (`FreeformGradient`, `sample_freeform_gradient`)
//! - Envelope distort (`EnvelopeWarpPreset`, `EnvelopeWarp`, `apply_envelope_warp_to_path`)
//! - Multi-step vector blends (`VectorBlendSpacing`, `VectorBlendSpec`, `generate_vector_blend_steps`)
//! - 2D Gradient Mesh patches (`GradientMesh`, `GradientMeshPatchVertex`)
//! - 1/2/3-point Perspective Grid projection & snapping (`PerspectivePlane`, `PerspectiveGridSpec`)
//! - Procedural path effects (`ZigZag`, `PuckerBloat`, `Roughen`, `Twist`, `OffsetPath`)
//! - Deterministic evaluation and `expand_vector_effects` baking into editable contours.

use crate::{
    Color, Element, ElementKind, Id, Keyframe, PathCommand, Timeline, path_bounds,
    shape_tween_elements,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// 1. Freeform Gradient (Points & Lines)
// ============================================================================

/// A colouremitter point inside a [`FreeformGradient`] (`T-120`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreeformGradientPoint {
    pub id: Id,
    pub position: [f64; 2],
    pub color: Color,
    pub radius: f64,
    pub weight: f64,
}

/// A polyline colour emitter inside a [`FreeformGradient`] (`T-120`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreeformGradientLine {
    pub points: Vec<[f64; 2]>,
    pub start_color: Color,
    pub end_color: Color,
}

/// Freeform point-and-line colour gradient (`T-120`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FreeformGradient {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<FreeformGradientPoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<FreeformGradientLine>,
}

fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn lerp_color(a: Color, b: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|i| {
        lerp_f64(f64::from(a[i]), f64::from(b[i]), t)
            .round()
            .clamp(0.0, 255.0) as u8
    })
}

/// Sample a [`FreeformGradient`] at local coordinate `pt` using inverse-distance weighting
/// across point stops and polyline segments (`T-120`).
pub fn sample_freeform_gradient(grad: &FreeformGradient, pt: [f64; 2]) -> Color {
    let mut sum_r = 0.0;
    let mut sum_g = 0.0;
    let mut sum_b = 0.0;
    let mut sum_a = 0.0;
    let mut total_w = 0.0;

    for stop in &grad.points {
        let dx = pt[0] - stop.position[0];
        let dy = pt[1] - stop.position[1];
        let dist2 = dx * dx + dy * dy;
        if dist2 < 1e-9 {
            return stop.color;
        }
        let rad = stop.radius.max(1.0);
        let w = stop.weight.max(0.01) * (rad * rad) / (dist2 + 1e-3);
        sum_r += f64::from(stop.color[0]) * w;
        sum_g += f64::from(stop.color[1]) * w;
        sum_b += f64::from(stop.color[2]) * w;
        sum_a += f64::from(stop.color[3]) * w;
        total_w += w;
    }

    for line in &grad.lines {
        if line.points.len() < 2 {
            continue;
        }
        let mut total_len = 0.0;
        let mut seg_lengths = Vec::with_capacity(line.points.len() - 1);
        for pair in line.points.windows(2) {
            let d = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
            seg_lengths.push(d);
            total_len += d;
        }
        let mut accum = 0.0;
        for (pair, &seg_len) in line.points.windows(2).zip(&seg_lengths) {
            let a = pair[0];
            let b = pair[1];
            let vx = b[0] - a[0];
            let vy = b[1] - a[1];
            let len2 = vx * vx + vy * vy;
            let u = if len2 > 1e-9 {
                (((pt[0] - a[0]) * vx + (pt[1] - a[1]) * vy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let proj_x = a[0] + vx * u;
            let proj_y = a[1] + vy * u;
            let dist2 = (pt[0] - proj_x).powi(2) + (pt[1] - proj_y).powi(2);
            let arc_t = if total_len > 1e-9 {
                (accum + seg_len * u) / total_len
            } else {
                u
            };
            let c = lerp_color(line.start_color, line.end_color, arc_t);
            if dist2 < 1e-9 {
                return c;
            }
            let w = 1000.0 / (dist2 + 1e-3);
            sum_r += f64::from(c[0]) * w;
            sum_g += f64::from(c[1]) * w;
            sum_b += f64::from(c[2]) * w;
            sum_a += f64::from(c[3]) * w;
            total_w += w;
            accum += seg_len;
        }
    }

    if total_w <= 1e-12 {
        return [0, 0, 0, 255];
    }
    [
        (sum_r / total_w).round().clamp(0.0, 255.0) as u8,
        (sum_g / total_w).round().clamp(0.0, 255.0) as u8,
        (sum_b / total_w).round().clamp(0.0, 255.0) as u8,
        (sum_a / total_w).round().clamp(0.0, 255.0) as u8,
    ]
}

// ============================================================================
// 2. Envelope Distort / Warp
// ============================================================================

/// Parametric envelope warp presets (`Make with Warp` / `Make with Mesh`, `T-120`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeWarpPreset {
    #[default]
    Arc,
    Arch,
    Bulge,
    Flag,
    Wave,
    Fish,
    Rise,
    Squeeze,
    Twist,
    CustomGrid,
}

/// Envelope distortion parameters applied to a vector shape (`T-120`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvelopeWarp {
    #[serde(default)]
    pub preset: EnvelopeWarpPreset,
    /// Normalized bend in `-1.0..=1.0`.
    #[serde(default)]
    pub bend: f64,
    /// Horizontal distortion in `-1.0..=1.0`.
    #[serde(default)]
    pub horizontal_distortion: f64,
    /// Vertical distortion in `-1.0..=1.0`.
    #[serde(default)]
    pub vertical_distortion: f64,
    #[serde(default = "default_grid_dim")]
    pub rows: u32,
    #[serde(default = "default_grid_dim")]
    pub cols: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control_points: Vec<[f64; 2]>,
}

const fn default_grid_dim() -> u32 {
    4
}

impl Default for EnvelopeWarp {
    fn default() -> Self {
        Self {
            preset: EnvelopeWarpPreset::Arc,
            bend: 0.5,
            horizontal_distortion: 0.0,
            vertical_distortion: 0.0,
            rows: 4,
            cols: 4,
            control_points: Vec::new(),
        }
    }
}

/// Deform a 2D point `pt` inside `bounds = [min_x, min_y, width, height]` according to `warp` (`T-120`).
pub fn apply_envelope_warp_to_point(
    warp: &EnvelopeWarp,
    pt: [f64; 2],
    bounds: [f64; 4],
) -> [f64; 2] {
    let min_x = bounds[0];
    let min_y = bounds[1];
    let w = bounds[2].max(1e-6);
    let h = bounds[3].max(1e-6);
    let u = ((pt[0] - min_x) / w).clamp(0.0, 1.0);
    let v = ((pt[1] - min_y) / h).clamp(0.0, 1.0);
    let nx = u * 2.0 - 1.0; // -1..1
    let ny = v * 2.0 - 1.0; // -1..1
    let bend = warp.bend.clamp(-2.0, 2.0);

    let (mut dx, mut dy) = match warp.preset {
        EnvelopeWarpPreset::Arc | EnvelopeWarpPreset::Arch => {
            let arch = (1.0 - nx * nx) * bend * h * 0.5;
            (0.0, -arch)
        }
        EnvelopeWarpPreset::Bulge => {
            let factor = (1.0 - nx * nx) * bend * 0.5;
            (nx * (1.0 - ny * ny) * bend * w * 0.25, ny * factor * h)
        }
        EnvelopeWarpPreset::Flag | EnvelopeWarpPreset::Wave => {
            let wave = (u * std::f64::consts::TAU).sin() * bend * h * 0.35;
            (0.0, wave)
        }
        EnvelopeWarpPreset::Fish => {
            let pinch = (u * std::f64::consts::TAU).sin() * bend;
            (0.0, ny * pinch * h * 0.35)
        }
        EnvelopeWarpPreset::Rise => {
            let rise = u * bend * h * 0.5;
            (0.0, -rise)
        }
        EnvelopeWarpPreset::Squeeze => {
            let sq = (1.0 - ny * ny) * bend * w * 0.3;
            (-nx * sq, 0.0)
        }
        EnvelopeWarpPreset::Twist => {
            let r = (nx * nx + ny * ny).sqrt();
            let angle = (1.0 - r.min(1.0)) * bend * std::f64::consts::PI;
            let (s, c) = angle.sin_cos();
            let rx = nx * c - ny * s;
            let ry = nx * s + ny * c;
            ((rx - nx) * w * 0.5, (ry - ny) * h * 0.5)
        }
        EnvelopeWarpPreset::CustomGrid => {
            let rows = warp.rows.max(1) as usize;
            let cols = warp.cols.max(1) as usize;
            let grid_len = rows
                .checked_add(1)
                .and_then(|rows| cols.checked_add(1).and_then(|cols| rows.checked_mul(cols)));
            if grid_len.is_some_and(|len| warp.control_points.len() == len) {
                let gx = u * (cols as f64);
                let gy = v * (rows as f64);
                let c0 = (gx.floor() as usize).min(cols.saturating_sub(1));
                let r0 = (gy.floor() as usize).min(rows.saturating_sub(1));
                let c1 = (c0 + 1).min(cols);
                let r1 = (r0 + 1).min(rows);
                let fu = gx - (c0 as f64);
                let fv = gy - (r0 as f64);
                let idx = |r: usize, c: usize| r * (cols + 1) + c;
                let p00 = warp.control_points[idx(r0, c0)];
                let p10 = warp.control_points[idx(r0, c1)];
                let p01 = warp.control_points[idx(r1, c0)];
                let p11 = warp.control_points[idx(r1, c1)];
                let bx = lerp_f64(
                    lerp_f64(p00[0], p10[0], fu),
                    lerp_f64(p01[0], p11[0], fu),
                    fv,
                );
                let by = lerp_f64(
                    lerp_f64(p00[1], p10[1], fu),
                    lerp_f64(p01[1], p11[1], fu),
                    fv,
                );
                return [bx, by];
            }
            (0.0, 0.0)
        }
    };

    dx += ny * warp.horizontal_distortion.clamp(-1.0, 1.0) * w * 0.25;
    dy += nx * warp.vertical_distortion.clamp(-1.0, 1.0) * h * 0.25;
    [pt[0] + dx, pt[1] + dy]
}

/// Apply [`EnvelopeWarp`] across all control and anchor points of a vector path (`T-120`).
pub fn apply_envelope_warp_to_path(
    warp: &EnvelopeWarp,
    commands: &[PathCommand],
) -> Vec<PathCommand> {
    let Ok(Some(b)) = path_bounds(commands) else {
        return commands.to_vec();
    };
    let bounds = [
        b.x_min,
        b.y_min,
        (b.x_max - b.x_min).max(1.0),
        (b.y_max - b.y_min).max(1.0),
    ];
    commands
        .iter()
        .map(|cmd| match *cmd {
            PathCommand::MoveTo { x, y } => {
                let [nx, ny] = apply_envelope_warp_to_point(warp, [x, y], bounds);
                PathCommand::MoveTo { x: nx, y: ny }
            }
            PathCommand::LineTo { x, y } => {
                let [nx, ny] = apply_envelope_warp_to_point(warp, [x, y], bounds);
                PathCommand::LineTo { x: nx, y: ny }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let [ncx, ncy] = apply_envelope_warp_to_point(warp, [cx, cy], bounds);
                let [nx, ny] = apply_envelope_warp_to_point(warp, [x, y], bounds);
                PathCommand::QuadraticTo {
                    cx: ncx,
                    cy: ncy,
                    x: nx,
                    y: ny,
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
                let [ncx1, ncy1] = apply_envelope_warp_to_point(warp, [cx1, cy1], bounds);
                let [ncx2, ncy2] = apply_envelope_warp_to_point(warp, [cx2, cy2], bounds);
                let [nx, ny] = apply_envelope_warp_to_point(warp, [x, y], bounds);
                PathCommand::CubicTo {
                    cx1: ncx1,
                    cy1: ncy1,
                    cx2: ncx2,
                    cy2: ncy2,
                    x: nx,
                    y: ny,
                }
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

// ============================================================================
// 3. Multi-Step Vector Blend
// ============================================================================

/// Spacing mode for a multi-step vector blend between two shapes (`T-120`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum VectorBlendSpacing {
    SpecifiedSteps(u32),
    SmoothColor,
    SpecifiedDistance(f64),
}

impl Default for VectorBlendSpacing {
    fn default() -> Self {
        Self::SpecifiedSteps(4)
    }
}

/// Configuration for a multi-step vector blend (`T-120`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorBlendSpec {
    pub target_element_id: Id,
    #[serde(default)]
    pub spacing: VectorBlendSpacing,
    #[serde(default)]
    pub align_to_path: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spine: Option<Vec<PathCommand>>,
}

/// Generate intermediate vector blend step elements between `start_element` and `end_element` (`T-120`).
pub fn generate_vector_blend_steps(
    start_element: &Element,
    end_element: &Element,
    spec: &VectorBlendSpec,
) -> Vec<Element> {
    let dist = (end_element.transform.x - start_element.transform.x)
        .hypot(end_element.transform.y - start_element.transform.y);
    let step_count = match spec.spacing {
        VectorBlendSpacing::SpecifiedSteps(n) => n.clamp(1, 128) as usize,
        VectorBlendSpacing::SmoothColor => 8usize,
        VectorBlendSpacing::SpecifiedDistance(step_dist) => {
            let d = step_dist.max(1.0);
            ((dist / d).floor() as usize).clamp(1, 128)
        }
    };

    let spine_guide = spec
        .spine
        .as_ref()
        .and_then(|cmds| crate::motion_path_polyline(cmds));

    let mut out = Vec::with_capacity(step_count);
    for i in 1..=step_count {
        let t = (i as f64) / ((step_count + 1) as f64);
        let morphed = shape_tween_elements(
            std::slice::from_ref(start_element),
            std::slice::from_ref(end_element),
            t,
        );
        let mut step_el = morphed
            .into_iter()
            .next()
            .unwrap_or_else(|| start_element.clone());
        // Evaluation must be pure: allocating a fresh global ID here made the same authored
        // frame produce different runtime paths on every sample. Keep each generated step tied
        // to its source element and ordinal instead (matching the deterministic IDs used by the
        // other non-destructive appearance expanders).
        step_el.id = format!("{}-blend-step-{i}", start_element.id);
        step_el.name = format!("{} Blend {}", start_element.name, i);
        step_el.transform.x = lerp_f64(start_element.transform.x, end_element.transform.x, t);
        step_el.transform.y = lerp_f64(start_element.transform.y, end_element.transform.y, t);
        step_el.transform.scale_x = lerp_f64(
            start_element.transform.scale_x,
            end_element.transform.scale_x,
            t,
        );
        step_el.transform.scale_y = lerp_f64(
            start_element.transform.scale_y,
            end_element.transform.scale_y,
            t,
        );
        step_el.transform.rotation = lerp_f64(
            start_element.transform.rotation,
            end_element.transform.rotation,
            t,
        );
        step_el.transform.alpha = lerp_f64(
            start_element.transform.alpha,
            end_element.transform.alpha,
            t,
        );

        if let Some(guide) = &spine_guide {
            let s = guide.length() * t;
            let [px, py] = guide.point_at(s);
            step_el.transform.x = px;
            step_el.transform.y = py;
            if spec.align_to_path {
                step_el.transform.rotation += guide.tangent_at(s);
            }
        }
        out.push(step_el);
    }
    out
}

// ============================================================================
// 4. 2D Gradient Mesh
// ============================================================================

/// One vertex of a 2D [`GradientMesh`] with position, tangent handles, and RGBA colour (`T-120`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientMeshPatchVertex {
    pub position: [f64; 2],
    pub handle_u: [f64; 2],
    pub handle_v: [f64; 2],
    pub color: Color,
}

/// Authorable `rows x cols` 2D Gradient Mesh patch grid (`T-120`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientMesh {
    pub rows: u32,
    pub cols: u32,
    pub vertices: Vec<GradientMeshPatchVertex>,
}

/// Maximum number of bicubic patches permitted in a single [`VectorMeshPaint`] (`T-501`).
pub const MAX_VECTOR_MESH_PATCHES: usize = 4096;
/// Maximum number of Gouraud triangles permitted in a single [`VectorMeshPaint`] (`T-501`).
pub const MAX_VECTOR_MESH_TRIANGLES: usize = 32_768;

/// One 16-control-point bicubic Tensor-Product Bézier patch (`4 x 4` row-major `points[r * 4 + c]`,
/// where `r` steps `v` in `0..=3` and `c` steps `u` in `0..=3`) with RGBA corner colours
/// `[c00 (u=0,v=0), c03 (u=1,v=0), c30 (u=0,v=1), c33 (u=1,v=1)]` (`T-501`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorMeshPatch {
    pub points: [[f64; 2]; 16],
    pub colors: [Color; 4],
}

impl VectorMeshPatch {
    /// Promote a 12-point bicubic Coons patch perimeter (arranged as row-major boundary points
    /// `[P00, P01, P02, P03, P13, P23, P33, P32, P31, P30, P20, P10]`, clockwise from `(u=0, v=0)`
    /// along `v=0`, `u=1`, `v=1` reversed, `u=0` reversed) and 4 corner colours
    /// `[c00, c03, c30, c33]` into an exact 16-point Tensor-Product Bézier patch
    /// (ISO 32000-1 §8.7.4.5.7 / SVG2 Mesh Gradients).
    pub fn from_coons_12(b: [[f64; 2]; 12], colors: [Color; 4]) -> Self {
        let p00 = b[0];
        let p01 = b[1];
        let p02 = b[2];
        let p03 = b[3];
        let p13 = b[4];
        let p23 = b[5];
        let p33 = b[6];
        let p32 = b[7];
        let p31 = b[8];
        let p30 = b[9];
        let p20 = b[10];
        let p10 = b[11];

        let coons_inner = |a00: [f64; 2],
                           a01: [f64; 2],
                           a10: [f64; 2],
                           a03: [f64; 2],
                           a30: [f64; 2],
                           a31: [f64; 2],
                           a13: [f64; 2],
                           a33: [f64; 2]|
         -> [f64; 2] {
            [
                (-4.0 * a00[0] + 6.0 * (a01[0] + a10[0]) - 2.0 * (a03[0] + a30[0])
                    + 3.0 * (a31[0] + a13[0])
                    - a33[0])
                    / 9.0,
                (-4.0 * a00[1] + 6.0 * (a01[1] + a10[1]) - 2.0 * (a03[1] + a30[1])
                    + 3.0 * (a31[1] + a13[1])
                    - a33[1])
                    / 9.0,
            ]
        };

        let p11 = coons_inner(p00, p01, p10, p03, p30, p31, p13, p33);
        let p12 = coons_inner(p03, p02, p13, p00, p33, p32, p10, p30);
        let p21 = coons_inner(p30, p31, p20, p33, p00, p01, p23, p03);
        let p22 = coons_inner(p33, p32, p23, p30, p03, p02, p20, p00);

        Self {
            points: [
                p00, p01, p02, p03, p10, p11, p12, p13, p20, p21, p22, p23, p30, p31, p32, p33,
            ],
            colors,
        }
    }

    /// Construct from PDF `/ShadingType 6` Coons patch stream order (`p0..p11` where `p0..p3` step
    /// `v: 0->1` at `u=0`, `p3..p6` step `u: 0->1` at `v=1`, `p6..p9` step `v: 1->0` at `u=1`,
    /// `p9..p11,p0` step `u: 1->0` at `v=0`, and PDF corner colours `[c0=(0,0), c1=(0,1), c2=(1,1), c3=(1,0)]`).
    pub fn from_pdf_coons_12(p: [[f64; 2]; 12], pdf_colors: [Color; 4]) -> Self {
        let boundary = [
            p[0], p[11], p[10], p[9], p[8], p[7], p[6], p[5], p[4], p[3], p[2], p[1],
        ];
        Self::from_coons_12(
            boundary,
            [pdf_colors[0], pdf_colors[3], pdf_colors[1], pdf_colors[2]],
        )
    }

    /// Construct from PDF `/ShadingType 7` Tensor-Product patch stream spiral order (`p0..p15` per
    /// ISO 32000-1 Table 83 and PDF corner colours `[c0=(0,0), c1=(0,1), c2=(1,1), c3=(1,0)]`).
    pub fn from_pdf_tensor_16(p: [[f64; 2]; 16], pdf_colors: [Color; 4]) -> Self {
        Self {
            points: [
                p[0], p[11], p[10], p[9], p[1], p[12], p[15], p[8], p[2], p[13], p[14], p[7], p[3],
                p[4], p[5], p[6],
            ],
            colors: [pdf_colors[0], pdf_colors[3], pdf_colors[1], pdf_colors[2]],
        }
    }

    /// Convert back to PDF `/ShadingType 7` spiral point order `p0..p15` and PDF corner colours
    /// `[c0=(0,0), c1=(0,1), c2=(1,1), c3=(1,0)]` for lossless PDF/AI vector export.
    pub fn to_pdf_tensor_16(&self) -> ([[f64; 2]; 16], [Color; 4]) {
        let m = &self.points;
        let pdf_pts = [
            m[0], m[4], m[8], m[12], m[13], m[14], m[15], m[11], m[7], m[3], m[2], m[1], m[5],
            m[9], m[10], m[6],
        ];
        let pdf_cols = [
            self.colors[0],
            self.colors[2],
            self.colors[3],
            self.colors[1],
        ];
        (pdf_pts, pdf_cols)
    }

    /// Evaluate the bicubic Bernstein surface `S(u, v)` and smooth corner colour at `(u, v)` in `0.0..=1.0`.
    pub fn evaluate(&self, u: f64, v: f64, linear_rgb: bool) -> ([f64; 2], Color) {
        let u = u.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        let iu = 1.0 - u;
        let iv = 1.0 - v;
        let bu = [iu * iu * iu, 3.0 * u * iu * iu, 3.0 * u * u * iu, u * u * u];
        let bv = [iv * iv * iv, 3.0 * v * iv * iv, 3.0 * v * v * iv, v * v * v];
        let mut x = 0.0;
        let mut y = 0.0;
        for (r, &wr) in bv.iter().enumerate() {
            for (c, &wc) in bu.iter().enumerate() {
                let w = wr * wc;
                let pt = self.points[r * 4 + c];
                x += w * pt[0];
                y += w * pt[1];
            }
        }
        let color = bilerp_patch_color(self.colors, u, v, linear_rgb);
        ([x, y], color)
    }
}

/// One Gouraud-interpolated vector triangle (`ShadingType 4` / `ShadingType 5` / Freeform / Angular) (`T-501`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorMeshTriangle {
    pub points: [[f64; 2]; 3],
    pub colors: [Color; 3],
}

///Tessellated vertex-coloured vector triangle buffer produced from a [`VectorMeshPaint`] (`T-501`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TessellatedColorMesh {
    pub vertices: Vec<([f64; 2], Color)>,
    pub indices: Vec<u32>,
}

/// True-vector mesh paint storing bicubic Tensor/Coons patches and/or Gouraud triangles in local
/// element space (`T-501`). Never rasterizes to a bitmap resource.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VectorMeshPaint {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<VectorMeshPatch>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triangles: Vec<VectorMeshTriangle>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub linear_rgb: bool,
}

fn srgb_to_lin_ch(c: u8) -> f64 {
    let s = f64::from(c) / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn lin_to_srgb_ch(l: f64) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let s = if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

fn bilerp_patch_color(colors: [Color; 4], u: f64, v: f64, linear_rgb: bool) -> Color {
    let [c00, c03, c30, c33] = colors;
    if linear_rgb {
        let mut rgb = [0u8; 3];
        for ch in 0..3 {
            let top = lerp_f64(srgb_to_lin_ch(c00[ch]), srgb_to_lin_ch(c03[ch]), u);
            let bot = lerp_f64(srgb_to_lin_ch(c30[ch]), srgb_to_lin_ch(c33[ch]), u);
            rgb[ch] = lin_to_srgb_ch(lerp_f64(top, bot, v));
        }
        let top_a = lerp_f64(f64::from(c00[3]), f64::from(c03[3]), u);
        let bot_a = lerp_f64(f64::from(c30[3]), f64::from(c33[3]), u);
        let a = lerp_f64(top_a, bot_a, v).round().clamp(0.0, 255.0) as u8;
        [rgb[0], rgb[1], rgb[2], a]
    } else {
        let top = lerp_color(c00, c03, u);
        let bot = lerp_color(c30, c33, u);
        lerp_color(top, bot, v)
    }
}

impl VectorMeshPaint {
    pub fn is_empty(&self) -> bool {
        self.patches.is_empty() && self.triangles.is_empty()
    }

    pub fn has_visible_alpha(&self) -> bool {
        self.patches
            .iter()
            .flat_map(|p| p.colors.iter())
            .chain(self.triangles.iter().flat_map(|t| t.colors.iter()))
            .any(|c| c[3] > 0)
    }

    pub fn apply_alpha_in_place(&mut self, alpha_mul: f64) {
        let m = alpha_mul.clamp(0.0, 1.0);
        if (m - 1.0).abs() <= 1e-9 {
            return;
        }
        for patch in &mut self.patches {
            for col in &mut patch.colors {
                col[3] = (f64::from(col[3]) * m).round().clamp(0.0, 255.0) as u8;
            }
        }
        for tri in &mut self.triangles {
            for col in &mut tri.colors {
                col[3] = (f64::from(col[3]) * m).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.is_empty() {
            return Err("vector mesh paint must contain at least one patch or triangle");
        }
        if self.patches.len() > MAX_VECTOR_MESH_PATCHES {
            return Err("vector mesh patch count exceeds 4096");
        }
        if self.triangles.len() > MAX_VECTOR_MESH_TRIANGLES {
            return Err("vector mesh triangle count exceeds 32768");
        }
        for patch in &self.patches {
            if !patch
                .points
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() <= 1e7)
            {
                return Err("vector mesh patch coordinates must be finite and within +-1e7");
            }
        }
        for tri in &self.triangles {
            if !tri
                .points
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() <= 1e7)
            {
                return Err("vector mesh triangle coordinates must be finite and within +-1e7");
            }
        }
        Ok(())
    }

    pub fn map_points_in_place(&mut self, mut f: impl FnMut([f64; 2]) -> [f64; 2]) {
        for patch in &mut self.patches {
            for pt in &mut patch.points {
                *pt = f(*pt);
            }
        }
        for tri in &mut self.triangles {
            for pt in &mut tri.points {
                *pt = f(*pt);
            }
        }
    }

    pub fn transform_in_place(&mut self, m: [f64; 6]) {
        self.map_points_in_place(|[x, y]| [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]);
    }

    /// Bounding box `[min_x, min_y, max_x, max_y]` across all patch control points and triangles.
    pub fn bounds(&self) -> Option<[f64; 4]> {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut any = false;
        for pt in self
            .patches
            .iter()
            .flat_map(|p| p.points.iter())
            .chain(self.triangles.iter().flat_map(|t| t.points.iter()))
        {
            if pt[0].is_finite() && pt[1].is_finite() {
                min_x = min_x.min(pt[0]);
                min_y = min_y.min(pt[1]);
                max_x = max_x.max(pt[0]);
                max_y = max_y.max(pt[1]);
                any = true;
            }
        }
        any.then_some([min_x, min_y, max_x, max_y])
    }

    /// Representative average colour (ensuring non-zero alpha when any vertex is visible) used for
    /// hit-testing, solid fallbacks, and swatch summaries.
    pub fn representative_color(&self) -> Color {
        let mut sum = [0u64; 4];
        let mut count = 0u64;
        let mut max_a = 0u8;
        for c in self
            .patches
            .iter()
            .flat_map(|p| p.colors.iter())
            .chain(self.triangles.iter().flat_map(|t| t.colors.iter()))
        {
            for i in 0..4 {
                sum[i] += u64::from(c[i]);
            }
            max_a = max_a.max(c[3]);
            count += 1;
        }
        if count == 0 {
            return [0, 0, 0, 255];
        }
        let mut out = [
            (sum[0] / count) as u8,
            (sum[1] / count) as u8,
            (sum[2] / count) as u8,
            (sum[3] / count) as u8,
        ];
        if out[3] == 0 && max_a > 0 {
            out[3] = 1;
        }
        out
    }

    /// Subdivide all bicubic patches and Gouraud triangles into a crack-free vertex-coloured
    /// triangle mesh in local element coordinates, applying `color_transform`.
    pub fn tessellate(
        &self,
        steps_override: Option<u8>,
        color_transform: crate::ColorTransform,
    ) -> TessellatedColorMesh {
        const MAX_TOTAL_VERTICES: usize = 65_536;
        let patch_count = self.patches.len();
        let default_steps: usize = if let Some(explicit) = steps_override {
            usize::from(explicit.clamp(2, 24))
        } else if patch_count == 0 {
            1
        } else if patch_count <= 16 {
            12
        } else if patch_count <= 64 {
            8
        } else if patch_count <= 256 {
            6
        } else {
            4
        };
        let max_steps_by_budget = MAX_TOTAL_VERTICES
            .saturating_sub(self.triangles.len() * 3)
            .checked_div(patch_count)
            .map_or(default_steps, |raw_v| {
                (((raw_v.max(9)) as f64).sqrt().floor() as usize)
                    .saturating_sub(1)
                    .clamp(2, 24)
            });
        let steps = default_steps.min(max_steps_by_budget);

        let mut out = TessellatedColorMesh {
            vertices: Vec::with_capacity(
                patch_count
                    .saturating_mul((steps + 1) * (steps + 1))
                    .saturating_add(self.triangles.len() * 3)
                    .min(MAX_TOTAL_VERTICES),
            ),
            indices: Vec::with_capacity(
                patch_count
                    .saturating_mul(steps * steps * 6)
                    .saturating_add(self.triangles.len() * 3)
                    .min(MAX_TOTAL_VERTICES * 6),
            ),
        };

        for patch in &self.patches {
            if out.vertices.len().saturating_add((steps + 1) * (steps + 1)) > MAX_TOTAL_VERTICES {
                break;
            }
            let base = out.vertices.len() as u32;
            let stride = (steps + 1) as u32;
            for r in 0..=steps {
                let v = (r as f64) / (steps as f64);
                for c in 0..=steps {
                    let u = (c as f64) / (steps as f64);
                    let (pt, col) = patch.evaluate(u, v, self.linear_rgb);
                    out.vertices.push((pt, color_transform.apply(col)));
                }
            }
            for r in 0..(steps as u32) {
                for c in 0..(steps as u32) {
                    let i00 = base + r * stride + c;
                    let i10 = i00 + 1;
                    let i01 = i00 + stride;
                    let i11 = i01 + 1;
                    // Alternate diagonal on checkerboard cells to eliminate directional Gouraud Mach banding
                    if (r + c) % 2 == 0 {
                        out.indices
                            .extend_from_slice(&[i00, i10, i11, i00, i11, i01]);
                    } else {
                        out.indices
                            .extend_from_slice(&[i00, i10, i01, i10, i11, i01]);
                    }
                }
            }
        }

        for tri in &self.triangles {
            if out.vertices.len().saturating_add(3) > MAX_TOTAL_VERTICES {
                break;
            }
            let base = out.vertices.len() as u32;
            for i in 0..3 {
                out.vertices
                    .push((tri.points[i], color_transform.apply(tri.colors[i])));
            }
            out.indices.extend_from_slice(&[base, base + 1, base + 2]);
        }

        out
    }
}

impl GradientMesh {
    /// Convert this authorable `(rows + 1) x (cols + 1)` [`GradientMesh`] vertex/tangent grid into
    /// an exact bicubic [`VectorMeshPaint`] (`rows * cols` [`VectorMeshPatch`]es) (`T-501`).
    pub fn to_vector_mesh_paint(&self) -> Option<VectorMeshPaint> {
        let rows = self.rows.max(1) as usize;
        let cols = self.cols.max(1) as usize;
        let vertex_count = gradient_mesh_vertex_count(self.rows.max(1), self.cols.max(1))?;
        if self.vertices.len() < vertex_count || rows.saturating_mul(cols) > MAX_VECTOR_MESH_PATCHES
        {
            return None;
        }
        let stride = cols + 1;
        let v_at = |r: usize, c: usize| &self.vertices[r * stride + c];
        let mut patches = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            for c in 0..cols {
                let v00 = v_at(r, c);
                let v03 = v_at(r, c + 1);
                let v30 = v_at(r + 1, c);
                let v33 = v_at(r + 1, c + 1);
                // Symmetric opposing tangent reflection when moving backward toward next vertex:
                let p00 = v00.position;
                let p03 = v03.position;
                let p30 = v30.position;
                let p33 = v33.position;
                let p01 = v00.handle_u;
                let p02 = [
                    p03[0] - (v03.handle_u[0] - p03[0]),
                    p03[1] - (v03.handle_u[1] - p03[1]),
                ];
                let p31 = v30.handle_u;
                let p32 = [
                    p33[0] - (v33.handle_u[0] - p33[0]),
                    p33[1] - (v33.handle_u[1] - p33[1]),
                ];
                let p10 = v00.handle_v;
                let p20 = [
                    p30[0] - (v30.handle_v[0] - p30[0]),
                    p30[1] - (v30.handle_v[1] - p30[1]),
                ];
                let p13 = v03.handle_v;
                let p23 = [
                    p33[0] - (v33.handle_v[0] - p33[0]),
                    p33[1] - (v33.handle_v[1] - p33[1]),
                ];
                let boundary = [p00, p01, p02, p03, p13, p23, p33, p32, p31, p30, p20, p10];
                patches.push(VectorMeshPatch::from_coons_12(
                    boundary,
                    [v00.color, v03.color, v30.color, v33.color],
                ));
            }
        }
        Some(VectorMeshPaint {
            patches,
            triangles: Vec::new(),
            linear_rgb: false,
        })
    }
}

impl FreeformGradient {
    /// Sample this [`FreeformGradient`] over `bounds = [min_x, min_y, max_x, max_y]` into a smooth
    /// Gouraud triangle [`VectorMeshPaint`] (`T-501`).
    pub fn to_vector_mesh_paint(&self, bounds: [f64; 4]) -> Option<VectorMeshPaint> {
        if self.points.is_empty() && self.lines.is_empty() {
            return None;
        }
        let [x0, y0, x1, y1] = bounds;
        let w = (x1 - x0).max(1.0);
        let h = (y1 - y0).max(1.0);
        const GRID: usize = 16;
        let mut grid = vec![([0.0, 0.0], [0u8; 4]); (GRID + 1) * (GRID + 1)];
        for r in 0..=GRID {
            let y = y0 + (r as f64 / GRID as f64) * h;
            for c in 0..=GRID {
                let x = x0 + (c as f64 / GRID as f64) * w;
                grid[r * (GRID + 1) + c] = ([x, y], sample_freeform_gradient(self, [x, y]));
            }
        }
        let mut triangles = Vec::with_capacity(GRID * GRID * 2);
        for r in 0..GRID {
            for c in 0..GRID {
                let (p00, c00) = grid[r * (GRID + 1) + c];
                let (p10, c10) = grid[r * (GRID + 1) + c + 1];
                let (p01, c01) = grid[(r + 1) * (GRID + 1) + c];
                let (p11, c11) = grid[(r + 1) * (GRID + 1) + c + 1];
                triangles.push(VectorMeshTriangle {
                    points: [p00, p10, p11],
                    colors: [c00, c10, c11],
                });
                triangles.push(VectorMeshTriangle {
                    points: [p00, p11, p01],
                    colors: [c00, c11, c01],
                });
            }
        }
        Some(VectorMeshPaint {
            patches: Vec::new(),
            triangles,
            linear_rgb: false,
        })
    }
}

impl GradientMesh {
    /// Construct a regular `(rows + 1) x (cols + 1)` gradient mesh over `bounds = [x, y, width, height]`.
    pub fn new_grid(bounds: [f64; 4], rows: u32, cols: u32, base_color: Color) -> Self {
        let rows = rows.clamp(1, 32);
        let cols = cols.clamp(1, 32);
        let [bx, by, bw, bh] = bounds;
        let dx = bw / (cols as f64);
        let dy = bh / (rows as f64);
        let mut vertices = Vec::with_capacity(((rows + 1) * (cols + 1)) as usize);
        for r in 0..=rows {
            for c in 0..=cols {
                let x = bx + (c as f64) * dx;
                let y = by + (r as f64) * dy;
                vertices.push(GradientMeshPatchVertex {
                    position: [x, y],
                    handle_u: [x + dx / 3.0, y],
                    handle_v: [x, y + dy / 3.0],
                    color: base_color,
                });
            }
        }
        Self {
            rows,
            cols,
            vertices,
        }
    }

    pub fn set_vertex_color(&mut self, row: u32, col: u32, color: Color) -> bool {
        if row > self.rows || col > self.cols {
            return false;
        }
        let Some(vertex_count) = gradient_mesh_vertex_count(self.rows, self.cols) else {
            return false;
        };
        // A truncated or otherwise inconsistent grid is malformed. Do not let a low index
        // (notably 0,0) mutate an apparently valid prefix when the declared dimensions cannot
        // actually be represented by the stored vertex array.
        if self.vertices.len() < vertex_count {
            return false;
        }
        let Some(idx) = usize::try_from(row)
            .ok()
            .and_then(|row| {
                usize::try_from(self.cols)
                    .ok()
                    .and_then(|cols| cols.checked_add(1))
                    .and_then(|stride| row.checked_mul(stride))
            })
            .and_then(|offset| {
                usize::try_from(col)
                    .ok()
                    .and_then(|col| offset.checked_add(col))
            })
        else {
            return false;
        };
        let Some(v) = self.vertices.get_mut(idx) else {
            return false;
        };
        v.color = color;
        true
    }

    /// Sample the interpolated `(position, color)` at normalized `(u, v)` in `0.0..=1.0` (`T-120`).
    pub fn sample_mesh(&self, u: f64, v: f64) -> ([f64; 2], Color) {
        let rows = self.rows.max(1) as usize;
        let cols = self.cols.max(1) as usize;
        let Some(vertex_count) = gradient_mesh_vertex_count(self.rows.max(1), self.cols.max(1))
        else {
            return ([0.0, 0.0], [0, 0, 0, 255]);
        };
        if self.vertices.len() < vertex_count {
            return ([0.0, 0.0], [0, 0, 0, 255]);
        }
        let u = u.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        let gx = u * (cols as f64);
        let gy = v * (rows as f64);
        let c0 = (gx.floor() as usize).min(cols.saturating_sub(1));
        let r0 = (gy.floor() as usize).min(rows.saturating_sub(1));
        let c1 = (c0 + 1).min(cols);
        let r1 = (r0 + 1).min(rows);
        let fu = gx - (c0 as f64);
        let fv = gy - (r0 as f64);
        // Smoothstep Hermite weight for patch continuity
        let su = fu * fu * (3.0 - 2.0 * fu);
        let sv = fv * fv * (3.0 - 2.0 * fv);

        let idx = |r: usize, c: usize| r * (cols + 1) + c;
        let v00 = &self.vertices[idx(r0, c0)];
        let v10 = &self.vertices[idx(r0, c1)];
        let v01 = &self.vertices[idx(r1, c0)];
        let v11 = &self.vertices[idx(r1, c1)];

        let px = lerp_f64(
            lerp_f64(v00.position[0], v10.position[0], su),
            lerp_f64(v01.position[0], v11.position[0], su),
            sv,
        );
        let py = lerp_f64(
            lerp_f64(v00.position[1], v10.position[1], su),
            lerp_f64(v01.position[1], v11.position[1], su),
            sv,
        );
        let top_c = lerp_color(v00.color, v10.color, su);
        let bot_c = lerp_color(v01.color, v11.color, su);
        let color = lerp_color(top_c, bot_c, sv);
        ([px, py], color)
    }
}

fn gradient_mesh_vertex_count(rows: u32, cols: u32) -> Option<usize> {
    usize::try_from(rows)
        .ok()?
        .checked_add(1)?
        .checked_mul(usize::try_from(cols).ok()?.checked_add(1)?)
}

// ============================================================================
// 5. Perspective Grid
// ============================================================================

/// Active plane on a 1/2/3-point [`PerspectiveGridSpec`] (`T-120`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerspectivePlane {
    #[default]
    LeftGrid,
    RightGrid,
    HorizontalGrid,
}

/// Authoring Perspective Grid configuration stored on a timeline (`T-120`, format 9).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerspectiveGridSpec {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub active_plane: PerspectivePlane,
    pub horizon_y: f64,
    pub left_vanishing_x: f64,
    pub right_vanishing_x: f64,
    pub grid_cell_size: f64,
    pub grid_extent: f64,
}

impl Default for PerspectiveGridSpec {
    fn default() -> Self {
        Self {
            enabled: false,
            active_plane: PerspectivePlane::LeftGrid,
            horizon_y: 300.0,
            left_vanishing_x: 100.0,
            right_vanishing_x: 700.0,
            grid_cell_size: 32.0,
            grid_extent: 400.0,
        }
    }
}

impl PerspectiveGridSpec {
    /// Project a local plane coordinate `[u, v]` onto stage coordinates on `plane` (`T-120`).
    pub fn project_point(&self, plane: PerspectivePlane, local: [f64; 2]) -> [f64; 2] {
        let center_x = (self.left_vanishing_x + self.right_vanishing_x) * 0.5;
        let base_y = self.horizon_y + self.grid_extent.max(1.0) * 0.5;
        let extent = self.grid_extent.max(1.0);
        let depth_t = (local[0] / extent).clamp(0.0, 1.0);
        match plane {
            PerspectivePlane::LeftGrid => {
                let x = lerp_f64(center_x, self.left_vanishing_x, depth_t);
                let y_anchor = lerp_f64(base_y, self.horizon_y, depth_t);
                let scale = 1.0 - depth_t * 0.7;
                [x, y_anchor - local[1] * scale]
            }
            PerspectivePlane::RightGrid => {
                let x = lerp_f64(center_x, self.right_vanishing_x, depth_t);
                let y_anchor = lerp_f64(base_y, self.horizon_y, depth_t);
                let scale = 1.0 - depth_t * 0.7;
                [x, y_anchor - local[1] * scale]
            }
            PerspectivePlane::HorizontalGrid => {
                let u = (local[0] / extent).clamp(0.0, 1.0);
                let v = (local[1] / extent).clamp(0.0, 1.0);
                let lx = lerp_f64(center_x, self.left_vanishing_x, u);
                let rx = lerp_f64(center_x, self.right_vanishing_x, v);
                let x = (lx + rx) - center_x;
                let y = lerp_f64(base_y, self.horizon_y, (u + v) * 0.5);
                [x, y]
            }
        }
    }

    /// Snap a stage point `pt` to the nearest perspective grid intersection within `tolerance` (`T-120`).
    pub fn snap_point(&self, pt: [f64; 2], tolerance: f64) -> [f64; 2] {
        if !self.enabled {
            return pt;
        }
        let cell = self.grid_cell_size.max(4.0);
        let steps = (self.grid_extent.max(cell) / cell).round().clamp(1.0, 32.0) as i32;
        let mut best = (tolerance * tolerance, pt);
        for iu in 0..=steps {
            for iv in -steps..=steps {
                let candidate =
                    self.project_point(self.active_plane, [(iu as f64) * cell, (iv as f64) * cell]);
                let d2 = (candidate[0] - pt[0]).powi(2) + (candidate[1] - pt[1]).powi(2);
                if d2 <= best.0 {
                    best = (d2, candidate);
                }
            }
        }
        best.1
    }
}

// ============================================================================
// 6. Procedural Vector Effects (ZigZag, PuckerBloat, Roughen, Twist, OffsetPath)
// ============================================================================

/// Non-destructive procedural vector path operators (`T-120`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum ProceduralVectorEffect {
    ZigZag {
        size: f64,
        ridges_per_segment: u32,
        smooth: bool,
    },
    PuckerBloat {
        amount: f64,
    },
    Roughen {
        size: f64,
        detail: u32,
        seed: u32,
    },
    Twist {
        angle_deg: f64,
    },
    OffsetPath {
        offset: f64,
    },
}

fn deterministic_hash_f64(seed: u32, index: u32) -> f64 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ index.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    ((x as f64) / (u32::MAX as f64)) * 2.0 - 1.0
}

/// Apply a [`ProceduralVectorEffect`] to `commands` and return the resulting path (`T-120`).
pub fn apply_procedural_effect_to_path(
    effect: &ProceduralVectorEffect,
    commands: &[PathCommand],
) -> Vec<PathCommand> {
    if commands.is_empty() {
        return Vec::new();
    }
    match *effect {
        ProceduralVectorEffect::OffsetPath { offset } => crate::apply_vector_modifier(
            commands,
            &crate::VectorModifier::OffsetContour { delta: offset },
        ),
        ProceduralVectorEffect::Twist { angle_deg } => {
            let Ok(Some(b)) = path_bounds(commands) else {
                return commands.to_vec();
            };
            let cx = (b.x_min + b.x_max) * 0.5;
            let cy = (b.y_min + b.y_max) * 0.5;
            let max_r = ((b.x_max - b.x_min).hypot(b.y_max - b.y_min) * 0.5).max(1.0);
            let max_rad = angle_deg.to_radians();
            let twist_pt = |x: f64, y: f64| -> (f64, f64) {
                let dx = x - cx;
                let dy = y - cy;
                let r = dx.hypot(dy);
                let theta = (1.0 - (r / max_r).clamp(0.0, 1.0)) * max_rad;
                let (s, c) = theta.sin_cos();
                (cx + dx * c - dy * s, cy + dx * s + dy * c)
            };
            commands
                .iter()
                .map(|cmd| match *cmd {
                    PathCommand::MoveTo { x, y } => {
                        let (nx, ny) = twist_pt(x, y);
                        PathCommand::MoveTo { x: nx, y: ny }
                    }
                    PathCommand::LineTo { x, y } => {
                        let (nx, ny) = twist_pt(x, y);
                        PathCommand::LineTo { x: nx, y: ny }
                    }
                    PathCommand::QuadraticTo { cx, cy, x, y } => {
                        let (ncx, ncy) = twist_pt(cx, cy);
                        let (nx, ny) = twist_pt(x, y);
                        PathCommand::QuadraticTo {
                            cx: ncx,
                            cy: ncy,
                            x: nx,
                            y: ny,
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
                        let (ncx1, ncy1) = twist_pt(cx1, cy1);
                        let (ncx2, ncy2) = twist_pt(cx2, cy2);
                        let (nx, ny) = twist_pt(x, y);
                        PathCommand::CubicTo {
                            cx1: ncx1,
                            cy1: ncy1,
                            cx2: ncx2,
                            cy2: ncy2,
                            x: nx,
                            y: ny,
                        }
                    }
                    PathCommand::Close => PathCommand::Close,
                })
                .collect()
        }
        ProceduralVectorEffect::PuckerBloat { amount } => {
            let Ok(Some(b)) = path_bounds(commands) else {
                return commands.to_vec();
            };
            let cx = (b.x_min + b.x_max) * 0.5;
            let cy = (b.y_min + b.y_max) * 0.5;
            let factor = (amount / 100.0).clamp(-1.0, 1.0);
            let mut out = Vec::with_capacity(commands.len());
            let mut cur = (cx, cy);
            for cmd in commands {
                match *cmd {
                    PathCommand::MoveTo { x, y } => {
                        cur = (x, y);
                        out.push(PathCommand::MoveTo { x, y });
                    }
                    PathCommand::LineTo { x, y } => {
                        let mx = (cur.0 + x) * 0.5;
                        let my = (cur.1 + y) * 0.5;
                        let qx = lerp_f64(mx, cx, -factor);
                        let qy = lerp_f64(my, cy, -factor);
                        out.push(PathCommand::QuadraticTo {
                            cx: qx,
                            cy: qy,
                            x,
                            y,
                        });
                        cur = (x, y);
                    }
                    other => {
                        if let PathCommand::QuadraticTo { x, y, .. }
                        | PathCommand::CubicTo { x, y, .. } = other
                        {
                            cur = (x, y);
                        }
                        out.push(other);
                    }
                }
            }
            out
        }
        ProceduralVectorEffect::ZigZag {
            size,
            ridges_per_segment,
            smooth,
        } => {
            let ridges = ridges_per_segment.clamp(1, 32);
            let mut out = Vec::new();
            let mut cur = (0.0, 0.0);
            let mut start = (0.0, 0.0);
            for cmd in commands {
                match *cmd {
                    PathCommand::MoveTo { x, y } => {
                        cur = (x, y);
                        start = (x, y);
                        out.push(PathCommand::MoveTo { x, y });
                    }
                    PathCommand::LineTo { x, y } => {
                        let dx = x - cur.0;
                        let dy = y - cur.1;
                        let len = dx.hypot(dy);
                        if len > 1e-6 {
                            let nx = -dy / len;
                            let ny = dx / len;
                            for r in 1..=ridges {
                                let t = (r as f64) / (ridges as f64);
                                let mid_t = ((r as f64) - 0.5) / (ridges as f64);
                                let sign = if r % 2 == 0 { -1.0 } else { 1.0 };
                                let px = cur.0 + dx * mid_t + nx * size * sign;
                                let py = cur.1 + dy * mid_t + ny * size * sign;
                                let ex = cur.0 + dx * t;
                                let ey = cur.1 + dy * t;
                                if smooth {
                                    out.push(PathCommand::QuadraticTo {
                                        cx: px,
                                        cy: py,
                                        x: ex,
                                        y: ey,
                                    });
                                } else {
                                    out.push(PathCommand::LineTo { x: px, y: py });
                                    out.push(PathCommand::LineTo { x: ex, y: ey });
                                }
                            }
                        } else {
                            out.push(PathCommand::LineTo { x, y });
                        }
                        cur = (x, y);
                    }
                    PathCommand::Close => {
                        cur = start;
                        out.push(PathCommand::Close);
                    }
                    other @ (PathCommand::QuadraticTo { x, y, .. }
                    | PathCommand::CubicTo { x, y, .. }) => {
                        cur = (x, y);
                        out.push(other);
                    }
                }
            }
            out
        }
        ProceduralVectorEffect::Roughen { size, detail, seed } => {
            let sub = detail.clamp(1, 16);
            let mut out = Vec::new();
            let mut cur = (0.0, 0.0);
            let mut start = (0.0, 0.0);
            let mut step_idx = 0u32;
            for cmd in commands {
                match *cmd {
                    PathCommand::MoveTo { x, y } => {
                        cur = (x, y);
                        start = (x, y);
                        out.push(PathCommand::MoveTo { x, y });
                    }
                    PathCommand::LineTo { x, y } => {
                        for s in 1..=sub {
                            let t = (s as f64) / (sub as f64);
                            let jx = if s < sub {
                                deterministic_hash_f64(seed, step_idx) * size
                            } else {
                                0.0
                            };
                            let jy = if s < sub {
                                deterministic_hash_f64(seed.wrapping_add(17), step_idx) * size
                            } else {
                                0.0
                            };
                            step_idx = step_idx.wrapping_add(1);
                            out.push(PathCommand::LineTo {
                                x: lerp_f64(cur.0, x, t) + jx,
                                y: lerp_f64(cur.1, y, t) + jy,
                            });
                        }
                        cur = (x, y);
                    }
                    PathCommand::Close => {
                        cur = start;
                        out.push(PathCommand::Close);
                    }
                    other @ (PathCommand::QuadraticTo { x, y, .. }
                    | PathCommand::CubicTo { x, y, .. }) => {
                        cur = (x, y);
                        out.push(other);
                    }
                }
            }
            out
        }
    }
}

// ============================================================================
// 7. ElementVectorEffects & Expand/Bake
// ============================================================================

/// Non-destructive vector effects stack attached to an element on a [`Keyframe`] (`T-120`, format 9).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ElementVectorEffects {
    pub element_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope_warp: Option<EnvelopeWarp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub procedural_effects: Vec<ProceduralVectorEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_blend: Option<VectorBlendSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeform_gradient: Option<FreeformGradient>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient_mesh: Option<GradientMesh>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_mesh: Option<VectorMeshPaint>,
}

impl ElementVectorEffects {
    pub fn is_empty(&self) -> bool {
        self.envelope_warp.is_none()
            && self.procedural_effects.is_empty()
            && self.vector_blend.is_none()
            && self.freeform_gradient.is_none()
            && self.gradient_mesh.is_none()
            && self.vector_mesh.is_none()
    }

    /// Resolve any active vector mesh paint (`vector_mesh`, `gradient_mesh`, or `freeform_gradient`)
    /// into a concrete [`VectorMeshPaint`] in local element coordinates, applying `envelope_warp`
    /// to control points when present (`T-501`).
    pub fn resolved_vector_mesh(&self, element: &Element) -> Option<VectorMeshPaint> {
        let mut mesh = if let Some(vm) = &self.vector_mesh {
            vm.clone()
        } else if let Some(gm) = &self.gradient_mesh {
            gm.to_vector_mesh_paint()?
        } else if let Some(ff) = &self.freeform_gradient {
            let (commands, _) = element.to_path_commands()?;
            let b = path_bounds(&commands).ok().flatten()?;
            ff.to_vector_mesh_paint([b.x_min, b.y_min, b.x_max, b.y_max])?
        } else {
            return None;
        };
        if let Some(warp) = &self.envelope_warp
            && let Some((commands, _)) = element.to_path_commands()
            && let Ok(Some(b)) = path_bounds(&commands)
        {
            let bounds = [
                b.x_min,
                b.y_min,
                (b.x_max - b.x_min).max(1e-6),
                (b.y_max - b.y_min).max(1e-6),
            ];
            mesh.map_points_in_place(|pt| apply_envelope_warp_to_point(warp, pt, bounds));
        }
        Some(mesh)
    }
}

/// Deterministically expand/bake `effects` (envelope warp, procedural effects, freeform gradient /
/// mesh representative fill) onto `element`, returning a concrete vector path element (`T-120`).
pub fn expand_vector_effects(element: &Element, effects: &ElementVectorEffects) -> Element {
    let mut result = element.clone();
    let Some((mut commands, fill_rule)) = result.to_path_commands() else {
        return result;
    };

    if let Some(warp) = &effects.envelope_warp {
        commands = apply_envelope_warp_to_path(warp, &commands);
    }
    for proc_eff in &effects.procedural_effects {
        commands = apply_procedural_effect_to_path(proc_eff, &commands);
    }

    let (mut fill, stroke, stroke_width, fill_gradient, stroke_gradient) = match &result.kind {
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            fill,
            stroke,
            stroke_width,
            ..
        } => (*fill, *stroke, *stroke_width, None, None),
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => (
            *fill,
            *stroke,
            *stroke_width,
            fill_gradient.clone(),
            stroke_gradient.clone(),
        ),
        _ => return result,
    };

    if let Some(vm) = &effects.vector_mesh {
        fill = vm.representative_color();
    } else if let Some(ff) = &effects.freeform_gradient
        && let Ok(Some(b)) = path_bounds(&commands)
    {
        fill = sample_freeform_gradient(ff, [(b.x_min + b.x_max) * 0.5, (b.y_min + b.y_max) * 0.5]);
    } else if let Some(mesh) = &effects.gradient_mesh {
        let (_, sampled_c) = mesh.sample_mesh(0.5, 0.5);
        fill = sampled_c;
    }

    result.transform.primitive = None;
    result.kind = if fill_gradient.is_some() || stroke_gradient.is_some() {
        ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
        }
    } else {
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        }
    };
    result
}

impl Keyframe {
    pub fn vector_effects(&self, element_id: &str) -> Option<&ElementVectorEffects> {
        self.vector_effects
            .iter()
            .find(|v| v.element_id == element_id)
    }

    pub fn set_vector_effects(&mut self, effects: ElementVectorEffects) {
        if effects.is_empty() {
            self.vector_effects
                .retain(|v| v.element_id != effects.element_id);
            return;
        }
        if let Some(slot) = self
            .vector_effects
            .iter_mut()
            .find(|v| v.element_id == effects.element_id)
        {
            *slot = effects;
        } else {
            self.vector_effects.push(effects);
        }
    }

    /// Deterministically bake an element's [`ElementVectorEffects`] into its editable geometry and
    /// remove the non-destructive effect entry (`T-120`).
    pub fn expand_element_vector_effects(&mut self, element_id: &str) -> bool {
        let Some(eff) = self.vector_effects(element_id).cloned() else {
            return false;
        };
        let Some(idx) = self.elements.iter().position(|e| e.id == element_id) else {
            return false;
        };
        let baked = expand_vector_effects(&self.elements[idx], &eff);
        self.elements[idx] = baked;
        self.vector_effects.retain(|v| v.element_id != element_id);
        true
    }
}

impl Timeline {
    /// Active [`PerspectiveGridSpec`] stored on this timeline (`T-120`, format 9).
    pub fn perspective_grid(&self) -> Option<PerspectiveGridSpec> {
        self.layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .find_map(|k| k.perspective_grid)
    }

    /// Set or clear the [`PerspectiveGridSpec`] on this timeline (`T-120`, format 9).
    pub fn set_perspective_grid(&mut self, grid: Option<PerspectiveGridSpec>) {
        for layer in &mut self.layers {
            for key in &mut layer.keyframes {
                key.perspective_grid = None;
            }
        }
        if let Some(first_layer) = self.layers.first_mut()
            && let Some(first_key) = first_layer.keyframes.first_mut()
        {
            first_key.perspective_grid = grid;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppearancePaintEntry, ElementAppearance, Layer, LayerProperties, Transform};

    #[test]
    fn freeform_gradient_envelope_warp_blend_mesh_perspective_and_procedural_effects() {
        // 1. Freeform gradient sampling (points + lines)
        let grad = FreeformGradient {
            points: vec![
                FreeformGradientPoint {
                    id: "p1".into(),
                    position: [0.0, 0.0],
                    color: [255, 0, 0, 255],
                    radius: 50.0,
                    weight: 1.0,
                },
                FreeformGradientPoint {
                    id: "p2".into(),
                    position: [100.0, 0.0],
                    color: [0, 0, 255, 255],
                    radius: 50.0,
                    weight: 1.0,
                },
            ],
            lines: vec![FreeformGradientLine {
                points: vec![[0.0, 50.0], [100.0, 50.0]],
                start_color: [0, 255, 0, 255],
                end_color: [255, 255, 0, 255],
            }],
        };
        assert_eq!(
            sample_freeform_gradient(&grad, [0.0, 0.0]),
            [255, 0, 0, 255]
        );
        let mid = sample_freeform_gradient(&grad, [50.0, 0.0]);
        assert!(mid[0] > 80 && mid[2] > 80);

        // 2. Envelope warp + Procedural effects + Expand
        let rect = Element {
            id: "r1".into(),
            name: "Banner".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 120.0,
                height: 60.0,
                fill: [200, 40, 40, 255],
                stroke: [0, 0, 0, 255],
                stroke_width: 2.0,
            },
            ..Default::default()
        };
        let effects = ElementVectorEffects {
            element_id: "r1".into(),
            envelope_warp: Some(EnvelopeWarp {
                preset: EnvelopeWarpPreset::Flag,
                bend: 0.6,
                ..Default::default()
            }),
            procedural_effects: vec![
                ProceduralVectorEffect::ZigZag {
                    size: 4.0,
                    ridges_per_segment: 2,
                    smooth: false,
                },
                ProceduralVectorEffect::Roughen {
                    size: 1.5,
                    detail: 2,
                    seed: 42,
                },
            ],
            vector_blend: None,
            freeform_gradient: Some(grad),
            gradient_mesh: None,
            vector_mesh: None,
        };
        let expanded = expand_vector_effects(&rect, &effects);
        assert!(matches!(expanded.kind, ElementKind::Path { .. }));

        // 3. Vector blend steps
        let end_rect = Element {
            id: "r2".into(),
            name: "BannerEnd".into(),
            transform: Transform {
                x: 200.0,
                y: 100.0,
                ..Default::default()
            },
            ..rect.clone()
        };
        let steps = generate_vector_blend_steps(
            &rect,
            &end_rect,
            &VectorBlendSpec {
                target_element_id: "r2".into(),
                spacing: VectorBlendSpacing::SpecifiedSteps(3),
                align_to_path: false,
                spine: None,
            },
        );
        assert_eq!(steps.len(), 3);
        assert!((steps[1].transform.x - 100.0).abs() < 1e-3);

        // 4. Gradient Mesh
        let mut mesh = GradientMesh::new_grid([0.0, 0.0, 100.0, 100.0], 2, 2, [20, 20, 20, 255]);
        assert!(mesh.set_vertex_color(1, 1, [240, 120, 60, 255]));
        let (pt, col) = mesh.sample_mesh(0.5, 0.5);
        assert!((pt[0] - 50.0).abs() < 1e-3 && (pt[1] - 50.0).abs() < 1e-3);
        assert_eq!(col, [240, 120, 60, 255]);

        // 5. Perspective Grid projection & snapping
        let pgrid = PerspectiveGridSpec {
            enabled: true,
            active_plane: PerspectivePlane::LeftGrid,
            ..Default::default()
        };
        let p0 = pgrid.project_point(PerspectivePlane::LeftGrid, [64.0, 32.0]);
        let snapped = pgrid.snap_point([p0[0] + 2.0, p0[1] - 1.5], 12.0);
        assert!((snapped[0] - p0[0]).abs() < 1e-6 && (snapped[1] - p0[1]).abs() < 1e-6);
    }

    #[test]
    fn procedural_line_effects_continue_from_curve_endpoints() {
        let commands = vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::CubicTo {
                cx1: 2.0,
                cy1: 4.0,
                cx2: 8.0,
                cy2: 4.0,
                x: 10.0,
                y: 0.0,
            },
            PathCommand::LineTo { x: 20.0, y: 0.0 },
        ];

        let zigzag = apply_procedural_effect_to_path(
            &ProceduralVectorEffect::ZigZag {
                size: 2.0,
                ridges_per_segment: 1,
                smooth: false,
            },
            &commands,
        );
        assert!(matches!(
            zigzag.get(2),
            Some(PathCommand::LineTo { x, y })
                if (*x - 15.0).abs() < 1e-9 && (*y - 2.0).abs() < 1e-9
        ));

        let rough = apply_procedural_effect_to_path(
            &ProceduralVectorEffect::Roughen {
                size: 0.0,
                detail: 2,
                seed: 7,
            },
            &commands,
        );
        assert!(matches!(
            rough.get(2),
            Some(PathCommand::LineTo { x, y })
                if (*x - 15.0).abs() < 1e-9 && y.abs() < 1e-9
        ));
    }

    #[test]
    fn vector_blend_evaluation_uses_stable_ids() {
        let start = Element {
            id: "start".into(),
            name: "Start".into(),
            kind: ElementKind::Rectangle {
                width: 10.0,
                height: 10.0,
                fill: [255, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        let end = Element {
            id: "end".into(),
            transform: crate::Transform {
                x: 100.0,
                ..crate::Transform::default()
            },
            ..start.clone()
        };
        let spec = VectorBlendSpec {
            target_element_id: end.id.clone(),
            spacing: VectorBlendSpacing::SpecifiedSteps(3),
            align_to_path: false,
            spine: None,
        };

        let first = generate_vector_blend_steps(&start, &end, &spec);
        let second = generate_vector_blend_steps(&start, &end, &spec);
        assert_eq!(first, second);
        assert_eq!(
            first
                .iter()
                .map(|element| element.id.as_str())
                .collect::<Vec<_>>(),
            [
                "start-blend-step-1",
                "start-blend-step-2",
                "start-blend-step-3"
            ]
        );
    }

    #[test]
    fn sampled_vector_blend_ids_avoid_authored_collisions_deterministically() {
        let rectangle = |id: &str, x: f64| Element {
            id: id.into(),
            transform: Transform {
                x,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 10.0,
                height: 10.0,
                fill: [255, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        let layer = Layer {
            id: "layer".into(),
            name: "Layer".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![
                    rectangle("start", 0.0),
                    rectangle("target", 100.0),
                    rectangle("start-blend-step-1", 200.0),
                    rectangle("start-blend-step-1-2", 300.0),
                    rectangle("start-app-1", 400.0),
                    rectangle("start-app-1-2", 500.0),
                ],
                vector_effects: vec![ElementVectorEffects {
                    element_id: "start".into(),
                    vector_blend: Some(VectorBlendSpec {
                        target_element_id: "target".into(),
                        spacing: VectorBlendSpacing::SpecifiedSteps(2),
                        align_to_path: false,
                        spine: None,
                    }),
                    ..ElementVectorEffects::default()
                }],
                appearances: vec![ElementAppearance {
                    element_id: "start".into(),
                    entries: vec![
                        AppearancePaintEntry::Fill {
                            color: [255, 0, 0, 255],
                            gradient: None,
                            swatch_id: None,
                            offset: [0.0, 0.0],
                            opacity: 1.0,
                        },
                        AppearancePaintEntry::Fill {
                            color: [0, 0, 255, 255],
                            gradient: None,
                            swatch_id: None,
                            offset: [0.0, 0.0],
                            opacity: 1.0,
                        },
                    ],
                    ..ElementAppearance::default()
                }],
                ..Keyframe::default()
            }],
            properties: LayerProperties::default(),
        };

        let first = crate::sample_layer(&layer, 0);
        let second = crate::sample_layer(&layer, 0);
        let first_ids: Vec<_> = first.iter().map(|element| element.id.clone()).collect();
        let second_ids: Vec<_> = second.iter().map(|element| element.id.clone()).collect();
        assert_eq!(first_ids, second_ids);
        assert_eq!(first_ids.len(), 9);
        assert_eq!(
            first_ids
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            9
        );
        assert!(first_ids.iter().any(|id| id == "start-blend-step-1-3"));
        assert!(first_ids.iter().any(|id| id == "start-app-1-3"));
    }

    #[test]
    fn malformed_mesh_dimensions_fail_closed_without_overflow() {
        let mut mesh = GradientMesh {
            rows: u32::MAX,
            cols: u32::MAX,
            vertices: vec![GradientMeshPatchVertex {
                position: [0.0, 0.0],
                handle_u: [0.0, 0.0],
                handle_v: [0.0, 0.0],
                color: [1, 2, 3, 4],
            }],
        };
        assert_eq!(mesh.sample_mesh(0.5, 0.5), ([0.0, 0.0], [0, 0, 0, 255]));
        assert!(!mesh.set_vertex_color(0, 0, [255; 4]));
        assert!(!mesh.set_vertex_color(1, 1, [255; 4]));
        assert_eq!(mesh.vertices[0].color, [1, 2, 3, 4]);

        let warp = EnvelopeWarp {
            preset: EnvelopeWarpPreset::CustomGrid,
            rows: u32::MAX,
            cols: u32::MAX,
            control_points: Vec::new(),
            ..EnvelopeWarp::default()
        };
        assert_eq!(
            apply_envelope_warp_to_point(&warp, [5.0, 5.0], [0.0, 0.0, 10.0, 10.0]),
            [5.0, 5.0]
        );
    }
}
