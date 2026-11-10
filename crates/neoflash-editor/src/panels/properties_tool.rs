//! Sticky Stage tool workflows and inspector bindings for Deco, Spray Brush, Bone/Bind,
//! Puppet/Asset Warp, and 3D Translate/Rotate (`T-180`, `T-186`).

use crate::app::Editor;
use crate::selection::SelectionTarget;
use crate::stage::deco_spray::{
    StageDecoOptions, StageSprayOptions, generate_stage_deco_gesture, generate_stage_spray_gesture,
};
use crate::stage::ik::{
    PuppetPin, bake_armature_to_keyframes, connect_armature_bone, deform_shape_with_pins,
    solve_armature_ccd,
};
use crate::stage_mapping::StageMapping;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use neoflash_core::*;
use neoflash_ui::components as nf;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;
use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

/// Sticky Stage authoring submodes (`T-180`, `T-186`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StageStickyToolMode {
    #[default]
    None,
    Deco,
    SprayBrush,
    Bone,
    Bind,
    PuppetWarp,
    Translate3D,
    Rotate3D,
}

#[allow(dead_code)]
#[derive(Clone, Default)]
struct StageToolSessionState {
    pub sticky_mode: StageStickyToolMode,
    pub deco_options: StageDecoOptions,
    pub spray_options: StageSprayOptions,
    pub gesture_points_screen: Vec<Pos2>,
    pub gesture_points_local: Vec<[f64; 2]>,
    pub active_armature_id: Option<Id>,
    pub active_bone_id: Option<Id>,
    pub bone_joint_specs: Vec<IkJointSpec>,
    pub bone_connect_from: Option<(Id, Pos2)>,
    pub bone_effector_drag: Option<ActiveEffectorDrag>,
    pub bind_shape_id: Option<Id>,
    pub bind_selected_vertices: Vec<usize>,
    pub bind_skins: BTreeMap<Id, IkShapeSkin>,
    pub puppet_selected_pin_id: Option<Id>,
    pub puppet_pin_drag: Option<ActivePuppetPinDrag>,
}

#[derive(Clone)]
struct ActiveEffectorDrag {
    armature_id: Id,
    before_project: Project,
}

#[derive(Clone)]
struct ActivePuppetPinDrag {
    element_id: Id,
    pin_id: Id,
    base_element: Element,
    before_project: Project,
}

static STAGE_TOOL_SESSIONS: LazyLock<Mutex<BTreeMap<String, StageToolSessionState>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

impl Editor {
    /// Set the active sticky Stage workflow mode (`Deco`, `SprayBrush`, `Bone`, `Bind`, `PuppetWarp`, `Translate3D`, `Rotate3D`) (`T-180`, `T-186`).
    #[allow(dead_code)]
    pub fn set_stage_sticky_tool_mode(&mut self, mode: StageStickyToolMode) {
        let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
        let sess = sessions.entry(self.clipboard.session.clone()).or_default();
        sess.sticky_mode = mode;
        sess.gesture_points_screen.clear();
        sess.gesture_points_local.clear();
        sess.bone_connect_from = None;
        sess.bone_effector_drag = None;
        sess.puppet_pin_drag = None;
    }

    /// Return the explicitly set sticky Stage workflow mode (`T-180`, `T-186`).
    pub fn stage_sticky_tool_mode(&self) -> StageStickyToolMode {
        let sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
        sessions
            .get(&self.clipboard.session)
            .map(|s| s.sticky_mode)
            .unwrap_or(StageStickyToolMode::None)
    }

    /// Resolve the effective sticky Stage workflow mode (`T-180`, `T-186`).
    pub(crate) fn effective_stage_sticky_tool_mode(
        &self,
        _ctx: Option<&egui::Context>,
    ) -> StageStickyToolMode {
        self.stage_sticky_tool_mode()
    }

    /// Configure Stage Deco tool options (`T-180`).
    #[allow(dead_code)]
    pub fn set_stage_deco_options(&mut self, options: StageDecoOptions) {
        let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
        sessions
            .entry(self.clipboard.session.clone())
            .or_default()
            .deco_options = options;
    }

    /// Read current Stage Deco tool options (`T-180`).
    pub fn stage_deco_options(&self) -> StageDecoOptions {
        let sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
        sessions
            .get(&self.clipboard.session)
            .map(|s| s.deco_options.clone())
            .unwrap_or_else(|| StageDecoOptions {
                primary_color: self.fill,
                secondary_color: self.stroke,
                ..StageDecoOptions::default()
            })
    }

    /// Configure Stage Spray Brush options (`T-180`).
    #[allow(dead_code)]
    pub fn set_stage_spray_options(&mut self, options: StageSprayOptions) {
        let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
        sessions
            .entry(self.clipboard.session.clone())
            .or_default()
            .spray_options = options;
    }

    /// Read current Stage Spray Brush options (`T-180`).
    pub fn stage_spray_options(&self) -> StageSprayOptions {
        let sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
        sessions
            .get(&self.clipboard.session)
            .map(|s| s.spray_options.clone())
            .unwrap_or_else(|| StageSprayOptions {
                symbol_id: self.library_selected.clone(),
                fill: self.fill,
                ..StageSprayOptions::default()
            })
    }

    /// Execute a Stage Deco gesture at `screen_points` mapped into the current Edit-in-Place context (`T-180`).
    /// Keeps generated elements selected, refuses locked/hidden layers, and commits one Undo step.
    pub fn apply_stage_deco_screen_gesture(
        &mut self,
        origin: Pos2,
        mapping: StageMapping,
        screen_points: &[Pos2],
    ) -> Vec<Id> {
        let local_points: Vec<[f64; 2]> = screen_points
            .iter()
            .map(|&pt| {
                let p = mapping.screen_to_edit(origin, self.zoom.max(0.01), pt);
                [f64::from(p.x), f64::from(p.y)]
            })
            .collect();
        self.apply_stage_deco_local_gesture(&local_points)
    }

    /// Execute a Stage Deco gesture directly in local edit coordinates (`T-180`).
    pub fn apply_stage_deco_local_gesture(&mut self, local_points: &[[f64; 2]]) -> Vec<Id> {
        if !self.layer_editable() || local_points.is_empty() {
            return Vec::new();
        }
        self.finish_edits();
        let mut opts = self.stage_deco_options();
        if opts.primary_color[3] == 0 {
            opts.primary_color = self.fill;
        }
        if opts.secondary_color[3] == 0 {
            opts.secondary_color = self.stroke;
        }
        let generated = generate_stage_deco_gesture(local_points, &opts);
        if generated.is_empty() {
            return Vec::new();
        }
        let sym_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let start_frame = self.frame;
        let Some(layer_id) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .map(|l| l.id.clone())
        else {
            return Vec::new();
        };

        let mut candidate = self.project.clone();
        Self::ensure_key_for_project(
            &mut candidate,
            &sym_id,
            &layer_id,
            start_frame,
            false,
            self.workspace.timeline_prefs.auto_keyframe,
        );
        let fps = f64::from(candidate.fps);
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return Vec::new();
        };

        let mut selected_ids = Vec::new();
        let step = opts.step_frames.clamp(1, 30);
        if opts.animate_pattern && generated.len() > 1 {
            let max_frame = start_frame + (generated.len() as u32 - 1) * step;
            sym.timeline.frame_count = sym.timeline.frame_count.max(max_frame + 1);
            let Some(layer) = sym.timeline.layers.get_mut(layer_idx) else {
                return Vec::new();
            };
            let source_layer = layer.clone();
            for (idx, _el) in generated.iter().enumerate() {
                let target_f = start_frame + (idx as u32) * step;
                let mut progressive = sample_layer_at_fps(&source_layer, target_f, fps);
                let preserved_len = progressive.len();
                for item in generated.iter().take(idx + 1) {
                    let mut c = item.clone();
                    if idx > 0 {
                        c.id = new_id("deco-step");
                    } else {
                        selected_ids.push(c.id.clone());
                    }
                    progressive.push(c);
                }
                if idx == 0 {
                    selected_ids = progressive
                        .iter()
                        .skip(preserved_len)
                        .map(|e| e.id.clone())
                        .collect();
                }
                if let Some(existing) = layer.keyframes.iter_mut().find(|k| k.frame == target_f) {
                    existing.elements = progressive;
                } else {
                    layer.keyframes.push(Keyframe {
                        frame: target_f,
                        elements: progressive,
                        ..Keyframe::default()
                    });
                }
            }
            layer.keyframes.sort_by_key(|k| k.frame);
        } else if let Some(kf) = sym
            .timeline
            .layers
            .get_mut(layer_idx)
            .and_then(|layer| layer.keyframes.iter_mut().find(|k| k.frame == start_frame))
        {
            for el in generated {
                selected_ids.push(el.id.clone());
                kf.elements.push(el);
            }
        }
        if selected_ids.is_empty() {
            return Vec::new();
        }
        candidate.upgrade_format_for_features();
        if candidate.validate().is_err() {
            return Vec::new();
        }
        let targets: Vec<SelectionTarget> = selected_ids
            .iter()
            .map(|id| SelectionTarget {
                layer: layer_id.clone(),
                element: id.clone(),
            })
            .collect();
        self.transact_named("Deco tool", |editor| {
            editor.project = candidate;
            editor.set_selection(targets);
        });
        selected_ids
    }

    /// Execute a Stage Spray Brush drag gesture at `screen_points` mapped into the current Edit-in-Place context (`T-180`).
    /// Keeps sprayed elements selected, refuses locked/hidden layers, and commits one Undo step.
    pub fn apply_stage_spray_screen_gesture(
        &mut self,
        origin: Pos2,
        mapping: StageMapping,
        screen_points: &[Pos2],
    ) -> Vec<Id> {
        let local_points: Vec<[f64; 2]> = screen_points
            .iter()
            .map(|&pt| {
                let p = mapping.screen_to_edit(origin, self.zoom.max(0.01), pt);
                [f64::from(p.x), f64::from(p.y)]
            })
            .collect();
        self.apply_stage_spray_local_gesture(&local_points)
    }

    /// Execute a Stage Spray Brush drag gesture directly in local edit coordinates (`T-180`).
    pub fn apply_stage_spray_local_gesture(&mut self, local_points: &[[f64; 2]]) -> Vec<Id> {
        if !self.layer_editable() || local_points.is_empty() {
            return Vec::new();
        }
        self.finish_edits();
        let mut opts = self.stage_spray_options();
        let current_sym_id = self.symbol_id().to_owned();
        if !opts.use_dots {
            let chosen = opts
                .symbol_id
                .clone()
                .or_else(|| self.library_selected.clone());
            opts.symbol_id = chosen.filter(|id| {
                id != &current_sym_id
                    && !self.project.is_scene(id)
                    && self.project.symbol(id).is_some()
            });
            if opts.symbol_id.is_none() {
                opts.use_dots = true;
            }
        }
        let sprayed = generate_stage_spray_gesture(local_points, &opts);
        if sprayed.is_empty() {
            return Vec::new();
        }
        let layer_idx = self.layer;
        let frame = self.frame;
        let Some(layer_id) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .map(|l| l.id.clone())
        else {
            return Vec::new();
        };
        let mut candidate = self.project.clone();
        Self::ensure_key_for_project(
            &mut candidate,
            &current_sym_id,
            &layer_id,
            frame,
            false,
            self.workspace.timeline_prefs.auto_keyframe,
        );
        let Some(sym) = candidate.symbol_mut(&current_sym_id) else {
            return Vec::new();
        };
        let Some(layer) = sym.timeline.layers.get_mut(layer_idx) else {
            return Vec::new();
        };
        let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == frame) else {
            return Vec::new();
        };
        let mut selected_ids = Vec::with_capacity(sprayed.len());
        for el in sprayed {
            selected_ids.push(el.id.clone());
            kf.elements.push(el);
        }
        candidate.upgrade_format_for_features();
        if candidate.validate().is_err() {
            return Vec::new();
        }
        let targets: Vec<SelectionTarget> = selected_ids
            .iter()
            .map(|id| SelectionTarget {
                layer: layer_id.clone(),
                element: id.clone(),
            })
            .collect();
        self.transact_named("Spray brush", |editor| {
            editor.project = candidate;
            editor.set_selection(targets);
        });
        selected_ids
    }

    // -------------------------------------------------------------------------
    // T-186: Bone / Bind & Puppet / Asset Warp Stage Authoring Workflows
    // -------------------------------------------------------------------------

    /// Connect an `IkBone` between `from_element_id` and `to_element_id` on the active layer/keyframe
    /// and commit as one Undo step (`T-186`).
    pub fn stage_connect_ik_bone(
        &mut self,
        from_element_id: &str,
        to_element_id: &str,
    ) -> Option<Id> {
        if !self.layer_editable() {
            return None;
        }
        self.finish_edits();
        let sym_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let layer_id = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .map(|l| l.id.clone())?;
        let mut candidate = self.project.clone();
        Self::ensure_key_for_project(
            &mut candidate,
            &sym_id,
            &layer_id,
            frame,
            false,
            self.workspace.timeline_prefs.auto_keyframe,
        );
        let sym = candidate.symbol_mut(&sym_id)?;
        let layer = sym.timeline.layers.get_mut(layer_idx)?;
        let kf = layer.keyframes.iter_mut().find(|k| k.frame == frame)?;

        let mut armature = kf
            .ik_armatures
            .first()
            .cloned()
            .unwrap_or_else(|| IkArmature {
                id: new_id("armature"),
                name: "Armature".into(),
                bones: Vec::new(),
                effector_bone_id: String::new(),
                target_pos: [0.0, 0.0],
                pole_pos: None,
            });
        let bone_id =
            connect_armature_bone(&mut armature, &kf.elements, from_element_id, to_element_id)?;
        let arm_id = armature.id.clone();
        if kf.ik_armatures.is_empty() {
            kf.ik_armatures.push(armature.clone());
        } else {
            kf.ik_armatures[0] = armature.clone();
        }
        candidate.upgrade_format_for_features();
        candidate.validate().ok()?;

        self.transact_named("Connect IK bone", |editor| {
            editor.project = candidate;
        });

        let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
        let sess = sessions.entry(self.clipboard.session.clone()).or_default();
        sess.active_armature_id = Some(arm_id);
        sess.active_bone_id = Some(bone_id.clone());
        for b in &armature.bones {
            if !sess.bone_joint_specs.iter().any(|s| s.bone_id == b.id) {
                sess.bone_joint_specs.push(IkJointSpec {
                    bone_id: b.id.clone(),
                    ..IkJointSpec::default()
                });
            }
        }
        Some(bone_id)
    }

    /// Drag the active IK effector to `target_local_pos` (and optional `pole_local_pos`), solve the armature
    /// pose using `solve_armature_ccd`, deform any bound `IkShapeSkin` shapes, and commit as one Undo step (`T-186`).
    pub fn stage_drag_ik_effector(
        &mut self,
        target_local_pos: [f64; 2],
        pole_local_pos: Option<[f64; 2]>,
    ) -> bool {
        if !self.layer_editable() {
            return false;
        }
        self.finish_edits();
        let sym_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let Some(layer_id) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .map(|l| l.id.clone())
        else {
            return false;
        };
        let (joint_specs, skins) = {
            let sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
            sessions
                .get(&self.clipboard.session)
                .map(|s| (s.bone_joint_specs.clone(), s.bind_skins.clone()))
                .unwrap_or_default()
        };

        let mut candidate = self.project.clone();
        Self::ensure_key_for_project(
            &mut candidate,
            &sym_id,
            &layer_id,
            frame,
            false,
            self.workspace.timeline_prefs.auto_keyframe,
        );
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return false;
        };
        let Some(layer) = sym.timeline.layers.get_mut(layer_idx) else {
            return false;
        };
        let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == frame) else {
            return false;
        };
        let Some(armature) = kf.ik_armatures.first_mut() else {
            return false;
        };
        armature.target_pos = target_local_pos;
        if let Some(pole) = pole_local_pos {
            armature.pole_pos = Some(pole);
        }
        let arm_clone = armature.clone();
        solve_armature_ccd(&mut kf.elements, &arm_clone, &joint_specs);
        for (shape_id, skin) in &skins {
            if skin.armature_id == arm_clone.id
                && let Some(pos) = kf.elements.iter().position(|e| &e.id == shape_id)
            {
                let deformed =
                    deform_shape_with_ik_skin(&kf.elements[pos], skin, &arm_clone, &kf.elements);
                kf.elements[pos] = deformed;
            }
        }
        candidate.upgrade_format_for_features();
        if candidate.validate().is_err() {
            return false;
        }
        self.transact_named("Pose IK armature", |editor| {
            editor.project = candidate;
        });
        true
    }

    /// Select shape control-point vertices in Bind mode (`T-186`).
    pub fn stage_bind_select_vertices(
        &mut self,
        shape_element_id: &str,
        vertex_indices: Vec<usize>,
    ) -> bool {
        let Some(el) = self.find_current_frame_element(shape_element_id) else {
            return false;
        };
        let count = match &el.kind {
            ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
                extract_path_control_points(commands).len()
            }
            _ => 0,
        };
        if count == 0 {
            return false;
        }
        let valid: Vec<usize> = vertex_indices.into_iter().filter(|&i| i < count).collect();
        let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
        let sess = sessions.entry(self.clipboard.session.clone()).or_default();
        sess.bind_shape_id = Some(shape_element_id.to_owned());
        sess.bind_selected_vertices = valid;
        true
    }

    /// Assign selected shape vertices to `bone_id` in Bind mode and update the shape skin (`T-186`).
    pub fn stage_bind_assign_vertices_to_bone(
        &mut self,
        shape_element_id: &str,
        bone_id: &str,
        vertex_indices: &[usize],
        weight: f64,
        exclusive: bool,
    ) -> Option<IkShapeSkin> {
        if !self.layer_editable() {
            return None;
        }
        self.finish_edits();
        let sym_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let layer_id = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .map(|l| l.id.clone())?;
        let mut candidate = self.project.clone();
        Self::ensure_key_for_project(
            &mut candidate,
            &sym_id,
            &layer_id,
            frame,
            false,
            self.workspace.timeline_prefs.auto_keyframe,
        );
        let sym = candidate.symbol_mut(&sym_id)?;
        let layer = sym.timeline.layers.get_mut(layer_idx)?;
        let kf = layer.keyframes.iter_mut().find(|k| k.frame == frame)?;
        let armature = kf.ik_armatures.first()?.clone();
        let shape_idx = kf.elements.iter().position(|e| e.id == shape_element_id)?;

        let mut skin = {
            let sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
            sessions
                .get(&self.clipboard.session)
                .and_then(|s| s.bind_skins.get(shape_element_id).cloned())
        }
        .or_else(|| {
            auto_bind_shape_to_armature(&kf.elements[shape_idx], &armature, &kf.elements)
        })?;

        bind_vertices_to_bone(&mut skin, bone_id, vertex_indices, weight, exclusive);
        let deformed =
            deform_shape_with_ik_skin(&kf.elements[shape_idx], &skin, &armature, &kf.elements);
        kf.elements[shape_idx] = deformed;
        candidate.upgrade_format_for_features();
        candidate.validate().ok()?;

        self.transact_named("Bind vertices to bone", |editor| {
            editor.project = candidate;
        });

        let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
        let sess = sessions.entry(self.clipboard.session.clone()).or_default();
        sess.bind_shape_id = Some(shape_element_id.to_owned());
        sess.bind_selected_vertices = vertex_indices.to_vec();
        sess.bind_skins
            .insert(shape_element_id.to_owned(), skin.clone());
        Some(skin)
    }

    /// Bake the active layer's IK armature span from `start_frame` to `end_frame` into keyframes in one Undo step (`T-186`).
    pub fn stage_bake_ik_to_keyframes(&mut self, start_frame: u32, end_frame: u32) -> usize {
        if !self.layer_editable() || end_frame < start_frame {
            return 0;
        }
        self.finish_edits();
        let joint_specs = {
            let sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
            sessions
                .get(&self.clipboard.session)
                .map(|s| s.bone_joint_specs.clone())
                .unwrap_or_default()
        };
        let sym_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let mut candidate = self.project.clone();
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return 0;
        };
        sym.timeline.frame_count = sym.timeline.frame_count.max(end_frame + 1);
        let Some(layer) = sym.timeline.layers.get_mut(layer_idx) else {
            return 0;
        };
        let baked = bake_armature_to_keyframes(layer, start_frame, end_frame, &joint_specs);
        if baked == 0 {
            return 0;
        }
        candidate.upgrade_format_for_features();
        if candidate.validate().is_err() {
            return 0;
        }
        self.transact_named("Convert IK to keyframes", |editor| {
            editor.project = candidate;
        });
        baked
    }

    /// Place or select a `PuppetPin` (`WarpPin`) on `element_id` at `local_pt` in one Undo step (`T-186`).
    pub fn stage_place_puppet_pin(
        &mut self,
        element_id: &str,
        local_pt: [f64; 2],
        kind: WarpPinKind,
    ) -> Option<Id> {
        if !self.layer_editable() {
            return None;
        }
        self.finish_edits();
        let sym_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let layer_id = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .map(|l| l.id.clone())?;
        let mut candidate = self.project.clone();
        Self::ensure_key_for_project(
            &mut candidate,
            &sym_id,
            &layer_id,
            frame,
            false,
            self.workspace.timeline_prefs.auto_keyframe,
        );
        let layer = candidate
            .symbol_mut(&sym_id)?
            .timeline
            .layers
            .get_mut(layer_idx)?;
        let key_index = layer.keyframes.iter().rposition(|key| key.frame <= frame)?;
        let kf = &mut layer.keyframes[key_index];
        if !kf.elements.iter().any(|element| element.id == element_id) {
            return None;
        }
        let mut rig = kf
            .asset_warps
            .iter()
            .find(|warp| warp.element_id == element_id)
            .cloned()
            .unwrap_or_else(|| AssetWarpRig {
                element_id: element_id.to_owned(),
                pins: Vec::new(),
                envelop_subdivisions: 4,
            });
        let pin_id = upsert_warp_pin(&mut rig, local_pt, 10.0, kind);
        kf.set_asset_warp(rig);
        candidate.upgrade_format_for_features();
        candidate.validate().ok()?;
        self.transact_named("Set Asset Warp", |editor| {
            editor.project = candidate;
        });
        let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
        let sess = sessions.entry(self.clipboard.session.clone()).or_default();
        sess.puppet_selected_pin_id = Some(pin_id.clone());
        let _ = sym_id;
        Some(pin_id)
    }

    /// Drag a `PuppetPin` on `element_id` to `new_local_pos` (with optional `rotation_delta_deg`),
    /// deform the element mesh live via `deform_shape_with_pins`, and commit as one Undo step (`T-186`).
    pub fn stage_drag_puppet_pin(
        &mut self,
        element_id: &str,
        pin_id: &str,
        new_local_pos: [f64; 2],
        rotation_delta_deg: f64,
    ) -> bool {
        if !self.layer_editable() {
            return false;
        }
        self.finish_edits();
        let sym_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let Some(layer_id) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .map(|l| l.id.clone())
        else {
            return false;
        };
        let mut candidate = self.project.clone();
        // Ensure a keyframe in the candidate so the keyed pose and pin drag share one Undo step.
        Self::ensure_key_for_project(
            &mut candidate,
            &sym_id,
            &layer_id,
            frame,
            false,
            self.workspace.timeline_prefs.auto_keyframe,
        );
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return false;
        };
        let Some(layer) = sym.timeline.layers.get_mut(layer_idx) else {
            return false;
        };
        let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == frame) else {
            return false;
        };
        let Some(rig) = kf
            .asset_warps
            .iter_mut()
            .find(|w| w.element_id == element_id)
        else {
            return false;
        };
        let Some(pin) = rig.pins.iter_mut().find(|p| p.id == pin_id) else {
            return false;
        };
        pin.pos = new_local_pos;
        if rotation_delta_deg.is_finite() {
            pin.rotation = (pin.rotation + rotation_delta_deg).clamp(-360.0, 360.0);
        }
        let pins_clone = rig.pins.clone();
        if let Some(el_slot) = kf.elements.iter_mut().find(|e| e.id == element_id) {
            *el_slot = deform_shape_with_pins(el_slot, &pins_clone);
        }
        candidate.upgrade_format_for_features();
        if candidate.validate().is_err() {
            return false;
        }
        self.transact_named("Deform puppet pin", |editor| {
            editor.project = candidate;
        });
        true
    }

    /// Delete `pin_id` from `element_id`'s `AssetWarpRig` in one Undo step (`T-186`).
    pub fn stage_delete_puppet_pin(&mut self, element_id: &str, pin_id: &str) -> bool {
        if !self.layer_editable() {
            return false;
        }
        self.finish_edits();
        let layer_idx = self.layer;
        let frame = self.frame;
        let Some(mut rig) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.keyframes.iter().find(|k| k.frame == frame))
            .and_then(|k| {
                k.asset_warps
                    .iter()
                    .find(|w| w.element_id == element_id)
                    .cloned()
            })
        else {
            return false;
        };
        let before = rig.pins.len();
        rig.pins.retain(|p| p.id != pin_id);
        if rig.pins.len() == before {
            return false;
        }
        self.set_element_asset_warp(rig)
    }

    /// Reset all `PuppetPin`s on `element_id` to their rest positions in one Undo step (`T-186`).
    #[allow(dead_code)]
    pub fn stage_reset_puppet_pins(&mut self, element_id: &str) -> bool {
        self.reset_element_asset_warp(element_id)
    }

    /// Live Stage input and overlay rendering for sticky Deco, Spray Brush, Bone, Bind, Puppet Warp,
    /// and 3D Translate / 3D Rotate modes (`T-180`, `T-186`).
    pub(crate) fn stage_sticky_workflow_step(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        rect: Rect,
        scene: &[DrawItem],
    ) -> bool {
        let mode = self.effective_stage_sticky_tool_mode(Some(ui.ctx()));
        match mode {
            StageStickyToolMode::None => {
                if self.selection.targets.len() == 1
                    && let Some(el) = self.selected_sampled()
                    && matches!(el.kind, ElementKind::Instance { .. })
                {
                    let st = self.element_3d_state(&el.id);
                    let has_3d = st.translate_3d.iter().any(|v| v.abs() > 1e-4)
                        || st.rotate_3d.iter().any(|v| v.abs() > 1e-4);
                    if has_3d {
                        let prefer_rotate = st.rotate_3d.iter().any(|v| v.abs() > 1e-4)
                            && st.translate_3d.iter().all(|v| v.abs() <= 1e-4);
                        self.stage_3d_gizmo_step(ui, painter, origin, mapping, prefer_rotate)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            StageStickyToolMode::Translate3D => {
                self.stage_3d_gizmo_step(ui, painter, origin, mapping, false)
            }
            StageStickyToolMode::Rotate3D => {
                self.stage_3d_gizmo_step(ui, painter, origin, mapping, true)
            }
            StageStickyToolMode::Deco | StageStickyToolMode::SprayBrush => {
                self.stage_deco_or_spray_step(ui, painter, origin, mapping, rect, mode)
            }
            StageStickyToolMode::Bone => {
                self.stage_bone_tool_step(ui, painter, origin, mapping, rect, scene)
            }
            StageStickyToolMode::Bind => {
                self.stage_bind_tool_step(ui, painter, origin, mapping, rect)
            }
            StageStickyToolMode::PuppetWarp => {
                self.stage_puppet_warp_step(ui, painter, origin, mapping, rect)
            }
        }
    }

    fn stage_deco_or_spray_step(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        rect: Rect,
        mode: StageStickyToolMode,
    ) -> bool {
        let (pressed, down, released, pos) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos().or_else(|| i.pointer.latest_pos()),
            )
        });
        let Some(pos) = pos else {
            return false;
        };
        let session_key = self.clipboard.session.clone();
        if pressed && rect.contains(pos) {
            if !self.layer_editable() {
                self.message("Unlock and show a normal layer to draw on the Stage.");
                return true;
            }
            let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
            let sess = sessions.entry(session_key).or_default();
            sess.gesture_points_screen = vec![pos];
            return true;
        }
        let active_pts = {
            let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
            let sess = sessions.entry(session_key.clone()).or_default();
            if sess.gesture_points_screen.is_empty() {
                return false;
            }
            if down
                && sess
                    .gesture_points_screen
                    .last()
                    .is_some_and(|last| last.distance(pos) >= 3.0)
            {
                sess.gesture_points_screen.push(pos);
            }
            sess.gesture_points_screen.clone()
        };

        // Paint live stroke/footprint preview on the Stage.
        let accent = neoflash_ui::palette(ui.ctx()).accent_default;
        if active_pts.len() >= 2 {
            painter.add(egui::Shape::line(
                active_pts.clone(),
                Stroke::new(1.5_f32, accent),
            ));
        }
        if mode == StageStickyToolMode::SprayBrush {
            let opts = self.stage_spray_options();
            let r = (opts.brush_width.max(opts.brush_height) as f32 * 0.5 * self.zoom).max(6.0);
            painter.circle_stroke(pos, r, Stroke::new(1.0_f32, accent));
        } else {
            painter.circle_stroke(pos, 8.0, Stroke::new(1.5_f32, accent));
        }

        if released || !down {
            {
                let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
                if let Some(sess) = sessions.get_mut(&session_key) {
                    sess.gesture_points_screen.clear();
                }
            }
            if mode == StageStickyToolMode::Deco {
                self.apply_stage_deco_screen_gesture(origin, mapping, &active_pts);
            } else {
                self.apply_stage_spray_screen_gesture(origin, mapping, &active_pts);
            }
        }
        true
    }

    fn stage_bone_tool_step(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        rect: Rect,
        scene: &[DrawItem],
    ) -> bool {
        let accent = neoflash_ui::palette(ui.ctx()).accent_default;
        let guide_col = Color32::from_rgb(245, 178, 52);

        // Paint existing IK armatures and joints on the current keyframe.
        let mut effector_screen: Option<(Id, Pos2)> = None;
        if let Some(layer) = self.symbol().timeline.layers.get(self.layer)
            && let Some(kf) = layer.keyframes.iter().find(|k| k.frame <= self.frame)
        {
            for arm in &kf.ik_armatures {
                let poses = compute_bone_world_poses(arm, &kf.elements);
                for (bone_id, head, tail, _angle) in &poses {
                    let h_scr = mapping.edit_to_screen(
                        origin,
                        self.zoom,
                        Pos2::new(head[0] as f32, head[1] as f32),
                    );
                    let t_scr = mapping.edit_to_screen(
                        origin,
                        self.zoom,
                        Pos2::new(tail[0] as f32, tail[1] as f32),
                    );
                    painter.line_segment([h_scr, t_scr], Stroke::new(2.5_f32, accent));
                    painter.circle_filled(h_scr, 5.0, accent);
                    painter.circle_filled(t_scr, 4.5, guide_col);
                    if bone_id == &arm.effector_bone_id {
                        effector_screen = Some((arm.id.clone(), t_scr));
                    }
                }
            }
        }

        let (pressed, down, released, pos) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos().or_else(|| i.pointer.latest_pos()),
            )
        });
        let Some(pos) = pos else {
            return false;
        };
        let session_key = self.clipboard.session.clone();

        if pressed && rect.contains(pos) {
            if let Some((arm_id, eff_pt)) = effector_screen
                && eff_pt.distance(pos) <= 12.0
            {
                let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
                let sess = sessions.entry(session_key).or_default();
                sess.bone_effector_drag = Some(ActiveEffectorDrag {
                    armature_id: arm_id,
                    before_project: self.project.clone(),
                });
                return true;
            }
            let local = mapping.screen_to_edit(origin, self.zoom, pos);
            if let Some(hit) = self.pick_placement(scene, painter, local, mapping) {
                let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
                let sess = sessions.entry(session_key).or_default();
                sess.bone_connect_from = Some((hit.element, pos));
                return true;
            }
        }

        let (connect_from, eff_drag) = {
            let sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
            sessions
                .get(&session_key)
                .map(|s| (s.bone_connect_from.clone(), s.bone_effector_drag.clone()))
                .unwrap_or((None, None))
        };

        if let Some((from_id, start_scr)) = connect_from {
            painter.line_segment([start_scr, pos], Stroke::new(2.0_f32, guide_col));
            if released || !down {
                {
                    let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
                    if let Some(sess) = sessions.get_mut(&session_key) {
                        sess.bone_connect_from = None;
                    }
                }
                let local = mapping.screen_to_edit(origin, self.zoom, pos);
                if let Some(to_hit) = self.pick_placement(scene, painter, local, mapping)
                    && to_hit.element != from_id
                {
                    self.stage_connect_ik_bone(&from_id, &to_hit.element);
                }
            }
            return true;
        }

        if let Some(drag) = eff_drag {
            let local = mapping.screen_to_edit(origin, self.zoom, pos);
            if released || !down {
                self.project = drag.before_project;
                {
                    let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
                    if let Some(sess) = sessions.get_mut(&session_key) {
                        sess.bone_effector_drag = None;
                    }
                }
                let _ = drag.armature_id;
                self.stage_drag_ik_effector([f64::from(local.x), f64::from(local.y)], None);
            }
            return true;
        }
        false
    }

    fn stage_bind_tool_step(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        rect: Rect,
    ) -> bool {
        let Some(el) = self.selected_sampled() else {
            return false;
        };
        let (ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. }) =
            &el.kind
        else {
            return false;
        };
        let pts = extract_path_control_points(commands);
        let matrix = transform_matrix(el.transform);
        let accent = neoflash_ui::palette(ui.ctx()).accent_default;
        let selected_verts = {
            let sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
            sessions
                .get(&self.clipboard.session)
                .map(|s| s.bind_selected_vertices.clone())
                .unwrap_or_default()
        };
        for (idx, pt) in pts.iter().enumerate() {
            let stage_pt = crate::stage::affine(matrix, Pos2::new(pt[0] as f32, pt[1] as f32));
            let scr = mapping.edit_to_screen(origin, self.zoom, stage_pt);
            let is_sel = selected_verts.contains(&idx);
            painter.circle_filled(
                scr,
                if is_sel { 5.0 } else { 3.5 },
                if is_sel {
                    Color32::from_rgb(245, 178, 52)
                } else {
                    accent
                },
            );
        }
        let (pressed, pos) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.interact_pos().or_else(|| i.pointer.latest_pos()),
            )
        });
        if pressed
            && let Some(pos) = pos
            && rect.contains(pos)
        {
            for (idx, pt) in pts.iter().enumerate() {
                let stage_pt = crate::stage::affine(matrix, Pos2::new(pt[0] as f32, pt[1] as f32));
                let scr = mapping.edit_to_screen(origin, self.zoom, stage_pt);
                if scr.distance(pos) <= 9.0 {
                    self.stage_bind_select_vertices(&el.id, vec![idx]);
                    let active_bone = {
                        let sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
                        sessions
                            .get(&self.clipboard.session)
                            .and_then(|s| s.active_bone_id.clone())
                    };
                    if let Some(bone_id) = active_bone {
                        self.stage_bind_assign_vertices_to_bone(
                            &el.id,
                            &bone_id,
                            &[idx],
                            1.0,
                            false,
                        );
                    }
                    return true;
                }
            }
        }
        false
    }

    fn stage_puppet_warp_step(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        rect: Rect,
    ) -> bool {
        let Some(el) = self.selected_sampled() else {
            return false;
        };
        let matrix = transform_matrix(el.transform);
        let pin_col = Color32::from_rgb(245, 178, 52);
        let rig = self
            .symbol()
            .timeline
            .layers
            .get(self.layer)
            .and_then(|l| l.keyframes.iter().rfind(|k| k.frame <= self.frame))
            .and_then(|k| k.asset_warps.iter().find(|w| w.element_id == el.id))
            .cloned();

        if let Some(rig_ref) = &rig {
            for pin in &rig_ref.pins {
                let stage_pt =
                    crate::stage::affine(matrix, Pos2::new(pin.pos[0] as f32, pin.pos[1] as f32));
                let scr = mapping.edit_to_screen(origin, self.zoom, stage_pt);
                painter.circle_filled(scr, 5.5, pin_col);
                painter.circle_stroke(scr, 7.5, Stroke::new(1.5_f32, Color32::BLACK));
            }
        }

        let (pressed, down, released, pos, alt_held) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos().or_else(|| i.pointer.latest_pos()),
                i.modifiers.alt,
            )
        });
        let Some(pos) = pos else {
            return false;
        };
        let session_key = self.clipboard.session.clone();

        if pressed && rect.contains(pos) {
            if let Some(rig_ref) = &rig {
                for pin in &rig_ref.pins {
                    let stage_pt = crate::stage::affine(
                        matrix,
                        Pos2::new(pin.pos[0] as f32, pin.pos[1] as f32),
                    );
                    let scr = mapping.edit_to_screen(origin, self.zoom, stage_pt);
                    if scr.distance(pos) <= 10.0 {
                        if alt_held {
                            self.stage_delete_puppet_pin(&el.id, &pin.id);
                            return true;
                        }
                        let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
                        let sess = sessions.entry(session_key).or_default();
                        sess.puppet_selected_pin_id = Some(pin.id.clone());
                        sess.puppet_pin_drag = Some(ActivePuppetPinDrag {
                            element_id: el.id.clone(),
                            pin_id: pin.id.clone(),
                            base_element: el.clone(),
                            before_project: self.project.clone(),
                        });
                        return true;
                    }
                }
            }
            let stage_pt = mapping.screen_to_edit(origin, self.zoom, pos);
            if let Some(local_pt) = crate::stage::inverse(matrix, stage_pt) {
                self.stage_place_puppet_pin(
                    &el.id,
                    [f64::from(local_pt.x), f64::from(local_pt.y)],
                    WarpPinKind::Open,
                );
                return true;
            }
        }

        let pin_drag = {
            let sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
            sessions
                .get(&session_key)
                .and_then(|s| s.puppet_pin_drag.clone())
        };
        if let Some(drag) = pin_drag {
            let stage_pt = mapping.screen_to_edit(origin, self.zoom, pos);
            let local_pt = crate::stage::inverse(matrix, stage_pt).unwrap_or(stage_pt);
            if released || !down {
                self.project = drag.before_project;
                {
                    let mut sessions = STAGE_TOOL_SESSIONS.lock().unwrap();
                    if let Some(sess) = sessions.get_mut(&session_key) {
                        sess.puppet_pin_drag = None;
                    }
                }
                let _ = drag.base_element;
                let _ = PuppetPin {
                    id: drag.pin_id.clone(),
                    kind: WarpPinKind::Open,
                    rest_pos: [0.0, 0.0],
                    pos: [f64::from(local_pt.x), f64::from(local_pt.y)],
                    rotation: 0.0,
                    stiffness: 1.0,
                };
                self.stage_drag_puppet_pin(
                    &drag.element_id,
                    &drag.pin_id,
                    [f64::from(local_pt.x), f64::from(local_pt.y)],
                    0.0,
                );
            }
            return true;
        }
        false
    }

    /// Promoted IK Bone / Armature inspector UI helper (`T-186`).
    #[allow(dead_code)]
    pub(crate) fn bone_inspector_section_ui(&mut self, ui: &mut egui::Ui) {
        let p = neoflash_ui::palette(ui.ctx());
        nf::SectionHeader::new("IK Bone & Armature")
            .id_salt("ik-bone-inspector")
            .default_open(true)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = space::S2;
                ui.label(
                    rich(
                        "Connect instances or shapes to build an IK chain, or bake poses to keyframes.",
                        TextStyle::Small,
                    )
                    .color(p.text_secondary),
                );
                if ui.add(nf::Button::ghost("Convert IK to keyframes")).clicked() {
                    let end = self.symbol().timeline.frame_count.saturating_sub(1);
                    self.stage_bake_ik_to_keyframes(0, end);
                }
            });
    }
}
