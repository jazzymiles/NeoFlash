//! Stage authoring helpers for Bone/Bind IK armatures, Puppet/Asset Warp pins, and 3D Gizmos (`T-186`).

use super::*;

/// Alias for `WarpPin` used by Stage Puppet / Asset Warp authoring workflows (`T-186`).
pub type PuppetPin = WarpPin;

/// Solve an `IkArmature` toward its `target_pos` (and optional `pole_pos`) using CCD/FABRIK (`T-186`).
pub fn solve_armature_ccd(
    elements: &mut [Element],
    armature: &IkArmature,
    joint_specs: &[IkJointSpec],
) {
    if joint_specs.is_empty() {
        solve_ik_armature(elements, armature);
    } else {
        solve_ik_armature_fabrik_ccd(elements, armature, joint_specs);
    }
}

/// Connect or append a new `IkBone` between `parent_element_id` (or an existing parent bone) and `child_element_id`
/// in `armature` (`T-186`).
pub fn connect_armature_bone(
    armature: &mut IkArmature,
    elements: &[Element],
    from_element_id: &str,
    to_element_id: &str,
) -> Option<Id> {
    if from_element_id == to_element_id {
        return None;
    }
    let from_el = elements.iter().find(|e| e.id == from_element_id)?;
    let to_el = elements.iter().find(|e| e.id == to_element_id)?;
    let dx = to_el.transform.x - from_el.transform.x;
    let dy = to_el.transform.y - from_el.transform.y;
    let dist = dx.hypot(dy).max(8.0);

    // Ensure root bone exists for `from_element_id`.
    let parent_bone_id = if let Some(existing) = armature
        .bones
        .iter()
        .find(|b| b.element_id == from_element_id)
    {
        existing.id.clone()
    } else {
        let root_id = new_id("bone");
        armature.bones.push(IkBone {
            id: root_id.clone(),
            element_id: from_element_id.to_owned(),
            parent_bone_id: None,
            length: dist,
            min_angle: None,
            max_angle: None,
            stiffness: 0.0,
        });
        root_id
    };

    if let Some(existing_child) = armature
        .bones
        .iter()
        .find(|b| b.element_id == to_element_id)
    {
        armature.effector_bone_id = existing_child.id.clone();
        armature.target_pos = [to_el.transform.x, to_el.transform.y];
        return Some(existing_child.id.clone());
    }

    let child_bone_id = new_id("bone");
    let leaf_len = (dist * 0.5).max(12.0);
    armature.bones.push(IkBone {
        id: child_bone_id.clone(),
        element_id: to_element_id.to_owned(),
        parent_bone_id: Some(parent_bone_id),
        length: leaf_len,
        min_angle: None,
        max_angle: None,
        stiffness: 0.0,
    });
    armature.effector_bone_id = child_bone_id.clone();
    armature.target_pos = [to_el.transform.x + leaf_len, to_el.transform.y];
    Some(child_bone_id)
}

/// Deform an `Element`'s vector path commands using a slice of `PuppetPin`s (`WarpPin`s) (`T-186`).
pub fn deform_shape_with_pins(element: &Element, pins: &[PuppetPin]) -> Element {
    if pins.is_empty() {
        return element.clone();
    }
    let map_local = |x: f64, y: f64| -> (f64, f64) {
        let out = deform_point_with_pins([x, y], pins);
        (out[0], out[1])
    };
    let deform_cmds = |commands: &[PathCommand]| -> Vec<PathCommand> {
        commands
            .iter()
            .map(|cmd| match *cmd {
                PathCommand::MoveTo { x, y } => {
                    let (nx, ny) = map_local(x, y);
                    PathCommand::MoveTo { x: nx, y: ny }
                }
                PathCommand::LineTo { x, y } => {
                    let (nx, ny) = map_local(x, y);
                    PathCommand::LineTo { x: nx, y: ny }
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let (ncx, ncy) = map_local(cx, cy);
                    let (nx, ny) = map_local(x, y);
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
                    let (nc1x, nc1y) = map_local(cx1, cy1);
                    let (nc2x, nc2y) = map_local(cx2, cy2);
                    let (nx, ny) = map_local(x, y);
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
            })
            .collect()
    };

    let mut out = element.clone();
    match &mut out.kind {
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            *commands = deform_cmds(commands);
        }
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let base = rectangle_path_commands(*width, *height, RectangleOptions::default());
            out.kind = ElementKind::Path {
                commands: deform_cmds(&base),
                fill_rule: FillRule::NonZero,
                fill: *fill,
                stroke: *stroke,
                stroke_width: *stroke_width,
            };
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let base = oval_path_commands(*width, *height, OvalOptions::default());
            out.kind = ElementKind::Path {
                commands: deform_cmds(&base),
                fill_rule: FillRule::NonZero,
                fill: *fill,
                stroke: *stroke,
                stroke_width: *stroke_width,
            };
        }
        _ => {
            // For symbol instances or bitmaps, shift/rotate transform by weighted pin displacement.
            let mut sum_w = 0.0;
            let mut dx = 0.0;
            let mut dy = 0.0;
            let mut drot = 0.0;
            for pin in pins {
                let w = pin.stiffness.max(0.25);
                dx += (pin.pos[0] - pin.rest_pos[0]) * w;
                dy += (pin.pos[1] - pin.rest_pos[1]) * w;
                drot += pin.rotation * w;
                sum_w += w;
            }
            if sum_w > 1e-6 {
                out.transform.x += dx / sum_w;
                out.transform.y += dy / sum_w;
                out.transform.rotation += drot / sum_w;
            }
        }
    }
    out
}

/// Bake an IK armature span on `layer` from `start_frame` through `end_frame` (inclusive) into
/// explicit frame-by-frame keyframes with interpolated target poses (`T-186`).
pub fn bake_armature_to_keyframes(
    layer: &mut Layer,
    start_frame: u32,
    end_frame: u32,
    joint_specs: &[IkJointSpec],
) -> usize {
    if end_frame < start_frame {
        return 0;
    }
    let Some(source_idx) = layer
        .keyframes
        .iter()
        .rposition(|k| k.frame <= start_frame && !k.ik_armatures.is_empty())
        .or_else(|| {
            layer
                .keyframes
                .iter()
                .position(|k| !k.ik_armatures.is_empty())
        })
    else {
        return 0;
    };
    let base_key = layer.keyframes[source_idx].clone();
    let base_armatures = base_key.ik_armatures.clone();
    let end_target_armatures: Option<Vec<IkArmature>> = layer
        .keyframes
        .iter()
        .find(|k| k.frame == end_frame && !k.ik_armatures.is_empty())
        .map(|k| k.ik_armatures.clone());

    let span_len = (end_frame - start_frame).max(1) as f64;
    let mut baked = 0usize;
    for f in start_frame..=end_frame {
        let t = if end_frame == start_frame {
            1.0
        } else {
            f64::from(f - start_frame) / span_len
        };
        let mut elements = base_key.elements.clone();
        let mut armatures = base_armatures.clone();
        for (arm_i, arm) in armatures.iter_mut().enumerate() {
            if let Some(end_arms) = &end_target_armatures
                && let Some(end_arm) = end_arms.get(arm_i)
            {
                arm.target_pos = [
                    arm.target_pos[0] + (end_arm.target_pos[0] - arm.target_pos[0]) * t,
                    arm.target_pos[1] + (end_arm.target_pos[1] - arm.target_pos[1]) * t,
                ];
            }
            solve_armature_ccd(&mut elements, arm, joint_specs);
        }
        if let Some(existing) = layer.keyframes.iter_mut().find(|k| k.frame == f) {
            existing.elements = elements;
            existing.ik_armatures = armatures;
        } else {
            let mut kf = base_key.clone();
            kf.frame = f;
            kf.elements = elements;
            kf.ik_armatures = armatures;
            kf.tween = None;
            kf.keyed_properties = None;
            layer.keyframes.push(kf);
        }
        baked += 1;
    }
    layer.keyframes.sort_by_key(|k| k.frame);
    baked
}

/// Interactive 3D Stage gizmo handle kind (`T-186`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gizmo3dHandle {
    TranslateX,
    TranslateY,
    TranslateZ,
    RotateX,
    RotateY,
    RotateZ,
    VanishingPoint,
    PerspectiveAngle,
}

/// Authoring 3D transform state for an element on the Stage (`T-186`).
#[derive(Clone, Debug, PartialEq)]
pub struct Stage3dAuthoringState {
    pub translate_3d: [f64; 3],
    pub rotate_3d: [f64; 3],
    pub perspective_angle: f64,
    pub vanishing_point: [f64; 2],
}

impl Default for Stage3dAuthoringState {
    fn default() -> Self {
        Self {
            translate_3d: [0.0, 0.0, 0.0],
            rotate_3d: [0.0, 0.0, 0.0],
            perspective_angle: 55.0,
            vanishing_point: [275.0, 200.0],
        }
    }
}

impl Stage3dAuthoringState {
    /// Convert Flash perspective field-of-view angle (`1.0..=179.0` degrees) to focal length in pixels.
    pub fn focal_length(&self, stage_width: f64) -> f64 {
        let angle_rad = self.perspective_angle.clamp(5.0, 170.0).to_radians();
        let half_tan = (angle_rad * 0.5).tan().max(0.05);
        (stage_width.max(200.0) * 0.5 / half_tan).clamp(50.0, 5000.0)
    }

    pub fn to_display_pose(&self) -> Display3dPose {
        Display3dPose {
            x: self.translate_3d[0],
            y: self.translate_3d[1],
            z: self.translate_3d[2],
            scale_x: 1.0,
            scale_y: 1.0,
            scale_z: 1.0,
            rotation_x: self.rotate_3d[0],
            rotation_y: self.rotate_3d[1],
            rotation_z: self.rotate_3d[2],
        }
    }

    /// Apply a 2D Stage pointer drag delta (`dx`, `dy` in Stage coordinates) to the active 3D gizmo handle.
    pub fn apply_handle_drag(&mut self, handle: Gizmo3dHandle, dx: f64, dy: f64) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        match handle {
            Gizmo3dHandle::TranslateX => {
                self.translate_3d[0] = (self.translate_3d[0] + dx).clamp(-10_000.0, 10_000.0);
            }
            Gizmo3dHandle::TranslateY => {
                self.translate_3d[1] = (self.translate_3d[1] + dy).clamp(-10_000.0, 10_000.0);
            }
            Gizmo3dHandle::TranslateZ => {
                self.translate_3d[2] =
                    (self.translate_3d[2] + (dy - dx) * 1.5).clamp(-5_000.0, 5_000.0);
            }
            Gizmo3dHandle::RotateX => {
                self.rotate_3d[0] = (self.rotate_3d[0] - dy * 0.6).clamp(-360.0, 360.0);
            }
            Gizmo3dHandle::RotateY => {
                self.rotate_3d[1] = (self.rotate_3d[1] + dx * 0.6).clamp(-360.0, 360.0);
            }
            Gizmo3dHandle::RotateZ => {
                self.rotate_3d[2] = (self.rotate_3d[2] + (dx + dy) * 0.5).clamp(-360.0, 360.0);
            }
            Gizmo3dHandle::VanishingPoint => {
                self.vanishing_point[0] = (self.vanishing_point[0] + dx).clamp(-10_000.0, 10_000.0);
                self.vanishing_point[1] = (self.vanishing_point[1] + dy).clamp(-10_000.0, 10_000.0);
            }
            Gizmo3dHandle::PerspectiveAngle => {
                self.perspective_angle = (self.perspective_angle + dx * 0.25).clamp(5.0, 170.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_element(id: &str, name: &str, kind: ElementKind) -> Element {
        let mut el = generate_deco_elements(
            DecoEffectKind::FlowerBrush,
            [0.0, 0.0],
            100.0,
            20.0,
            [0, 0, 0, 255],
            [0, 0, 0, 255],
        )
        .into_iter()
        .next()
        .expect("template element");
        el.id = id.into();
        el.name = name.into();
        el.transform = Transform::default();
        el.kind = kind;
        el
    }

    #[test]
    fn ik_connect_solve_and_bake_armature_to_keyframes_work() {
        let mut e1 = test_element(
            "upper",
            "Upper",
            ElementKind::Rectangle {
                width: 40.0,
                height: 12.0,
                fill: [200, 80, 80, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
        );
        e1.transform.x = 50.0;
        e1.transform.y = 100.0;
        let mut e2 = test_element(
            "lower",
            "Lower",
            ElementKind::Rectangle {
                width: 40.0,
                height: 12.0,
                fill: [80, 80, 200, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
        );
        e2.transform.x = 110.0;
        e2.transform.y = 100.0;

        let mut armature = IkArmature {
            id: "arm-1".into(),
            name: "Arm".into(),
            bones: Vec::new(),
            effector_bone_id: String::new(),
            target_pos: [110.0, 100.0],
            pole_pos: None,
        };
        let elements = vec![e1, e2];
        let leaf = connect_armature_bone(&mut armature, &elements, "upper", "lower")
            .expect("should connect bone");
        assert_eq!(armature.bones.len(), 2);
        assert_eq!(armature.effector_bone_id, leaf);

        armature.target_pos = [90.0, 160.0];
        let proj = Project::blank();
        let mut layer = proj.symbols[0].timeline.layers[0].clone();
        layer.keyframes[0].elements = elements;
        layer.keyframes[0].ik_armatures = vec![armature];

        let baked = bake_armature_to_keyframes(&mut layer, 0, 3, &[]);
        assert_eq!(baked, 4);
        assert_eq!(layer.keyframes.len(), 4);
    }

    #[test]
    fn puppet_pin_deform_and_3d_gizmo_handles_update_pose() {
        let rect = test_element(
            "shape-1",
            "Rect",
            ElementKind::Rectangle {
                width: 80.0,
                height: 40.0,
                fill: [60, 180, 90, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
        );
        let pins = vec![
            PuppetPin {
                id: "p1".into(),
                kind: WarpPinKind::Fixed,
                rest_pos: [0.0, 0.0],
                pos: [0.0, 0.0],
                rotation: 0.0,
                stiffness: 1.0,
            },
            PuppetPin {
                id: "p2".into(),
                kind: WarpPinKind::Open,
                rest_pos: [80.0, 40.0],
                pos: [100.0, 65.0],
                rotation: 12.0,
                stiffness: 1.0,
            },
        ];
        let deformed = deform_shape_with_pins(&rect, &pins);
        assert!(matches!(deformed.kind, ElementKind::Path { .. }));

        let mut state3d = Stage3dAuthoringState::default();
        state3d.apply_handle_drag(Gizmo3dHandle::TranslateX, 24.0, 0.0);
        state3d.apply_handle_drag(Gizmo3dHandle::TranslateZ, 0.0, 20.0);
        state3d.apply_handle_drag(Gizmo3dHandle::RotateY, 30.0, 0.0);
        state3d.apply_handle_drag(Gizmo3dHandle::VanishingPoint, 15.0, -10.0);
        state3d.apply_handle_drag(Gizmo3dHandle::PerspectiveAngle, 40.0, 0.0);
        assert_eq!(state3d.translate_3d[0], 24.0);
        assert!(state3d.translate_3d[2] > 0.0);
        assert!(state3d.rotate_3d[1] > 0.0);
        assert_eq!(state3d.vanishing_point, [290.0, 190.0]);
        assert_eq!(state3d.perspective_angle, 65.0);
        let inst = test_element(
            "inst-1",
            "Inst",
            ElementKind::Instance {
                symbol_id: "sym-1".into(),
                playback: InstancePlayback::Independent,
            },
        );
        let projected = project_element_3d(
            &inst,
            &state3d.to_display_pose(),
            state3d.focal_length(550.0),
            state3d.vanishing_point,
        );
        assert!((projected.transform.x - inst.transform.x).abs() > 1e-3);
    }
}
