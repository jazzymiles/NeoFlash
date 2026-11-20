use super::PointRole;
use super::deform_envelope::{element_stage_bounds, map_element_stage_points};
use super::handles_recognition::{inverse_f64, move_selected_anchors};
use super::smooth_reshape::{editable_element_commands, write_editable_element_commands};
use crate::app::Editor;
use eframe::egui::Pos2;
use neoflash_core::*;
use std::collections::HashMap;

fn deform_stage_point_with_asset_warp_pins(
    point: [f64; 2],
    pins: &[WarpPin],
    falloff_radius: f64,
) -> [f64; 2] {
    if pins.is_empty() {
        return point;
    }
    if pins.len() == 1 {
        let pin = &pins[0];
        let dx = point[0] - pin.rest_pos[0];
        let dy = point[1] - pin.rest_pos[1];
        let d2 = dx * dx + dy * dy;
        let r2 = falloff_radius.max(20.0).powi(2);
        let w = 1.0 / (1.0 + d2 / r2);
        let (rx, ry) = if pin.rotation.abs() > 1e-6 && pin.kind != WarpPinKind::Fixed {
            let rad = pin.rotation.to_radians();
            let c = rad.cos();
            let s = rad.sin();
            (dx * c - dy * s, dx * s + dy * c)
        } else {
            (dx, dy)
        };
        let target_x = pin.pos[0] + rx;
        let target_y = pin.pos[1] + ry;
        return [
            point[0] + (target_x - point[0]) * w,
            point[1] + (target_y - point[1]) * w,
        ];
    }
    let mut sum_w = 0.0;
    let mut acc_x = 0.0;
    let mut acc_y = 0.0;
    for pin in pins {
        let dx = point[0] - pin.rest_pos[0];
        let dy = point[1] - pin.rest_pos[1];
        let d2 = dx * dx + dy * dy;
        let w = pin.stiffness.clamp(0.05, 1.0) / (d2 + 16.0);
        let (rx, ry) = if pin.rotation.abs() > 1e-6 && pin.kind != WarpPinKind::Fixed {
            let rad = pin.rotation.to_radians();
            let c = rad.cos();
            let s = rad.sin();
            (dx * c - dy * s, dx * s + dy * c)
        } else {
            (dx, dy)
        };
        let target_pos = if pin.kind == WarpPinKind::Fixed {
            pin.rest_pos
        } else {
            pin.pos
        };
        acc_x += (target_pos[0] + rx) * w;
        acc_y += (target_pos[1] + ry) * w;
        sum_w += w;
    }
    if sum_w > 1e-9 {
        [acc_x / sum_w, acc_y / sum_w]
    } else {
        point
    }
}

impl Editor {
    fn active_asset_warp_element_id(&self) -> Option<Id> {
        if let Some(el) = self.selected_element() {
            return Some(el.id.clone());
        }
        if let Some(target) = self.selection.targets.first() {
            return Some(target.element.clone());
        }
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let key = layer.key_at(self.frame)?;
        key.elements
            .iter()
            .find(|e| !key.controller_elements.contains(&e.id))
            .map(|e| e.id.clone())
    }

    /// Return the Stage-space `WarpPin`s `(pin_id, [stage_x, stage_y], kind)` for the selected
    /// element (`T-271`).
    pub fn asset_warp_stage_pins(&self) -> Vec<(Id, [f64; 2], WarpPinKind)> {
        let Some(target_id) = self.active_asset_warp_element_id() else {
            return Vec::new();
        };
        if let Some(layer) = self.symbol().timeline.layers.get(self.layer)
            && let Some(key) = layer.key_at(self.frame)
            && let Some(rig) = key.asset_warp(&target_id)
        {
            return rig
                .pins
                .iter()
                .map(|p| (p.id.clone(), p.pos, p.kind))
                .collect();
        }
        Vec::new()
    }

    /// Hit-test a Stage-space point against the active `WarpPin` handles (`T-271`).
    pub fn hit_test_asset_warp_pin(&self, stage_pt: [f64; 2], tolerance: f64) -> Option<Id> {
        let tol_sq = tolerance.max(1.5).powi(2);
        self.asset_warp_stage_pins()
            .into_iter()
            .filter_map(|(id, pos, _)| {
                let d2 = (pos[0] - stage_pt[0]).powi(2) + (pos[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((id, d2))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, _)| id)
    }

    /// Re-evaluate the mesh deformation of `element_id` from its undeformed base shape and
    /// active `WarpPin`s (`T-271`).
    pub fn deform_shape_with_warp_pins(&mut self, element_id: &str) -> bool {
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let pins: Vec<WarpPin> = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .and_then(|k| k.asset_warp(element_id))
            .map(|r| r.pins.clone())
            .unwrap_or_default();

        let current_el = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .and_then(|k| k.elements.iter().find(|e| e.id == element_id))
            .cloned();
        let Some(current_el) = current_el else {
            return false;
        };
        let base_key = (symbol_id, frame, element_id.to_owned());
        let base_el = self
            .vector
            .asset_warp_base_elements
            .entry(base_key)
            .or_insert_with(|| current_el.clone())
            .clone();

        let warped_el = if pins.is_empty() {
            base_el
        } else {
            let radius = element_stage_bounds(&base_el)
                .map(|[x0, y0, x1, y1]| (x1 - x0).max(y1 - y0).max(40.0) * 0.75)
                .unwrap_or(100.0);
            let Some(el) = map_element_stage_points(&base_el, 4, |pt| {
                deform_stage_point_with_asset_warp_pins(pt, &pins, radius)
            }) else {
                return false;
            };
            el
        };

        let symbol = self.symbol_mut();
        let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
            return false;
        };
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
            return false;
        };
        if let Some(slot) = key.elements.iter_mut().find(|e| e.id == element_id) {
            *slot = warped_el;
            return true;
        }
        false
    }

    pub(crate) fn add_asset_warp_pin_in_place(&mut self, stage_pt: [f64; 2]) -> Option<Id> {
        if !stage_pt[0].is_finite() || !stage_pt[1].is_finite() {
            return None;
        }
        let target_id = self.active_asset_warp_element_id()?;
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        if let Some(el) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .and_then(|k| k.elements.iter().find(|e| e.id == target_id))
            .cloned()
        {
            let has_existing_pins = self
                .symbol()
                .timeline
                .layers
                .get(layer_idx)
                .and_then(|l| l.key_at(frame))
                .and_then(|k| k.asset_warp(&target_id))
                .is_some_and(|r| !r.pins.is_empty());
            if !has_existing_pins {
                self.vector
                    .asset_warp_base_elements
                    .insert((symbol_id, frame, target_id.clone()), el);
            } else {
                self.vector
                    .asset_warp_base_elements
                    .entry((symbol_id, frame, target_id.clone()))
                    .or_insert(el);
            }
        }

        let pin_id = new_id("wpin");
        let new_pin = WarpPin {
            id: pin_id.clone(),
            kind: WarpPinKind::Open,
            rest_pos: stage_pt,
            pos: stage_pt,
            rotation: 0.0,
            stiffness: 1.0,
        };
        let symbol = self.symbol_mut();
        let layer = symbol.timeline.layers.get_mut(layer_idx)?;
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let key = layer.keyframes.iter_mut().find(|k| k.frame == key_frame)?;
        if let Some(rig) = key.asset_warp_mut(&target_id) {
            if rig.pins.len() >= WarpPin::MAX_PINS {
                return None;
            }
            rig.pins.push(new_pin);
        } else {
            key.set_asset_warp(AssetWarpRig {
                element_id: target_id,
                pins: vec![new_pin],
                envelop_subdivisions: 4,
            });
        }
        self.vector.selected_warp_pin = Some(pin_id.clone());
        Some(pin_id)
    }

    /// Add a new `WarpPin` at `stage_pt` on the selected shape/element in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn add_asset_warp_pin(&mut self, stage_pt: [f64; 2]) -> Option<Id> {
        self.history.begin(&self.project);
        let pin_id = self.add_asset_warp_pin_in_place(stage_pt)?;
        self.commit_history();
        Some(pin_id)
    }

    pub(crate) fn apply_asset_warp_pin_in_place(
        &mut self,
        pin_id: &str,
        stage_pt: [f64; 2],
    ) -> bool {
        if !stage_pt[0].is_finite() || !stage_pt[1].is_finite() {
            return false;
        }
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let mut target_element_id: Option<Id> = None;

        if let Some(el_id) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .and_then(|k| {
                k.asset_warps
                    .iter()
                    .find(|r| r.pins.iter().any(|p| p.id == pin_id))
                    .map(|r| r.element_id.clone())
            })
            && let Some(el) = self
                .symbol()
                .timeline
                .layers
                .get(layer_idx)
                .and_then(|l| l.key_at(frame))
                .and_then(|k| k.elements.iter().find(|e| e.id == el_id))
                .cloned()
        {
            self.vector
                .asset_warp_base_elements
                .entry((symbol_id, frame, el_id))
                .or_insert(el);
        }

        let mut moved = false;
        {
            let symbol = self.symbol_mut();
            if let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) {
                let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
                if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                    for rig in &mut key.asset_warps {
                        if let Some(pin) = rig.pins.iter_mut().find(|p| p.id == pin_id) {
                            if (pin.pos[0] - stage_pt[0]).hypot(pin.pos[1] - stage_pt[1]) > 1e-6 {
                                pin.pos = stage_pt;
                                moved = true;
                            }
                            target_element_id = Some(rig.element_id.clone());
                        }
                    }
                }
            }
        }
        if !moved {
            return false;
        }
        self.vector.selected_warp_pin = Some(pin_id.to_owned());
        if let Some(el_id) = target_element_id {
            let _ = self.deform_shape_with_warp_pins(&el_id);
        }
        true
    }

    /// Move an existing `WarpPin` to `stage_pt` and deform the bound shape in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn move_asset_warp_pin(&mut self, pin_id: &str, stage_pt: [f64; 2]) -> bool {
        self.history.begin(&self.project);
        if !self.apply_asset_warp_pin_in_place(pin_id, stage_pt) {
            return false;
        }
        self.commit_history();
        true
    }

    /// Remove `pin_id` from the active `AssetWarpRig` and update the shape mesh in 1 Undo step (`T-271`).
    pub fn remove_asset_warp_pin(&mut self, pin_id: &str) -> bool {
        self.history.begin(&self.project);
        let layer_idx = self.layer;
        let frame = self.frame;
        let mut target_element_id: Option<Id> = None;
        let mut removed = false;
        {
            let symbol = self.symbol_mut();
            if let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) {
                let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
                if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                    for rig in &mut key.asset_warps {
                        let before_len = rig.pins.len();
                        rig.pins.retain(|p| p.id != pin_id);
                        if rig.pins.len() != before_len {
                            removed = true;
                            target_element_id = Some(rig.element_id.clone());
                        }
                    }
                    key.asset_warps.retain(|r| !r.pins.is_empty());
                }
            }
        }
        if !removed {
            return false;
        }
        if self.vector.selected_warp_pin.as_deref() == Some(pin_id) {
            self.vector.selected_warp_pin = None;
        }
        if let Some(el_id) = target_element_id {
            let _ = self.deform_shape_with_warp_pins(&el_id);
        }
        self.commit_history();
        true
    }

    /// Return `(armature_id, bone_id, head_stage, tail_stage, world_angle_deg)` for all `IkBone`s
    /// on the active keyframe (`T-271`).
    #[allow(clippy::type_complexity)]
    pub fn ik_bone_stage_poses(&self) -> Vec<(Id, Id, [f64; 2], [f64; 2], f64)> {
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return Vec::new();
        };
        let Some(key) = layer.key_at(self.frame) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for arm in &key.ik_armatures {
            for (bone_id, head, tail, world_angle) in compute_bone_world_poses(arm, &key.elements) {
                out.push((arm.id.clone(), bone_id, head, tail, world_angle));
            }
        }
        out
    }

    /// Hit-test a Stage-space point against `IkBone` gizmos, returning
    /// `Some((armature_id, bone_id, is_tail_effector))` (`T-271`).
    pub fn hit_test_ik_bone_gizmo(
        &self,
        stage_pt: [f64; 2],
        tolerance: f64,
    ) -> Option<(Id, Id, bool)> {
        let tol = tolerance.max(2.0);
        let tol_sq = tol * tol;
        let poses = self.ik_bone_stage_poses();
        // 1. Prioritize bone tails / effectors
        if let Some((arm_id, bone_id, _)) = poses
            .iter()
            .filter_map(|(arm_id, bone_id, _, tail, _)| {
                let d2 = (tail[0] - stage_pt[0]).powi(2) + (tail[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((arm_id.clone(), bone_id.clone(), d2))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
        {
            return Some((arm_id, bone_id, true));
        }
        // 2. Next check bone heads / joints
        if let Some((arm_id, bone_id, _)) = poses
            .iter()
            .filter_map(|(arm_id, bone_id, head, _, _)| {
                let d2 = (head[0] - stage_pt[0]).powi(2) + (head[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((arm_id.clone(), bone_id.clone(), d2))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
        {
            return Some((arm_id, bone_id, false));
        }
        // 3. Check bone body segment distance
        poses
            .into_iter()
            .filter_map(|(arm_id, bone_id, head, tail, _)| {
                let vx = tail[0] - head[0];
                let vy = tail[1] - head[1];
                let len_sq = vx * vx + vy * vy;
                let t = if len_sq > 1e-9 {
                    (((stage_pt[0] - head[0]) * vx + (stage_pt[1] - head[1]) * vy) / len_sq)
                        .clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let proj = [head[0] + vx * t, head[1] + vy * t];
                let d2 = (proj[0] - stage_pt[0]).powi(2) + (proj[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((arm_id, bone_id, t >= 0.5, d2))
            })
            .min_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(arm_id, bone_id, is_tail, _)| (arm_id, bone_id, is_tail))
    }

    /// Create a new `IkBone` between `start_stage` and `end_stage` in 1 Undo step (`T-271`).
    pub fn create_ik_bone_on_stage(
        &mut self,
        start_stage: [f64; 2],
        end_stage: [f64; 2],
    ) -> Option<(Id, Id)> {
        if !start_stage[0].is_finite()
            || !start_stage[1].is_finite()
            || !end_stage[0].is_finite()
            || !end_stage[1].is_finite()
        {
            return None;
        }
        let dx = end_stage[0] - start_stage[0];
        let dy = end_stage[1] - start_stage[1];
        let length = dx.hypot(dy).max(16.0);
        let world_angle = dy.atan2(dx).to_degrees();

        self.history.begin(&self.project);
        let parent_hit = self
            .ik_bone_stage_poses()
            .into_iter()
            .filter_map(|(arm_id, bone_id, _head, tail, _angle)| {
                let d = (tail[0] - start_stage[0]).hypot(tail[1] - start_stage[1]);
                (d <= 28.0).then_some((arm_id, bone_id, d))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

        let layer_idx = self.layer;
        let frame = self.frame;
        let bone_id = new_id("ikbone");
        let bone_el_id = new_id("ikjoint");

        let arm_id = {
            let symbol = self.symbol_mut();
            let layer = symbol.timeline.layers.get_mut(layer_idx)?;
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let key = layer.keyframes.iter_mut().find(|k| k.frame == key_frame)?;

            key.elements.push(Element {
                id: bone_el_id.clone(),
                name: format!("Bone {}", key.controller_elements.len() + 1),
                transform: Transform {
                    x: start_stage[0],
                    y: start_stage[1],
                    rotation: world_angle,
                    alpha: 0.0,
                    ..Transform::default()
                },
                kind: ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                        PathCommand::LineTo { x: 1.0, y: 0.0 },
                    ],
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            });
            key.controller_elements.push(bone_el_id.clone());

            if let Some((arm_id, parent_id, _)) = parent_hit
                && let Some(arm) = key.ik_armatures.iter_mut().find(|a| a.id == arm_id)
            {
                arm.bones.push(IkBone {
                    id: bone_id.clone(),
                    element_id: bone_el_id,
                    parent_bone_id: Some(parent_id),
                    length,
                    min_angle: None,
                    max_angle: None,
                    stiffness: 1.0,
                });
                arm.effector_bone_id = bone_id.clone();
                arm.target_pos = end_stage;
                arm_id
            } else if let Some(arm) = key.ik_armatures.first_mut() {
                let last_bone_id = arm.bones.last().map(|b| b.id.clone());
                arm.bones.push(IkBone {
                    id: bone_id.clone(),
                    element_id: bone_el_id,
                    parent_bone_id: last_bone_id,
                    length,
                    min_angle: None,
                    max_angle: None,
                    stiffness: 1.0,
                });
                arm.effector_bone_id = bone_id.clone();
                arm.target_pos = end_stage;
                arm.id.clone()
            } else {
                let new_arm_id = new_id("arm");
                key.ik_armatures.push(IkArmature {
                    id: new_arm_id.clone(),
                    name: "Armature 1".into(),
                    bones: vec![IkBone {
                        id: bone_id.clone(),
                        element_id: bone_el_id,
                        parent_bone_id: None,
                        length,
                        min_angle: None,
                        max_angle: None,
                        stiffness: 1.0,
                    }],
                    effector_bone_id: bone_id.clone(),
                    target_pos: end_stage,
                    pole_pos: None,
                });
                new_arm_id
            }
        };

        let poses: Vec<(Id, [f64; 2], [f64; 2], f64)> = self
            .ik_bone_stage_poses()
            .into_iter()
            .filter(|(a_id, _, _, _, _)| a_id == &arm_id)
            .map(|(_, b_id, h, t, ang)| (b_id, h, t, ang))
            .collect();
        let symbol_id = self.symbol_id().to_owned();
        for el in self.selected_vector_elements_on_active_key() {
            self.vector.ik_base_elements.insert(
                (symbol_id.clone(), frame, el.id.clone()),
                (el, poses.clone()),
            );
        }

        self.vector.selected_ik_bone = Some(bone_id.clone());
        self.commit_history();
        Some((arm_id, bone_id))
    }

    fn parse_armature_bound_vertices(arm_name: &str) -> HashMap<Id, Vec<(usize, Id, f64, f64)>> {
        let mut out: HashMap<Id, Vec<(usize, Id, f64, f64)>> = HashMap::new();
        let Some((_, meta)) = arm_name.split_once("|bind:") else {
            return out;
        };
        for entry in meta.split(';') {
            let parts: Vec<&str> = entry.split(',').collect();
            if parts.len() == 5
                && let Ok(v_idx) = parts[1].parse::<usize>()
                && let Ok(ox) = parts[3].parse::<f64>()
                && let Ok(oy) = parts[4].parse::<f64>()
            {
                out.entry(parts[0].to_owned()).or_default().push((
                    v_idx,
                    parts[2].to_owned(),
                    ox,
                    oy,
                ));
            }
        }
        out
    }

    fn encode_armature_bound_vertices(
        base_name: &str,
        bindings: &HashMap<Id, Vec<(usize, Id, f64, f64)>>,
    ) -> String {
        let clean_base = base_name
            .split_once("|bind:")
            .map(|(b, _)| b)
            .unwrap_or(base_name);
        let mut entries = Vec::new();
        let mut shape_ids: Vec<&Id> = bindings.keys().collect();
        shape_ids.sort();
        for shape_id in shape_ids {
            if let Some(list) = bindings.get(shape_id) {
                for (v_idx, bone_id, ox, oy) in list {
                    entries.push(format!("{shape_id},{v_idx},{bone_id},{ox:.4},{oy:.4}"));
                }
            }
        }
        if entries.is_empty() {
            clean_base.to_owned()
        } else {
            format!("{clean_base}|bind:{}", entries.join(";"))
        }
    }

    fn apply_ik_pose_to_scene_in_place(&mut self, armature_id: &str) {
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let Some(key_snapshot) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .cloned()
        else {
            return;
        };
        let Some(arm) = key_snapshot
            .ik_armatures
            .iter()
            .find(|a| a.id == armature_id)
            .cloned()
        else {
            return;
        };
        let current_poses = compute_bone_world_poses(&arm, &key_snapshot.elements);
        if current_poses.is_empty() {
            return;
        }
        let explicit_vertex_bindings = Self::parse_armature_bound_vertices(&arm.name);

        let mut deformed_shapes: Vec<(Id, Element)> = Vec::new();
        for el in &key_snapshot.elements {
            if key_snapshot.controller_elements.contains(&el.id) || el.to_path_commands().is_none()
            {
                continue;
            }
            let base_key = (symbol_id.clone(), frame, el.id.clone());
            let (base_el, rest_poses) = self
                .vector
                .ik_base_elements
                .entry(base_key)
                .or_insert_with(|| (el.clone(), current_poses.clone()))
                .clone();

            if let Some(v_bindings) = explicit_vertex_bindings.get(&el.id)
                && !v_bindings.is_empty()
            {
                let Some(mut cmds) = editable_element_commands(&base_el)
                    .or_else(|| base_el.to_path_commands().map(|(c, _)| c))
                else {
                    continue;
                };
                let mat = transform_matrix(base_el.transform);
                let anchor_cmds: Vec<usize> = cmds
                    .iter()
                    .enumerate()
                    .filter_map(|(i, cmd)| (!matches!(cmd, PathCommand::Close)).then_some(i))
                    .collect();
                for &(v_idx, ref bone_id, rest_ox, rest_oy) in v_bindings {
                    let Some(&cmd_idx) = anchor_cmds.get(v_idx).or(Some(&v_idx)) else {
                        continue;
                    };
                    let Some((_, head, _, world_angle)) =
                        current_poses.iter().find(|(id, _, _, _)| id == bone_id)
                    else {
                        continue;
                    };
                    let rad = world_angle.to_radians();
                    let c = rad.cos();
                    let s = rad.sin();
                    let stage_x = head[0] + rest_ox * c - rest_oy * s;
                    let stage_y = head[1] + rest_ox * s + rest_oy * c;
                    let [lx, ly] = if let Some((lx, ly)) = inverse_f64(mat, stage_x, stage_y) {
                        [lx, ly]
                    } else {
                        [stage_x - base_el.transform.x, stage_y - base_el.transform.y]
                    };
                    if let Some(cur_anchor) = crate::vector::handles(&cmds)
                        .into_iter()
                        .find(|h| h.command == cmd_idx && h.role == PointRole::Anchor)
                    {
                        let delta = Pos2::new(lx as f32, ly as f32) - cur_anchor.point;
                        move_selected_anchors(&mut cmds, &[cmd_idx], delta);
                    }
                }
                let mut updated = base_el.clone();
                write_editable_element_commands(&mut updated, cmds);
                deformed_shapes.push((el.id.clone(), updated));
                continue;
            }

            if !rest_poses.is_empty() {
                let Some(warped) = map_element_stage_points(&base_el, 4, |pt| {
                    let mut w_sum = 0.0;
                    let mut out_x = 0.0;
                    let mut out_y = 0.0;
                    for (bone_id, cur_head, _, cur_angle) in &current_poses {
                        let Some((_, rest_head, rest_tail, rest_angle)) =
                            rest_poses.iter().find(|(id, _, _, _)| id == bone_id)
                        else {
                            continue;
                        };
                        let vx = rest_tail[0] - rest_head[0];
                        let vy = rest_tail[1] - rest_head[1];
                        let len_sq = vx * vx + vy * vy;
                        let t = if len_sq > 1e-9 {
                            (((pt[0] - rest_head[0]) * vx + (pt[1] - rest_head[1]) * vy) / len_sq)
                                .clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let proj = [rest_head[0] + vx * t, rest_head[1] + vy * t];
                        let d2 = (pt[0] - proj[0]).powi(2) + (pt[1] - proj[1]).powi(2);
                        let w = 1.0 / (d2 + 16.0);
                        let r_rad = rest_angle.to_radians();
                        let rc = r_rad.cos();
                        let rs = r_rad.sin();
                        let rel_x = pt[0] - rest_head[0];
                        let rel_y = pt[1] - rest_head[1];
                        let local_x = rel_x * rc + rel_y * rs;
                        let local_y = -rel_x * rs + rel_y * rc;
                        let c_rad = cur_angle.to_radians();
                        let cc = c_rad.cos();
                        let cs = c_rad.sin();
                        let mapped_x = cur_head[0] + local_x * cc - local_y * cs;
                        let mapped_y = cur_head[1] + local_x * cs + local_y * cc;
                        out_x += mapped_x * w;
                        out_y += mapped_y * w;
                        w_sum += w;
                    }
                    if w_sum > 1e-9 {
                        [out_x / w_sum, out_y / w_sum]
                    } else {
                        pt
                    }
                }) else {
                    continue;
                };
                deformed_shapes.push((el.id.clone(), warped));
            }
        }

        let symbol = self.symbol_mut();
        let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
            return;
        };
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
            return;
        };
        for (id, warped) in deformed_shapes {
            if let Some(slot) = key.elements.iter_mut().find(|e| e.id == id) {
                *slot = warped;
            }
        }
    }

    pub(crate) fn apply_ik_bone_effector_in_place(
        &mut self,
        armature_id: &str,
        bone_id: &str,
        target_stage: [f64; 2],
    ) -> bool {
        if !target_stage[0].is_finite() || !target_stage[1].is_finite() {
            return false;
        }
        let initial_poses: Vec<(Id, [f64; 2], [f64; 2], f64)> = self
            .ik_bone_stage_poses()
            .into_iter()
            .filter(|(a_id, _, _, _, _)| a_id == armature_id)
            .map(|(_, b_id, h, t, ang)| (b_id, h, t, ang))
            .collect();
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        for el in self.selected_vector_elements_on_active_key() {
            self.vector
                .ik_base_elements
                .entry((symbol_id.clone(), frame, el.id.clone()))
                .or_insert_with(|| (el, initial_poses.clone()));
        }

        let solved = {
            let symbol = self.symbol_mut();
            let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
                return false;
            };
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
                return false;
            };
            let Some(arm) = key.ik_armatures.iter_mut().find(|a| a.id == armature_id) else {
                return false;
            };
            if !arm.bones.iter().any(|b| b.id == bone_id) {
                return false;
            }
            arm.effector_bone_id = bone_id.to_owned();
            arm.target_pos = target_stage;
            let arm_clone = arm.clone();
            solve_ik_armature_fabrik_ccd(&mut key.elements, &arm_clone, &[]);
            true
        };
        if !solved {
            return false;
        }
        self.vector.selected_ik_bone = Some(bone_id.to_owned());
        self.apply_ik_pose_to_scene_in_place(armature_id);
        true
    }

    /// Drag an `IkBone` joint/effector to `target_stage`, solve inverse kinematics, and update
    /// bound elements/shapes in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn move_ik_bone_effector(
        &mut self,
        armature_id: &str,
        bone_id: &str,
        target_stage: [f64; 2],
    ) -> bool {
        self.history.begin(&self.project);
        if !self.apply_ik_bone_effector_in_place(armature_id, bone_id, target_stage) {
            return false;
        }
        self.commit_history();
        true
    }

    /// Solve IK for `armature_id`'s active or leaf effector bone toward `target_stage` in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn pose_ik_armature(&mut self, armature_id: &str, target_stage: [f64; 2]) -> bool {
        let poses = self.ik_bone_stage_poses();
        let bone_id = self
            .vector
            .selected_ik_bone
            .clone()
            .filter(|sel| {
                poses
                    .iter()
                    .any(|(a_id, b_id, _, _, _)| a_id == armature_id && b_id == sel)
            })
            .or_else(|| {
                poses
                    .iter()
                    .rev()
                    .find(|(a_id, _, _, _, _)| a_id == armature_id)
                    .map(|(_, b_id, _, _, _)| b_id.clone())
            });
        let Some(bone_id) = bone_id else {
            return false;
        };
        self.move_ik_bone_effector(armature_id, &bone_id, target_stage)
    }

    /// Solve the active IK chain toward `target_stage` in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn solve_ik_chain(&mut self, target_stage: [f64; 2]) -> bool {
        let Some((arm_id, _, _, _, _)) = self.ik_bone_stage_poses().into_iter().next() else {
            return false;
        };
        self.pose_ik_armature(&arm_id, target_stage)
    }

    pub(crate) fn apply_bind_stage_point_in_place(
        &mut self,
        stage_pt: [f64; 2],
        unbind: bool,
    ) -> bool {
        if !stage_pt[0].is_finite() || !stage_pt[1].is_finite() {
            return false;
        }
        let poses = self.ik_bone_stage_poses();
        if poses.is_empty() {
            return false;
        }

        let chosen_bone = self
            .vector
            .selected_ik_bone
            .as_ref()
            .and_then(|sel| poses.iter().find(|(_, b_id, _, _, _)| b_id == sel).cloned())
            .or_else(|| {
                poses
                    .iter()
                    .map(|pose| {
                        let (_, _, head, tail, _) = pose;
                        let vx = tail[0] - head[0];
                        let vy = tail[1] - head[1];
                        let len_sq = vx * vx + vy * vy;
                        let t = if len_sq > 1e-9 {
                            (((stage_pt[0] - head[0]) * vx + (stage_pt[1] - head[1]) * vy) / len_sq)
                                .clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let proj = [head[0] + vx * t, head[1] + vy * t];
                        let d2 = (proj[0] - stage_pt[0]).powi(2) + (proj[1] - stage_pt[1]).powi(2);
                        (pose.clone(), d2)
                    })
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(p, _)| p)
            });
        let Some((arm_id, bone_id, bone_head, _bone_tail, bone_angle)) = chosen_bone else {
            return false;
        };
        self.vector.selected_ik_bone = Some(bone_id.clone());

        let els = self.selected_vector_elements_on_active_key();
        let Some(el) = els.first().cloned() else {
            return false;
        };
        let Some(cmds) =
            editable_element_commands(&el).or_else(|| el.to_path_commands().map(|(c, _)| c))
        else {
            return false;
        };
        let mat = transform_matrix(el.transform);
        let mut anchor_pts: Vec<(usize, [f64; 2])> = Vec::new();
        for cmd in &cmds {
            let pt = match *cmd {
                PathCommand::MoveTo { x, y }
                | PathCommand::LineTo { x, y }
                | PathCommand::QuadraticTo { x, y, .. }
                | PathCommand::CubicTo { x, y, .. } => {
                    let sx = mat[0] * x + mat[2] * y + mat[4];
                    let sy = mat[1] * x + mat[3] * y + mat[5];
                    Some([sx, sy])
                }
                PathCommand::Close => None,
            };
            if let Some(p) = pt {
                let idx = anchor_pts.len();
                anchor_pts.push((idx, p));
            }
        }
        let Some(&(v_idx, v_pos)) = anchor_pts.iter().min_by(|a, b| {
            let da = (a.1[0] - stage_pt[0]).powi(2) + (a.1[1] - stage_pt[1]).powi(2);
            let db = (b.1[0] - stage_pt[0]).powi(2) + (b.1[1] - stage_pt[1]).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        }) else {
            return false;
        };

        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let arm_poses: Vec<(Id, [f64; 2], [f64; 2], f64)> = poses
            .into_iter()
            .filter(|(a, _, _, _, _)| a == &arm_id)
            .map(|(_, b, h, t, ang)| (b, h, t, ang))
            .collect();
        self.vector
            .ik_base_elements
            .entry((symbol_id, frame, el.id.clone()))
            .or_insert_with(|| (el.clone(), arm_poses));

        let rad = bone_angle.to_radians();
        let c = rad.cos();
        let s = rad.sin();
        let rel_x = v_pos[0] - bone_head[0];
        let rel_y = v_pos[1] - bone_head[1];
        let rest_offset_x = rel_x * c + rel_y * s;
        let rest_offset_y = -rel_x * s + rel_y * c;

        let symbol = self.symbol_mut();
        let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
            return false;
        };
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
            return false;
        };
        let Some(arm) = key.ik_armatures.iter_mut().find(|a| a.id == arm_id) else {
            return false;
        };
        let mut map = Self::parse_armature_bound_vertices(&arm.name);
        let list = map.entry(el.id.clone()).or_default();
        let mut changed = false;
        if unbind {
            let before_len = list.len();
            list.retain(|(vi, bid, _, _)| !(*vi == v_idx && bid == &bone_id));
            changed = list.len() != before_len;
        } else if let Some(existing) = list.iter_mut().find(|(vi, _, _, _)| *vi == v_idx) {
            if existing.1 != bone_id {
                existing.1 = bone_id;
                existing.2 = rest_offset_x;
                existing.3 = rest_offset_y;
                changed = true;
            }
        } else {
            list.push((v_idx, bone_id, rest_offset_x, rest_offset_y));
            changed = true;
        }
        if changed {
            arm.name = Self::encode_armature_bound_vertices(&arm.name, &map);
        }
        changed
    }

    /// Bind or unbind the nearest contour vertex of the selected shape at `stage_pt` to the
    /// active/nearest `IkBone` in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn bind_stage_point_to_bone(&mut self, stage_pt: [f64; 2], unbind: bool) -> bool {
        self.history.begin(&self.project);
        if !self.apply_bind_stage_point_in_place(stage_pt, unbind) {
            return false;
        }
        self.commit_history();
        true
    }

    /// Return the sorted list of contour vertex indices of `shape_id` bound to `bone_id` (`T-271`).
    pub fn bound_vertices_for_bone(&self, shape_id: &str, bone_id: &str) -> Vec<usize> {
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return Vec::new();
        };
        let Some(key) = layer.key_at(self.frame) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for arm in &key.ik_armatures {
            let map = Self::parse_armature_bound_vertices(&arm.name);
            if let Some(list) = map.get(shape_id) {
                for (v_idx, bid, _, _) in list {
                    if bid == bone_id {
                        out.push(*v_idx);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}
