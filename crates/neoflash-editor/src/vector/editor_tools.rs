use super::cache_paint::{PEN_CLOSE_RADIUS, PathPaint, paint_path};
use super::handles_recognition::{
    EndpointSide, OpenEndpointMatch, RecognisedShape, collapse_anchor_handles, handles,
    map_commands_inverse, move_handle, move_selected_anchors, open_contour_endpoints,
    polyline_length, pull_anchor_tangent, recognise_primitive_shape, recognise_straight_line,
    reverse_open_contour, set_contour_end, set_contour_start, simplify,
};
use super::smooth_reshape::{
    delete_anchors_from_path, editable_element_commands, path_segment_hit_local,
    smooth_path_commands, straighten_path_commands, write_editable_element_commands,
};
use super::{Anchor, PathDraft, PathHandle, PointDrag, PointRole, smooth_pencil_tangents};
use crate::{
    app::{Editor, Tool},
    stage_mapping::StageMapping,
    theme::UiPalette,
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use neoflash_core::*;
use neoflash_ui::components::canvas as cv;

impl Editor {
    /// End only the active pointer phase before changing the viewport. Pen
    /// keeps its existing anchors/controls for the next press; Pencil commits
    /// samples already collected in the old view. A later move/release cannot
    /// append geometry until a new primary press starts another phase.
    pub(crate) fn end_vector_pointer_phase(&mut self) {
        let finish_pen_close = self
            .vector
            .draft
            .as_ref()
            .is_some_and(|draft| !draft.pencil && draft.closing && draft.anchors.len() >= 2);
        let finish_pencil = self.vector.draft.as_mut().is_some_and(|draft| {
            let active = draft.dragging || draft.closing;
            draft.dragging = false;
            draft.closing = false;
            draft.pencil && active
        });
        if finish_pen_close {
            self.finish_path(true);
        } else if finish_pencil {
            self.finish_path(false);
        }
    }

    pub(crate) fn finish_point_drag(&mut self) {
        if let Some(drag) = self.vector.point_drag.take()
            && drag.began
        {
            if drag.changed {
                self.commit_history();
            } else {
                self.history.cancel(&mut self.project);
            }
        }
    }
    pub(crate) fn finish_live_deform_drag(&mut self) {
        if let Some(drag) = self.vector.live_deform_drag.take()
            && !matches!(
                drag.kind,
                crate::vector::StageLiveDeformKind::IkBoneCreate { .. }
            )
        {
            if drag.changed {
                self.commit_history();
            } else {
                self.history.cancel(&mut self.project);
            }
        }
    }
    pub(crate) fn finish_edits(&mut self) {
        self.finish_shape_hint_drag(false);
        self.finish_motion_curve_drag();
        self.finish_motion_key_drag();
        self.finish_live_deform_drag();
        self.authoring_tools.cancel();
        self.finish_property_edit();
        self.finish_selection_gesture();
        self.finish_stage_text_typing();
        self.shape_draft = None;
        self.finish_point_drag();
        if self.vector.draft.is_some() {
            self.finish_path(false);
        }
    }
    pub(crate) fn cancel_vector(&mut self) {
        if self
            .vector
            .point_drag
            .as_ref()
            .is_some_and(|drag| drag.began)
        {
            self.history.cancel(&mut self.project);
        }
        if let Some(drag) = self.vector.live_deform_drag.take()
            && !matches!(
                drag.kind,
                crate::vector::StageLiveDeformKind::IkBoneCreate { .. }
            )
        {
            self.history.cancel(&mut self.project);
        }
        let ik_base_elements = std::mem::take(&mut self.vector.ik_base_elements);
        let asset_warp_base_elements = std::mem::take(&mut self.vector.asset_warp_base_elements);
        let free_transform_submode = self.vector.free_transform_submode;
        let envelope_lattice_session = self.vector.envelope_lattice_session.take();
        let perspective_quad_session = self.vector.perspective_quad_session.take();
        self.vector = crate::vector::VectorState {
            ik_base_elements,
            asset_warp_base_elements,
            free_transform_submode,
            envelope_lattice_session,
            perspective_quad_session,
            ..Default::default()
        };
    }
    pub(crate) fn cancel_edits(&mut self) {
        if self.has_standalone_swf_preview() {
            self.playing = false;
            self.reset_authored_preview();
        }
        self.cancel_shape_hint_drag();
        self.cancel_motion_drags();
        self.authoring_tools.cancel();
        self.finish_property_edit();
        self.cancel_selection_gesture();
        self.exit_stage_text_edit();
        self.cancel_vector();
        self.shape_draft = None;
        self.timeline_key_drag = None;
        self.scale9_guide_drag = None;
        self.cancel_guide_drag();
    }
    pub(crate) fn set_playing(&mut self, playing: bool) {
        self.finish_edits();
        if playing && !self.playing {
            self.rewind_if_finished();
        }
        self.timeline_key_drag = None;
        self.playing = playing;
        self.accumulator = 0.;
        self.sync_authored_preview();
        self.last_tick = std::time::Instant::now();
    }
    pub(crate) fn set_tool(&mut self, tool: Tool) {
        if tool == self.tool
            && tool == Tool::Pen
            && self
                .vector
                .draft
                .as_ref()
                .is_some_and(|draft| draft.anchors.len() >= 2)
        {
            self.finish_path(false);
            return;
        }
        if tool != self.tool {
            if !self.playing && self.has_standalone_swf_preview() {
                self.reset_authored_preview();
            }
            self.finish_edits();
            if tool != Tool::Text {
                self.exit_stage_text_edit();
            }
            self.timeline_key_drag = None;
            let ik_base_elements = std::mem::take(&mut self.vector.ik_base_elements);
            let asset_warp_base_elements =
                std::mem::take(&mut self.vector.asset_warp_base_elements);
            let free_transform_submode = self.vector.free_transform_submode;
            let envelope_lattice_session = self.vector.envelope_lattice_session.take();
            let perspective_quad_session = self.vector.perspective_quad_session.take();
            let selected_warp_pin = self.vector.selected_warp_pin.take();
            let selected_ik_bone = self.vector.selected_ik_bone.take();
            self.vector = crate::vector::VectorState {
                ik_base_elements,
                asset_warp_base_elements,
                free_transform_submode,
                envelope_lattice_session,
                perspective_quad_session,
                selected_warp_pin,
                selected_ik_bone,
                ..Default::default()
            };
            if self.authoring_tools.primitive_drawn_selection && tool != Tool::Select {
                self.clear_selection();
            }
            self.authoring_tools.primitive_drawn_selection = false;
        }
        self.tool = tool;
        if self.selection.is_empty()
            && !matches!(tool, Tool::Select | Tool::Razor | Tool::Hand | Tool::Zoom)
        {
            self.timeline_edit.focused = false;
        }
        let sticky = match tool {
            Tool::Deco => crate::stage::properties_tool::StageStickyToolMode::Deco,
            Tool::SprayBrush => crate::stage::properties_tool::StageStickyToolMode::SprayBrush,
            Tool::Bone => crate::stage::properties_tool::StageStickyToolMode::Bone,
            Tool::Bind => crate::stage::properties_tool::StageStickyToolMode::Bind,
            Tool::AssetWarp => crate::stage::properties_tool::StageStickyToolMode::PuppetWarp,
            Tool::Translate3D => crate::stage::properties_tool::StageStickyToolMode::Translate3D,
            Tool::Rotate3D => crate::stage::properties_tool::StageStickyToolMode::Rotate3D,
            _ => crate::stage::properties_tool::StageStickyToolMode::None,
        };
        self.set_stage_sticky_tool_mode(sticky);
    }
    fn find_nearest_open_endpoint(&self, pt: Pos2, tol: f32) -> Option<OpenEndpointMatch> {
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let sampled = sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps));
        let no_gradients = self.authoring_modify.drawing_fill_gradient.is_none()
            && self.authoring_modify.drawing_stroke_gradient.is_none();
        let mut best: Option<(f32, OpenEndpointMatch)> = None;
        for el in sampled.iter().rev() {
            let ElementKind::Path {
                commands,
                fill,
                stroke,
                stroke_width,
                ..
            } = &el.kind
            else {
                continue;
            };
            let Some((local_start, local_end)) = open_contour_endpoints(commands) else {
                continue;
            };
            let matrix = transform_matrix(el.transform);
            let same_style = no_gradients
                && fill[3] == 0
                && *stroke == self.stroke
                && (*stroke_width - self.stroke_width).abs() < 1e-6;
            for (side, local_pt) in [
                (EndpointSide::Start, local_start),
                (EndpointSide::End, local_end),
            ] {
                let world_point =
                    crate::stage::affine(matrix, Pos2::new(local_pt.0 as f32, local_pt.1 as f32));
                let dist = world_point.distance(pt);
                if dist <= tol && best.as_ref().is_none_or(|(b, _)| dist < *b) {
                    best = Some((
                        dist,
                        OpenEndpointMatch {
                            element_id: el.id.clone(),
                            side,
                            world_point,
                            matrix,
                            same_style,
                        },
                    ));
                }
            }
        }
        best.map(|(_, m)| m)
    }

    fn build_joined_open_path(
        &self,
        new_world_commands: &[PathCommand],
        start_match: Option<&OpenEndpointMatch>,
        end_match: Option<&OpenEndpointMatch>,
    ) -> Option<(Id, Vec<PathCommand>, bool)> {
        let (primary, is_start_primary) = match (start_match, end_match) {
            (Some(s), _) if s.same_style => (s, true),
            (_, Some(e)) if e.same_style => (e, false),
            _ => return None,
        };
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let sampled = sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps));
        let existing = sampled.iter().find(|e| e.id == primary.element_id)?;
        let ElementKind::Path {
            commands: existing_cmds,
            ..
        } = &existing.kind
        else {
            return None;
        };
        let (existing_start, existing_end) = open_contour_endpoints(existing_cmds)?;
        let local_new = map_commands_inverse(new_world_commands, primary.matrix)?;
        if local_new.len() < 2 {
            return None;
        }
        let other_closes_same = if is_start_primary {
            end_match.is_some_and(|e| e.element_id == primary.element_id && e.side != primary.side)
        } else {
            start_match
                .is_some_and(|s| s.element_id == primary.element_id && s.side != primary.side)
        };
        let has_curve = existing_cmds.iter().chain(local_new.iter()).any(|c| {
            matches!(
                c,
                PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
            )
        });
        let can_close =
            other_closes_same && (has_curve || existing_cmds.len() + local_new.len() >= 5);

        let mut merged = Vec::with_capacity(existing_cmds.len() + local_new.len());
        let mut closed_loop = false;
        match (is_start_primary, primary.side) {
            (true, EndpointSide::End) => {
                // Existing start..end + new start..end
                let mut tail = local_new;
                set_contour_start(&mut tail, existing_end);
                if can_close {
                    set_contour_end(&mut tail, existing_start);
                }
                merged.extend_from_slice(existing_cmds);
                merged.extend_from_slice(&tail[1..]);
                if can_close {
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                }
            }
            (false, EndpointSide::End) => {
                // Existing start..end + reversed(new)
                let mut rev = reverse_open_contour(&local_new)?;
                set_contour_start(&mut rev, existing_end);
                if can_close {
                    set_contour_end(&mut rev, existing_start);
                }
                merged.extend_from_slice(existing_cmds);
                merged.extend_from_slice(&rev[1..]);
                if can_close {
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                }
            }
            (false, EndpointSide::Start) => {
                // New start..end + existing start..end
                let mut head = local_new;
                set_contour_end(&mut head, existing_start);
                if can_close {
                    let head_start = open_contour_endpoints(&head)?.0;
                    let mut rest = existing_cmds.clone();
                    set_contour_end(&mut rest, head_start);
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&rest[1..]);
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                } else {
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&existing_cmds[1..]);
                }
            }
            (true, EndpointSide::Start) => {
                // Reversed(new) + existing start..end
                let mut head = reverse_open_contour(&local_new)?;
                set_contour_end(&mut head, existing_start);
                if can_close {
                    let head_start = open_contour_endpoints(&head)?.0;
                    let mut rest = existing_cmds.clone();
                    set_contour_end(&mut rest, head_start);
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&rest[1..]);
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                } else {
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&existing_cmds[1..]);
                }
            }
        }
        validate_path_commands(&merged).ok()?;
        Some((primary.element_id.clone(), merged, closed_loop))
    }

    pub(crate) fn finish_path(&mut self, closed: bool) {
        let Some(mut draft) = self.vector.draft.take() else {
            return;
        };
        if draft.limit_reached {
            self.message("Pencil stroke exceeds 16000 samples. Draw shorter strokes; this draft was discarded.");
            return;
        }
        if draft.anchors.len() < 2 {
            return;
        }
        let target = self.project.symbol(&draft.symbol).and_then(|symbol| {
            (draft.frame < symbol.timeline.frame_count)
                .then(|| {
                    symbol.timeline.layers.iter().position(|layer| {
                        layer.id == draft.layer_id
                            && layer.properties.role != LayerRole::Folder
                            && neoflash_core::layer_effectively_visible(&symbol.timeline, layer)
                            && !neoflash_core::layer_effectively_locked(&symbol.timeline, layer)
                            && (draft.symbol != self.symbol_id()
                                || self.is_layer_id_effectively_soloed(&layer.id))
                    })
                })
                .flatten()
        });
        let Some(target_layer) = target else {
            self.message("Cannot finish path: its original layer/frame is no longer editable. Draft cancelled.");
            return;
        };
        // Recognition/smoothing tolerances are screen-point preferences. Under a
        // non-uniform parent transform, use the largest scale so accepted local
        // distances never exceed the requested screen tolerance.
        let zoom =
            (draft.pointer_zoom as f64 * draft.pointer_mapping.scale_bounds().1).max(1e-3) as f32;
        let mut drawing = self.workspace.drawing.clone();
        if draft.pencil {
            use crate::{tool_preferences::PencilMode, workspace::Tolerance};
            if self.workspace.tools.pencil_smoothing == 0. {
                drawing.smooth_curves = Tolerance::Off;
            }
            match self.workspace.tools.pencil_mode {
                PencilMode::Straighten => {}
                PencilMode::Smooth => {
                    drawing.recognise_lines = Tolerance::Off;
                    drawing.recognise_shapes = Tolerance::Off;
                }
                PencilMode::Ink => {
                    drawing.recognise_lines = Tolerance::Off;
                    drawing.recognise_shapes = Tolerance::Off;
                    drawing.smooth_curves = Tolerance::Off;
                    drawing.connect_lines = Tolerance::Off;
                }
            }
        }
        let raw_anchors = draft.anchors.clone();

        // 1. Preferences ▸ Drawing ▸ Recognise shapes (Pencil only).
        let recognised_shape = if draft.pencil {
            recognise_primitive_shape(&raw_anchors, closed, zoom, drawing.recognise_shapes)
        } else {
            None
        };

        // 2. Preferences ▸ Drawing ▸ Connect lines (self-close check).
        let connect_tol = drawing.connect_lines.connect_lines_px().map(|px| px / zoom);
        let mut effective_closed = closed;
        let mut self_closed = false;
        if !effective_closed
            && recognised_shape.is_none()
            && let Some(tol) = connect_tol
        {
            let first = raw_anchors[0].point;
            let last = raw_anchors[raw_anchors.len() - 1].point;
            if raw_anchors.len() >= 3
                && polyline_length(&raw_anchors) >= tol * 3.0
                && first.distance(last) <= tol
            {
                effective_closed = true;
                self_closed = true;
            }
        }

        // 3. Preferences ▸ Drawing ▸ Recognise lines & Smooth curves (Pencil only).
        if draft.pencil && recognised_shape.is_none() {
            if !effective_closed
                && let Some([a, b]) =
                    recognise_straight_line(&raw_anchors, zoom, drawing.recognise_lines)
            {
                draft.anchors = vec![
                    Anchor {
                        point: a,
                        tangent: Vec2::ZERO,
                    },
                    Anchor {
                        point: b,
                        tangent: Vec2::ZERO,
                    },
                ];
            } else if let Some(rdp_px) = drawing.smooth_curves.smooth_rdp_px() {
                draft.anchors = simplify(
                    &draft.anchors,
                    rdp_px / zoom * (self.workspace.tools.pencil_smoothing as f32 / 50.),
                );
            }
        }

        if draft.pencil
            && self.workspace.tools.pencil_mode == crate::tool_preferences::PencilMode::Smooth
            && drawing.smooth_curves != crate::workspace::Tolerance::Off
            && recognised_shape.is_none()
        {
            smooth_pencil_tangents(&mut draft.anchors);
        }

        // If self-closing left a trailing point right on top of the start anchor, drop it so
        // `PathDraft::commands(true)` closes cleanly without a tiny hook segment, or cancel
        // self-close if simplification reduced the stroke to a 2-point out-and-back line.
        if self_closed && let Some(tol) = connect_tol {
            if draft.anchors.len() > 3
                && draft.anchors.last().is_some_and(|last| {
                    last.tangent.length_sq() <= 0.01
                        && last.point.distance(draft.anchors[0].point) <= tol
                })
            {
                draft.anchors.pop();
            } else if draft.anchors.len() == 3
                && draft.anchors[2].tangent.length_sq() <= 0.01
                && draft.anchors[2].point.distance(draft.anchors[0].point) <= tol
            {
                effective_closed = false;
            }
        }
        if draft.anchors.len() < 2 && recognised_shape.is_none() {
            return;
        }

        if let Some(shape) = recognised_shape {
            let (kind, x, y, msg) = match shape {
                RecognisedShape::Rectangle {
                    x,
                    y,
                    width,
                    height,
                } => (
                    ElementKind::Rectangle {
                        width,
                        height,
                        fill: self.fill,
                        stroke: self.stroke,
                        stroke_width: self.stroke_width,
                    },
                    x,
                    y,
                    "Rectangle recognised.",
                ),
                RecognisedShape::Ellipse {
                    x,
                    y,
                    width,
                    height,
                } => (
                    ElementKind::Ellipse {
                        width,
                        height,
                        fill: self.fill,
                        stroke: self.stroke,
                        stroke_width: self.stroke_width,
                    },
                    x,
                    y,
                    "Oval recognised.",
                ),
            };
            if draft.symbol == self.symbol_id()
                && draft.frame == self.frame
                && target_layer == self.layer
            {
                self.add_element(kind, x, y);
            } else {
                self.with_temporary_symbol_target(
                    draft.symbol.clone(),
                    draft.frame,
                    target_layer,
                    |editor| editor.add_element(kind, x, y),
                );
            }
            self.message(msg);
            return;
        }

        let draft_symbol = draft.symbol.clone();
        let draft_frame = draft.frame;
        let live_target = draft_symbol == self.symbol_id()
            && draft_frame == self.frame
            && target_layer == self.layer;
        let mut finish = |editor: &mut Editor| {
            // 4. Preferences ▸ Drawing ▸ Connect lines (snap / join open path endpoints on
            // the original layer even if the playhead or edit session has since moved).
            let mut joined = None;
            if !effective_closed && let Some(tol) = connect_tol {
                let start_pt = draft.anchors[0].point;
                let end_pt = draft.anchors[draft.anchors.len() - 1].point;
                let start_match = editor.find_nearest_open_endpoint(start_pt, tol);
                let end_match = editor.find_nearest_open_endpoint(end_pt, tol).filter(|e| {
                    start_match
                        .as_ref()
                        .is_none_or(|s| s.element_id != e.element_id || s.side != e.side)
                });
                if let Some(ref s) = start_match
                    && (draft.anchors.len() > 2 || s.world_point.distance(end_pt) > 1e-3)
                {
                    draft.anchors[0].point = s.world_point;
                }
                if let Some(ref e) = end_match {
                    let current_start = draft.anchors[0].point;
                    if (draft.anchors.len() > 2 || e.world_point.distance(current_start) > 1e-3)
                        && let Some(last) = draft.anchors.last_mut()
                    {
                        last.point = e.world_point;
                    }
                }
                if drawing.object_drawing {
                    let world_cmds = draft.commands(false);
                    joined = editor.build_joined_open_path(
                        &world_cmds,
                        start_match.as_ref(),
                        end_match.as_ref(),
                    );
                }
            }

            if let Some((target_id, merged_commands, closed_loop)) = joined {
                let join_fill = editor.fill;
                editor.transact_named("Path", |s| {
                    s.ensure_key(false);
                    let layer = s.layer;
                    let frame = s.frame;
                    if let Some(el) = s.symbol_mut().timeline.layers[layer]
                        .key_at_mut(frame)
                        .and_then(|k| k.elements.iter_mut().find(|e| e.id == target_id))
                        && let ElementKind::Path { commands, fill, .. } = &mut el.kind
                    {
                        *commands = merged_commands.clone();
                        if closed_loop {
                            *fill = join_fill;
                        }
                    }
                    s.select_single_id(target_id.clone());
                });
                closed_loop
            } else {
                let commands = draft.commands(effective_closed);
                editor.add_element(
                    ElementKind::Path {
                        commands,
                        fill_rule: FillRule::NonZero,
                        fill: if effective_closed {
                            editor.fill
                        } else {
                            [0, 0, 0, 0]
                        },
                        stroke: editor.stroke,
                        stroke_width: editor.stroke_width,
                    },
                    0.,
                    0.,
                );
                effective_closed
            }
        };
        let final_closed = if live_target {
            finish(self)
        } else {
            self.with_temporary_symbol_target(draft_symbol, draft_frame, target_layer, finish)
        };
        self.message(if final_closed {
            "Closed vector path created."
        } else {
            "Open vector path created. Use A to edit its points."
        });
    }

    pub(crate) fn vector_stage(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        _page: Rect,
        response: &egui::Response,
    ) {
        let zoom = self.zoom;
        let local = |pos: Pos2| mapping.screen_to_edit(origin, zoom, pos);
        let screen = |pos: Pos2| mapping.edit_to_screen(origin, zoom, pos);
        if self.vector.point_drag.as_ref().is_some_and(|d| {
            !self.point_drag_matches(d)
                || d.origin != origin
                || d.zoom != self.zoom
                || d.mapping != mapping
        }) {
            self.cancel_vector();
        }
        if self.vector.draft.as_ref().is_some_and(|draft| {
            (draft.dragging || draft.closing)
                && (draft.pointer_origin != origin
                    || draft.pointer_zoom != zoom
                    || draft.pointer_mapping != mapping)
        }) {
            self.end_vector_pointer_phase();
        }
        if self.vector.draft.as_ref().is_some_and(|d| {
            d.symbol != self.symbol_id()
                || d.frame != self.frame
                || d.layer != self.layer
                || self.symbol().timeline.layers[self.layer].id != d.layer_id
                || (d.pencil && self.tool != Tool::Pencil)
                || (!d.pencil && self.tool != Tool::Pen)
        }) {
            self.finish_path(false);
        }
        let events = ui.input(|i| i.events.clone());
        let canvas_hover = ui.input(|i| i.pointer.hover_pos()).filter(|&pos| {
            response.rect.contains(pos)
                && ui
                    .ctx()
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer == response.layer_id)
        });
        if self.tool == Tool::Pen
            && self.vector.draft.is_none()
            && self.layer_editable()
            && let Some(hover_pos) = canvas_hover
        {
            let alt = ui.input(|i| i.modifiers.alt);
            if self
                .pen_anchor_candidate(origin, mapping, hover_pos, alt)
                .is_some()
            {
                self.stage_cursor = Some(if alt {
                    neoflash_ui::cursors::PEN_CONVERT
                } else {
                    neoflash_ui::cursors::PEN_REMOVE
                });
            } else if self
                .pen_subdivide_candidate(origin, mapping, hover_pos)
                .is_some()
            {
                self.stage_cursor = Some(neoflash_ui::cursors::PEN_ADD);
            }
        }
        if matches!(self.tool, Tool::Pen | Tool::Pencil) && self.layer_editable() {
            for event in &events {
                match *event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers,
                    } if response.rect.contains(pos)
                        && ui
                            .ctx()
                            .layer_id_at(pos)
                            .is_none_or(|layer| layer == response.layer_id) =>
                    {
                        self.playing = false;
                        if self.tool == Tool::Pen
                            && self.vector.draft.is_none()
                            && (self.try_pen_anchor_action(origin, mapping, pos, modifiers)
                                || self.try_pen_subdivide(origin, mapping, pos))
                        {
                            continue;
                        }
                        let closes = self.tool == Tool::Pen
                            && self.vector.draft.as_ref().is_some_and(|draft| {
                                draft.anchors.len() >= 2
                                    && draft.anchors.first().is_some_and(|first| {
                                        screen(first.point).distance(pos) < PEN_CLOSE_RADIUS
                                    })
                            });
                        if closes {
                            // Commit on press: a lost native release or viewport transition must
                            // not turn an explicit close click into another open anchor.
                            self.stage_view.snap_ring = None;
                            self.finish_path(true);
                            continue;
                        }
                        // Pen anchors snap to guides and grid (ST-04); pencil strokes are freehand.
                        let position = if self.tool == Tool::Pen {
                            local(self.snap_screen(mapping, origin, pos))
                        } else {
                            local(pos)
                        };
                        if self.tool == Tool::Pen
                            && (modifiers.command || modifiers.ctrl)
                            && self
                                .vector
                                .draft
                                .as_ref()
                                .is_some_and(|draft| draft.anchors.len() >= 2)
                        {
                            self.finish_path(false);
                            continue;
                        }
                        let symbol = self.symbol_id().to_owned();
                        let layer_id = self.symbol().timeline.layers[self.layer].id.clone();
                        let draft = self.vector.draft.get_or_insert_with(|| PathDraft {
                            anchors: vec![],
                            pencil: self.tool == Tool::Pencil,
                            dragging: false,
                            closing: false,
                            limit_reached: false,
                            symbol,
                            frame: self.frame,
                            layer: self.layer,
                            layer_id,
                            pointer_origin: origin,
                            pointer_zoom: zoom,
                            pointer_mapping: mapping,
                        });
                        draft.pointer_origin = origin;
                        draft.pointer_zoom = zoom;
                        draft.pointer_mapping = mapping;
                        draft.anchors.push(Anchor {
                            point: position,
                            tangent: Vec2::ZERO,
                        });
                        draft.dragging = true;
                    }
                    egui::Event::PointerMoved(pos) => {
                        if let Some(draft) = &mut self.vector.draft
                            && draft.dragging
                        {
                            if draft.pencil {
                                if draft.anchors.len() >= 16_000 {
                                    draft.limit_reached = true;
                                }
                                if draft.anchors.len() < 16_000
                                    && draft
                                        .anchors
                                        .last()
                                        .is_some_and(|a| screen(a.point).distance(pos) >= 1.5)
                                {
                                    draft.anchors.push(Anchor {
                                        point: local(pos),
                                        tangent: Vec2::ZERO,
                                    });
                                }
                            } else if let Some(anchor) = draft.anchors.last_mut() {
                                anchor.tangent = local(pos) - anchor.point;
                            }
                        }
                    }
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        ..
                    } => {
                        let mut finish = None;
                        if let Some(draft) = &mut self.vector.draft {
                            if draft.closing {
                                finish = Some(true);
                            } else if draft.dragging && draft.pencil {
                                if draft
                                    .anchors
                                    .last()
                                    .is_some_and(|a| a.point.distance(local(pos)) > 0.1)
                                {
                                    draft.anchors.push(Anchor {
                                        point: local(pos),
                                        tangent: Vec2::ZERO,
                                    });
                                }
                                finish = Some(false);
                            } else if draft.dragging
                                && let Some(anchor) = draft.anchors.last_mut()
                            {
                                // Release position is authoritative even if no
                                // separate PointerMoved event was delivered.
                                anchor.tangent = local(pos) - anchor.point;
                            }
                            draft.dragging = false;
                        }
                        if let Some(closed) = finish {
                            self.finish_path(closed);
                        }
                    }
                    _ => {}
                }
            }
        }
        if self.tool == Tool::Subselect
            || (self.tool == Tool::Pen && self.vector.point_drag.is_some())
        {
            self.edit_path_points(ui, p, origin, mapping, response);
        }
        if let Some(draft) = &self.vector.draft {
            let blue = UiPalette::current(p.ctx()).overlay;
            paint_path(
                p,
                &draft.commands(false),
                PathPaint {
                    rule: FillRule::NonZero,
                    fill: Color32::TRANSPARENT,
                    stroke: blue,
                    width: 1.5 / (zoom as f64 * mapping.scale_bounds().1.max(1e-6)),
                    tolerance: 0.2,
                    fingerprint: None,
                },
                screen,
            );
            // `NF2/Anchor Point` + `NF2/Bezier Handle`; the anchor being placed is the selected one,
            // the first anchor shows Hover while a click there would close the path.
            let hover = response.hover_pos();
            let closes = |at: Pos2| {
                !draft.pencil
                    && draft.anchors.len() >= 2
                    && hover.is_some_and(|pos| at.distance(pos) < PEN_CLOSE_RADIUS)
            };
            if !draft.pencil {
                let last = draft.anchors.len().saturating_sub(1);
                for (i, anchor) in draft.anchors.iter().enumerate() {
                    let at = screen(anchor.point);
                    if anchor.tangent.length_sq() > 0.1 {
                        for handle in [anchor.point + anchor.tangent, anchor.point - anchor.tangent]
                        {
                            cv::paint_bezier_handle(p, p.ctx(), at, screen(handle));
                        }
                    }
                    let state = if i == last {
                        cv::AnchorState::Selected
                    } else if i == 0 && closes(at) {
                        cv::AnchorState::Hover
                    } else {
                        cv::AnchorState::Unselected
                    };
                    cv::paint_anchor(p, p.ctx(), at, state);
                }
                if let Some(first) = draft.anchors.first()
                    && closes(screen(first.point))
                {
                    self.stage_cursor = Some(neoflash_ui::cursors::PEN_CLOSE);
                }
            }
            if !draft.pencil
                && self.workspace.drawing.pen_preview
                && let (Some(last), Some(hover)) = (draft.anchors.last(), response.hover_pos())
                && !draft.dragging
            {
                p.line_segment(
                    [screen(last.point), hover],
                    Stroke::new(1.0_f32, blue.gamma_multiply(0.5)),
                );
            }
        }
    }

    fn edit_path_points(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        response: &egui::Response,
    ) {
        let Some(element) = self.selected_sampled() else {
            return;
        };
        let Some(commands) = editable_element_commands(&element) else {
            return;
        };
        let matrix = transform_matrix(element.transform);
        let zoom = self.zoom;
        let screen =
            |pos: Pos2| mapping.edit_to_screen(origin, zoom, crate::stage::affine(matrix, pos));
        let handles = handles(&commands);
        // `NF2/Bezier Handle` from each anchor to its control points, then `NF2/Anchor Point`s.
        let mut previous = Pos2::ZERO;
        for command in &commands {
            match *command {
                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                    previous = Pos2::new(x as f32, y as f32)
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let control = screen(Pos2::new(cx as f32, cy as f32));
                    let end = Pos2::new(x as f32, y as f32);
                    cv::paint_bezier_handle(p, p.ctx(), screen(previous), control);
                    cv::paint_bezier_handle(p, p.ctx(), screen(end), control);
                    previous = end;
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let end = Pos2::new(x as f32, y as f32);
                    let first = screen(Pos2::new(cx1 as f32, cy1 as f32));
                    let second = screen(Pos2::new(cx2 as f32, cy2 as f32));
                    cv::paint_bezier_handle(p, p.ctx(), screen(previous), first);
                    cv::paint_bezier_handle(p, p.ctx(), screen(end), second);
                    previous = end;
                }
                PathCommand::Close => {}
            }
        }
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|&pos| {
            self.vector.point_drag.is_none()
                && response.rect.contains(pos)
                && ui
                    .ctx()
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer == response.layer_id)
        });
        let dragged = self
            .vector
            .point_drag
            .as_ref()
            .map(|drag| (drag.handle.command, drag.handle.role));
        let near = |at: Pos2| hover.is_some_and(|pos| at.distance(pos) < 8.);
        for handle in &handles {
            if handle.role == PointRole::Anchor {
                let at = screen(handle.point);
                let is_selected = dragged == Some((handle.command, handle.role))
                    || self.is_anchor_selected(&element.id, handle.command);
                let state = if is_selected {
                    cv::AnchorState::Selected
                } else if near(at) {
                    cv::AnchorState::Hover
                } else {
                    cv::AnchorState::Unselected
                };
                cv::paint_anchor(p, p.ctx(), at, state);
            }
        }
        let near_any_handle = handles.iter().any(|h| near(screen(h.point)));
        let alt_hover = ui.input(|i| i.modifiers.alt);
        if dragged.is_some() || near_any_handle {
            self.stage_cursor = Some(neoflash_ui::cursors::SUBSELECT_POINT);
        } else if alt_hover
            && let Some(hover_pos) = hover
            && path_segment_hit_local(&commands, matrix, mapping, origin, zoom, hover_pos).is_some()
        {
            self.stage_cursor = Some(neoflash_ui::cursors::PEN_ADD);
        }
        let double_clicked = response.double_clicked() && !self.stage_view.guide_pointer;
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                } if response.rect.contains(pos)
                    && self.selected_editable()
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == response.layer_id) =>
                {
                    if let Some(handle) = handles
                        .iter()
                        .rev()
                        .filter(|h| screen(h.point).distance(pos) < 8.)
                        .min_by(|a, b| {
                            screen(a.point)
                                .distance_sq(pos)
                                .total_cmp(&screen(b.point).distance_sq(pos))
                        })
                        .copied()
                    {
                        self.finish_edits();
                        self.playing = false;
                        if handle.role == PointRole::Anchor {
                            if modifiers.shift {
                                let selected =
                                    self.toggle_selected_anchor(&element.id, handle.command);
                                if !selected {
                                    continue;
                                }
                            } else if !self.is_anchor_selected(&element.id, handle.command) {
                                self.vector.selected_anchors =
                                    Some((element.id.clone(), vec![handle.command]));
                            }
                        }
                        self.vector.point_drag = Some(PointDrag {
                            handle,
                            original: commands.clone(),
                            inverse: matrix,
                            symbol: self.symbol_id().to_owned(),
                            frame: self.frame,
                            layer: self.selection.single().unwrap().layer.clone(),
                            origin,
                            zoom,
                            mapping,
                            press: pos,
                            began: false,
                            changed: false,
                            element: element.id.clone(),
                            pull_tangent: handle.role == PointRole::Anchor && modifiers.alt,
                        });
                    } else if (modifiers.alt || double_clicked)
                        && let Some((cmd_idx, t)) =
                            path_segment_hit_local(&commands, matrix, mapping, origin, zoom, pos)
                        && let Some((new_commands, inserted)) =
                            subdivide_path_at(&commands, cmd_idx, t)
                        && validate_path_commands(&new_commands).is_ok()
                    {
                        let element_id = element.id.clone();
                        self.finish_edits();
                        self.playing = false;
                        self.transact(|s| {
                            s.ensure_selected_key();
                            if let Some(el) = s.selected_element_mut() {
                                write_editable_element_commands(el, new_commands);
                            }
                        });
                        self.vector.selected_anchors = Some((element_id, vec![inserted]));
                        return;
                    } else if self.tool == Tool::Subselect {
                        self.finish_edits();
                        self.playing = false;
                        self.vector.subselect_marquee = Some((pos, pos));
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    if let Some((_, current)) = &mut self.vector.subselect_marquee {
                        *current = pos;
                    } else {
                        self.update_path_drag(pos, origin, mapping);
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers,
                } => {
                    if let Some((start, _)) = self.vector.subselect_marquee.take() {
                        if start.distance(pos) >= 3.0 {
                            let marquee = Rect::from_two_pos(start, pos);
                            let mut hit_indices: Vec<usize> = handles
                                .iter()
                                .filter(|h| {
                                    h.role == PointRole::Anchor && marquee.contains(screen(h.point))
                                })
                                .map(|h| h.command)
                                .collect();
                            hit_indices.sort_unstable();
                            hit_indices.dedup();
                            if modifiers.shift
                                && let Some((sel_id, existing)) = &mut self.vector.selected_anchors
                                && *sel_id == element.id
                            {
                                for idx in hit_indices {
                                    if !existing.contains(&idx) {
                                        existing.push(idx);
                                    }
                                }
                            } else {
                                self.vector.selected_anchors =
                                    Some((element.id.clone(), hit_indices));
                            }
                        }
                    } else {
                        self.update_path_drag(pos, origin, mapping);
                        self.finish_point_drag();
                    }
                }
                _ => {}
            }
        }
        if let Some((start, current)) = self.vector.subselect_marquee
            && start.distance(current) >= 3.0
        {
            cv::paint_marquee(p, p.ctx(), Rect::from_two_pos(start, current));
        }
    }
    fn update_path_drag(&mut self, pos: Pos2, origin: Pos2, mapping: StageMapping) {
        let Some(mut drag) = self.vector.point_drag.take() else {
            return;
        };
        if !self.point_drag_matches(&drag)
            || drag.origin != origin
            || drag.zoom != self.zoom
            || drag.mapping != mapping
        {
            if drag.began {
                self.history.cancel(&mut self.project);
            }
            return;
        }
        if !drag.began && pos.distance(drag.press) < 3. {
            self.vector.point_drag = Some(drag);
            return;
        }
        // The dragged anchor or control point snaps to guides and grid (ST-04).
        let anchor = crate::stage::affine(drag.inverse, drag.handle.point);
        let pos = self.snap_drag(drag.mapping, drag.origin, drag.press, pos, anchor);
        let local = |point| {
            crate::stage::inverse(
                drag.inverse,
                drag.mapping.screen_to_edit(drag.origin, drag.zoom, point),
            )
        };
        let (Some(current), Some(press)) = (local(pos), local(drag.press)) else {
            self.vector.point_drag = Some(drag);
            return;
        };
        let mut commands = drag.original.clone();
        if drag.pull_tangent && drag.handle.role == PointRole::Anchor {
            pull_anchor_tangent(
                &mut commands,
                drag.handle.command,
                current - drag.handle.point,
            );
        } else if drag.handle.role == PointRole::Anchor
            && let Some((sel_id, indices)) = &self.vector.selected_anchors
            && *sel_id == drag.element
            && indices.len() > 1
            && indices.contains(&drag.handle.command)
        {
            move_selected_anchors(&mut commands, indices, current - press);
        } else {
            move_handle(
                &mut commands,
                drag.handle,
                drag.handle.point + (current - press),
            );
        }
        drag.changed = commands != drag.original;
        if drag.changed && !drag.began {
            self.history.begin(&self.project);
            self.ensure_selected_key();
            drag.began = true;
        }
        if drag.began
            && let Some(el) = self.selected_element_mut()
        {
            write_editable_element_commands(el, commands);
        }
        self.vector.point_drag = Some(drag);
    }
    fn point_drag_matches(&self, drag: &PointDrag) -> bool {
        matches!(self.tool, Tool::Subselect | Tool::Pen)
            && self.symbol_id() == drag.symbol
            && self.frame == drag.frame
            && self
                .selection
                .single()
                .is_some_and(|target| target.layer == drag.layer && target.element == drag.element)
            && self.selected_editable()
    }

    pub(crate) fn is_anchor_selected(&self, element_id: &str, command: usize) -> bool {
        self.vector
            .selected_anchors
            .as_ref()
            .is_some_and(|(id, indices)| id == element_id && indices.contains(&command))
    }

    pub(crate) fn toggle_selected_anchor(&mut self, element_id: &str, command: usize) -> bool {
        match &mut self.vector.selected_anchors {
            Some((id, indices)) if id == element_id => {
                if let Some(pos) = indices.iter().position(|&i| i == command) {
                    indices.remove(pos);
                    false
                } else {
                    indices.push(command);
                    true
                }
            }
            _ => {
                self.vector.selected_anchors = Some((element_id.to_owned(), vec![command]));
                true
            }
        }
    }

    pub(crate) fn on_selected_path_segment(
        &self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
    ) -> bool {
        if !self.selected_editable() {
            return false;
        }
        let Some(element) = self.selected_sampled() else {
            return false;
        };
        let Some(commands) = editable_element_commands(&element) else {
            return false;
        };
        let matrix = transform_matrix(element.transform);
        let zoom = self.zoom;
        let screen =
            |pt: Pos2| mapping.edit_to_screen(origin, zoom, crate::stage::affine(matrix, pt));
        if handles(&commands)
            .iter()
            .any(|h| screen(h.point).distance(pos) < 8.)
        {
            return false;
        }
        path_segment_hit_local(&commands, matrix, mapping, origin, zoom, pos).is_some()
    }

    pub(crate) fn pen_anchor_candidate(
        &self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
        alt: bool,
    ) -> Option<(
        crate::selection::SelectionTarget,
        Element,
        Vec<PathCommand>,
        PathHandle,
    )> {
        let zoom = self.zoom;
        let try_element = |target: crate::selection::SelectionTarget, element: Element| {
            let commands = editable_element_commands(&element)?;
            let matrix = transform_matrix(element.transform);
            let screen =
                |pt: Pos2| mapping.edit_to_screen(origin, zoom, crate::stage::affine(matrix, pt));
            let handle = handles(&commands)
                .into_iter()
                .rev()
                .filter(|h| h.role == PointRole::Anchor && screen(h.point).distance(pos) < 8.)
                .min_by(|a, b| {
                    screen(a.point)
                        .distance_sq(pos)
                        .total_cmp(&screen(b.point).distance_sq(pos))
                })?;
            if !alt {
                let remaining = delete_anchors_from_path(&commands, &[handle.command]);
                if remaining.is_empty()
                    || remaining == commands
                    || validate_path_commands(&remaining).is_err()
                {
                    return None;
                }
            }
            Some((target, element, commands, handle))
        };

        if self.selected_editable()
            && let Some(target) = self.selection.single().cloned()
            && let Some(element) = self.selected_sampled()
            && let Some(found) = try_element(target, element)
        {
            return Some(found);
        }

        let timeline = &self.symbol().timeline;
        for layer in timeline.layers.iter().rev() {
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
                || !self.is_layer_id_effectively_soloed(&layer.id)
            {
                continue;
            }
            for element in sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps))
                .into_iter()
                .rev()
            {
                let target = crate::selection::SelectionTarget {
                    layer: layer.id.clone(),
                    element: element.id.clone(),
                };
                if let Some(found) = try_element(target, element) {
                    return Some(found);
                }
            }
        }
        None
    }

    pub(crate) fn try_pen_anchor_action(
        &mut self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
        modifiers: egui::Modifiers,
    ) -> bool {
        let Some((target, element, commands, handle)) =
            self.pen_anchor_candidate(origin, mapping, pos, modifiers.alt)
        else {
            return false;
        };
        let element_id = target.element.clone();
        self.finish_edits();
        self.playing = false;
        if modifiers.alt {
            let collapsed = collapse_anchor_handles(&commands, handle.command);
            let base_commands =
                if collapsed != commands && validate_path_commands(&collapsed).is_ok() {
                    let updated = collapsed.clone();
                    let target_clone = target.clone();
                    self.transact(|s| {
                        s.ensure_key_for(&target_clone.layer, false);
                        if let Some(layer_idx) = s.target_layer_index(&target_clone) {
                            let frame = s.frame;
                            if let Some(key) =
                                s.symbol_mut().timeline.layers[layer_idx].key_at_mut(frame)
                                && let Some(el) = key
                                    .elements
                                    .iter_mut()
                                    .find(|e| e.id == target_clone.element)
                            {
                                write_editable_element_commands(el, updated);
                            }
                        }
                        s.set_selection(vec![target_clone]);
                    });
                    collapsed
                } else {
                    self.set_selection(vec![target.clone()]);
                    commands
                };
            self.vector.selected_anchors = Some((element_id.clone(), vec![handle.command]));
            self.vector.point_drag = Some(PointDrag {
                handle,
                original: base_commands,
                inverse: transform_matrix(element.transform),
                symbol: self.symbol_id().to_owned(),
                frame: self.frame,
                layer: target.layer,
                origin,
                zoom: self.zoom,
                mapping,
                press: pos,
                began: false,
                changed: false,
                element: element_id,
                pull_tangent: true,
            });
            return true;
        }
        let remaining = delete_anchors_from_path(&commands, &[handle.command]);
        if remaining.is_empty()
            || remaining == commands
            || validate_path_commands(&remaining).is_err()
        {
            return false;
        }
        self.transact(|s| {
            s.ensure_key_for(&target.layer, false);
            if let Some(layer_idx) = s.target_layer_index(&target) {
                let frame = s.frame;
                if let Some(key) = s.symbol_mut().timeline.layers[layer_idx].key_at_mut(frame)
                    && let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element)
                {
                    write_editable_element_commands(el, remaining);
                }
            }
            s.set_selection(vec![target]);
        });
        self.vector.selected_anchors = None;
        true
    }

    pub(crate) fn nudge_selected_vector_points(&mut self, dx: f64, dy: f64) -> bool {
        if self.tool != Tool::Subselect || !self.selected_editable() {
            return false;
        }
        let Some(element) = self.selected_sampled() else {
            return false;
        };
        let Some(mut commands) = editable_element_commands(&element) else {
            return false;
        };
        let Some((selected_id, indices)) = self.vector.selected_anchors.clone() else {
            return false;
        };
        if selected_id != element.id || indices.is_empty() {
            return false;
        }
        let [dx, dy] = self.stage_mapping().stage_delta_to_local([dx, dy]);
        let m = transform_matrix(element.transform);
        let (Some(p0), Some(p1)) = (
            crate::stage::inverse(m, Pos2::ZERO),
            crate::stage::inverse(m, Pos2::new(dx as f32, dy as f32)),
        ) else {
            return false;
        };
        let local_delta = p1 - p0;
        let before = commands.clone();
        move_selected_anchors(&mut commands, &indices, local_delta);
        if commands == before || validate_path_commands(&commands).is_err() {
            return false;
        }
        self.playing = false;
        self.transact(|s| {
            s.ensure_selected_key();
            if let Some(el) = s.selected_element_mut() {
                write_editable_element_commands(el, commands);
            }
        });
        true
    }

    pub(crate) fn pen_subdivide_candidate(
        &self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
    ) -> Option<(crate::selection::SelectionTarget, Vec<PathCommand>, usize)> {
        let zoom = self.zoom;
        let try_element =
            |target: crate::selection::SelectionTarget,
             element: Element|
             -> Option<(crate::selection::SelectionTarget, Vec<PathCommand>, usize)> {
                let commands = editable_element_commands(&element)?;
                let matrix = transform_matrix(element.transform);
                let screen = |pt: Pos2| {
                    mapping.edit_to_screen(origin, zoom, crate::stage::affine(matrix, pt))
                };
                if handles(&commands)
                    .iter()
                    .any(|h| h.role == PointRole::Anchor && screen(h.point).distance(pos) < 8.)
                {
                    return None;
                }
                let (cmd_idx, t) =
                    path_segment_hit_local(&commands, matrix, mapping, origin, zoom, pos)?;
                let (new_commands, inserted) = subdivide_path_at(&commands, cmd_idx, t)?;
                validate_path_commands(&new_commands).ok()?;
                Some((target, new_commands, inserted))
            };

        if self.selected_editable()
            && let Some(target) = self.selection.single().cloned()
            && let Some(element) = self.selected_sampled()
            && let Some(found) = try_element(target, element)
        {
            return Some(found);
        }

        let timeline = &self.symbol().timeline;
        for layer in timeline.layers.iter().rev() {
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
                || !self.is_layer_id_effectively_soloed(&layer.id)
            {
                continue;
            }
            for element in sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps))
                .into_iter()
                .rev()
            {
                let target = crate::selection::SelectionTarget {
                    layer: layer.id.clone(),
                    element: element.id.clone(),
                };
                if let Some(found) = try_element(target, element) {
                    return Some(found);
                }
            }
        }
        None
    }

    pub(crate) fn try_pen_subdivide(
        &mut self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
    ) -> bool {
        let Some((target, new_commands, inserted)) =
            self.pen_subdivide_candidate(origin, mapping, pos)
        else {
            return false;
        };
        let element_id = target.element.clone();
        self.finish_edits();
        self.playing = false;
        self.transact(|s| {
            s.ensure_key_for(&target.layer, false);
            if let Some(layer_idx) = s.target_layer_index(&target) {
                let frame = s.frame;
                if let Some(key) = s.symbol_mut().timeline.layers[layer_idx].key_at_mut(frame)
                    && let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element)
                {
                    write_editable_element_commands(el, new_commands);
                }
            }
            s.set_selection(vec![target]);
        });
        self.vector.selected_anchors = Some((element_id, vec![inserted]));
        true
    }

    pub(crate) fn delete_selected_vector_points(&mut self) -> bool {
        if !self.selected_editable() {
            return false;
        }
        let Some(target) = self.selection.single().cloned() else {
            return false;
        };
        let Some(element) = self.selected_sampled() else {
            return false;
        };
        let Some(commands) = editable_element_commands(&element) else {
            return false;
        };
        let Some((selected_id, indices)) = self.vector.selected_anchors.clone() else {
            return false;
        };
        if selected_id != element.id || indices.is_empty() {
            return false;
        }
        let remaining = delete_anchors_from_path(&commands, &indices);
        if remaining == commands {
            return false;
        }
        self.playing = false;
        if remaining.is_empty() {
            self.transact(|s| {
                s.ensure_key_for(&target.layer, false);
                if let Some(index) = s.target_layer_index(&target) {
                    let frame = s.frame;
                    if let Some(key) = s.symbol_mut().timeline.layers[index].key_at_mut(frame) {
                        key.elements.retain(|el| el.id != target.element);
                    }
                }
                s.prune_authoring_groups();
                s.clear_selection();
            });
            self.vector.selected_anchors = None;
            return true;
        }
        if validate_path_commands(&remaining).is_err() {
            return false;
        }
        self.transact(|s| {
            s.ensure_selected_key();
            if let Some(el) = s.selected_element_mut() {
                write_editable_element_commands(el, remaining);
            }
        });
        self.vector.selected_anchors = None;
        true
    }

    /// True when at least one selected element on an editable layer is a shape or path
    /// that can be smoothed or straightened with the Selection tool (`V`) modifiers.
    pub(crate) fn can_smooth_or_straighten_selection(&self) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let timeline = &self.symbol().timeline;
        self.selection.targets.iter().any(|target| {
            let Some(layer_idx) = self.target_layer_index(target) else {
                return false;
            };
            let layer = &timeline.layers[layer_idx];
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
                || !self.is_layer_id_effectively_soloed(&layer.id)
            {
                return false;
            }
            sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps))
                .into_iter()
                .find(|el| el.id == target.element)
                .is_some_and(|el| {
                    el.transform.primitive.is_none() && editable_element_commands(&el).is_some()
                })
        })
    }

    /// Smooths selected path/shape contours in one Undo step (`Selection` tool `Smooth` modifier).
    pub(crate) fn smooth_selection(&mut self) -> bool {
        self.apply_selection_contour_modifier(smooth_path_commands)
    }

    /// Straightens selected path/shape contours in one Undo step (`Selection` tool `Straighten` modifier).
    pub(crate) fn straighten_selection(&mut self) -> bool {
        self.apply_selection_contour_modifier(straighten_path_commands)
    }

    fn apply_selection_contour_modifier(
        &mut self,
        op: fn(&[PathCommand], f32) -> Vec<PathCommand>,
    ) -> bool {
        if !self.can_smooth_or_straighten_selection() {
            return false;
        }
        let zoom = self.zoom.max(crate::stage::MIN_STAGE_ZOOM);
        let timeline = &self.symbol().timeline;
        let mut updates: Vec<(crate::selection::SelectionTarget, Vec<PathCommand>)> = Vec::new();
        for target in &self.selection.targets {
            let Some(layer_idx) = self.target_layer_index(target) else {
                continue;
            };
            let layer = &timeline.layers[layer_idx];
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
                || !self.is_layer_id_effectively_soloed(&layer.id)
            {
                continue;
            }
            let Some(sampled) = sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps))
                .into_iter()
                .find(|el| el.id == target.element)
            else {
                continue;
            };
            if sampled.transform.primitive.is_some() {
                continue;
            }
            let Some(commands) = editable_element_commands(&sampled) else {
                continue;
            };
            let next = op(&commands, zoom);
            if next != commands && validate_path_commands(&next).is_ok() {
                updates.push((target.clone(), next));
            }
        }
        if updates.is_empty() {
            return false;
        }
        self.finish_edits();
        self.playing = false;
        self.transact(|s| {
            let mut ensured_layers: Vec<Id> = Vec::new();
            for (target, next_commands) in updates {
                if !ensured_layers.contains(&target.layer) {
                    s.ensure_key_for(&target.layer, false);
                    ensured_layers.push(target.layer.clone());
                }
                if let Some(layer_idx) = s.target_layer_index(&target) {
                    let frame = s.frame;
                    if let Some(key) = s.symbol_mut().timeline.layers[layer_idx].key_at_mut(frame)
                        && let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element)
                    {
                        write_editable_element_commands(el, next_commands);
                    }
                }
            }
        });
        true
    }
}
