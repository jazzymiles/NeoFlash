//! Stage 3D Translation / 3D Rotation gizmo and Movie Clip / Bone inspector helpers (`T-186`, `T-190`).

use crate::app::{Editor, ScriptTarget};
use crate::selection::SelectionTarget;
use crate::stage::ik::{Gizmo3dHandle, Stage3dAuthoringState};
use crate::stage_mapping::StageMapping;
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use neoflash_core::*;
use neoflash_ui::components as nf;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;
use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Default)]
struct MovieClip3dSession {
    states: BTreeMap<Id, Stage3dAuthoringState>,
    base_elements: BTreeMap<Id, Element>,
    active_drag: Option<Active3dDrag>,
}

#[derive(Clone)]
struct Active3dDrag {
    target: SelectionTarget,
    element_id: Id,
    handle: Gizmo3dHandle,
    start_local: Pos2,
    start_state: Stage3dAuthoringState,
    base_element: Element,
    before_project: Project,
}

static MOVIE_CLIP_3D_SESSIONS: LazyLock<Mutex<BTreeMap<String, MovieClip3dSession>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// Screen-space positions of the 3D gizmo handles for a selected element (`T-186`).
#[derive(Clone, Copy, Debug)]
pub struct Gizmo3dScreenGeometry {
    pub center: Pos2,
    pub x_tip: Pos2,
    pub y_tip: Pos2,
    pub z_tip: Pos2,
    pub vanishing_point: Pos2,
    pub perspective_handle: Pos2,
    pub ring_radius: f32,
}

impl Gizmo3dScreenGeometry {
    pub fn hit_test(&self, pos: Pos2, rotate_mode: bool) -> Option<Gizmo3dHandle> {
        let hit_r = 10.0_f32;
        if self.vanishing_point.distance(pos) <= hit_r {
            return Some(Gizmo3dHandle::VanishingPoint);
        }
        if self.perspective_handle.distance(pos) <= hit_r {
            return Some(Gizmo3dHandle::PerspectiveAngle);
        }
        if rotate_mode {
            if self.x_tip.distance(pos) <= hit_r {
                return Some(Gizmo3dHandle::RotateX);
            }
            if self.y_tip.distance(pos) <= hit_r {
                return Some(Gizmo3dHandle::RotateY);
            }
            let dist = self.center.distance(pos);
            if (dist - self.ring_radius).abs() <= 8.0 || self.z_tip.distance(pos) <= hit_r {
                return Some(Gizmo3dHandle::RotateZ);
            }
        } else {
            if self.x_tip.distance(pos) <= hit_r {
                return Some(Gizmo3dHandle::TranslateX);
            }
            if self.y_tip.distance(pos) <= hit_r {
                return Some(Gizmo3dHandle::TranslateY);
            }
            if self.z_tip.distance(pos) <= hit_r || self.center.distance(pos) <= hit_r {
                return Some(Gizmo3dHandle::TranslateZ);
            }
        }
        None
    }
}

impl Editor {
    /// Return the single selected editable `MovieClip` or `Button` instance (`(target, element, symbol_kind)`),
    /// or `None` if the selection is empty, multi-selection, grouped, on a locked/hidden layer, or a `Graphic` / non-instance (`T-190`).
    pub(crate) fn selected_actionable_instance(
        &self,
    ) -> Option<(SelectionTarget, Element, SymbolKind)> {
        if self.selection.targets.len() != 1 || self.can_ungroup_selection() {
            return None;
        }
        let target = self.selection.targets[0].clone();
        if !self.target_editable(&target) {
            return None;
        }
        let element = self
            .selected_element()
            .cloned()
            .or_else(|| self.selected_sampled())?;
        let ElementKind::Instance { symbol_id, .. } = &element.kind else {
            return None;
        };
        let symbol = self.project.symbol(symbol_id)?;
        if !matches!(symbol.kind, SymbolKind::MovieClip | SymbolKind::Button) {
            return None;
        }
        Some((target, element, symbol.kind))
    }

    /// True when Stage `Actions…` (`F9`) is enabled for the current selection (`T-190`).
    pub fn can_open_selected_object_actions(&self) -> bool {
        self.selected_actionable_instance().is_some()
    }

    /// Determine the default `ObjectEvent` when opening Actions for `element` (`T-190`):
    /// - Empty `Button` -> `ObjectEvent::Click`
    /// - Empty `MovieClip` -> `ObjectEvent::Load`
    /// - Existing handlers -> preserve `current_event` if present on `element.scripts`, otherwise first handler's event.
    pub fn default_object_event_for_instance(
        element: &Element,
        kind: SymbolKind,
        current_event: ObjectEvent,
    ) -> ObjectEvent {
        if !element.scripts.is_empty() {
            if element.scripts.iter().any(|s| s.event == current_event) {
                return current_event;
            }
            return element.scripts[0].event;
        }
        match kind {
            SymbolKind::Button => ObjectEvent::Click,
            SymbolKind::MovieClip | SymbolKind::Graphic => ObjectEvent::Load,
        }
    }

    /// Open the Actions panel targeting the selected MovieClip or Button instance (`T-190`).
    /// Does NOT create an `ObjectScript`, keyframe, Undo entry, or dirty state when handlers are empty.
    pub fn open_selected_object_actions(&mut self) -> bool {
        let Some((_target, element, kind)) = self.selected_actionable_instance() else {
            return false;
        };
        self.set_playing(false);
        self.script_target = ScriptTarget::Object;
        self.script_event =
            Self::default_object_event_for_instance(&element, kind, self.script_event);
        self.show_panel(crate::workspace::PanelId::Actions);
        true
    }

    /// Open Actions (`F9`) for the current context (`T-190`):
    /// - If a single eligible MovieClip/Button instance is selected (and the user hasn't focused a Project source with no selection),
    ///   target `ScriptTarget::Object` with the symbol-appropriate default event (`Click` for Button, `Load` for MovieClip).
    /// - Otherwise target the current Frame (`ScriptTarget::Frame`) or preserve an explicitly focused `ScriptTarget::Source`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn open_actions_for_current_context(&mut self) {
        if self.can_open_selected_object_actions() {
            self.open_selected_object_actions();
            return;
        }
        self.set_playing(false);
        if !(self.script_target == ScriptTarget::Source && self.selected_source.is_some()) {
            self.script_target = ScriptTarget::Frame;
        }
        self.show_panel(crate::workspace::PanelId::Actions);
    }

    /// Read the 3D authoring state of `element_id` (`T-186`).
    pub fn element_3d_state(&self, element_id: &str) -> Stage3dAuthoringState {
        let default_vp = [
            f64::from(self.project.width) * 0.5,
            f64::from(self.project.height) * 0.5,
        ];
        let sessions = MOVIE_CLIP_3D_SESSIONS.lock().unwrap();
        sessions
            .get(&self.clipboard.session)
            .and_then(|s| s.states.get(element_id).cloned())
            .unwrap_or(Stage3dAuthoringState {
                vanishing_point: default_vp,
                ..Stage3dAuthoringState::default()
            })
    }

    /// Apply `state` to `element_id` on the current keyframe and commit as one Undo step (`T-186`).
    pub fn set_element_3d_state(&mut self, element_id: &str, state: Stage3dAuthoringState) -> bool {
        if !self.layer_editable() {
            return false;
        }
        self.finish_edits();
        let stage_w = f64::from(self.project.width);
        let focal = state.focal_length(stage_w);
        let pose = state.to_display_pose();
        let vp = state.vanishing_point;

        // Capture or retrieve baseline unprojected element so repeated 3D edits do not accumulate drift.
        let base_el = {
            let mut sessions = MOVIE_CLIP_3D_SESSIONS.lock().unwrap();
            let sess = sessions.entry(self.clipboard.session.clone()).or_default();
            if let Some(base) = sess.base_elements.get(element_id).cloned() {
                base
            } else {
                let Some(found) = self.find_current_frame_element(element_id) else {
                    return false;
                };
                sess.base_elements
                    .insert(element_id.to_owned(), found.clone());
                found
            }
        };

        let projected = project_element_3d(&base_el, &pose, focal, vp);
        let mut candidate = self.project.clone();
        let sym_id = self.symbol_id().to_owned();
        let frame = self.frame;
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return false;
        };
        let mut updated = false;
        let blocked_layers: std::collections::HashSet<Id> = sym
            .timeline
            .layers
            .iter()
            .filter(|layer| {
                layer_effectively_locked(&sym.timeline, layer)
                    || !layer_effectively_visible(&sym.timeline, layer)
            })
            .map(|layer| layer.id.clone())
            .collect();
        for layer in &mut sym.timeline.layers {
            if blocked_layers.contains(&layer.id) {
                continue;
            }
            if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == frame)
                && let Some(slot) = key.elements.iter_mut().find(|e| e.id == element_id)
            {
                *slot = projected.clone();
                updated = true;
                break;
            }
        }
        if !updated {
            return false;
        }
        candidate.upgrade_format_for_features();
        if candidate.validate().is_err() {
            return false;
        }
        self.transact_named("Transform 3D", |editor| {
            editor.project = candidate;
        });
        let mut sessions = MOVIE_CLIP_3D_SESSIONS.lock().unwrap();
        let sess = sessions.entry(self.clipboard.session.clone()).or_default();
        sess.states.insert(element_id.to_owned(), state);
        true
    }

    /// Drag a 3D Stage gizmo handle on `element_id` by `(dx, dy)` in Stage coordinates and commit as one Undo step (`T-186`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn stage_drag_3d_gizmo(
        &mut self,
        element_id: &str,
        handle: Gizmo3dHandle,
        dx: f64,
        dy: f64,
    ) -> bool {
        let mut state = self.element_3d_state(element_id);
        state.apply_handle_drag(handle, dx, dy);
        self.set_element_3d_state(element_id, state)
    }

    pub(crate) fn find_current_frame_element(&self, element_id: &str) -> Option<Element> {
        let sym = self.symbol();
        for layer in &sym.timeline.layers {
            for el in sample_layer_in_at_fps(
                &sym.timeline,
                layer,
                self.frame,
                f64::from(self.project.fps),
            ) {
                if el.id == element_id {
                    return Some(el);
                }
            }
        }
        None
    }

    /// Compute the screen-space geometry of the 3D Stage gizmo for the currently selected element (`T-186`).
    pub(crate) fn selected_3d_gizmo_screen_geometry(
        &self,
        origin: Pos2,
        mapping: StageMapping,
    ) -> Option<(SelectionTarget, Element, Gizmo3dScreenGeometry)> {
        if self.selection.targets.len() != 1 {
            return None;
        }
        let target = self.selection.targets[0].clone();
        if !self.target_editable(&target) {
            return None;
        }
        let element = self.selected_sampled()?;
        let state = self.element_3d_state(&element.id);
        let center_local = Pos2::new(
            (element.transform.x + state.translate_3d[0] * 0.15) as f32,
            (element.transform.y + state.translate_3d[1] * 0.15) as f32,
        );
        let center = mapping.edit_to_screen(origin, self.zoom, center_local);
        let axis_len = 56.0_f32;
        let rot_z_rad = (element.transform.rotation + state.rotate_3d[2]).to_radians() as f32;
        let (sz, cz) = rot_z_rad.sin_cos();
        let x_tip = center + Vec2::new(cz * axis_len, sz * axis_len);
        let y_tip = center + Vec2::new(-sz * axis_len, cz * axis_len);
        let z_tip = center + Vec2::new(-axis_len * 0.55, -axis_len * 0.55);
        let vp_local = Pos2::new(
            state.vanishing_point[0] as f32,
            state.vanishing_point[1] as f32,
        );
        let vanishing_point = mapping.edit_to_screen(origin, self.zoom, vp_local);
        let perspective_handle =
            vanishing_point + Vec2::new(state.perspective_angle as f32 * 0.6, -24.0);
        Some((
            target,
            element,
            Gizmo3dScreenGeometry {
                center,
                x_tip,
                y_tip,
                z_tip,
                vanishing_point,
                perspective_handle,
                ring_radius: axis_len * 0.85,
            },
        ))
    }

    /// Paint the 3D Translation / 3D Rotation gizmo and handle live Stage drag (`T-186`).
    pub(crate) fn stage_3d_gizmo_step(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        rotate_mode: bool,
    ) -> bool {
        let Some((target, element, geom)) = self.selected_3d_gizmo_screen_geometry(origin, mapping)
        else {
            return false;
        };
        let x_col = Color32::from_rgb(230, 68, 68);
        let y_col = Color32::from_rgb(56, 196, 98);
        let z_col = Color32::from_rgb(66, 138, 245);
        let vp_col = Color32::from_rgb(245, 178, 52);

        painter.circle_stroke(
            geom.center,
            geom.ring_radius,
            Stroke::new(
                1.5_f32,
                if rotate_mode {
                    z_col
                } else {
                    z_col.gamma_multiply(0.45)
                },
            ),
        );
        painter.line_segment([geom.center, geom.x_tip], Stroke::new(2.0_f32, x_col));
        painter.circle_filled(geom.x_tip, 4.5, x_col);
        painter.line_segment([geom.center, geom.y_tip], Stroke::new(2.0_f32, y_col));
        painter.circle_filled(geom.y_tip, 4.5, y_col);
        painter.line_segment([geom.center, geom.z_tip], Stroke::new(2.0_f32, z_col));
        painter.circle_filled(geom.z_tip, 4.5, z_col);

        // Vanishing point crosshair and perspective angle handle.
        painter.line_segment(
            [
                geom.vanishing_point - Vec2::new(6.0, 0.0),
                geom.vanishing_point + Vec2::new(6.0, 0.0),
            ],
            Stroke::new(1.5_f32, vp_col),
        );
        painter.line_segment(
            [
                geom.vanishing_point - Vec2::new(0.0, 6.0),
                geom.vanishing_point + Vec2::new(0.0, 6.0),
            ],
            Stroke::new(1.5_f32, vp_col),
        );
        painter.line_segment(
            [geom.vanishing_point, geom.perspective_handle],
            Stroke::new(1.0_f32, vp_col),
        );
        painter.circle_filled(geom.perspective_handle, 4.0, vp_col);

        let (pressed, down, released, pointer_pos) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos().or_else(|| i.pointer.latest_pos()),
            )
        });
        let Some(pos) = pointer_pos else {
            return false;
        };

        let session_key = self.clipboard.session.clone();
        if pressed && let Some(handle) = geom.hit_test(pos, rotate_mode) {
            self.finish_edits();
            self.ensure_key_for(&target.layer, false);
            let start_local = mapping.screen_to_edit(origin, self.zoom, pos);
            let start_state = self.element_3d_state(&element.id);
            let base_element = {
                let mut sessions = MOVIE_CLIP_3D_SESSIONS.lock().unwrap();
                let sess = sessions.entry(session_key.clone()).or_default();
                sess.base_elements
                    .entry(element.id.clone())
                    .or_insert_with(|| element.clone())
                    .clone()
            };
            let before_project = self.project.clone();
            let mut sessions = MOVIE_CLIP_3D_SESSIONS.lock().unwrap();
            let sess = sessions.entry(session_key).or_default();
            sess.active_drag = Some(Active3dDrag {
                target,
                element_id: element.id,
                handle,
                start_local,
                start_state,
                base_element,
                before_project,
            });
            return true;
        }

        let active_drag = {
            let sessions = MOVIE_CLIP_3D_SESSIONS.lock().unwrap();
            sessions
                .get(&session_key)
                .and_then(|s| s.active_drag.clone())
        };
        let Some(drag) = active_drag else {
            return false;
        };

        if down {
            let cur_local = mapping.screen_to_edit(origin, self.zoom, pos);
            let dx = f64::from(cur_local.x - drag.start_local.x);
            let dy = f64::from(cur_local.y - drag.start_local.y);
            let mut preview_state = drag.start_state.clone();
            preview_state.apply_handle_drag(drag.handle, dx, dy);
            let focal = preview_state.focal_length(f64::from(self.project.width));
            let projected = project_element_3d(
                &drag.base_element,
                &preview_state.to_display_pose(),
                focal,
                preview_state.vanishing_point,
            );
            let sym_id = self.symbol_id().to_owned();
            let frame = self.frame;
            if let Some(sym) = self.project.symbol_mut(&sym_id)
                && let Some(layer) = sym
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == drag.target.layer)
                && let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == frame)
                && let Some(slot) = key.elements.iter_mut().find(|e| e.id == drag.element_id)
            {
                *slot = projected;
            }
            let mut sessions = MOVIE_CLIP_3D_SESSIONS.lock().unwrap();
            let sess = sessions.entry(session_key).or_default();
            sess.states.insert(drag.element_id, preview_state);
            return true;
        }

        if released || !down {
            let cur_local = mapping.screen_to_edit(origin, self.zoom, pos);
            let dx = f64::from(cur_local.x - drag.start_local.x);
            let dy = f64::from(cur_local.y - drag.start_local.y);
            // Restore pre-drag project before committing one atomic Undo step.
            self.project = drag.before_project.clone();
            {
                let mut sessions = MOVIE_CLIP_3D_SESSIONS.lock().unwrap();
                if let Some(sess) = sessions.get_mut(&session_key) {
                    sess.active_drag = None;
                }
            }
            let mut final_state = drag.start_state;
            final_state.apply_handle_drag(drag.handle, dx, dy);
            self.set_element_3d_state(&drag.element_id, final_state);
            return true;
        }
        false
    }

    /// Render the promoted 3D Position and View / Bone inspector section for a Movie Clip or active IK bone (`T-186`).
    #[allow(dead_code)]
    pub(crate) fn movie_clip_3d_and_bone_inspector_ui(&mut self, ui: &mut egui::Ui) {
        let Some(element) = self.selected_sampled() else {
            return;
        };
        let p = neoflash_ui::palette(ui.ctx());
        let mut state = self.element_3d_state(&element.id);
        let before = state.clone();
        nf::SectionHeader::new("3D Position and View")
            .id_salt("mc-3d-position-view")
            .default_open(true)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = space::S2;
                let w = ((ui.available_width() - space::S2 * 2.0) / 3.0).max(52.0);
                ui.horizontal(|ui| {
                    ui.add(
                        nf::Field::new(&mut state.translate_3d[0], "3D X")
                            .prefix("X")
                            .decimals(1)
                            .width(w),
                    );
                    ui.add(
                        nf::Field::new(&mut state.translate_3d[1], "3D Y")
                            .prefix("Y")
                            .decimals(1)
                            .width(w),
                    );
                    ui.add(
                        nf::Field::new(&mut state.translate_3d[2], "3D Z")
                            .prefix("Z")
                            .decimals(1)
                            .width(w),
                    );
                });
                ui.horizontal(|ui| {
                    ui.add(
                        nf::Field::new(&mut state.rotate_3d[0], "Rotate X")
                            .prefix("RX")
                            .decimals(1)
                            .width(w),
                    );
                    ui.add(
                        nf::Field::new(&mut state.rotate_3d[1], "Rotate Y")
                            .prefix("RY")
                            .decimals(1)
                            .width(w),
                    );
                    ui.add(
                        nf::Field::new(&mut state.rotate_3d[2], "Rotate Z")
                            .prefix("RZ")
                            .decimals(1)
                            .width(w),
                    );
                });
                let pair_w = ((ui.available_width() - space::S2) * 0.5).max(72.0);
                ui.horizontal(|ui| {
                    ui.add(
                        nf::Field::new(&mut state.perspective_angle, "Perspective Angle")
                            .prefix("FOV")
                            .range(5.0..=170.0)
                            .decimals(1)
                            .width(pair_w),
                    );
                    if ui.add(nf::Button::ghost("Reset 3D")).clicked() {
                        state = Stage3dAuthoringState {
                            vanishing_point: [
                                f64::from(self.project.width) * 0.5,
                                f64::from(self.project.height) * 0.5,
                            ],
                            ..Stage3dAuthoringState::default()
                        };
                    }
                });
                ui.label(rich("Vanishing Point", TextStyle::Small).color(p.text_secondary));
                ui.horizontal(|ui| {
                    ui.add(
                        nf::Field::new(&mut state.vanishing_point[0], "Vanishing Point X")
                            .prefix("VX")
                            .decimals(1)
                            .width(pair_w),
                    );
                    ui.add(
                        nf::Field::new(&mut state.vanishing_point[1], "Vanishing Point Y")
                            .prefix("VY")
                            .decimals(1)
                            .width(pair_w),
                    );
                });
            });
        if state != before {
            self.set_element_3d_state(&element.id, state);
        }
    }
}
