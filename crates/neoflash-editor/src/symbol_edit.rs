//! Session-only navigation state for isolated and in-place symbol editing.
//!
//! The authored project contains symbols and placements, but never the editor's
//! breadcrumb, viewport or chosen placement.  In-place levels therefore keep an
//! exact parent target and resolve its sampled transform from the live project.

use crate::app::Editor;
use crate::selection::{Selection, SelectionTarget};
use eframe::egui::Vec2;
use neoflash_core::{
    FrameRange, Id, InstancePoseError, Project, affine_multiply, evaluate_instance_pose,
    invert_affine_matrix, sample_layer_in_at_fps,
};
use std::{collections::HashMap, sync::Arc};

const IDENTITY: [f64; 6] = [1., 0., 0., 1., 0., 0.];

fn map_instance_pose_error(error: InstancePoseError) -> SymbolEditError {
    match error {
        InstancePoseError::SingularMatrix => SymbolEditError::SingularPlacement,
        InstancePoseError::NonFiniteMatrix => SymbolEditError::InvalidPlacementTransform,
        _ => SymbolEditError::MissingPlacement,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SymbolEditEntry {
    Scene,
    Isolated,
    InPlace {
        parent_symbol: Id,
        parent_frame: u32,
        /// Paused independent-clip frames for the whole displayed parent tree.
        /// The parent document is still re-evaluated live while these clocks stay frozen.
        parent_clip_frames: Arc<HashMap<Vec<Id>, u32>>,
        target: SelectionTarget,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SymbolEditView {
    pub frame: u32,
    pub layer: usize,
    pub selection: Selection,
    pub zoom: f32,
    pub pan: Vec2,
    pub ruler_zero: [f64; 2],
    pub frame_range: Option<FrameRange>,
    pub timeline_focused: bool,
}

impl SymbolEditView {
    fn initial(symbol: Id, zoom: f32, pan: Vec2) -> Self {
        Self {
            frame: 0,
            layer: 0,
            selection: Selection {
                symbol,
                targets: vec![],
            },
            zoom,
            pan,
            ruler_zero: [0., 0.],
            frame_range: None,
            timeline_focused: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SymbolEditLevel {
    pub symbol: Id,
    pub entry: SymbolEditEntry,
    /// Last live view of this level, refreshed before navigating away from it.
    pub saved_view: SymbolEditView,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SymbolEditSession {
    pub levels: Vec<SymbolEditLevel>,
    pub revision: u64,
}

impl SymbolEditSession {
    pub(crate) fn for_scene(scene: Id, zoom: f32, pan: Vec2) -> Self {
        Self {
            levels: vec![SymbolEditLevel {
                saved_view: SymbolEditView::initial(scene.clone(), zoom, pan),
                symbol: scene,
                entry: SymbolEditEntry::Scene,
            }],
            revision: 0,
        }
    }

    pub(crate) fn path(&self) -> impl Iterator<Item = &str> {
        self.levels.iter().map(|level| level.symbol.as_str())
    }

    pub(crate) fn current_symbol(&self) -> Option<&str> {
        self.levels.last().map(|level| level.symbol.as_str())
    }

    pub(crate) fn current_entry(&self) -> Option<&SymbolEditEntry> {
        self.levels.last().map(|level| &level.entry)
    }

    pub(crate) fn is_scene(&self) -> bool {
        matches!(self.current_entry(), Some(SymbolEditEntry::Scene))
    }

    pub(crate) fn is_isolated(&self) -> bool {
        matches!(self.current_entry(), Some(SymbolEditEntry::Isolated))
    }

    pub(crate) fn is_in_place(&self) -> bool {
        matches!(self.current_entry(), Some(SymbolEditEntry::InPlace { .. }))
    }

    /// First breadcrumb level belonging to the active visual context. An
    /// isolated level deliberately hides everything before this index.
    pub(crate) fn context_start(&self) -> usize {
        self.levels
            .iter()
            .rposition(|level| {
                matches!(
                    &level.entry,
                    SymbolEditEntry::Scene | SymbolEditEntry::Isolated
                )
            })
            .unwrap_or(0)
    }

    pub(crate) fn context_is_isolated(&self) -> bool {
        self.levels
            .get(self.context_start())
            .is_some_and(|level| matches!(&level.entry, SymbolEditEntry::Isolated))
    }

    fn placement_matrix(
        &self,
        project: &Project,
        level_index: usize,
    ) -> Result<[f64; 6], SymbolEditError> {
        let level = self
            .levels
            .get(level_index)
            .ok_or(SymbolEditError::MissingLevel)?;
        let SymbolEditEntry::InPlace {
            parent_symbol,
            parent_frame,
            target,
            ..
        } = &level.entry
        else {
            return Ok(IDENTITY);
        };
        let previous = level_index
            .checked_sub(1)
            .and_then(|index| self.levels.get(index))
            .ok_or(SymbolEditError::MissingParent)?;
        if previous.symbol != *parent_symbol {
            return Err(SymbolEditError::ParentMismatch);
        }
        let pose = evaluate_instance_pose(
            project,
            parent_symbol,
            *parent_frame,
            &target.layer,
            &target.element,
        )
        .map_err(map_instance_pose_error)?;
        if pose.symbol_id != level.symbol {
            return Err(SymbolEditError::PlacementMismatch);
        }
        if pose.nonlinear_scale9 {
            return Err(SymbolEditError::NonlinearPlacement);
        }
        Ok(pose.matrix)
    }

    /// One cumulative local-to-context matrix per breadcrumb level. Scene and
    /// isolated levels begin a fresh context; in-place levels compose exact live
    /// sampled placement matrices from their frozen parent frames.
    pub(crate) fn resolved_matrices(
        &self,
        project: &Project,
    ) -> Result<Vec<[f64; 6]>, SymbolEditError> {
        if self.levels.is_empty() {
            return Err(SymbolEditError::MissingLevel);
        }
        let mut matrices = Vec::with_capacity(self.levels.len());
        let mut cumulative = IDENTITY;
        for (index, level) in self.levels.iter().enumerate() {
            if project.symbol(&level.symbol).is_none() {
                return Err(SymbolEditError::MissingSymbol);
            }
            match &level.entry {
                SymbolEditEntry::Scene | SymbolEditEntry::Isolated => cumulative = IDENTITY,
                SymbolEditEntry::InPlace { .. } => {
                    cumulative =
                        affine_multiply(cumulative, self.placement_matrix(project, index)?);
                    invert_affine_matrix(cumulative).ok_or(SymbolEditError::SingularPlacement)?;
                }
            }
            matrices.push(cumulative);
        }
        Ok(matrices)
    }

    pub(crate) fn current_matrix(&self, project: &Project) -> Result<[f64; 6], SymbolEditError> {
        self.resolved_matrices(project)?
            .last()
            .copied()
            .ok_or(SymbolEditError::MissingLevel)
    }

    fn valid_prefix_len(&self, project: &Project) -> usize {
        for index in 0..self.levels.len() {
            let level = &self.levels[index];
            if project.symbol(&level.symbol).is_none() {
                return index;
            }
            match &level.entry {
                SymbolEditEntry::Scene => {
                    if index != 0 || !project.is_scene(&level.symbol) {
                        return index;
                    }
                }
                SymbolEditEntry::Isolated => {
                    if index == 0 {
                        return index;
                    }
                }
                SymbolEditEntry::InPlace { .. } => {
                    if self.placement_matrix(project, index).is_err() {
                        return index;
                    }
                }
            }
        }
        self.levels.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SymbolEditError {
    MissingLevel,
    MissingSymbol,
    MissingParent,
    ParentMismatch,
    MissingPlacement,
    PlacementMismatch,
    SingularPlacement,
    InvalidPlacementTransform,
    NonlinearPlacement,
}

impl SymbolEditError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::SingularPlacement => {
                "Cannot edit this instance in place because its transform is not invertible."
            }
            Self::InvalidPlacementTransform => {
                "Cannot edit this instance in place because its evaluated transform is invalid."
            }
            Self::NonlinearPlacement => {
                "Cannot edit this nine-slice instance in place because its scaled geometry is not affine."
            }
            _ => "Cannot edit this instance in place because its placement is no longer available.",
        }
    }
}

impl Editor {
    pub(crate) fn edit_path(&self) -> impl Iterator<Item = &str> {
        self.edit_session.path()
    }

    pub(crate) fn edit_depth(&self) -> usize {
        self.edit_session.levels.len()
    }

    pub(crate) fn editing_scene(&self) -> bool {
        self.edit_session.is_scene()
    }

    pub(crate) fn editing_isolated(&self) -> bool {
        self.edit_session.is_isolated()
    }

    pub(crate) fn editing_in_place(&self) -> bool {
        self.edit_session.is_in_place()
    }

    pub(crate) fn edit_context_start(&self) -> usize {
        self.edit_session.context_start()
    }

    pub(crate) fn edit_context_is_isolated(&self) -> bool {
        self.editing_isolated() || self.edit_session.context_is_isolated()
    }

    pub(crate) fn edit_local_to_context_matrix(&self) -> Result<[f64; 6], SymbolEditError> {
        self.edit_session.current_matrix(&self.project)
    }

    pub(crate) fn resolved_edit_matrices(&self) -> Result<Vec<[f64; 6]>, SymbolEditError> {
        self.edit_session.resolved_matrices(&self.project)
    }

    fn capture_symbol_edit_view(&self) -> SymbolEditView {
        SymbolEditView {
            frame: self.frame,
            layer: self.layer,
            selection: self.selection.clone(),
            zoom: self.zoom,
            pan: self.pan,
            ruler_zero: self.stage_view.zero,
            frame_range: self.frame_range(),
            timeline_focused: self.timeline_edit.focused,
        }
    }

    fn restore_symbol_edit_view(&mut self, view: SymbolEditView) {
        self.frame = view.frame;
        self.layer = view.layer;
        self.selection = view.selection;
        self.zoom = view.zoom;
        self.pan = view.pan;
        self.stage_view.zero = view.ruler_zero;
        self.clear_timeline_frame_selection();
        if let Some(range) = view.frame_range {
            self.select_frame_range(range);
        }
        self.timeline_edit.focused = view.timeline_focused;
    }

    fn sync_editing_mirror(&mut self) {
        self.editing = self.edit_session.path().map(str::to_owned).collect();
    }

    /// Run one synchronous authoring operation against a captured symbol/frame/layer, then
    /// restore the live navigation session and view exactly. Long-lived gestures use this when
    /// their original frame is no longer the one currently shown (for example, finishing a Pen
    /// draft after the playhead moves). The temporary level is never exposed to the UI.
    pub(crate) fn with_temporary_symbol_target<R>(
        &mut self,
        symbol: Id,
        frame: u32,
        layer: usize,
        operation: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved_session = self.edit_session.clone();
        let saved_view = self.capture_symbol_edit_view();
        let revision = saved_session.revision;
        let entry = if self.project.is_scene(&symbol) {
            SymbolEditEntry::Scene
        } else {
            SymbolEditEntry::Isolated
        };
        self.edit_session = SymbolEditSession {
            levels: vec![SymbolEditLevel {
                saved_view: SymbolEditView::initial(symbol.clone(), self.zoom, self.pan),
                symbol: symbol.clone(),
                entry,
            }],
            revision,
        };
        self.sync_editing_mirror();
        self.frame = frame;
        self.layer = layer;
        self.selection = Selection {
            symbol,
            targets: vec![],
        };

        let result = operation(self);

        self.edit_session = saved_session;
        self.sync_editing_mirror();
        self.restore_symbol_edit_view(saved_view);
        result
    }

    fn prepare_symbol_edit_transition(&mut self) {
        self.finish_edits();
        self.timeline_key_drag = None;
        self.vector = Default::default();
        self.graphic_drop_position = None;
        self.clear_simple_button_state();
        self.reset_authored_preview();
        self.playing = false;
        self.accumulator = 0.;
        self.cancel_guide_drag();
        self.stage_view.selected_guide = None;
        self.stage_view.snap_ring = None;
        self.stage_view.zero_drag = false;
    }

    fn save_current_symbol_edit_view(&mut self) {
        let view = self.capture_symbol_edit_view();
        if let Some(level) = self.edit_session.levels.last_mut() {
            level.saved_view = view;
        }
    }

    /// Clean symbol-definition editing. The current parent stays in the breadcrumb,
    /// but it is not rendered as an editable context.
    pub(crate) fn enter_symbol_isolated(&mut self, id: Id) -> bool {
        if self.project.symbol(&id).is_none() {
            return false;
        }
        let reroot_current = self.symbol_id() == id;
        if reroot_current && !self.editing_in_place() {
            return false;
        }
        self.prepare_symbol_edit_transition();
        if !reroot_current {
            self.save_current_symbol_edit_view();
        }
        self.clear_timeline_frame_selection();
        let initial = SymbolEditView::initial(id.clone(), self.zoom, Vec2::ZERO);
        if reroot_current {
            let level = self
                .edit_session
                .levels
                .last_mut()
                .expect("an active symbol edit session always has a current level");
            level.entry = SymbolEditEntry::Isolated;
            level.saved_view = initial;
        } else {
            self.edit_session.levels.push(SymbolEditLevel {
                saved_view: initial,
                symbol: id.clone(),
                entry: SymbolEditEntry::Isolated,
            });
        }
        self.edit_session.revision = self.edit_session.revision.wrapping_add(1);
        self.sync_editing_mirror();
        self.frame = 0;
        self.layer = 0;
        self.selection = Selection {
            symbol: id,
            targets: vec![],
        };
        self.pan = Vec2::ZERO;
        self.stage_view.zero = [0., 0.];
        true
    }

    /// Edit the currently selected exact instance at its frozen parent frame.
    pub(crate) fn enter_selected_symbol_in_place(&mut self) -> bool {
        let Some(target) = self.selection.single().cloned() else {
            return false;
        };
        let Some(placement) = self.selected_sampled() else {
            return false;
        };
        let neoflash_core::ElementKind::Instance { symbol_id, .. } = placement.kind else {
            return false;
        };
        self.enter_symbol_at(target, symbol_id)
    }

    /// Resolve a library symbol to a concrete current-frame placement without
    /// guessing. A selected matching placement wins; otherwise exactly one
    /// visible placement is required.
    pub(crate) fn enter_symbol_in_place_by_id(&mut self, id: &str) -> bool {
        if let Some(target) = self.selection.single().cloned()
            && self.selected_sampled().is_some_and(|element| {
                matches!(element.kind, neoflash_core::ElementKind::Instance { ref symbol_id, .. } if symbol_id == id)
            })
        {
            return self.enter_symbol_at(target, id.to_owned());
        }

        let mut targets = Vec::new();
        let timeline = &self.symbol().timeline;
        for layer in &timeline.layers {
            if !neoflash_core::layer_effectively_visible(timeline, layer) {
                continue;
            }
            for element in
                sample_layer_in_at_fps(timeline, layer, self.frame, f64::from(self.project.fps))
            {
                if element.visible
                    && matches!(element.kind, neoflash_core::ElementKind::Instance { ref symbol_id, .. } if symbol_id == id)
                {
                    targets.push(SelectionTarget {
                        layer: layer.id.clone(),
                        element: element.id,
                    });
                }
            }
        }
        match targets.as_slice() {
            [target] => self.enter_symbol_at(target.clone(), id.to_owned()),
            [] => {
                self.message("Edit in place needs a visible instance on the current frame.");
                false
            }
            _ => {
                self.message(
                    "Select the instance to edit in place because this symbol has several visible placements.",
                );
                false
            }
        }
    }

    /// Edit one exact placement in context. The selected placement must resolve at
    /// the current parent frame and its transform must be invertible.
    pub(crate) fn enter_symbol_at(&mut self, target: SelectionTarget, id: Id) -> bool {
        let parent_symbol = self.symbol_id().to_owned();
        let parent_frame = self.frame;
        let preview_frame = self.presented_instance_frame(&target.element);
        let parent_clip_frames = Arc::new(self.presented_clip_frames());
        let pose = match evaluate_instance_pose(
            &self.project,
            &parent_symbol,
            parent_frame,
            &target.layer,
            &target.element,
        )
        .map_err(map_instance_pose_error)
        {
            Ok(pose) if pose.symbol_id == id && !pose.nonlinear_scale9 => pose,
            Ok(pose) => {
                let error = if pose.nonlinear_scale9 {
                    SymbolEditError::NonlinearPlacement
                } else {
                    SymbolEditError::PlacementMismatch
                };
                self.message(error.message());
                return false;
            }
            Err(error) => {
                self.message(error.message());
                return false;
            }
        };
        let child_frame = preview_frame.unwrap_or(pose.frame);
        let candidate = SymbolEditLevel {
            saved_view: SymbolEditView {
                frame: child_frame,
                ..SymbolEditView::initial(id.clone(), self.zoom, self.pan)
            },
            symbol: id.clone(),
            entry: SymbolEditEntry::InPlace {
                parent_symbol,
                parent_frame,
                parent_clip_frames,
                target,
            },
        };
        let mut probe = self.edit_session.clone();
        probe.levels.push(candidate.clone());
        if let Err(error) = probe.current_matrix(&self.project) {
            self.message(error.message());
            return false;
        }

        self.prepare_symbol_edit_transition();
        self.save_current_symbol_edit_view();
        self.clear_timeline_frame_selection();
        self.edit_session.levels.push(candidate);
        self.edit_session.revision = self.edit_session.revision.wrapping_add(1);
        self.sync_editing_mirror();
        self.frame = child_frame;
        self.layer = 0;
        self.selection = Selection {
            symbol: id,
            targets: vec![],
        };
        self.stage_view.zero = [0., 0.];
        true
    }

    /// Replace the breadcrumb with a scene level. This is session-only and never
    /// creates an Undo entry.
    pub(crate) fn reset_symbol_edit_session(&mut self, scene: Id) {
        self.prepare_symbol_edit_transition();
        self.clear_timeline_frame_selection();
        self.frame = 0;
        self.layer = 0;
        self.selection = Selection {
            symbol: scene.clone(),
            targets: vec![],
        };
        self.stage_view.zero = [0., 0.];
        let revision = self.edit_session.revision.wrapping_add(1);
        self.edit_session = SymbolEditSession::for_scene(scene, self.zoom, self.pan);
        self.edit_session.revision = revision;
        self.sync_editing_mirror();
    }

    /// Safely unwind levels invalidated by Undo, deletion, replacement or a
    /// newly singular placement. Returns true when the active level changed.
    pub(crate) fn normalize_symbol_edit_session(&mut self) -> bool {
        let valid = self.edit_session.valid_prefix_len(&self.project);
        if valid == self.edit_session.levels.len() && valid != 0 {
            self.sync_editing_mirror();
            return false;
        }

        self.object_gesture = None;
        self.vector = Default::default();
        self.authoring_tools.cancel();
        self.shape_draft = None;
        self.timeline_key_drag = None;
        self.graphic_drop_position = None;
        self.clear_simple_button_state();

        if valid == 0 {
            let revision = self.edit_session.revision.wrapping_add(1);
            let scene = self.project.root.clone();
            self.frame = 0;
            self.layer = 0;
            self.selection = Selection {
                symbol: scene.clone(),
                targets: vec![],
            };
            self.edit_session = SymbolEditSession::for_scene(scene, self.zoom, self.pan);
            self.edit_session.revision = revision;
        } else {
            let restore = self.edit_session.levels[valid - 1].saved_view.clone();
            self.edit_session.levels.truncate(valid);
            self.restore_symbol_edit_view(restore);
        }
        if valid != 0 {
            self.edit_session.revision = self.edit_session.revision.wrapping_add(1);
        }
        self.sync_editing_mirror();
        self.reset_authored_preview();
        self.playing = false;
        true
    }

    pub(crate) fn leave_symbol_to_level(&mut self, index: usize) -> bool {
        if index >= self.edit_session.levels.len() || index + 1 == self.edit_session.levels.len() {
            return false;
        }
        self.prepare_symbol_edit_transition();
        let restore = self.edit_session.levels[index].saved_view.clone();
        self.edit_session.levels.truncate(index + 1);
        self.edit_session.revision = self.edit_session.revision.wrapping_add(1);
        self.sync_editing_mirror();
        self.restore_symbol_edit_view(restore);
        true
    }
}
