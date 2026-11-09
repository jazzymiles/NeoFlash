//! Complete Flash CS4–CS6 / Adobe Animate Inverse Kinematics (`Bone Tool`, `Bind Tool`,
//! `IkShapeSkin`, hybrid FABRIK + constrained CCD solver with pins, X/Y translation and
//! spring/damping), Puppet / Asset Warp triangulated mesh (`WarpMesh`), procedural `Deco Tool`
//! & `Spray Brush` generators, and 3D perspective projection (`3D Rotation` & `3D Translation`) (`T-164`).

use crate::as3_fte_3d::{Display3dPose, compose_display_matrix3d, local3d_to_global};
use crate::evaluate::transform_matrix;
use crate::geometry::{deform_point_with_pins, path_bounds};
use crate::{
    AssetWarpRig, Color, Element, ElementKind, FillRule, Id, IkArmature, IkBone, InstancePlayback,
    PathCommand, Transform, WarpPin, WarpPinKind, new_id,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// 1. Inverse Kinematics: Extended Joint Specs, Bind Tool Skinning & FABRIK/CCD
// ============================================================================

/// Extended Flash CS4–CS6 / Animate joint constraints and spring parameters for an [`IkBone`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IkJointSpec {
    pub bone_id: Id,
    pub enable_rotation: bool,
    pub constrain_rotation: bool,
    pub min_angle_deg: f64,
    pub max_angle_deg: f64,
    pub enable_x_translation: bool,
    pub constrain_x_translation: bool,
    pub min_x_offset: f64,
    pub max_x_offset: f64,
    pub enable_y_translation: bool,
    pub constrain_y_translation: bool,
    pub min_y_offset: f64,
    pub max_y_offset: f64,
    /// Flash CS5.5+ Pin constraint locking this bone's tail/joint in world space.
    pub pinned: bool,
    /// Joint speed in `[0.0..=1.0]` (`100%` = `1.0`; `0%` = locked).
    pub speed: f64,
    /// Spring strength in `[0.0..=100.0]`.
    pub spring_strength: f64,
    /// Spring damping in `[0.0..=100.0]`.
    pub spring_damping: f64,
}

impl Default for IkJointSpec {
    fn default() -> Self {
        Self {
            bone_id: String::new(),
            enable_rotation: true,
            constrain_rotation: false,
            min_angle_deg: -45.0,
            max_angle_deg: 45.0,
            enable_x_translation: false,
            constrain_x_translation: false,
            min_x_offset: -50.0,
            max_x_offset: 50.0,
            enable_y_translation: false,
            constrain_y_translation: false,
            min_y_offset: -50.0,
            max_y_offset: 50.0,
            pinned: false,
            speed: 1.0,
            spring_strength: 0.0,
            spring_damping: 0.0,
        }
    }
}

fn valid_joint_spec(spec: &IkJointSpec) -> bool {
    !spec.bone_id.is_empty()
        && [
            spec.min_angle_deg,
            spec.max_angle_deg,
            spec.min_x_offset,
            spec.max_x_offset,
            spec.min_y_offset,
            spec.max_y_offset,
            spec.speed,
            spec.spring_strength,
            spec.spring_damping,
        ]
        .iter()
        .all(|value| value.is_finite())
        && (!spec.constrain_rotation || spec.min_angle_deg <= spec.max_angle_deg)
        && (!spec.constrain_x_translation || spec.min_x_offset <= spec.max_x_offset)
        && (!spec.constrain_y_translation || spec.min_y_offset <= spec.max_y_offset)
}

fn joint_spec<'a>(specs: &'a [IkJointSpec], bone_id: &str) -> Option<&'a IkJointSpec> {
    specs
        .iter()
        .take(IkBone::MAX_BONES)
        .find(|spec| spec.bone_id == bone_id && valid_joint_spec(spec))
}

/// Weight of one bone on a control point of an IK Shape (`Bind Tool`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IkBoneWeight {
    pub bone_id: Id,
    pub weight: f64,
}

/// Binding of a single path control point (`vertex_index`) to one or more bones (`Bind Tool`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IkVertexBinding {
    pub vertex_index: usize,
    pub weights: Vec<IkBoneWeight>,
}

/// Bind Tool skin mapping all control points of a vector shape (`shape_element_id`) to an [`IkArmature`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IkShapeSkin {
    pub shape_element_id: Id,
    pub armature_id: Id,
    /// Rest-pose bone head positions and world angles `(bone_id, [head_x, head_y], world_angle_deg)`.
    pub rest_bones: Vec<(Id, [f64; 2], f64)>,
    /// Rest-pose local control points in the same deterministic order as `bindings`.
    ///
    /// Older transient bindings did not retain these points, so deserialization keeps a
    /// compatibility fallback to the supplied shape geometry when this is empty.
    #[serde(default)]
    pub rest_points: Vec<[f64; 2]>,
    pub bindings: Vec<IkVertexBinding>,
}

fn map_affine(matrix: [f64; 6], point: [f64; 2]) -> [f64; 2] {
    [
        matrix[0] * point[0] + matrix[2] * point[1] + matrix[4],
        matrix[1] * point[0] + matrix[3] * point[1] + matrix[5],
    ]
}

fn inverse_affine(matrix: [f64; 6]) -> Option<[f64; 6]> {
    if matrix.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let det = matrix[0] * matrix[3] - matrix[1] * matrix[2];
    if !det.is_finite() || det.abs() <= 1e-12 {
        return None;
    }
    let inv_det = det.recip();
    let a = matrix[3] * inv_det;
    let b = -matrix[1] * inv_det;
    let c = -matrix[2] * inv_det;
    let d = matrix[0] * inv_det;
    Some([
        a,
        b,
        c,
        d,
        -(a * matrix[4] + c * matrix[5]),
        -(b * matrix[4] + d * matrix[5]),
    ])
}

/// Extract all control points (anchors and Bézier handles) from a `PathCommand` slice in deterministic order.
pub fn extract_path_control_points(commands: &[PathCommand]) -> Vec<[f64; 2]> {
    let mut pts = Vec::new();
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => pts.push([x, y]),
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                pts.push([cx, cy]);
                pts.push([x, y]);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                pts.push([cx1, cy1]);
                pts.push([cx2, cy2]);
                pts.push([x, y]);
            }
            PathCommand::Close => {}
        }
    }
    pts
}

fn point_to_segment_dist2(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let vx = b[0] - a[0];
    let vy = b[1] - a[1];
    let len2 = vx * vx + vy * vy;
    if len2 <= 1e-12 {
        let dx = p[0] - a[0];
        let dy = p[1] - a[1];
        return dx * dx + dy * dy;
    }
    let t = (((p[0] - a[0]) * vx + (p[1] - a[1]) * vy) / len2).clamp(0.0, 1.0);
    let proj_x = a[0] + t * vx;
    let proj_y = a[1] + t * vy;
    let dx = p[0] - proj_x;
    let dy = p[1] - proj_y;
    dx * dx + dy * dy
}

/// Compute world-space `(head_pos, tail_pos, world_angle_deg)` for every bone in `armature` from `elements`.
pub fn compute_bone_world_poses(
    armature: &IkArmature,
    elements: &[Element],
) -> Vec<(Id, [f64; 2], [f64; 2], f64)> {
    let mut out = Vec::with_capacity(armature.bones.len().min(IkBone::MAX_BONES));
    for bone in armature.bones.iter().take(IkBone::MAX_BONES) {
        let (hx, hy, angle) = elements
            .iter()
            .find(|e| e.id == bone.element_id)
            .map(|e| (e.transform.x, e.transform.y, e.transform.rotation))
            .unwrap_or((0.0, 0.0, 0.0));
        let len = bone.length.max(1.0);
        let rad = angle.to_radians();
        let tx = hx + len * rad.cos();
        let ty = hy + len * rad.sin();
        out.push((bone.id.clone(), [hx, hy], [tx, ty], angle));
    }
    out
}

/// Automatically bind all control points of `shape` to the bones of `armature` using inverse-segment-distance envelopes.
pub fn auto_bind_shape_to_armature(
    shape: &Element,
    armature: &IkArmature,
    elements: &[Element],
) -> Option<IkShapeSkin> {
    if shape.id.is_empty()
        || armature.id.is_empty()
        || armature.bones.is_empty()
        || armature.bones.len() > IkBone::MAX_BONES
    {
        return None;
    }
    let (commands, _) = shape.to_path_commands()?;
    let bone_poses = compute_bone_world_poses(armature, elements);
    if bone_poses.is_empty() {
        return None;
    }
    let rest_bones: Vec<(Id, [f64; 2], f64)> = bone_poses
        .iter()
        .map(|(id, head, _, ang)| (id.clone(), *head, *ang))
        .collect();
    let local_pts = extract_path_control_points(&commands);
    let shape_matrix = transform_matrix(shape.transform);
    let mut bindings = Vec::with_capacity(local_pts.len());

    for (vertex_index, &lp) in local_pts.iter().enumerate() {
        let wp = map_affine(shape_matrix, lp);
        let mut raw_weights: Vec<(Id, f64)> = bone_poses
            .iter()
            .map(|(bid, head, tail, _)| {
                let d2 = point_to_segment_dist2(wp, *head, *tail);
                let w = 1.0 / (d2.powf(1.25) + 1.0);
                (bid.clone(), w)
            })
            .collect();
        // Keep the top 3 bones per vertex (matching Flash CS4 Bind Tool envelope limit) and normalize.
        raw_weights.sort_by(|a, b| b.1.total_cmp(&a.1));
        raw_weights.truncate(3);
        let sum: f64 = raw_weights.iter().map(|(_, w)| *w).sum();
        let weights = if sum > 1e-12 {
            raw_weights
                .into_iter()
                .map(|(bone_id, w)| IkBoneWeight {
                    bone_id,
                    weight: w / sum,
                })
                .collect()
        } else {
            vec![IkBoneWeight {
                bone_id: bone_poses[0].0.clone(),
                weight: 1.0,
            }]
        };
        bindings.push(IkVertexBinding {
            vertex_index,
            weights,
        });
    }

    Some(IkShapeSkin {
        shape_element_id: shape.id.clone(),
        armature_id: armature.id.clone(),
        rest_bones,
        rest_points: local_pts,
        bindings,
    })
}

/// Bind Tool (`Z`) operation: explicitly assign or update the weight of `bone_id` on `vertex_indices` and renormalize.
pub fn bind_vertices_to_bone(
    skin: &mut IkShapeSkin,
    bone_id: &str,
    vertex_indices: &[usize],
    weight: f64,
    exclusive: bool,
) {
    if bone_id.is_empty() || !weight.is_finite() {
        return;
    }
    let clamped = weight.clamp(0.01, 1.0);
    for &v_idx in vertex_indices {
        let Some(binding) = skin.bindings.iter_mut().find(|b| b.vertex_index == v_idx) else {
            continue;
        };
        if exclusive {
            binding.weights = vec![IkBoneWeight {
                bone_id: bone_id.to_owned(),
                weight: 1.0,
            }];
            continue;
        }
        if let Some(existing) = binding.weights.iter_mut().find(|w| w.bone_id == bone_id) {
            existing.weight = clamped;
        } else {
            binding.weights.push(IkBoneWeight {
                bone_id: bone_id.to_owned(),
                weight: clamped,
            });
        }
        let sum: f64 = binding.weights.iter().map(|w| w.weight).sum();
        if sum > 1e-12 {
            for w in &mut binding.weights {
                w.weight /= sum;
            }
        }
    }
}

/// Deform an IK Shape (`shape`) using its [`IkShapeSkin`] and the posed bone elements (`posed_elements`).
pub fn deform_shape_with_ik_skin(
    shape: &Element,
    skin: &IkShapeSkin,
    armature: &IkArmature,
    posed_elements: &[Element],
) -> Element {
    if skin.shape_element_id != shape.id || skin.armature_id != armature.id {
        return shape.clone();
    }
    let Some((commands, fill_rule)) = shape.to_path_commands() else {
        return shape.clone();
    };
    let posed_bones = compute_bone_world_poses(armature, posed_elements);
    if posed_bones.is_empty() || skin.rest_bones.is_empty() {
        return shape.clone();
    }

    let shape_matrix = transform_matrix(shape.transform);
    let Some(shape_inverse) = inverse_affine(shape_matrix) else {
        return shape.clone();
    };
    let bindings_by_vertex: std::collections::HashMap<usize, &IkVertexBinding> = skin
        .bindings
        .iter()
        .map(|binding| (binding.vertex_index, binding))
        .collect();

    let transform_point = |v_idx: usize, fallback_lp: [f64; 2]| -> [f64; 2] {
        let lp = skin.rest_points.get(v_idx).copied().unwrap_or(fallback_lp);
        let wp = map_affine(shape_matrix, lp);
        let Some(binding) = bindings_by_vertex.get(&v_idx) else {
            return lp;
        };
        let mut acc_x = 0.0;
        let mut acc_y = 0.0;
        let mut w_sum = 0.0;
        for bw in &binding.weights {
            if !bw.weight.is_finite() || bw.weight <= 0.0 {
                continue;
            }
            let Some((_, rest_head, rest_ang)) =
                skin.rest_bones.iter().find(|(id, _, _)| id == &bw.bone_id)
            else {
                continue;
            };
            let Some((_, posed_head, _, posed_ang)) =
                posed_bones.iter().find(|(id, _, _, _)| id == &bw.bone_id)
            else {
                continue;
            };
            let d_ang = (posed_ang - rest_ang).to_radians();
            let (s, c) = d_ang.sin_cos();
            let rx = wp[0] - rest_head[0];
            let ry = wp[1] - rest_head[1];
            let tx = posed_head[0] + (c * rx - s * ry);
            let ty = posed_head[1] + (s * rx + c * ry);
            if !tx.is_finite() || !ty.is_finite() {
                continue;
            }
            acc_x += bw.weight * tx;
            acc_y += bw.weight * ty;
            w_sum += bw.weight;
        }
        if w_sum <= 1e-12 || !w_sum.is_finite() || !acc_x.is_finite() || !acc_y.is_finite() {
            lp
        } else {
            let mapped = map_affine(shape_inverse, [acc_x / w_sum, acc_y / w_sum]);
            if mapped.iter().all(|value| value.is_finite()) {
                mapped
            } else {
                lp
            }
        }
    };

    let mut cursor_idx = 0usize;
    let mut deformed = Vec::with_capacity(commands.len());
    for cmd in commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                let [nx, ny] = transform_point(cursor_idx, [x, y]);
                cursor_idx += 1;
                deformed.push(PathCommand::MoveTo { x: nx, y: ny });
            }
            PathCommand::LineTo { x, y } => {
                let [nx, ny] = transform_point(cursor_idx, [x, y]);
                cursor_idx += 1;
                deformed.push(PathCommand::LineTo { x: nx, y: ny });
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let [ncx, ncy] = transform_point(cursor_idx, [cx, cy]);
                let [nx, ny] = transform_point(cursor_idx + 1, [x, y]);
                cursor_idx += 2;
                deformed.push(PathCommand::QuadraticTo {
                    cx: ncx,
                    cy: ncy,
                    x: nx,
                    y: ny,
                });
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let [ncx1, ncy1] = transform_point(cursor_idx, [cx1, cy1]);
                let [ncx2, ncy2] = transform_point(cursor_idx + 1, [cx2, cy2]);
                let [nx, ny] = transform_point(cursor_idx + 2, [x, y]);
                cursor_idx += 3;
                deformed.push(PathCommand::CubicTo {
                    cx1: ncx1,
                    cy1: ncy1,
                    cx2: ncx2,
                    cy2: ncy2,
                    x: nx,
                    y: ny,
                });
            }
            PathCommand::Close => deformed.push(PathCommand::Close),
        }
    }

    let mut out = shape.clone();
    out.transform.primitive = None;
    match &shape.kind {
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
        } => {
            out.kind = ElementKind::Path {
                commands: deformed,
                fill_rule,
                fill: *fill,
                stroke: *stroke,
                stroke_width: *stroke_width,
            };
        }
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            out.kind = ElementKind::PaintedPath {
                commands: deformed,
                fill_rule,
                fill: *fill,
                stroke: *stroke,
                fill_gradient: fill_gradient.clone(),
                stroke_gradient: stroke_gradient.clone(),
                stroke_width: *stroke_width,
            };
        }
        _ => {}
    }
    out
}

/// Hybrid FABRIK + constrained CCD solver supporting pinned joints, rotation constraints, X/Y translation, and joint speed.
pub fn solve_ik_armature_fabrik_ccd(
    elements: &mut [Element],
    armature: &IkArmature,
    joint_specs: &[IkJointSpec],
) {
    if armature.bones.is_empty()
        || armature.bones.len() > IkBone::MAX_BONES
        || !armature.target_pos.iter().all(|value| value.is_finite())
        || armature
            .bones
            .iter()
            .any(|bone| !bone.length.is_finite() || bone.length <= 0.0)
        || armature.bones.iter().any(|bone| {
            elements
                .iter()
                .find(|element| element.id == bone.element_id)
                .is_none_or(|element| {
                    ![
                        element.transform.x,
                        element.transform.y,
                        element.transform.rotation,
                    ]
                    .iter()
                    .all(|value| value.is_finite())
                })
        })
    {
        return;
    }
    let mut chain_indices = Vec::new();
    let mut current_id = Some(armature.effector_bone_id.as_str());
    let mut guard = 0usize;
    while let Some(bid) = current_id {
        if guard >= IkBone::MAX_BONES {
            break;
        }
        let Some(idx) = armature.bones.iter().position(|b| b.id == bid) else {
            break;
        };
        if chain_indices.contains(&idx) {
            break;
        }
        chain_indices.push(idx);
        current_id = armature.bones[idx].parent_bone_id.as_deref();
        guard += 1;
    }
    chain_indices.reverse();
    if chain_indices.is_empty() {
        return;
    }

    let n = chain_indices.len();
    let mut joint_pos = vec![[0.0, 0.0]; n + 1];
    let mut world_angles = vec![0.0_f64; n];
    let mut lengths = vec![0.0_f64; n];
    let mut pinned = vec![false; n + 1];
    pinned[0] = true; // Root joint is anchored unless X/Y translation is enabled.

    let root_el_id = &armature.bones[chain_indices[0]].element_id;
    let root_origin = elements
        .iter()
        .find(|e| &e.id == root_el_id)
        .map(|e| [e.transform.x, e.transform.y])
        .unwrap_or([0.0, 0.0]);
    joint_pos[0] = root_origin;

    for (k, &b_idx) in chain_indices.iter().enumerate() {
        let bone = &armature.bones[b_idx];
        let spec = joint_spec(joint_specs, &bone.id);
        let rot = elements
            .iter()
            .find(|e| e.id == bone.element_id)
            .map(|e| e.transform.rotation)
            .unwrap_or(0.0);
        world_angles[k] = rot;
        lengths[k] = bone.length.max(1.0);
        let rad = rot.to_radians();
        joint_pos[k + 1] = [
            joint_pos[k][0] + lengths[k] * rad.cos(),
            joint_pos[k][1] + lengths[k] * rad.sin(),
        ];
        if spec.is_some_and(|s| s.pinned) {
            pinned[k + 1] = true;
        }
    }

    let pinned_rest = joint_pos.clone();
    let target = armature.target_pos;

    // If root bone allows X/Y translation, translate root toward target within translation limits before rotational solve.
    if let Some(root_spec) = joint_spec(joint_specs, &armature.bones[chain_indices[0]].id) {
        let total_reach: f64 = lengths.iter().sum();
        let dx = target[0] - joint_pos[0][0];
        let dy = target[1] - joint_pos[0][1];
        let dist = dx.hypot(dy);
        if dist > total_reach {
            let excess = dist - total_reach;
            if root_spec.enable_x_translation {
                let mut ox = (dx / dist) * excess * root_spec.speed.clamp(0.0, 1.0);
                if root_spec.constrain_x_translation {
                    ox = ox.clamp(root_spec.min_x_offset, root_spec.max_x_offset);
                }
                joint_pos[0][0] = root_origin[0] + ox;
            }
            if root_spec.enable_y_translation {
                let mut oy = (dy / dist) * excess * root_spec.speed.clamp(0.0, 1.0);
                if root_spec.constrain_y_translation {
                    oy = oy.clamp(root_spec.min_y_offset, root_spec.max_y_offset);
                }
                joint_pos[0][1] = root_origin[1] + oy;
            }
            for k in 0..n {
                let rad = world_angles[k].to_radians();
                joint_pos[k + 1] = [
                    joint_pos[k][0] + lengths[k] * rad.cos(),
                    joint_pos[k][1] + lengths[k] * rad.sin(),
                ];
            }
        }
    }

    // Constrained CCD + pin-locking passes (up to 28 iterations).
    for _iter in 0..28 {
        let eff = joint_pos[n];
        if (eff[0] - target[0]).hypot(eff[1] - target[1]) <= 1e-3 {
            break;
        }
        for k in (0..n).rev() {
            // Do not rotate a bone at or above a pinned joint if doing so would displace the pinned joint.
            if pinned[(k + 1)..=n].iter().any(|&p| p) {
                continue;
            }
            let bone = &armature.bones[chain_indices[k]];
            let spec = joint_spec(joint_specs, &bone.id);
            if spec.is_some_and(|s| !s.enable_rotation || s.speed <= 1e-6) {
                continue;
            }
            let pivot = joint_pos[k];
            let eff = joint_pos[n];
            let to_eff = [eff[0] - pivot[0], eff[1] - pivot[1]];
            let to_target = [target[0] - pivot[0], target[1] - pivot[1]];
            if to_eff[0].hypot(to_eff[1]) <= 1e-6 || to_target[0].hypot(to_target[1]) <= 1e-6 {
                continue;
            }
            let ang_eff = to_eff[1].atan2(to_eff[0]).to_degrees();
            let ang_target = to_target[1].atan2(to_target[0]).to_degrees();
            let mut delta = (ang_target - ang_eff + 540.0).rem_euclid(360.0) - 180.0;
            let speed = spec
                .map(|s| s.speed.clamp(0.0, 1.0))
                .unwrap_or_else(|| (1.0 - bone.stiffness.clamp(0.0, 0.95)).max(0.05));
            delta *= speed;

            let parent_angle = if k > 0 { world_angles[k - 1] } else { 0.0 };
            let mut rel_angle = world_angles[k] + delta - parent_angle;
            let min_a = spec
                .filter(|s| s.constrain_rotation)
                .map(|s| s.min_angle_deg)
                .or(bone.min_angle);
            let max_a = spec
                .filter(|s| s.constrain_rotation)
                .map(|s| s.max_angle_deg)
                .or(bone.max_angle);
            if let Some(lo) = min_a {
                rel_angle = rel_angle.max(lo);
            }
            if let Some(hi) = max_a {
                rel_angle = rel_angle.min(hi);
            }
            let applied = (parent_angle + rel_angle) - world_angles[k];
            for j in k..n {
                world_angles[j] += applied;
                let rad = world_angles[j].to_radians();
                joint_pos[j + 1] = [
                    joint_pos[j][0] + lengths[j] * rad.cos(),
                    joint_pos[j][1] + lengths[j] * rad.sin(),
                ];
            }
        }
        // Enforce any pinned joint positions.
        for k in 1..=n {
            if pinned[k] {
                joint_pos[k] = pinned_rest[k];
            }
        }
    }

    for (k, &b_idx) in chain_indices.iter().enumerate() {
        let bone = &armature.bones[b_idx];
        if let Some(el) = elements.iter_mut().find(|e| e.id == bone.element_id) {
            el.transform.x = joint_pos[k][0];
            el.transform.y = joint_pos[k][1];
            el.transform.rotation = world_angles[k];
        }
    }
}

/// Advance Flash CS5+ bone spring physics (`Strength` & `Damping`) deterministically by one frame step (`dt_sec`).
pub fn step_ik_spring_physics(
    elements: &mut [Element],
    armature: &IkArmature,
    joint_specs: &[IkJointSpec],
    angular_velocities: &mut [f64],
    rest_angles: &[f64],
    dt_sec: f64,
) {
    if !dt_sec.is_finite() {
        return;
    }
    let dt = dt_sec.clamp(1.0 / 240.0, 0.1);
    for (idx, bone) in armature.bones.iter().take(IkBone::MAX_BONES).enumerate() {
        let Some(el) = elements.iter_mut().find(|e| e.id == bone.element_id) else {
            continue;
        };
        let spec = joint_spec(joint_specs, &bone.id);
        let strength = spec
            .map(|s| s.spring_strength)
            .unwrap_or(0.0)
            .clamp(0.0, 100.0);
        let damping = spec
            .map(|s| s.spring_damping)
            .unwrap_or(0.0)
            .clamp(0.0, 100.0);
        if strength <= 1e-6 {
            continue;
        }
        let target_angle = rest_angles
            .get(idx)
            .copied()
            .filter(|angle| angle.is_finite())
            .unwrap_or(el.transform.rotation);
        let vel = angular_velocities.get_mut(idx);
        let disp = target_angle - el.transform.rotation;
        let stiffness_k = strength * 1.8;
        let damping_c = (damping / 100.0) * 12.0;
        if let Some(v) = vel {
            if !v.is_finite() {
                *v = 0.0;
            }
            *v += (stiffness_k * disp - damping_c * (*v)) * dt;
            el.transform.rotation += (*v) * dt;
        }
    }
}

// ============================================================================
// 2. Puppet / Asset Warp Tool: Triangulated Deformation Mesh & Pin Operations
// ============================================================================

/// Triangulated deformation mesh for Adobe Animate's Asset Warp / Puppet Tool (`T-164`).
#[derive(Clone, Debug, PartialEq)]
pub struct WarpMesh {
    pub rest_vertices: Vec<[f64; 2]>,
    pub deformed_vertices: Vec<[f64; 2]>,
    pub triangles: Vec<[usize; 3]>,
    pub boundary_edges: Vec<[usize; 2]>,
}

/// Build a structured triangulated [`WarpMesh`] over `element`'s local bounding box and `rig`.
pub fn build_asset_warp_mesh(element: &Element, rig: &AssetWarpRig) -> Option<WarpMesh> {
    if rig.element_id != element.id
        || rig.pins.len() > WarpPin::MAX_PINS
        || rig.pins.iter().any(|pin| {
            [
                pin.rest_pos[0],
                pin.rest_pos[1],
                pin.pos[0],
                pin.pos[1],
                pin.rotation,
                pin.stiffness,
            ]
            .iter()
            .any(|value| !value.is_finite())
        })
    {
        return None;
    }
    let bounds = match &element.kind {
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. }
        | ElementKind::Video { width, height } => [0.0, 0.0, *width, *height],
        ElementKind::Bitmap { width, height, .. } => {
            [0.0, 0.0, f64::from(*width), f64::from(*height)]
        }
        _ => {
            let (cmds, _) = element.to_path_commands()?;
            let b = path_bounds(&cmds).ok()??;
            [b.x_min, b.y_min, b.x_max, b.y_max]
        }
    };
    if bounds.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let w = (bounds[2] - bounds[0]).max(1.0);
    let h = (bounds[3] - bounds[1]).max(1.0);
    let cols = usize::from(rig.envelop_subdivisions.clamp(2, 16));
    let rows = usize::from(rig.envelop_subdivisions.clamp(2, 16));

    let mut rest_vertices = Vec::with_capacity((cols + 1) * (rows + 1));
    let mut deformed_vertices = Vec::with_capacity((cols + 1) * (rows + 1));
    for r in 0..=rows {
        let v = (r as f64) / (rows as f64);
        let y = bounds[1] + v * h;
        for c in 0..=cols {
            let u = (c as f64) / (cols as f64);
            let x = bounds[0] + u * w;
            let rest = [x, y];
            let def = deform_point_with_pins(rest, &rig.pins);
            rest_vertices.push(rest);
            deformed_vertices.push(def);
        }
    }

    let idx = |r: usize, c: usize| r * (cols + 1) + c;
    let mut triangles = Vec::with_capacity(cols * rows * 2);
    for r in 0..rows {
        for c in 0..cols {
            let i00 = idx(r, c);
            let i10 = idx(r, c + 1);
            let i01 = idx(r + 1, c);
            let i11 = idx(r + 1, c + 1);
            if (r + c) % 2 == 0 {
                triangles.push([i00, i10, i11]);
                triangles.push([i00, i11, i01]);
            } else {
                triangles.push([i00, i10, i01]);
                triangles.push([i10, i11, i01]);
            }
        }
    }

    let mut boundary_edges = Vec::new();
    for c in 0..cols {
        boundary_edges.push([idx(0, c), idx(0, c + 1)]);
        boundary_edges.push([idx(rows, c), idx(rows, c + 1)]);
    }
    for r in 0..rows {
        boundary_edges.push([idx(r, 0), idx(r + 1, 0)]);
        boundary_edges.push([idx(r, cols), idx(r + 1, cols)]);
    }

    Some(WarpMesh {
        rest_vertices,
        deformed_vertices,
        triangles,
        boundary_edges,
    })
}

/// Hit-test or add a [`WarpPin`] in element-local coordinates (`local_pt`) within `hit_radius`.
pub fn upsert_warp_pin(
    rig: &mut AssetWarpRig,
    local_pt: [f64; 2],
    hit_radius: f64,
    kind: WarpPinKind,
) -> Id {
    if local_pt.iter().any(|value| !value.is_finite()) || !hit_radius.is_finite() {
        return String::new();
    }
    let hit_radius = hit_radius.max(0.0);
    if let Some(pin) = rig
        .pins
        .iter_mut()
        .find(|p| (p.pos[0] - local_pt[0]).hypot(p.pos[1] - local_pt[1]) <= hit_radius)
    {
        if pin.kind == WarpPinKind::Open {
            pin.pos = local_pt;
        }
        return pin.id.clone();
    }
    if rig.pins.len() >= WarpPin::MAX_PINS {
        return rig.pins.last().map(|p| p.id.clone()).unwrap_or_default();
    }
    let id = new_id("pin");
    rig.pins.push(WarpPin {
        id: id.clone(),
        kind,
        rest_pos: local_pt,
        pos: local_pt,
        rotation: 0.0,
        stiffness: 1.0,
    });
    id
}

// ============================================================================
// 3. Deco Tool (8 Procedural Modes) & Spray Brush Stroke Generator
// ============================================================================

/// All 8 Flash CS4–CS6 `Deco Tool` procedural vector drawing modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecoEffectKind {
    #[default]
    VineFill,
    GridFill,
    SymmetryBrush,
    DecoratedBrush,
    FlameBrush,
    FlowerBrush,
    LightningBrush,
    TreeBrush,
}

impl DecoEffectKind {
    pub const ALL: [Self; 8] = [
        Self::VineFill,
        Self::GridFill,
        Self::SymmetryBrush,
        Self::DecoratedBrush,
        Self::FlameBrush,
        Self::FlowerBrush,
        Self::LightningBrush,
        Self::TreeBrush,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::VineFill => "Vine fill",
            Self::GridFill => "Grid fill",
            Self::SymmetryBrush => "Symmetry brush",
            Self::DecoratedBrush => "Decorated brush",
            Self::FlameBrush => "Flame brush",
            Self::FlowerBrush => "Flower brush",
            Self::LightningBrush => "Lightning brush",
            Self::TreeBrush => "Tree brush",
        }
    }
}

/// Generate procedural vector artwork on the Stage for any of the 8 Flash CS4–CS6 [`DecoEffectKind`] modes.
pub fn generate_deco_elements(
    effect: DecoEffectKind,
    origin: [f64; 2],
    scale_pct: f64,
    spacing_px: f64,
    primary_color: Color,
    secondary_color: Color,
) -> Vec<Element> {
    if !origin.iter().all(|value| value.is_finite())
        || !scale_pct.is_finite()
        || !spacing_px.is_finite()
    {
        return Vec::new();
    }
    let scale = (scale_pct / 100.0).clamp(0.1, 5.0);
    let step = spacing_px.clamp(6.0, 200.0) * scale;
    let [ox, oy] = origin;
    let mut elements = Vec::new();

    match effect {
        DecoEffectKind::GridFill => {
            for r in 0..4 {
                for c in 0..5 {
                    elements.push(Element {
                        id: new_id("deco"),
                        name: format!("deco_grid_{r}_{c}"),
                        transform: Transform {
                            x: ox + f64::from(c) * step,
                            y: oy + f64::from(r) * step,
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: (step * 0.65).max(4.0),
                            height: (step * 0.65).max(4.0),
                            fill: if (r + c) % 2 == 0 {
                                primary_color
                            } else {
                                secondary_color
                            },
                            stroke: secondary_color,
                            stroke_width: 1.0,
                        },
                        ..Element::default()
                    });
                }
            }
        }
        DecoEffectKind::SymmetryBrush => {
            let spokes = 8usize;
            let radius = step * 1.6;
            for i in 0..spokes {
                let ang = (i as f64) * std::f64::consts::TAU / (spokes as f64);
                elements.push(Element {
                    id: new_id("deco"),
                    name: format!("deco_sym_{}", i + 1),
                    transform: Transform {
                        x: ox + radius * ang.cos(),
                        y: oy + radius * ang.sin(),
                        rotation: ang.to_degrees(),
                        ..Transform::default()
                    },
                    kind: ElementKind::Ellipse {
                        width: (step * 0.85).max(6.0),
                        height: (step * 0.38).max(4.0),
                        fill: primary_color,
                        stroke: secondary_color,
                        stroke_width: 1.5,
                    },
                    ..Element::default()
                });
            }
        }
        DecoEffectKind::VineFill => {
            let mut cmds = vec![PathCommand::MoveTo { x: 0.0, y: 0.0 }];
            for i in 1..=6 {
                let x = f64::from(i) * step;
                let y = if i % 2 == 0 {
                    -step * 0.45
                } else {
                    step * 0.45
                };
                cmds.push(PathCommand::QuadraticTo {
                    cx: x - step * 0.5,
                    cy: y,
                    x,
                    y: 0.0,
                });
                // Add leaf node at each vine crest
                elements.push(Element {
                    id: new_id("deco"),
                    name: format!("deco_leaf_{i}"),
                    transform: Transform {
                        x: ox + x - step * 0.35,
                        y: oy + y * 0.5,
                        rotation: if i % 2 == 0 { -30.0 } else { 30.0 },
                        ..Transform::default()
                    },
                    kind: ElementKind::Ellipse {
                        width: (step * 0.45).max(5.0),
                        height: (step * 0.22).max(3.0),
                        fill: primary_color,
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                });
            }
            elements.insert(
                0,
                Element {
                    id: new_id("deco"),
                    name: "deco_vine_stem".to_owned(),
                    transform: Transform {
                        x: ox,
                        y: oy,
                        ..Transform::default()
                    },
                    kind: ElementKind::Path {
                        commands: cmds,
                        fill_rule: FillRule::NonZero,
                        fill: [0, 0, 0, 0],
                        stroke: secondary_color,
                        stroke_width: (2.0 * scale).max(1.0),
                    },
                    ..Element::default()
                },
            );
        }
        DecoEffectKind::DecoratedBrush => {
            for i in 0..8 {
                let x = ox + f64::from(i) * (step * 0.75);
                let cmds = vec![
                    PathCommand::MoveTo {
                        x: 0.0,
                        y: -step * 0.3,
                    },
                    PathCommand::LineTo {
                        x: step * 0.3,
                        y: 0.0,
                    },
                    PathCommand::LineTo {
                        x: 0.0,
                        y: step * 0.3,
                    },
                    PathCommand::LineTo {
                        x: -step * 0.3,
                        y: 0.0,
                    },
                    PathCommand::Close,
                ];
                elements.push(Element {
                    id: new_id("deco"),
                    name: format!("deco_motif_{}", i + 1),
                    transform: Transform {
                        x,
                        y: oy,
                        ..Transform::default()
                    },
                    kind: ElementKind::Path {
                        commands: cmds,
                        fill_rule: FillRule::NonZero,
                        fill: if i % 2 == 0 {
                            primary_color
                        } else {
                            secondary_color
                        },
                        stroke: secondary_color,
                        stroke_width: 1.0,
                    },
                    ..Element::default()
                });
            }
        }
        DecoEffectKind::FlameBrush => {
            for i in 0..5 {
                let offset_x = (f64::from(i) - 2.0) * (step * 0.45);
                let h = step * (1.8 - (f64::from(i) - 2.0).abs() * 0.3);
                let w = step * 0.45;
                let cmds = vec![
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::QuadraticTo {
                        cx: -w,
                        cy: -h * 0.45,
                        x: 0.0,
                        y: -h,
                    },
                    PathCommand::QuadraticTo {
                        cx: w,
                        cy: -h * 0.45,
                        x: 0.0,
                        y: 0.0,
                    },
                    PathCommand::Close,
                ];
                elements.push(Element {
                    id: new_id("deco"),
                    name: format!("deco_flame_{}", i + 1),
                    transform: Transform {
                        x: ox + offset_x,
                        y: oy,
                        ..Transform::default()
                    },
                    kind: ElementKind::Path {
                        commands: cmds,
                        fill_rule: FillRule::NonZero,
                        fill: if i % 2 == 0 {
                            primary_color
                        } else {
                            secondary_color
                        },
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                });
            }
        }
        DecoEffectKind::FlowerBrush => {
            let petals = 6usize;
            let r = step * 0.55;
            for i in 0..petals {
                let ang = (i as f64) * std::f64::consts::TAU / (petals as f64);
                elements.push(Element {
                    id: new_id("deco"),
                    name: format!("deco_petal_{}", i + 1),
                    transform: Transform {
                        x: ox + r * ang.cos() - r * 0.4,
                        y: oy + r * ang.sin() - r * 0.4,
                        ..Transform::default()
                    },
                    kind: ElementKind::Ellipse {
                        width: (r * 0.85).max(6.0),
                        height: (r * 0.85).max(6.0),
                        fill: primary_color,
                        stroke: secondary_color,
                        stroke_width: 1.0,
                    },
                    ..Element::default()
                });
            }
            elements.push(Element {
                id: new_id("deco"),
                name: "deco_flower_center".to_owned(),
                transform: Transform {
                    x: ox - r * 0.3,
                    y: oy - r * 0.3,
                    ..Transform::default()
                },
                kind: ElementKind::Ellipse {
                    width: (r * 0.6).max(4.0),
                    height: (r * 0.6).max(4.0),
                    fill: secondary_color,
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            });
        }
        DecoEffectKind::LightningBrush => {
            let mut cmds = vec![PathCommand::MoveTo { x: 0.0, y: 0.0 }];
            let zig = [
                (0.2, -0.35),
                (0.42, 0.25),
                (0.65, -0.45),
                (0.82, 0.18),
                (1.15, -0.22),
                (1.45, 0.0),
            ];
            for &(ux, uy) in &zig {
                cmds.push(PathCommand::LineTo {
                    x: ux * step * 3.0,
                    y: uy * step,
                });
            }
            elements.push(Element {
                id: new_id("deco"),
                name: "deco_lightning_main".to_owned(),
                transform: Transform {
                    x: ox,
                    y: oy,
                    ..Transform::default()
                },
                kind: ElementKind::Path {
                    commands: cmds,
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke: primary_color,
                    stroke_width: (2.5 * scale).max(1.0),
                },
                ..Element::default()
            });
        }
        DecoEffectKind::TreeBrush => {
            let trunk_h = step * 2.2;
            let mut cmds = vec![
                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                PathCommand::LineTo {
                    x: 0.0,
                    y: -trunk_h,
                },
                PathCommand::MoveTo {
                    x: 0.0,
                    y: -trunk_h * 0.6,
                },
                PathCommand::LineTo {
                    x: -step * 0.9,
                    y: -trunk_h * 1.15,
                },
                PathCommand::MoveTo {
                    x: 0.0,
                    y: -trunk_h * 0.75,
                },
                PathCommand::LineTo {
                    x: step * 0.9,
                    y: -trunk_h * 1.25,
                },
            ];
            let _ = &mut cmds;
            elements.push(Element {
                id: new_id("deco"),
                name: "deco_tree_branches".to_owned(),
                transform: Transform {
                    x: ox,
                    y: oy,
                    ..Transform::default()
                },
                kind: ElementKind::Path {
                    commands: cmds,
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke: secondary_color,
                    stroke_width: (3.0 * scale).max(1.5),
                },
                ..Element::default()
            });
            for (i, (dx, dy)) in [
                (0.0, -trunk_h * 1.15),
                (-step * 0.9, -trunk_h * 1.25),
                (step * 0.9, -trunk_h * 1.35),
            ]
            .into_iter()
            .enumerate()
            {
                elements.push(Element {
                    id: new_id("deco"),
                    name: format!("deco_tree_canopy_{}", i + 1),
                    transform: Transform {
                        x: ox + dx - step * 0.45,
                        y: oy + dy - step * 0.45,
                        ..Transform::default()
                    },
                    kind: ElementKind::Ellipse {
                        width: (step * 0.9).max(8.0),
                        height: (step * 0.9).max(8.0),
                        fill: primary_color,
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                });
            }
        }
    }

    elements
}

/// Configuration for Flash CS4–CS6 `Spray Brush` strokes.
#[derive(Clone, Debug, PartialEq)]
pub struct SprayBrushConfig {
    pub symbol_id: Option<Id>,
    pub fill: Color,
    pub scale_w_pct: f64,
    pub scale_h_pct: f64,
    pub random_scaling: bool,
    pub rotate_symbol: bool,
    pub random_rotation: bool,
    pub brush_width: f64,
    pub brush_height: f64,
    pub brush_angle_deg: f64,
}

/// Generate deterministic `Spray Brush` particles along `stroke_points`.
pub fn generate_spray_brush_along_points(
    stroke_points: &[[f64; 2]],
    config: &SprayBrushConfig,
) -> Vec<Element> {
    if stroke_points.is_empty()
        || ![
            config.scale_w_pct,
            config.scale_h_pct,
            config.brush_width,
            config.brush_height,
            config.brush_angle_deg,
        ]
        .iter()
        .all(|value| value.is_finite())
    {
        return Vec::new();
    }
    let base_sx = (config.scale_w_pct / 100.0).clamp(0.1, 4.0);
    let base_sy = (config.scale_h_pct / 100.0).clamp(0.1, 4.0);
    let rx = (config.brush_width * 0.5).clamp(1.0, 200.0);
    let ry = (config.brush_height * 0.5).clamp(1.0, 200.0);
    let (ang_s, ang_c) = config.brush_angle_deg.to_radians().sin_cos();

    let mut out = Vec::with_capacity(stroke_points.len().saturating_mul(3));
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next_unit = || -> f64 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        ((seed & 0xFFFF_FFFF) as f64) / (u32::MAX as f64)
    };

    for (pt_idx, &[px, py]) in stroke_points.iter().enumerate() {
        if !px.is_finite() || !py.is_finite() {
            continue;
        }
        for sub in 0..3 {
            let r = next_unit().sqrt();
            let theta = next_unit() * std::f64::consts::TAU;
            let lx = r * rx * theta.cos();
            let ly = r * ry * theta.sin();
            let dx = ang_c * lx - ang_s * ly;
            let dy = ang_s * lx + ang_c * ly;

            let scale_mult = if config.random_scaling {
                0.5 + next_unit() * 0.9
            } else {
                1.0
            };
            let rot = if config.random_rotation {
                next_unit() * 360.0 - 180.0
            } else if config.rotate_symbol {
                config.brush_angle_deg
            } else {
                0.0
            };

            let kind = if let Some(sid) = &config.symbol_id {
                ElementKind::Instance {
                    symbol_id: sid.clone(),
                    playback: InstancePlayback::Independent,
                }
            } else {
                ElementKind::Ellipse {
                    width: 10.0,
                    height: 10.0,
                    fill: config.fill,
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                }
            };

            out.push(Element {
                id: new_id("spray"),
                name: format!("spray_{}_{}", pt_idx + 1, sub + 1),
                transform: Transform {
                    x: px + dx,
                    y: py + dy,
                    scale_x: base_sx * scale_mult,
                    scale_y: base_sy * scale_mult,
                    rotation: rot,
                    ..Transform::default()
                },
                kind,
                ..Element::default()
            });
        }
    }
    out
}

// ============================================================================
// 4. Interactive 3D Rotation (W) & 3D Translation (G) Projection
// ============================================================================

/// Project an element's contour or bounding box through a 3D pose (`Display3dPose`) and perspective camera.
pub fn project_element_3d(
    element: &Element,
    pose: &Display3dPose,
    focal_length: f64,
    projection_center: [f64; 2],
) -> Element {
    if ![
        pose.x,
        pose.y,
        pose.z,
        pose.scale_x,
        pose.scale_y,
        pose.scale_z,
        pose.rotation_x,
        pose.rotation_y,
        pose.rotation_z,
        focal_length,
        projection_center[0],
        projection_center[1],
    ]
    .iter()
    .all(|value| value.is_finite())
        || focal_length <= 1e-6
    {
        return element.clone();
    }
    let is_identity = pose.z.abs() <= 1e-6
        && pose.rotation_x.abs() <= 1e-6
        && pose.rotation_y.abs() <= 1e-6
        && pose.rotation_z.abs() <= 1e-6;
    if is_identity {
        return element.clone();
    }
    let m = compose_display_matrix3d(pose);
    let chain = [m];
    let proj = |lx: f64, ly: f64| -> [f64; 2] {
        let gp = local3d_to_global(&chain, [lx, ly, 0.0], focal_length, projection_center);
        [gp[0] - element.transform.x, gp[1] - element.transform.y]
    };

    let Some((commands, fill_rule)) = element.to_path_commands() else {
        let mut out = element.clone();
        let gp = local3d_to_global(&chain, [0.0, 0.0, 0.0], focal_length, projection_center);
        out.transform.x = gp[0];
        out.transform.y = gp[1];
        out.transform.rotation += pose.rotation_z;
        if focal_length > 1e-6 {
            let s = (focal_length / (focal_length + pose.z).max(10.0)).clamp(0.05, 20.0);
            out.transform.scale_x *= s * pose.rotation_y.to_radians().cos();
            out.transform.scale_y *= s * pose.rotation_x.to_radians().cos();
        }
        return out;
    };

    let projected: Vec<PathCommand> = commands
        .into_iter()
        .map(|cmd| match cmd {
            PathCommand::MoveTo { x, y } => {
                let [nx, ny] = proj(x, y);
                PathCommand::MoveTo { x: nx, y: ny }
            }
            PathCommand::LineTo { x, y } => {
                let [nx, ny] = proj(x, y);
                PathCommand::LineTo { x: nx, y: ny }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let [ncx, ncy] = proj(cx, cy);
                let [nx, ny] = proj(x, y);
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
                let [ncx1, ncy1] = proj(cx1, cy1);
                let [ncx2, ncy2] = proj(cx2, cy2);
                let [nx, ny] = proj(x, y);
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
        .collect();

    let mut out = element.clone();
    out.transform.primitive = None;
    match &element.kind {
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
        } => {
            out.kind = ElementKind::Path {
                commands: projected,
                fill_rule,
                fill: *fill,
                stroke: *stroke,
                stroke_width: *stroke_width,
            };
        }
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            out.kind = ElementKind::PaintedPath {
                commands: projected,
                fill_rule,
                fill: *fill,
                stroke: *stroke,
                fill_gradient: fill_gradient.clone(),
                stroke_gradient: stroke_gradient.clone(),
                stroke_width: *stroke_width,
            };
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ik_bind_tool_skinning_and_fabrik_ccd_with_pins_and_spring() {
        let mut elements = vec![
            Element {
                id: "b1_el".into(),
                name: "Upper".into(),
                transform: Transform {
                    x: 100.0,
                    y: 100.0,
                    rotation: 0.0,
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 60.0,
                    height: 12.0,
                    fill: [200, 80, 60, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
            Element {
                id: "b2_el".into(),
                name: "Lower".into(),
                transform: Transform {
                    x: 160.0,
                    y: 100.0,
                    rotation: 0.0,
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 60.0,
                    height: 12.0,
                    fill: [60, 140, 220, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
        ];

        let armature = IkArmature {
            id: "arm_1".into(),
            name: "Arm".into(),
            bones: vec![
                IkBone {
                    id: "bone_1".into(),
                    element_id: "b1_el".into(),
                    parent_bone_id: None,
                    length: 60.0,
                    min_angle: None,
                    max_angle: None,
                    stiffness: 0.2,
                },
                IkBone {
                    id: "bone_2".into(),
                    element_id: "b2_el".into(),
                    parent_bone_id: Some("bone_1".into()),
                    length: 60.0,
                    min_angle: None,
                    max_angle: None,
                    stiffness: 0.1,
                },
            ],
            effector_bone_id: "bone_2".into(),
            target_pos: [120.0, 180.0],
            pole_pos: None,
        };

        let mut skin = auto_bind_shape_to_armature(&elements[0], &armature, &elements).unwrap();
        assert!(!skin.bindings.is_empty());
        bind_vertices_to_bone(&mut skin, "bone_2", &[0, 1], 0.8, false);
        assert!(
            skin.bindings[0]
                .weights
                .iter()
                .any(|w| w.bone_id == "bone_2" && w.weight > 0.4)
        );

        let specs = vec![
            IkJointSpec {
                bone_id: "bone_1".into(),
                constrain_rotation: true,
                min_angle_deg: -60.0,
                max_angle_deg: 60.0,
                spring_strength: 50.0,
                spring_damping: 30.0,
                ..IkJointSpec::default()
            },
            IkJointSpec {
                bone_id: "bone_2".into(),
                ..IkJointSpec::default()
            },
        ];
        solve_ik_armature_fabrik_ccd(&mut elements, &armature, &specs);
        assert!(elements[0].transform.rotation.abs() <= 60.0 + 1e-6);

        // Persisted IkBone constraints are already stored in degrees. They must not be
        // converted as radians when the richer transient joint spec is absent.
        let mut constrained_armature = armature.clone();
        constrained_armature.bones[0].min_angle = Some(-5.0);
        constrained_armature.bones[0].max_angle = Some(5.0);
        let mut constrained_elements = elements.clone();
        constrained_elements[0].transform.rotation = 0.0;
        constrained_elements[1].transform.rotation = 0.0;
        solve_ik_armature_fabrik_ccd(&mut constrained_elements, &constrained_armature, &[]);
        assert!(constrained_elements[0].transform.rotation.abs() <= 5.0 + 1e-6);

        // Malformed transient constraints fail closed instead of panicking in f64::clamp.
        let invalid_specs = [IkJointSpec {
            bone_id: "bone_1".into(),
            enable_x_translation: true,
            constrain_x_translation: true,
            min_x_offset: 10.0,
            max_x_offset: -10.0,
            ..IkJointSpec::default()
        }];
        solve_ik_armature_fabrik_ccd(&mut constrained_elements, &armature, &invalid_specs);
        assert!(constrained_elements.iter().all(|element| {
            [
                element.transform.x,
                element.transform.y,
                element.transform.rotation,
            ]
            .iter()
            .all(|value| value.is_finite())
        }));

        let deformed = deform_shape_with_ik_skin(&elements[0], &skin, &armature, &elements);
        assert!(matches!(deformed.kind, ElementKind::Path { .. }));

        // Binding lookup follows the persisted vertex index rather than assuming vector order,
        // and retained rest points prevent a second live-pose evaluation from double-warping.
        let mut reordered_skin = skin.clone();
        reordered_skin.bindings.reverse();
        let reordered =
            deform_shape_with_ik_skin(&elements[0], &reordered_skin, &armature, &elements);
        assert_eq!(reordered.kind, deformed.kind);
        let deformed_again = deform_shape_with_ik_skin(&deformed, &skin, &armature, &elements);
        assert_eq!(deformed_again.kind, deformed.kind);

        let mut wrong_skin = skin.clone();
        wrong_skin.shape_element_id = "another-shape".into();
        assert_eq!(
            deform_shape_with_ik_skin(&elements[0], &wrong_skin, &armature, &elements),
            elements[0]
        );

        let mut vels = vec![0.0, 0.0];
        step_ik_spring_physics(
            &mut elements,
            &armature,
            &specs,
            &mut vels,
            &[0.0, 0.0],
            1.0 / 24.0,
        );
        assert!(vels[0].abs() > 0.0);

        let mut project = crate::Project::blank();
        project.symbols[0].timeline.layers[0].keyframes[0].elements = elements.clone();
        project.symbols[0].timeline.layers[0].keyframes[0].ik_armatures = vec![armature.clone()];
        project.upgrade_format_for_features();
        project.validate().unwrap();

        let mut invalid_project = project.clone();
        invalid_project.symbols[0].timeline.layers[0].keyframes[0].ik_armatures[0].bones[1]
            .parent_bone_id = Some("missing".into());
        assert!(invalid_project.validate().is_err());

        let mut invalid_pole = project;
        invalid_pole.symbols[0].timeline.layers[0].keyframes[0].ik_armatures[0].pole_pos =
            Some([f64::NAN, 0.0]);
        assert!(invalid_pole.validate().is_err());
    }

    #[test]
    fn puppet_asset_warp_mesh_deco_spray_and_3d_projection() {
        let rect = Element {
            id: "puppet_el".into(),
            name: "Puppet".into(),
            transform: Transform {
                x: 50.0,
                y: 50.0,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 100.0,
                height: 80.0,
                fill: [120, 200, 140, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        let mut rig = AssetWarpRig {
            element_id: "puppet_el".into(),
            pins: Vec::new(),
            envelop_subdivisions: 4,
        };
        let p1 = upsert_warp_pin(&mut rig, [10.0, 10.0], 8.0, WarpPinKind::Fixed);
        let p2 = upsert_warp_pin(&mut rig, [90.0, 70.0], 8.0, WarpPinKind::Open);
        assert_ne!(p1, p2);
        let pins_before_invalid = rig.pins.clone();
        assert!(upsert_warp_pin(&mut rig, [f64::NAN, 0.0], 8.0, WarpPinKind::Open).is_empty());
        assert_eq!(rig.pins, pins_before_invalid);
        rig.pins[1].pos = [110.0, 95.0];

        let mesh = build_asset_warp_mesh(&rect, &rig).unwrap();
        assert_eq!(mesh.rest_vertices.len(), 25);
        assert_eq!(mesh.triangles.len(), 32);
        assert_ne!(mesh.rest_vertices.last(), mesh.deformed_vertices.last());

        for mode in DecoEffectKind::ALL {
            let deco = generate_deco_elements(
                mode,
                [200.0, 150.0],
                100.0,
                28.0,
                [240, 100, 60, 255],
                [60, 180, 240, 255],
            );
            assert!(!deco.is_empty(), "Deco mode {mode:?} produced no elements");
        }

        let spray = generate_spray_brush_along_points(
            &[[100.0, 100.0], [140.0, 120.0]],
            &SprayBrushConfig {
                symbol_id: None,
                fill: [255, 200, 50, 255],
                scale_w_pct: 100.0,
                scale_h_pct: 100.0,
                random_scaling: true,
                rotate_symbol: true,
                random_rotation: true,
                brush_width: 40.0,
                brush_height: 40.0,
                brush_angle_deg: 15.0,
            },
        );
        assert_eq!(spray.len(), 6);

        let proj = project_element_3d(
            &rect,
            &Display3dPose {
                x: 50.0,
                y: 50.0,
                z: 80.0,
                rotation_x: 25.0,
                rotation_y: -20.0,
                rotation_z: 10.0,
                ..Display3dPose::default()
            },
            500.0,
            [200.0, 150.0],
        );
        assert!(matches!(proj.kind, ElementKind::Path { .. }));
    }
}
