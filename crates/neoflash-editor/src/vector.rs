//! Vector geometry rendering and direct authoring. Draft gestures are not document state.
mod cache_paint;
mod deform_envelope;
mod deform_warp;
mod editor_tools;
mod handles_recognition;
mod smooth_reshape;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub(crate) use cache_paint::{
    PathPaint, RetainedPathStyle, VectorPaintVisibility, paint_gradient_path, paint_path,
    paint_retained_stage_path, path_bounds_cached, tessellate, tessellate_cached,
    tessellate_cached_with_fingerprint, transformed_path_bounds_cached, vector_paint_visibility,
};
#[allow(unused_imports)]
pub use deform_envelope::PerspectiveGridHandle;
#[allow(unused_imports)]
pub(crate) use handles_recognition::{
    RecognisedShape, collapse_anchor_handles, handles, move_handle, move_selected_anchors,
    pull_anchor_tangent, recognise_primitive_shape, recognise_straight_line,
};
#[allow(unused_imports)]
pub(crate) use smooth_reshape::{
    ReshapeAction, ReshapeVertexKind, apply_reshape_action, classify_reshape_vertices,
    delete_anchors_from_path, editable_element_commands, hit_reshape_on_commands,
    path_segment_hit_local, smooth_path_commands, straighten_path_commands,
    write_editable_element_commands,
};

use crate::{app::Tool, stage_mapping::StageMapping};
use eframe::egui::{Pos2, Vec2};
use neoflash_core::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub point: Pos2,
    pub tangent: Vec2,
}
#[derive(Clone, Debug)]
pub(crate) struct PathDraft {
    pub anchors: Vec<Anchor>,
    pub pencil: bool,
    pub dragging: bool,
    pub closing: bool,
    pub limit_reached: bool,
    pub symbol: String,
    pub frame: u32,
    pub layer: usize,
    pub layer_id: String,
    /// View captured for the current press/drag phase. Pen anchors may span
    /// several phases and views; an individual pointer drag may not.
    pub pointer_origin: Pos2,
    pub pointer_zoom: f32,
    pub pointer_mapping: StageMapping,
}
fn smooth_pencil_tangents(anchors: &mut [Anchor]) {
    let points: Vec<_> = anchors.iter().map(|a| a.point).collect();
    for (index, anchor) in anchors.iter_mut().enumerate() {
        let before = points[index.saturating_sub(1)];
        let after = points[(index + 1).min(points.len() - 1)];
        // Catmull–Rom converted to symmetric cubic handles, bounded by adjacent segments.
        let tangent = (after - before) / 6.;
        let limit = if index == 0 {
            anchor.point.distance(after)
        } else if index + 1 == points.len() {
            anchor.point.distance(before)
        } else {
            anchor
                .point
                .distance(before)
                .min(anchor.point.distance(after))
        } / 3.;
        anchor.tangent = tangent.normalized() * tangent.length().min(limit);
    }
}
impl PathDraft {
    pub fn commands(&self, closed: bool) -> Vec<PathCommand> {
        let Some(first) = self.anchors.first() else {
            return vec![];
        };
        let mut commands = vec![PathCommand::MoveTo {
            x: first.point.x as f64,
            y: first.point.y as f64,
        }];
        let mut previous = first;
        for next in self
            .anchors
            .iter()
            .skip(1)
            .chain(if closed { Some(first) } else { None })
        {
            if previous.tangent.length_sq() > 0.01 || next.tangent.length_sq() > 0.01 {
                let c1 = previous.point + previous.tangent;
                let c2 = next.point - next.tangent;
                commands.push(PathCommand::CubicTo {
                    cx1: c1.x as f64,
                    cy1: c1.y as f64,
                    cx2: c2.x as f64,
                    cy2: c2.y as f64,
                    x: next.point.x as f64,
                    y: next.point.y as f64,
                });
            } else {
                commands.push(PathCommand::LineTo {
                    x: next.point.x as f64,
                    y: next.point.y as f64,
                });
            }
            previous = next;
        }
        if closed {
            commands.push(PathCommand::Close);
        }
        commands
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PointRole {
    Anchor,
    Control1,
    Control2,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct PathHandle {
    pub command: usize,
    pub role: PointRole,
    pub point: Pos2,
}
#[derive(Clone, Debug)]
pub(crate) struct PointDrag {
    pub handle: PathHandle,
    pub original: Vec<PathCommand>,
    pub inverse: [f64; 6],
    pub symbol: String,
    pub frame: u32,
    pub layer: String,
    pub origin: Pos2,
    pub zoom: f32,
    pub mapping: StageMapping,
    pub press: Pos2,
    pub began: bool,
    pub changed: bool,
    pub element: String,
    pub pull_tangent: bool,
}

/// Submode for `Tool::Transform` (`Tool::FreeTransform`) on the live Stage (`T-271`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FreeTransformSubmode {
    #[default]
    ScaleRotateSkew,
    Envelope,
    Perspective,
    Distort,
}

#[allow(non_upper_case_globals, dead_code)]
impl Tool {
    /// Alias for `Tool::Transform` matching the Free Transform tool name (`T-271`).
    pub const FreeTransform: Self = Self::Transform;
}

#[derive(Clone, Debug)]
pub(crate) struct EnvelopeLatticeSession {
    pub symbol_id: Id,
    pub layer_idx: usize,
    pub frame: u32,
    pub element_ids: Vec<Id>,
    pub baseline_bounds: [f64; 4],
    pub baseline_elements: Vec<(Id, Element)>,
    pub handles_by_hash: HashMap<u64, [[f64; 2]; 16]>,
}

#[derive(Clone, Debug)]
pub(crate) struct PerspectiveQuadSession {
    pub symbol_id: Id,
    pub layer_idx: usize,
    pub frame: u32,
    pub element_ids: Vec<Id>,
    pub baseline_bounds: [f64; 4],
    pub baseline_elements: Vec<(Id, Element)>,
    pub corners_by_hash: HashMap<u64, [[f64; 2]; 4]>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum StageLiveDeformKind {
    EnvelopeHandle {
        row: usize,
        col: usize,
    },
    PerspectiveCorner {
        corner: usize,
    },
    AssetWarpPin {
        pin_id: Id,
        element_id: Id,
        created_on_press: bool,
    },
    IkBoneEffector {
        armature_id: Id,
        bone_id: Id,
    },
    IkBoneCreate {
        start_stage: [f64; 2],
        current_stage: [f64; 2],
    },
    IkBindSweep {
        unbind: bool,
    },
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct StageLiveDeformDrag {
    pub kind: StageLiveDeformKind,
    pub before_project: Project,
    pub press_stage: [f64; 2],
    pub last_stage: [f64; 2],
    pub changed: bool,
}

#[derive(Default)]
pub(crate) struct VectorState {
    pub draft: Option<PathDraft>,
    pub point_drag: Option<PointDrag>,
    /// Selected anchor command indices for an editable path `(element_id, command_indices)`.
    pub selected_anchors: Option<(Id, Vec<usize>)>,
    /// Active Subselection (`Tool::Subselect`) marquee box `(press_screen, current_screen)`.
    pub subselect_marquee: Option<(Pos2, Pos2)>,
    /// Active Free Transform submode (`T-271`).
    pub free_transform_submode: FreeTransformSubmode,
    /// Cached 4x4 Envelope lattice session (`T-271`).
    pub envelope_lattice_session: Option<EnvelopeLatticeSession>,
    /// Cached 2x2 Perspective quad session (`T-271`).
    pub perspective_quad_session: Option<PerspectiveQuadSession>,
    /// Undeformed base shapes for Asset Warp pin deformation `(symbol_id, frame, element_id)` (`T-271`).
    pub asset_warp_base_elements: HashMap<(Id, u32, Id), Element>,
    /// Currently selected `WarpPin` ID on Stage (`T-271`).
    pub selected_warp_pin: Option<Id>,
    /// Currently selected `IkBone` ID on Stage (`T-271`).
    pub selected_ik_bone: Option<Id>,
    /// Undeformed base shapes and rest bone poses for IK deformation `(symbol_id, frame, element_id)` (`T-271`).
    #[allow(clippy::type_complexity)]
    pub ik_base_elements: HashMap<(Id, u32, Id), (Element, Vec<(Id, [f64; 2], [f64; 2], f64)>)>,
    /// Active live Stage drag transaction for `Envelope`, `Perspective`, `AssetWarp`, `Bone`, `Bind` (`T-271`).
    pub live_deform_drag: Option<StageLiveDeformDrag>,
}
