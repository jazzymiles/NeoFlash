//! Advanced vector effects and mesh/perspective authoring (`T-120`):
//! - Freeform (point & line) gradients (`FreeformGradient`, `sample_freeform_gradient`)
//! - Envelope distort (`EnvelopeWarpPreset`, `EnvelopeWarp`, `apply_envelope_warp_to_path`)
//! - Multi-step vector blends (`VectorBlendSpacing`, `VectorBlendSpec`, `generate_vector_blend_steps`)
//! - 2D Gradient Mesh patches (`GradientMesh`, `GradientMeshPatchVertex`)
//! - 1/2/3-point Perspective Grid projection & snapping (`PerspectivePlane`, `PerspectiveGridSpec`)
//! - Procedural path effects (`ZigZag`, `PuckerBloat`, `Roughen`, `Twist`, `OffsetPath`)
//! - Deterministic evaluation and `expand_vector_effects` baking into editable contours.

use crate::{
    Color, Element, ElementKind, Id, Keyframe, PathCommand, Timeline, new_id, path_bounds,
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
            if warp.control_points.len() == (rows + 1) * (cols + 1) {
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
        step_el.id = new_id("blend-step");
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
        let idx = (row * (self.cols + 1) + col) as usize;
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
        if self.vertices.len() < (rows + 1) * (cols + 1) {
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
                    other => out.push(other),
                }
            }
            out
        }
        ProceduralVectorEffect::Roughen { size, detail, seed } => {
            let sub = detail.clamp(1, 16);
            let mut out = Vec::new();
            let mut cur = (0.0, 0.0);
            let mut step_idx = 0u32;
            for cmd in commands {
                match *cmd {
                    PathCommand::MoveTo { x, y } => {
                        cur = (x, y);
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
                    other => out.push(other),
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
}

impl ElementVectorEffects {
    pub fn is_empty(&self) -> bool {
        self.envelope_warp.is_none()
            && self.procedural_effects.is_empty()
            && self.vector_blend.is_none()
            && self.freeform_gradient.is_none()
            && self.gradient_mesh.is_none()
    }
}

/// Deterministically expand/bake `effects` (envelope warp, procedural effects, freeform gradient /
/// mesh average fill) onto `element`, returning a concrete vector path element (`T-120`).
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

    if let Some(ff) = &effects.freeform_gradient
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
    use crate::Transform;

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
}
