use super::handles_recognition::{inverse_f64, map_commands_forward};
use super::smooth_reshape::{editable_element_commands, write_editable_element_commands};
use super::{EnvelopeLatticeSession, FreeTransformSubmode, PerspectiveQuadSession};
use crate::app::Editor;
use neoflash_core::*;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

/// Interactive Stage handle kind for `PerspectiveGridSpec` (`T-233`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerspectiveGridHandle {
    Horizon,
    LeftVanishingPoint,
    RightVanishingPoint,
}

#[allow(dead_code)]
pub(super) fn default_envelope_grid_points(rows: u32, cols: u32) -> Vec<[f64; 2]> {
    let rows = rows.clamp(1, 8);
    let cols = cols.clamp(1, 8);
    let mut pts = Vec::with_capacity(((rows + 1) * (cols + 1)) as usize);
    for r in 0..=rows {
        for c in 0..=cols {
            pts.push([c as f64 / cols as f64, r as f64 / rows as f64]);
        }
    }
    pts
}

#[allow(dead_code)]
impl Editor {
    /// Return the active `EnvelopeWarp` on the primary selected vector element, if any (`T-233`).
    pub fn selected_envelope_warp(&self) -> Option<EnvelopeWarp> {
        let fx = self.selected_element_vector_effects()?;
        fx.envelope_warp
    }

    /// Initialize or replace the `EnvelopeWarp` on the selected vector element in 1 Undo step (`T-233`).
    pub fn set_selected_envelope_warp(&mut self, warp: Option<EnvelopeWarp>) -> bool {
        let Some(element_id) = self
            .selected_element()
            .map(|e| e.id.clone())
            .or_else(|| self.selection.targets.first().map(|t| t.element.clone()))
        else {
            return false;
        };
        let layer_idx = self.layer;
        let frame = self.frame;
        let clean_warp = warp.map(|mut w| {
            w.rows = w.rows.clamp(1, 8);
            w.cols = w.cols.clamp(1, 8);
            w.bend = if w.bend.is_finite() {
                w.bend.clamp(-100.0, 100.0)
            } else {
                0.0
            };
            w.horizontal_distortion = if w.horizontal_distortion.is_finite() {
                w.horizontal_distortion.clamp(-100.0, 100.0)
            } else {
                0.0
            };
            w.vertical_distortion = if w.vertical_distortion.is_finite() {
                w.vertical_distortion.clamp(-100.0, 100.0)
            } else {
                0.0
            };
            let expected = ((w.rows + 1) * (w.cols + 1)) as usize;
            if w.control_points.len() != expected {
                w.control_points = default_envelope_grid_points(w.rows, w.cols);
            }
            w
        });
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        if !key.elements.iter().any(|e| e.id == element_id) {
            return false;
        }
        let existing_warp = key
            .vector_effects(&element_id)
            .and_then(|fx| fx.envelope_warp.clone());
        if existing_warp == clean_warp {
            return false;
        }
        self.transact_named("Set Envelope Warp", |editor| {
            let symbol = editor.symbol_mut();
            let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
                return;
            };
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
                return;
            };
            let mut fx = key
                .vector_effects(&element_id)
                .cloned()
                .unwrap_or(ElementVectorEffects {
                    element_id: element_id.clone(),
                    ..Default::default()
                });
            fx.envelope_warp = clean_warp;
            key.set_vector_effects(fx);
        });
        true
    }

    /// Compute the Stage-space handle positions `(row, col, [stage_x, stage_y])` for the selected
    /// element's `EnvelopeWarp` mesh (`T-233`).
    pub fn selected_envelope_stage_handles(&self) -> Vec<(u32, u32, [f64; 2])> {
        let Some(warp) = self.selected_envelope_warp() else {
            return Vec::new();
        };
        let Some(element_id) = self
            .selected_element()
            .map(|e| e.id.as_str())
            .or_else(|| self.selection.targets.first().map(|t| t.element.as_str()))
        else {
            return Vec::new();
        };
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return Vec::new();
        };
        let Some(key) = layer.key_at(self.frame) else {
            return Vec::new();
        };
        let Some(el) = key.elements.iter().find(|e| e.id == element_id) else {
            return Vec::new();
        };
        let Some((commands, _)) = el.to_path_commands() else {
            return Vec::new();
        };
        let Ok(Some(bounds)) = neoflash_core::path_bounds(&commands) else {
            return Vec::new();
        };
        let bw = (bounds.x_max - bounds.x_min).max(1.0);
        let bh = (bounds.y_max - bounds.y_min).max(1.0);
        let mat = transform_matrix(el.transform);
        let mut out = Vec::new();
        for r in 0..=warp.rows {
            for c in 0..=warp.cols {
                let idx = (r * (warp.cols + 1) + c) as usize;
                let uv = warp.control_points.get(idx).copied().unwrap_or([
                    c as f64 / warp.cols.max(1) as f64,
                    r as f64 / warp.rows.max(1) as f64,
                ]);
                let lx = bounds.x_min + uv[0] * bw;
                let ly = bounds.y_min + uv[1] * bh;
                let sx = mat[0] * lx + mat[2] * ly + mat[4];
                let sy = mat[1] * lx + mat[3] * ly + mat[5];
                out.push((r, c, [sx, sy]));
            }
        }
        out
    }

    /// Hit-test a Stage-space point against the selected element's `EnvelopeWarp` mesh handles (`T-233`).
    pub fn hit_test_envelope_stage_handle(
        &self,
        stage_point: [f64; 2],
        tolerance: f64,
    ) -> Option<(u32, u32)> {
        let tol_sq = tolerance.max(1.0).powi(2);
        self.selected_envelope_stage_handles()
            .into_iter()
            .filter_map(|(r, c, pos)| {
                let d2 = (pos[0] - stage_point[0]).powi(2) + (pos[1] - stage_point[1]).powi(2);
                (d2 <= tol_sq).then_some((r, c, d2))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(r, c, _)| (r, c))
    }

    /// Drag a control point `(row, col)` of the selected element's `EnvelopeWarp` mesh to a new
    /// normalized UV offset or Stage-space coordinate in 1 Undo step (`T-233`).
    pub fn drag_selected_envelope_handle_uv(
        &mut self,
        row: u32,
        col: u32,
        new_uv: [f64; 2],
    ) -> bool {
        if !new_uv[0].is_finite() || !new_uv[1].is_finite() {
            return false;
        }
        let clean_uv = [new_uv[0].clamp(-4.0, 5.0), new_uv[1].clamp(-4.0, 5.0)];
        let Some(mut warp) = self.selected_envelope_warp() else {
            return false;
        };
        if row > warp.rows || col > warp.cols {
            return false;
        }
        let expected = ((warp.rows + 1) * (warp.cols + 1)) as usize;
        if warp.control_points.len() != expected {
            warp.control_points = default_envelope_grid_points(warp.rows, warp.cols);
        }
        let idx = (row * (warp.cols + 1) + col) as usize;
        let Some(slot) = warp.control_points.get_mut(idx) else {
            return false;
        };
        if (slot[0] - clean_uv[0]).abs() <= 1e-6 && (slot[1] - clean_uv[1]).abs() <= 1e-6 {
            return false;
        }
        *slot = clean_uv;
        warp.preset = EnvelopeWarpPreset::CustomGrid;
        self.set_selected_envelope_warp(Some(warp))
    }

    /// Drag a control point `(row, col)` of the selected element's `EnvelopeWarp` mesh to a new
    /// Stage-space position `[stage_x, stage_y]` in 1 Undo step (`T-233`).
    pub fn drag_selected_envelope_handle_stage(
        &mut self,
        row: u32,
        col: u32,
        stage_pos: [f64; 2],
    ) -> bool {
        if !stage_pos[0].is_finite() || !stage_pos[1].is_finite() {
            return false;
        }
        let Some(element_id) = self
            .selected_element()
            .map(|e| e.id.as_str())
            .or_else(|| self.selection.targets.first().map(|t| t.element.as_str()))
        else {
            return false;
        };
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return false;
        };
        let Some(key) = layer.key_at(self.frame) else {
            return false;
        };
        let Some(el) = key.elements.iter().find(|e| e.id == element_id) else {
            return false;
        };
        let Some((commands, _)) = el.to_path_commands() else {
            return false;
        };
        let Ok(Some(bounds)) = neoflash_core::path_bounds(&commands) else {
            return false;
        };
        let bw = (bounds.x_max - bounds.x_min).max(1.0);
        let bh = (bounds.y_max - bounds.y_min).max(1.0);
        let m = transform_matrix(el.transform);
        let det = m[0] * m[3] - m[1] * m[2];
        let [lx, ly] = if det.abs() > 1e-9 {
            let dx = stage_pos[0] - m[4];
            let dy = stage_pos[1] - m[5];
            [
                (m[3] * dx - m[2] * dy) / det,
                (-m[1] * dx + m[0] * dy) / det,
            ]
        } else {
            [stage_pos[0] - el.transform.x, stage_pos[1] - el.transform.y]
        };
        let u = (lx - bounds.x_min) / bw;
        let v = (ly - bounds.y_min) / bh;
        self.drag_selected_envelope_handle_uv(row, col, [u, v])
    }

    /// Return the active `PerspectiveGridSpec` on the current timeline (`T-233`).
    pub fn active_perspective_grid(&self) -> Option<PerspectiveGridSpec> {
        self.symbol().timeline.perspective_grid()
    }

    /// Enable, update, or clear the `PerspectiveGridSpec` on the active timeline in 1 Undo step (`T-233`).
    pub fn apply_timeline_perspective_grid(&mut self, grid: Option<PerspectiveGridSpec>) -> bool {
        let clean = grid.map(|mut g| {
            if !g.horizon_y.is_finite() {
                g.horizon_y = 240.0;
            }
            if !g.left_vanishing_x.is_finite() {
                g.left_vanishing_x = -200.0;
            }
            if !g.right_vanishing_x.is_finite() {
                g.right_vanishing_x = 1000.0;
            }
            if (g.right_vanishing_x - g.left_vanishing_x).abs() < 10.0 {
                g.right_vanishing_x = g.left_vanishing_x + 100.0;
            }
            if !g.grid_cell_size.is_finite() || g.grid_cell_size <= 1.0 {
                g.grid_cell_size = 32.0;
            }
            if !g.grid_extent.is_finite() || g.grid_extent <= 10.0 {
                g.grid_extent = 480.0;
            }
            g
        });
        if self.active_perspective_grid() == clean {
            return false;
        }
        self.set_timeline_perspective_grid(clean);
        true
    }

    /// Toggle the active timeline's Perspective Grid on or off in 1 Undo step (`T-233`).
    pub fn toggle_perspective_grid(&mut self) -> bool {
        let mut grid = self.active_perspective_grid().unwrap_or_default();
        grid.enabled = !grid.enabled;
        self.apply_timeline_perspective_grid(Some(grid))
    }

    /// Switch the active `PerspectivePlane` (`LeftGrid`, `RightGrid`, `HorizontalGrid`) on the timeline's
    /// Perspective Grid in 1 Undo step (`T-233`).
    pub fn set_perspective_active_plane(&mut self, plane: PerspectivePlane) -> bool {
        let mut grid = self.active_perspective_grid().unwrap_or_default();
        grid.enabled = true;
        grid.active_plane = plane;
        self.apply_timeline_perspective_grid(Some(grid))
    }

    /// Drag a `PerspectiveGridHandle` (`Horizon`, `LeftVanishingPoint`, `RightVanishingPoint`) on
    /// Stage to `stage_pos` in 1 Undo step (`T-233`).
    pub fn drag_perspective_grid_handle(
        &mut self,
        handle: PerspectiveGridHandle,
        stage_pos: [f64; 2],
    ) -> bool {
        if !stage_pos[0].is_finite() || !stage_pos[1].is_finite() {
            return false;
        }
        let mut grid = self.active_perspective_grid().unwrap_or_default();
        grid.enabled = true;
        match handle {
            PerspectiveGridHandle::Horizon => {
                grid.horizon_y = stage_pos[1].clamp(-10_000.0, 10_000.0);
            }
            PerspectiveGridHandle::LeftVanishingPoint => {
                grid.left_vanishing_x = stage_pos[0]
                    .clamp(-20_000.0, 20_000.0)
                    .min(grid.right_vanishing_x - 20.0);
                grid.horizon_y = stage_pos[1].clamp(-10_000.0, 10_000.0);
            }
            PerspectiveGridHandle::RightVanishingPoint => {
                grid.right_vanishing_x = stage_pos[0]
                    .clamp(-20_000.0, 20_000.0)
                    .max(grid.left_vanishing_x + 20.0);
                grid.horizon_y = stage_pos[1].clamp(-10_000.0, 10_000.0);
            }
        }
        self.apply_timeline_perspective_grid(Some(grid))
    }

    /// Hit-test a Stage-space position against the active Perspective Grid vanishing/horizon
    /// handles (`T-233`).
    pub fn hit_test_perspective_grid_handle(
        &self,
        stage_pos: [f64; 2],
        tolerance: f64,
    ) -> Option<PerspectiveGridHandle> {
        let grid = self.active_perspective_grid()?;
        if !grid.enabled {
            return None;
        }
        let tol = tolerance.max(2.0);
        let left_d2 = (stage_pos[0] - grid.left_vanishing_x).powi(2)
            + (stage_pos[1] - grid.horizon_y).powi(2);
        if left_d2 <= tol * tol {
            return Some(PerspectiveGridHandle::LeftVanishingPoint);
        }
        let right_d2 = (stage_pos[0] - grid.right_vanishing_x).powi(2)
            + (stage_pos[1] - grid.horizon_y).powi(2);
        if right_d2 <= tol * tol {
            return Some(PerspectiveGridHandle::RightVanishingPoint);
        }
        if (stage_pos[1] - grid.horizon_y).abs() <= tol {
            return Some(PerspectiveGridHandle::Horizon);
        }
        None
    }
}

fn bernstein3(index: usize, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let u = 1.0 - t;
    match index {
        0 => u * u * u,
        1 => 3.0 * t * u * u,
        2 => 3.0 * t * t * u,
        3 => t * t * t,
        _ => 0.0,
    }
}

fn default_envelope_4x4_handles(bounds: [f64; 4]) -> [[f64; 2]; 16] {
    let [min_x, min_y, max_x, max_y] = bounds;
    let w = (max_x - min_x).max(1.0);
    let h = (max_y - min_y).max(1.0);
    let mut out = [[0.0; 2]; 16];
    for r in 0..4 {
        for c in 0..4 {
            out[r * 4 + c] = [min_x + (c as f64 / 3.0) * w, min_y + (r as f64 / 3.0) * h];
        }
    }
    out
}

fn default_perspective_2x2_corners(bounds: [f64; 4]) -> [[f64; 2]; 4] {
    let [min_x, min_y, max_x, max_y] = bounds;
    [
        [min_x, min_y],
        [max_x, min_y],
        [max_x, max_y],
        [min_x, max_y],
    ]
}

fn evaluate_envelope_4x4_ffd(
    pt: [f64; 2],
    baseline_bounds: [f64; 4],
    handles: &[[f64; 2]; 16],
) -> [f64; 2] {
    let [min_x, min_y, max_x, max_y] = baseline_bounds;
    let w = (max_x - min_x).max(1.0);
    let h = (max_y - min_y).max(1.0);
    let u = ((pt[0] - min_x) / w).clamp(0.0, 1.0);
    let v = ((pt[1] - min_y) / h).clamp(0.0, 1.0);
    let rest_handles = default_envelope_4x4_handles(baseline_bounds);
    let mut dx = 0.0;
    let mut dy = 0.0;
    for r in 0..4 {
        let bv = bernstein3(r, v);
        for c in 0..4 {
            let weight = bv * bernstein3(c, u);
            let idx = r * 4 + c;
            dx += weight * (handles[idx][0] - rest_handles[idx][0]);
            dy += weight * (handles[idx][1] - rest_handles[idx][1]);
        }
    }
    [pt[0] + dx, pt[1] + dy]
}

fn evaluate_perspective_quad_map(
    pt: [f64; 2],
    baseline_bounds: [f64; 4],
    corners: &[[f64; 2]; 4],
) -> [f64; 2] {
    let [min_x, min_y, max_x, max_y] = baseline_bounds;
    let w = (max_x - min_x).max(1.0);
    let h = (max_y - min_y).max(1.0);
    let u = (pt[0] - min_x) / w;
    let v = (pt[1] - min_y) / h;
    let top_x = corners[0][0] + (corners[1][0] - corners[0][0]) * u;
    let top_y = corners[0][1] + (corners[1][1] - corners[0][1]) * u;
    let bot_x = corners[3][0] + (corners[2][0] - corners[3][0]) * u;
    let bot_y = corners[3][1] + (corners[2][1] - corners[3][1]) * u;
    [top_x + (bot_x - top_x) * v, top_y + (bot_y - top_y) * v]
}

fn subdivide_cubic_commands(commands: &[PathCommand], splits: usize) -> Vec<PathCommand> {
    let splits = splits.max(1);
    let mut out = Vec::with_capacity(commands.len() * splits);
    let mut cur = [0.0_f64; 2];
    let mut start = [0.0_f64; 2];
    let split_once = |p0: [f64; 2],
                      p1: [f64; 2],
                      p2: [f64; 2],
                      p3: [f64; 2],
                      t: f64|
     -> ([[f64; 2]; 4], [[f64; 2]; 4]) {
        let lerp =
            |a: [f64; 2], b: [f64; 2], s: f64| [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s];
        let p01 = lerp(p0, p1, t);
        let p12 = lerp(p1, p2, t);
        let p23 = lerp(p2, p3, t);
        let p012 = lerp(p01, p12, t);
        let p123 = lerp(p12, p23, t);
        let p0123 = lerp(p012, p123, t);
        ([p0, p01, p012, p0123], [p0123, p123, p23, p3])
    };
    let append_subdivided = |out: &mut Vec<PathCommand>,
                             p0: [f64; 2],
                             p1: [f64; 2],
                             p2: [f64; 2],
                             p3: [f64; 2],
                             n: usize| {
        let mut rem_p0 = p0;
        let mut rem_p1 = p1;
        let mut rem_p2 = p2;
        let mut rem_p3 = p3;
        for i in 0..n {
            let rem = n - i;
            if rem <= 1 {
                out.push(PathCommand::CubicTo {
                    cx1: rem_p1[0],
                    cy1: rem_p1[1],
                    cx2: rem_p2[0],
                    cy2: rem_p2[1],
                    x: rem_p3[0],
                    y: rem_p3[1],
                });
                break;
            }
            let t = 1.0 / (rem as f64);
            let (left, right) = split_once(rem_p0, rem_p1, rem_p2, rem_p3, t);
            out.push(PathCommand::CubicTo {
                cx1: left[1][0],
                cy1: left[1][1],
                cx2: left[2][0],
                cy2: left[2][1],
                x: left[3][0],
                y: left[3][1],
            });
            rem_p0 = right[0];
            rem_p1 = right[1];
            rem_p2 = right[2];
            rem_p3 = right[3];
        }
    };
    for &cmd in commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                cur = [x, y];
                start = [x, y];
                out.push(cmd);
            }
            PathCommand::LineTo { x, y } => {
                let p0 = cur;
                let p3 = [x, y];
                let p1 = [p0[0] + (p3[0] - p0[0]) / 3.0, p0[1] + (p3[1] - p0[1]) / 3.0];
                let p2 = [
                    p0[0] + (p3[0] - p0[0]) * 2.0 / 3.0,
                    p0[1] + (p3[1] - p0[1]) * 2.0 / 3.0,
                ];
                append_subdivided(&mut out, p0, p1, p2, p3, splits);
                cur = p3;
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let p0 = cur;
                let p3 = [x, y];
                let p1 = [
                    p0[0] + (cx - p0[0]) * 2.0 / 3.0,
                    p0[1] + (cy - p0[1]) * 2.0 / 3.0,
                ];
                let p2 = [
                    p3[0] + (cx - p3[0]) * 2.0 / 3.0,
                    p3[1] + (cy - p3[1]) * 2.0 / 3.0,
                ];
                append_subdivided(&mut out, p0, p1, p2, p3, splits);
                cur = p3;
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                append_subdivided(&mut out, cur, [cx1, cy1], [cx2, cy2], [x, y], splits);
                cur = [x, y];
            }
            PathCommand::Close => {
                if (cur[0] - start[0]).hypot(cur[1] - start[1]) > 1e-4 {
                    let p0 = cur;
                    let p3 = start;
                    let p1 = [p0[0] + (p3[0] - p0[0]) / 3.0, p0[1] + (p3[1] - p0[1]) / 3.0];
                    let p2 = [
                        p0[0] + (p3[0] - p0[0]) * 2.0 / 3.0,
                        p0[1] + (p3[1] - p0[1]) * 2.0 / 3.0,
                    ];
                    append_subdivided(&mut out, p0, p1, p2, p3, splits);
                    cur = start;
                }
                out.push(PathCommand::Close);
            }
        }
    }
    out
}

pub(super) fn map_element_stage_points(
    base_element: &Element,
    subdivide: usize,
    map_stage: impl Fn([f64; 2]) -> [f64; 2],
) -> Option<Element> {
    let raw_cmds = editable_element_commands(base_element)
        .or_else(|| base_element.to_path_commands().map(|(c, _)| c))?;
    let commands = if subdivide > 1 && raw_cmds.len() <= 16 {
        subdivide_cubic_commands(&raw_cmds, subdivide)
    } else {
        subdivide_cubic_commands(&raw_cmds, 1)
    };
    let mat = transform_matrix(base_element.transform);
    let to_local = |stage_pt: [f64; 2]| -> [f64; 2] {
        if let Some((lx, ly)) = inverse_f64(mat, stage_pt[0], stage_pt[1]) {
            [lx, ly]
        } else {
            [
                stage_pt[0] - base_element.transform.x,
                stage_pt[1] - base_element.transform.y,
            ]
        }
    };
    let map_local = |lx: f64, ly: f64| -> [f64; 2] {
        let sx = mat[0] * lx + mat[2] * ly + mat[4];
        let sy = mat[1] * lx + mat[3] * ly + mat[5];
        to_local(map_stage([sx, sy]))
    };
    let mut warped = Vec::with_capacity(commands.len());
    for cmd in commands {
        let mapped = match cmd {
            PathCommand::MoveTo { x, y } => {
                let [nx, ny] = map_local(x, y);
                PathCommand::MoveTo { x: nx, y: ny }
            }
            PathCommand::LineTo { x, y } => {
                let [nx, ny] = map_local(x, y);
                PathCommand::LineTo { x: nx, y: ny }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let [ncx, ncy] = map_local(cx, cy);
                let [nx, ny] = map_local(x, y);
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
                let [nc1x, nc1y] = map_local(cx1, cy1);
                let [nc2x, nc2y] = map_local(cx2, cy2);
                let [nx, ny] = map_local(x, y);
                PathCommand::CubicTo {
                    cx1: nc1x,
                    cy1: nc1y,
                    cx2: nc2x,
                    cy2: nc2y,
                    x: nx,
                    y: ny,
                }
            }
            PathCommand::Close => PathCommand::Close,
        };
        warped.push(mapped);
    }
    let mut updated = base_element.clone();
    write_editable_element_commands(&mut updated, warped);
    Some(updated)
}

pub(super) fn element_stage_bounds(el: &Element) -> Option<[f64; 4]> {
    let (cmds, _) = el.to_path_commands()?;
    let mat = transform_matrix(el.transform);
    let stage_cmds = map_commands_forward(&cmds, mat);
    let b = neoflash_core::path_bounds(&stage_cmds).ok().flatten()?;
    Some([b.x_min, b.y_min, b.x_max, b.y_max])
}

fn elements_hash(elements: &[Element]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for el in elements {
        el.id.hash(&mut hasher);
        el.transform.x.to_bits().hash(&mut hasher);
        el.transform.y.to_bits().hash(&mut hasher);
        el.transform.scale_x.to_bits().hash(&mut hasher);
        el.transform.scale_y.to_bits().hash(&mut hasher);
        el.transform.rotation.to_bits().hash(&mut hasher);
        el.transform.skew.to_bits().hash(&mut hasher);
        if let Some((cmds, _)) = el.to_path_commands() {
            cmds.len().hash(&mut hasher);
            for cmd in cmds {
                match cmd {
                    PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                        x.to_bits().hash(&mut hasher);
                        y.to_bits().hash(&mut hasher);
                    }
                    PathCommand::QuadraticTo { cx, cy, x, y } => {
                        cx.to_bits().hash(&mut hasher);
                        cy.to_bits().hash(&mut hasher);
                        x.to_bits().hash(&mut hasher);
                        y.to_bits().hash(&mut hasher);
                    }
                    PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x,
                        y,
                    } => {
                        cx1.to_bits().hash(&mut hasher);
                        cy1.to_bits().hash(&mut hasher);
                        cx2.to_bits().hash(&mut hasher);
                        cy2.to_bits().hash(&mut hasher);
                        x.to_bits().hash(&mut hasher);
                        y.to_bits().hash(&mut hasher);
                    }
                    PathCommand::Close => 99_u8.hash(&mut hasher),
                }
            }
        }
    }
    hasher.finish()
}

impl Editor {
    /// Return the active Free Transform submode (`ScaleRotateSkew`, `Envelope`, `Perspective`, `Distort`) (`T-271`).
    pub fn free_transform_submode(&self) -> FreeTransformSubmode {
        self.vector.free_transform_submode
    }

    /// Switch the active Free Transform submode and synchronize `FreeTransformMode` (`T-271`).
    #[allow(dead_code)]
    pub fn set_free_transform_submode(&mut self, submode: FreeTransformSubmode) {
        self.vector.free_transform_submode = submode;
        self.authoring_tools.transform_mode = match submode {
            FreeTransformSubmode::ScaleRotateSkew => {
                crate::authoring_tools::FreeTransformMode::FreeTransform
            }
            FreeTransformSubmode::Envelope => crate::authoring_tools::FreeTransformMode::Envelope,
            FreeTransformSubmode::Perspective | FreeTransformSubmode::Distort => {
                crate::authoring_tools::FreeTransformMode::Distort
            }
        };
    }

    pub(super) fn selected_vector_elements_on_active_key(&self) -> Vec<Element> {
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return Vec::new();
        };
        let Some(key) = layer.key_at(self.frame) else {
            return Vec::new();
        };
        let mut ids: Vec<Id> = self
            .selection
            .targets
            .iter()
            .filter(|t| t.layer == layer.id)
            .map(|t| t.element.clone())
            .collect();
        if ids.is_empty()
            && let Some(el) = self.selected_element()
        {
            ids.push(el.id.clone());
        }
        if ids.is_empty() && key.elements.len() == 1 {
            ids.push(key.elements[0].id.clone());
        }
        key.elements
            .iter()
            .filter(|el| {
                ids.contains(&el.id)
                    && !key.controller_elements.contains(&el.id)
                    && el.to_path_commands().is_some()
            })
            .cloned()
            .collect()
    }

    pub(crate) fn selected_vector_bounds_on_stage(&self) -> Option<[f64; 4]> {
        let els = self.selected_vector_elements_on_active_key();
        if els.is_empty() {
            return None;
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for el in &els {
            if let Some([x0, y0, x1, y1]) = element_stage_bounds(el) {
                min_x = min_x.min(x0);
                min_y = min_y.min(y0);
                max_x = max_x.max(x1);
                max_y = max_y.max(y1);
            }
        }
        (min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite())
            .then_some([min_x, min_y, max_x, max_y])
    }

    /// Return the 16 (`4x4`) interactive `Envelope` lattice handles `(row, col, [stage_x, stage_y])`
    /// for the selected vector shape(s) on Stage (`T-271`).
    pub fn envelope_warp_handles(&self) -> Vec<(usize, usize, [f64; 2])> {
        let els = self.selected_vector_elements_on_active_key();
        if els.is_empty() {
            return Vec::new();
        }
        let cur_hash = elements_hash(&els);
        let el_ids: Vec<Id> = els.iter().map(|e| e.id.clone()).collect();
        let handles_16 = if let Some(session) = &self.vector.envelope_lattice_session
            && session.symbol_id == self.symbol_id()
            && session.layer_idx == self.layer
            && session.frame == self.frame
            && session.element_ids == el_ids
            && let Some(saved) = session.handles_by_hash.get(&cur_hash)
        {
            *saved
        } else if let Some(bounds) = self.selected_vector_bounds_on_stage() {
            default_envelope_4x4_handles(bounds)
        } else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(16);
        for r in 0..4 {
            for c in 0..4 {
                out.push((r, c, handles_16[r * 4 + c]));
            }
        }
        out
    }

    /// Hit-test a Stage-space point against the 16 (`4x4`) `Envelope` lattice handles (`T-271`).
    #[allow(dead_code)]
    pub fn hit_test_envelope_warp_handle(
        &self,
        stage_pt: [f64; 2],
        tolerance: f64,
    ) -> Option<(usize, usize)> {
        let tol_sq = tolerance.max(1.5).powi(2);
        self.envelope_warp_handles()
            .into_iter()
            .filter_map(|(r, c, pos)| {
                let d2 = (pos[0] - stage_pt[0]).powi(2) + (pos[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((r, c, d2))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(r, c, _)| (r, c))
    }

    pub(crate) fn apply_envelope_warp_handle_in_place(
        &mut self,
        row: usize,
        col: usize,
        target_stage: [f64; 2],
    ) -> bool {
        if row >= 4
            || col >= 4
            || !target_stage[0].is_finite()
            || !target_stage[1].is_finite()
            || !self.layer_editable()
        {
            return false;
        }
        let els = self.selected_vector_elements_on_active_key();
        if els.is_empty() {
            return false;
        }
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let el_ids: Vec<Id> = els.iter().map(|e| e.id.clone()).collect();
        let cur_hash = elements_hash(&els);

        let needs_init = self
            .vector
            .envelope_lattice_session
            .as_ref()
            .is_none_or(|s| {
                s.symbol_id != symbol_id
                    || s.layer_idx != layer_idx
                    || s.frame != frame
                    || s.element_ids != el_ids
                    || !s.handles_by_hash.contains_key(&cur_hash)
            });
        if needs_init {
            let Some(bounds) = self.selected_vector_bounds_on_stage() else {
                return false;
            };
            let initial_handles = default_envelope_4x4_handles(bounds);
            let mut handles_by_hash = HashMap::new();
            handles_by_hash.insert(cur_hash, initial_handles);
            self.vector.envelope_lattice_session = Some(EnvelopeLatticeSession {
                symbol_id: symbol_id.clone(),
                layer_idx,
                frame,
                element_ids: el_ids.clone(),
                baseline_bounds: bounds,
                baseline_elements: els.iter().map(|e| (e.id.clone(), e.clone())).collect(),
                handles_by_hash,
            });
        }

        let Some(session) = self.vector.envelope_lattice_session.as_ref() else {
            return false;
        };
        let baseline_bounds = session.baseline_bounds;
        let baseline_elements = session.baseline_elements.clone();
        let mut handles = session
            .handles_by_hash
            .get(&cur_hash)
            .copied()
            .unwrap_or_else(|| default_envelope_4x4_handles(baseline_bounds));
        let idx = row * 4 + col;
        if (handles[idx][0] - target_stage[0]).hypot(handles[idx][1] - target_stage[1]) <= 1e-6 {
            return false;
        }
        handles[idx] = target_stage;

        let bw = (baseline_bounds[2] - baseline_bounds[0]).max(1.0);
        let bh = (baseline_bounds[3] - baseline_bounds[1]).max(1.0);
        let uv_points: Vec<[f64; 2]> = handles
            .iter()
            .map(|[sx, sy]| {
                [
                    ((sx - baseline_bounds[0]) / bw).clamp(-4.0, 5.0),
                    ((sy - baseline_bounds[1]) / bh).clamp(-4.0, 5.0),
                ]
            })
            .collect();

        self.ensure_key(false);
        let symbol = self.symbol_mut();
        let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
            return false;
        };
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
            return false;
        };
        let mut updated_els = Vec::new();
        for (id, base_el) in &baseline_elements {
            let Some(warped_el) = map_element_stage_points(base_el, 4, |pt| {
                evaluate_envelope_4x4_ffd(pt, baseline_bounds, &handles)
            }) else {
                continue;
            };
            if let Some(slot) = key.elements.iter_mut().find(|e| &e.id == id) {
                *slot = warped_el.clone();
                updated_els.push(warped_el);
            }
            let mut fx = key
                .vector_effects(id)
                .cloned()
                .unwrap_or(ElementVectorEffects {
                    element_id: id.clone(),
                    ..Default::default()
                });
            fx.envelope_warp = Some(EnvelopeWarp {
                preset: EnvelopeWarpPreset::CustomGrid,
                rows: 3,
                cols: 3,
                control_points: uv_points.clone(),
                ..EnvelopeWarp::default()
            });
            key.set_vector_effects(fx);
        }
        if updated_els.is_empty() {
            return false;
        }
        let new_hash = elements_hash(&updated_els);
        if let Some(session) = self.vector.envelope_lattice_session.as_mut() {
            session.handles_by_hash.insert(new_hash, handles);
        }
        true
    }

    /// Move a `(row, col)` handle of the `4x4` `Envelope` lattice to `target_stage` in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn move_envelope_warp_handle(
        &mut self,
        row: usize,
        col: usize,
        target_stage: [f64; 2],
    ) -> bool {
        self.history.begin(&self.project);
        if !self.apply_envelope_warp_handle_in_place(row, col, target_stage) {
            self.history.cancel(&mut self.project);
            return false;
        }
        self.commit_history();
        true
    }

    /// Return the 4 (`2x2`) interactive `Perspective` corner handles `[TL, TR, BR, BL]` in Stage
    /// coordinates for the selected vector shape(s) (`T-271`).
    pub fn perspective_corners(&self) -> Option<[[f64; 2]; 4]> {
        let els = self.selected_vector_elements_on_active_key();
        if els.is_empty() {
            return None;
        }
        let cur_hash = elements_hash(&els);
        let el_ids: Vec<Id> = els.iter().map(|e| e.id.clone()).collect();
        if let Some(session) = &self.vector.perspective_quad_session
            && session.symbol_id == self.symbol_id()
            && session.layer_idx == self.layer
            && session.frame == self.frame
            && session.element_ids == el_ids
            && let Some(saved) = session.corners_by_hash.get(&cur_hash)
        {
            return Some(*saved);
        }
        let bounds = self.selected_vector_bounds_on_stage()?;
        Some(default_perspective_2x2_corners(bounds))
    }

    /// Hit-test a Stage-space point against the 4 (`2x2`) `Perspective` corner handles (`0..4`) (`T-271`).
    #[allow(dead_code)]
    pub fn hit_test_perspective_corner(&self, stage_pt: [f64; 2], tolerance: f64) -> Option<usize> {
        let corners = self.perspective_corners()?;
        let tol_sq = tolerance.max(1.5).powi(2);
        corners
            .iter()
            .enumerate()
            .filter_map(|(i, pos)| {
                let d2 = (pos[0] - stage_pt[0]).powi(2) + (pos[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((i, d2))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
    }

    pub(crate) fn apply_perspective_corner_in_place(
        &mut self,
        corner: usize,
        target_stage: [f64; 2],
        symmetric_taper: bool,
    ) -> bool {
        if corner >= 4
            || !target_stage[0].is_finite()
            || !target_stage[1].is_finite()
            || !self.layer_editable()
        {
            return false;
        }
        let els = self.selected_vector_elements_on_active_key();
        if els.is_empty() {
            return false;
        }
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let el_ids: Vec<Id> = els.iter().map(|e| e.id.clone()).collect();
        let cur_hash = elements_hash(&els);

        let needs_init = self
            .vector
            .perspective_quad_session
            .as_ref()
            .is_none_or(|s| {
                s.symbol_id != symbol_id
                    || s.layer_idx != layer_idx
                    || s.frame != frame
                    || s.element_ids != el_ids
                    || !s.corners_by_hash.contains_key(&cur_hash)
            });
        if needs_init {
            let Some(bounds) = self.selected_vector_bounds_on_stage() else {
                return false;
            };
            let initial_corners = default_perspective_2x2_corners(bounds);
            let mut corners_by_hash = HashMap::new();
            corners_by_hash.insert(cur_hash, initial_corners);
            self.vector.perspective_quad_session = Some(PerspectiveQuadSession {
                symbol_id: symbol_id.clone(),
                layer_idx,
                frame,
                element_ids: el_ids.clone(),
                baseline_bounds: bounds,
                baseline_elements: els.iter().map(|e| (e.id.clone(), e.clone())).collect(),
                corners_by_hash,
            });
        }

        let Some(session) = self.vector.perspective_quad_session.as_ref() else {
            return false;
        };
        let baseline_bounds = session.baseline_bounds;
        let baseline_elements = session.baseline_elements.clone();
        let mut corners = session
            .corners_by_hash
            .get(&cur_hash)
            .copied()
            .unwrap_or_else(|| default_perspective_2x2_corners(baseline_bounds));
        if (corners[corner][0] - target_stage[0]).hypot(corners[corner][1] - target_stage[1])
            <= 1e-6
        {
            return false;
        }
        let prev = corners[corner];
        corners[corner] = target_stage;
        if symmetric_taper {
            let dx = target_stage[0] - prev[0];
            let dy = target_stage[1] - prev[1];
            let horiz_mirror = match corner {
                0 => 1,
                1 => 0,
                2 => 3,
                _ => 2,
            };
            let vert_mirror = match corner {
                0 => 3,
                1 => 2,
                2 => 1,
                _ => 0,
            };
            if dx.abs() >= dy.abs() {
                corners[horiz_mirror][0] -= dx;
                corners[horiz_mirror][1] += dy;
            } else {
                corners[vert_mirror][0] += dx;
                corners[vert_mirror][1] -= dy;
            }
        }

        self.ensure_key(false);
        let symbol = self.symbol_mut();
        let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
            return false;
        };
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
            return false;
        };
        let mut updated_els = Vec::new();
        for (id, base_el) in &baseline_elements {
            let Some(warped_el) = map_element_stage_points(base_el, 2, |pt| {
                evaluate_perspective_quad_map(pt, baseline_bounds, &corners)
            }) else {
                continue;
            };
            if let Some(slot) = key.elements.iter_mut().find(|e| &e.id == id) {
                *slot = warped_el.clone();
                updated_els.push(warped_el);
            }
        }
        if updated_els.is_empty() {
            return false;
        }
        let new_hash = elements_hash(&updated_els);
        if let Some(session) = self.vector.perspective_quad_session.as_mut() {
            session.corners_by_hash.insert(new_hash, corners);
        }
        true
    }

    /// Move corner `corner` (`0..4`: TL, TR, BR, BL) of the `2x2` `Perspective` quad to `target_stage`
    /// in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn move_perspective_corner(&mut self, corner: usize, target_stage: [f64; 2]) -> bool {
        self.history.begin(&self.project);
        if !self.apply_perspective_corner_in_place(corner, target_stage, false) {
            self.history.cancel(&mut self.project);
            return false;
        }
        self.commit_history();
        true
    }
}
