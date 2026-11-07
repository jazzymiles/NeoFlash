use crate::{BlendMode, Color, Element, ElementKind, Id, Project, new_id};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

/// Maximum steps allowed in a single saved/replayed authoring command.
pub const MAX_COMMAND_STEPS: usize = 256;

/// Bounded, deterministic, side-effect-free authoring command step that can be recorded from
/// History, copied as JSON, saved as a named command, and replayed on another selection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AuthoringCommandStep {
    Translate { dx: f64, dy: f64 },
    Scale { sx: f64, sy: f64 },
    Rotate { degrees: f64 },
    Skew { degrees: f64 },
    SetAlpha { alpha: f64 },
    SetFillColor { color: Color },
    SetStrokeColor { color: Color },
    SetStrokeWidth { width: f64 },
    SetVisible { visible: bool },
    SetBlendMode { blend_mode: BlendMode },
    RenameSelection { name: String },
    DuplicateSelection { offset_x: f64, offset_y: f64 },
    DeleteSelection,
    SwapSymbol { target_symbol_id: Id },
}

/// Named sequence of replayable `AuthoringCommandStep`s.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedCommand {
    pub name: String,
    pub steps: Vec<AuthoringCommandStep>,
}

impl SavedCommand {
    pub fn validate(&self) -> Result<(), String> {
        let trimmed = self.name.trim();
        if trimmed.is_empty() || trimmed.len() > 128 || trimmed.chars().any(char::is_control) {
            return Err("Command name must be 1..=128 non-control characters.".into());
        }
        if self.steps.is_empty() || self.steps.len() > MAX_COMMAND_STEPS {
            return Err(format!(
                "Command must contain 1..={MAX_COMMAND_STEPS} steps."
            ));
        }
        for step in &self.steps {
            match step {
                AuthoringCommandStep::Translate { dx, dy } => {
                    if !dx.is_finite() || !dy.is_finite() || dx.abs() > 1e6 || dy.abs() > 1e6 {
                        return Err("Translate delta must be finite and within ±1e6.".into());
                    }
                }
                AuthoringCommandStep::Scale { sx, sy } => {
                    if !sx.is_finite()
                        || !sy.is_finite()
                        || sx.abs() < 1e-4
                        || sy.abs() < 1e-4
                        || sx.abs() > 1e4
                        || sy.abs() > 1e4
                    {
                        return Err(
                            "Scale factors must be finite and within [1e-4, 1e4] in magnitude."
                                .into(),
                        );
                    }
                }
                AuthoringCommandStep::Rotate { degrees }
                | AuthoringCommandStep::Skew { degrees } => {
                    if !degrees.is_finite() || degrees.abs() > 36000.0 {
                        return Err("Rotation/skew degrees must be finite.".into());
                    }
                }
                AuthoringCommandStep::SetAlpha { alpha } => {
                    if !alpha.is_finite() || !(0.0..=1.0).contains(alpha) {
                        return Err("Alpha must be in 0.0..=1.0.".into());
                    }
                }
                AuthoringCommandStep::SetStrokeWidth { width } => {
                    if !width.is_finite() || !(0.0..=10000.0).contains(width) {
                        return Err("Stroke width must be in 0.0..=10000.0.".into());
                    }
                }
                AuthoringCommandStep::DuplicateSelection { offset_x, offset_y } => {
                    if !offset_x.is_finite()
                        || !offset_y.is_finite()
                        || offset_x.abs() > 1e6
                        || offset_y.abs() > 1e6
                    {
                        return Err("Duplicate offset must be finite and within ±1e6.".into());
                    }
                }
                AuthoringCommandStep::RenameSelection { name } => {
                    if name.len() > 256 || name.chars().any(char::is_control) {
                        return Err("Element name exceeds 256 bytes or contains controls.".into());
                    }
                }
                AuthoringCommandStep::SwapSymbol { target_symbol_id } => {
                    if target_symbol_id.trim().is_empty() {
                        return Err("SwapSymbol target_symbol_id cannot be empty.".into());
                    }
                }
                AuthoringCommandStep::SetFillColor { .. }
                | AuthoringCommandStep::SetStrokeColor { .. }
                | AuthoringCommandStep::SetVisible { .. }
                | AuthoringCommandStep::SetBlendMode { .. }
                | AuthoringCommandStep::DeleteSelection => {}
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn from_json(raw: &str) -> Result<Self, String> {
        if raw.len() > 256 * 1024 {
            return Err("Command JSON exceeds 256 KiB.".into());
        }
        let cmd: Self = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        cmd.validate()?;
        Ok(cmd)
    }
}

/// Apply a sequence of `AuthoringCommandStep`s to `elements` for the IDs in `selected_ids`.
pub fn apply_command_steps_to_elements(
    elements: &mut Vec<Element>,
    selected_ids: &mut Vec<Id>,
    steps: &[AuthoringCommandStep],
    valid_symbol_ids: &HashSet<&str>,
) -> Result<usize, String> {
    if steps.is_empty() || steps.len() > MAX_COMMAND_STEPS {
        return Err(format!(
            "Command replay requires 1..={MAX_COMMAND_STEPS} steps."
        ));
    }
    if selected_ids.is_empty() {
        return Err("Select at least one stage element to replay command steps.".into());
    }
    let mut applied = 0usize;
    for step in steps {
        match step {
            AuthoringCommandStep::Translate { dx, dy } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.transform.x += *dx;
                    el.transform.y += *dy;
                    applied += 1;
                }
            }
            AuthoringCommandStep::Scale { sx, sy } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.transform.scale_x *= *sx;
                    el.transform.scale_y *= *sy;
                    applied += 1;
                }
            }
            AuthoringCommandStep::Rotate { degrees } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.transform.rotation += *degrees;
                    applied += 1;
                }
            }
            AuthoringCommandStep::Skew { degrees } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.transform.skew += *degrees;
                    applied += 1;
                }
            }
            AuthoringCommandStep::SetAlpha { alpha } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.transform.alpha = alpha.clamp(0.0, 1.0);
                    applied += 1;
                }
            }
            AuthoringCommandStep::SetFillColor { color } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    match &mut el.kind {
                        ElementKind::Rectangle { fill, .. }
                        | ElementKind::Ellipse { fill, .. }
                        | ElementKind::Path { fill, .. }
                        | ElementKind::PaintedPath { fill, .. } => {
                            *fill = *color;
                            applied += 1;
                        }
                        ElementKind::Text { color: tc, .. } => {
                            *tc = *color;
                            applied += 1;
                        }
                        ElementKind::TextBox { format, .. } => {
                            format.color = (u32::from(color[0]) << 16)
                                | (u32::from(color[1]) << 8)
                                | u32::from(color[2]);
                            applied += 1;
                        }
                        _ => {}
                    }
                }
            }
            AuthoringCommandStep::SetStrokeColor { color } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    match &mut el.kind {
                        ElementKind::Rectangle { stroke, .. }
                        | ElementKind::Ellipse { stroke, .. }
                        | ElementKind::Path { stroke, .. }
                        | ElementKind::PaintedPath { stroke, .. } => {
                            *stroke = *color;
                            applied += 1;
                        }
                        _ => {}
                    }
                }
            }
            AuthoringCommandStep::SetStrokeWidth { width } => {
                let w = width.clamp(0.0, 10000.0);
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    match &mut el.kind {
                        ElementKind::Rectangle { stroke_width, .. }
                        | ElementKind::Ellipse { stroke_width, .. }
                        | ElementKind::Path { stroke_width, .. }
                        | ElementKind::PaintedPath { stroke_width, .. } => {
                            *stroke_width = w;
                            applied += 1;
                        }
                        _ => {}
                    }
                }
            }
            AuthoringCommandStep::SetVisible { visible } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.visible = *visible;
                    applied += 1;
                }
            }
            AuthoringCommandStep::SetBlendMode { blend_mode } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.blend_mode = *blend_mode;
                    applied += 1;
                }
            }
            AuthoringCommandStep::RenameSelection { name } => {
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    el.name = name.clone();
                    applied += 1;
                }
            }
            AuthoringCommandStep::DuplicateSelection { offset_x, offset_y } => {
                let mut clones = Vec::new();
                let mut new_selected = Vec::new();
                for el in elements.iter().filter(|e| selected_ids.contains(&e.id)) {
                    let mut dup = el.clone();
                    dup.id = new_id("element");
                    dup.transform.x += *offset_x;
                    dup.transform.y += *offset_y;
                    new_selected.push(dup.id.clone());
                    clones.push(dup);
                    applied += 1;
                }
                if !clones.is_empty() {
                    elements.extend(clones);
                    *selected_ids = new_selected;
                }
            }
            AuthoringCommandStep::DeleteSelection => {
                let before = elements.len();
                elements.retain(|e| !selected_ids.contains(&e.id));
                let removed = before.saturating_sub(elements.len());
                applied += removed;
                selected_ids.clear();
            }
            AuthoringCommandStep::SwapSymbol { target_symbol_id } => {
                if !valid_symbol_ids.contains(target_symbol_id.as_str()) {
                    return Err(format!("Target symbol {target_symbol_id} does not exist."));
                }
                for el in elements.iter_mut().filter(|e| selected_ids.contains(&e.id)) {
                    if let ElementKind::Instance { symbol_id, .. } = &mut el.kind {
                        *symbol_id = target_symbol_id.clone();
                        applied += 1;
                    }
                }
            }
        }
    }
    Ok(applied)
}

fn infer_command_step(before: &Project, after: &Project) -> Option<AuthoringCommandStep> {
    for (sym_a, sym_b) in before.symbols.iter().zip(&after.symbols) {
        if sym_a.id != sym_b.id {
            continue;
        }
        for (layer_a, layer_b) in sym_a.timeline.layers.iter().zip(&sym_b.timeline.layers) {
            for (key_a, key_b) in layer_a.keyframes.iter().zip(&layer_b.keyframes) {
                if key_a == key_b {
                    continue;
                }
                if key_b.elements.len() < key_a.elements.len() {
                    return Some(AuthoringCommandStep::DeleteSelection);
                }
                if key_b.elements.len() > key_a.elements.len()
                    && let Some(added) = key_b.elements.last()
                    && let Some(orig) = key_a.elements.last()
                {
                    return Some(AuthoringCommandStep::DuplicateSelection {
                        offset_x: added.transform.x - orig.transform.x,
                        offset_y: added.transform.y - orig.transform.y,
                    });
                }
                for (el_a, el_b) in key_a.elements.iter().zip(&key_b.elements) {
                    if el_a.id != el_b.id || el_a == el_b {
                        continue;
                    }
                    let dx = el_b.transform.x - el_a.transform.x;
                    let dy = el_b.transform.y - el_a.transform.y;
                    if dx.abs() > 1e-6 || dy.abs() > 1e-6 {
                        return Some(AuthoringCommandStep::Translate { dx, dy });
                    }
                    if (el_b.transform.rotation - el_a.transform.rotation).abs() > 1e-6 {
                        return Some(AuthoringCommandStep::Rotate {
                            degrees: el_b.transform.rotation - el_a.transform.rotation,
                        });
                    }
                    if (el_b.transform.skew - el_a.transform.skew).abs() > 1e-6 {
                        return Some(AuthoringCommandStep::Skew {
                            degrees: el_b.transform.skew - el_a.transform.skew,
                        });
                    }
                    if el_a.transform.scale_x.abs() > 1e-6
                        && el_a.transform.scale_y.abs() > 1e-6
                        && ((el_b.transform.scale_x - el_a.transform.scale_x).abs() > 1e-6
                            || (el_b.transform.scale_y - el_a.transform.scale_y).abs() > 1e-6)
                    {
                        return Some(AuthoringCommandStep::Scale {
                            sx: el_b.transform.scale_x / el_a.transform.scale_x,
                            sy: el_b.transform.scale_y / el_a.transform.scale_y,
                        });
                    }
                    if (el_b.transform.alpha - el_a.transform.alpha).abs() > 1e-6 {
                        return Some(AuthoringCommandStep::SetAlpha {
                            alpha: el_b.transform.alpha,
                        });
                    }
                    if el_b.visible != el_a.visible {
                        return Some(AuthoringCommandStep::SetVisible {
                            visible: el_b.visible,
                        });
                    }
                    if el_b.blend_mode != el_a.blend_mode {
                        return Some(AuthoringCommandStep::SetBlendMode {
                            blend_mode: el_b.blend_mode,
                        });
                    }
                    if el_b.name != el_a.name {
                        return Some(AuthoringCommandStep::RenameSelection {
                            name: el_b.name.clone(),
                        });
                    }
                    match (&el_a.kind, &el_b.kind) {
                        (
                            ElementKind::Rectangle {
                                fill: fa,
                                stroke: sa,
                                stroke_width: wa,
                                ..
                            }
                            | ElementKind::Ellipse {
                                fill: fa,
                                stroke: sa,
                                stroke_width: wa,
                                ..
                            }
                            | ElementKind::Path {
                                fill: fa,
                                stroke: sa,
                                stroke_width: wa,
                                ..
                            }
                            | ElementKind::PaintedPath {
                                fill: fa,
                                stroke: sa,
                                stroke_width: wa,
                                ..
                            },
                            ElementKind::Rectangle {
                                fill: fb,
                                stroke: sb,
                                stroke_width: wb,
                                ..
                            }
                            | ElementKind::Ellipse {
                                fill: fb,
                                stroke: sb,
                                stroke_width: wb,
                                ..
                            }
                            | ElementKind::Path {
                                fill: fb,
                                stroke: sb,
                                stroke_width: wb,
                                ..
                            }
                            | ElementKind::PaintedPath {
                                fill: fb,
                                stroke: sb,
                                stroke_width: wb,
                                ..
                            },
                        ) => {
                            if fa != fb {
                                return Some(AuthoringCommandStep::SetFillColor { color: *fb });
                            }
                            if sa != sb {
                                return Some(AuthoringCommandStep::SetStrokeColor { color: *sb });
                            }
                            if (wa - wb).abs() > 1e-6 {
                                return Some(AuthoringCommandStep::SetStrokeWidth { width: *wb });
                            }
                        }
                        (
                            ElementKind::Instance { symbol_id: sa, .. },
                            ElementKind::Instance { symbol_id: sb, .. },
                        ) if sa != sb => {
                            return Some(AuthoringCommandStep::SwapSymbol {
                                target_symbol_id: sb.clone(),
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    None
}

/// Process-wide source of history revisions, so a replaced `History` never repeats one.
static REVISIONS: AtomicU64 = AtomicU64::new(1);

fn next_revision() -> u64 {
    REVISIONS.fetch_add(1, Ordering::Relaxed)
}

/// Deduplicate `Arc<str>` resource buffers against `reference` so history snapshots
/// share identical resource allocations even after reload or copy-on-write edits.
fn share_resources_from(reference: &Project, target: &mut Project) {
    for unit in &mut target.resources {
        if let Some(existing) = reference.resources.iter().find(|r| r.id == unit.id)
            && !std::sync::Arc::ptr_eq(&existing.data_base64, &unit.data_base64)
            && *existing.data_base64 == *unit.data_base64
        {
            unit.data_base64 = existing.data_base64.clone();
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct HistoryEntry {
    project: Project,
    label: String,
    command_step: Option<AuthoringCommandStep>,
}

/// Snapshot history with explicit transaction boundaries. A drag is one transaction.
#[derive(Clone, Debug)]
pub struct History {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    pending: Option<HistoryEntry>,
    capacity: usize,
    revision: u64,
}

impl Default for History {
    fn default() -> Self {
        Self::new(100)
    }
}

impl History {
    const DEFAULT_LABEL: &'static str = "Edit";

    pub fn new(capacity: usize) -> Self {
        Self {
            undo: vec![],
            redo: vec![],
            pending: None,
            capacity,
            revision: next_revision(),
        }
    }

    /// Maximum undo steps. Lowering it drops the oldest steps now; redo steps are kept.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        if self.undo.len() > capacity {
            let excess = self.undo.len() - capacity;
            self.undo.drain(..excess);
        }
    }

    fn normalize_label(raw: String) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            Self::DEFAULT_LABEL.to_owned()
        } else {
            trimmed.to_owned()
        }
    }

    /// Repeated begin calls preserve the original snapshot until commit or cancel.
    pub fn begin(&mut self, project: &Project) {
        if self.pending.is_none() {
            self.pending = Some(HistoryEntry {
                project: project.clone(),
                label: Self::DEFAULT_LABEL.to_owned(),
                command_step: None,
            });
        }
    }

    /// Repeated begin calls preserve the original snapshot; an unnamed (`"Edit"`) pending
    /// transaction upgrades its label when a named command runs inside it.
    pub fn begin_named(&mut self, project: &Project, label: impl Into<String>) {
        let label = Self::normalize_label(label.into());
        match &mut self.pending {
            None => {
                self.pending = Some(HistoryEntry {
                    project: project.clone(),
                    label,
                    command_step: None,
                });
            }
            Some(pending)
                if pending.label == Self::DEFAULT_LABEL && label != Self::DEFAULT_LABEL =>
            {
                pending.label = label;
            }
            Some(_) => {}
        }
    }

    /// Set or replace the label of the open pending transaction, if any.
    pub fn set_label(&mut self, label: impl Into<String>) {
        let label = Self::normalize_label(label.into());
        if let Some(pending) = &mut self.pending {
            pending.label = label;
        }
    }

    /// Explicitly attach a replayable `AuthoringCommandStep` to the open pending transaction
    /// (or the most recently committed undo step if no transaction is open).
    pub fn record_command_step(&mut self, step: AuthoringCommandStep) {
        if let Some(pending) = &mut self.pending {
            pending.command_step = Some(step);
        } else if let Some(last) = self.undo.last_mut() {
            last.command_step = Some(step);
        }
    }

    /// Commit after letting `normalize` adjust the edited project with the
    /// pre-edit snapshot (document housekeeping that depends on what changed).
    pub fn commit_with(
        &mut self,
        project: &mut Project,
        normalize: impl FnOnce(&Project, &mut Project),
    ) -> bool {
        if let Some(before) = &self.pending {
            normalize(&before.project, project);
        }
        self.commit(project)
    }

    pub fn commit(&mut self, project: &Project) -> bool {
        let Some(mut entry) = self.pending.take() else {
            return false;
        };
        if entry.project == *project {
            return false;
        }
        if entry.command_step.is_none() {
            entry.command_step = infer_command_step(&entry.project, project);
        }
        share_resources_from(project, &mut entry.project);
        if let Some(prev) = self.undo.last() {
            share_resources_from(&prev.project, &mut entry.project);
        }
        self.redo.clear();
        self.revision = next_revision();
        if self.capacity > 0 {
            self.undo.push(entry);
            if self.undo.len() > self.capacity {
                self.undo.remove(0);
            }
        }
        true
    }

    pub fn cancel(&mut self, project: &mut Project) -> bool {
        if let Some(entry) = self.pending.take() {
            *project = entry.project;
            self.revision = next_revision();
            true
        } else {
            false
        }
    }

    pub fn undo(&mut self, project: &mut Project) -> bool {
        self.commit(project);
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        let current = std::mem::replace(project, entry.project);
        self.redo.push(HistoryEntry {
            project: current,
            label: entry.label,
            command_step: entry.command_step,
        });
        self.revision = next_revision();
        true
    }

    pub fn redo(&mut self, project: &mut Project) -> bool {
        self.commit(project);
        let Some(entry) = self.redo.pop() else {
            return false;
        };
        let current = std::mem::replace(project, entry.project);
        self.undo.push(HistoryEntry {
            project: current,
            label: entry.label,
            command_step: entry.command_step,
        });
        self.revision = next_revision();
        true
    }

    /// Return the replayable `AuthoringCommandStep` recorded for 0-based undo step `index`.
    pub fn step_command(&self, index: usize) -> Option<&AuthoringCommandStep> {
        self.undo.get(index).and_then(|e| e.command_step.as_ref())
    }

    /// Return the most recently committed replayable `AuthoringCommandStep`.
    pub fn last_command_step(&self) -> Option<&AuthoringCommandStep> {
        self.undo.iter().rev().find_map(|e| e.command_step.as_ref())
    }

    /// Collect replayable `AuthoringCommandStep`s for the requested 0-based undo step indices.
    pub fn collect_command_steps(
        &self,
        indices: &[usize],
    ) -> Result<Vec<AuthoringCommandStep>, String> {
        if indices.is_empty() || indices.len() > MAX_COMMAND_STEPS {
            return Err(format!(
                "Select 1..={MAX_COMMAND_STEPS} history steps to replay or save."
            ));
        }
        let mut steps = Vec::with_capacity(indices.len());
        for &idx in indices {
            let Some(entry) = self.undo.get(idx) else {
                return Err(format!("History step {} is out of range.", idx + 1));
            };
            let Some(cmd) = &entry.command_step else {
                return Err(format!(
                    "Step \"{}\" cannot be replayed on a selection.",
                    entry.label
                ));
            };
            steps.push(cmd.clone());
        }
        Ok(steps)
    }

    /// Step backward or forward to `target_undo_len` (`0` = initial baseline before all steps,
    /// `i + 1` = state after 0-based step `i`). Commits any pending transaction first.
    pub fn step_to(&mut self, project: &mut Project, target_undo_len: usize) -> bool {
        self.commit(project);
        let total = self.undo.len() + self.redo.len();
        let target = target_undo_len.min(total);
        if target == self.undo.len() {
            return false;
        }
        while self.undo.len() > target {
            if !self.undo(project) {
                break;
            }
        }
        while self.undo.len() < target {
            if !self.redo(project) {
                break;
            }
        }
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.pending.is_some()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
    pub fn is_empty(&self) -> bool {
        self.undo.is_empty() && self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|entry| entry.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|entry| entry.label.as_str())
    }
    pub fn undo_labels(&self) -> impl ExactSizeIterator<Item = &str> {
        self.undo.iter().map(|entry| entry.label.as_str())
    }
    pub fn redo_labels(&self) -> impl ExactSizeIterator<Item = &str> {
        self.redo.iter().rev().map(|entry| entry.label.as_str())
    }
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.revision = next_revision();
    }

    /// Changes whenever a committed edit, Undo, Redo, cancel or clear changes the document this
    /// history tracks; unique across all histories in the process. Caches of rendered document
    /// content key on it.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResourceUnit;
    use std::sync::Arc;

    #[test]
    fn history_deduplicates_resource_arc_buffers_across_snapshots() {
        let mut project = Project::blank();
        project.format_version = 3;
        let bytes = vec![42u8; 4096];
        let unit =
            ResourceUnit::from_bytes("res-1", "images/hero.png", "image/png", &bytes).unwrap();
        project.resources.push(unit);

        let mut history = History::new(50);
        // Simulate a round-trip / distinct Arc<str> allocation with identical base64 content.
        history.begin(&project);
        project.name = "Edited".into();
        project.resources[0].data_base64 = Arc::from(project.resources[0].data_base64.as_ref());
        assert!(history.commit(&project));

        let live_arc = project.resources[0].data_base64.clone();
        assert!(history.undo(&mut project));
        assert!(
            Arc::ptr_eq(&project.resources[0].data_base64, &live_arc),
            "Undo snapshot must share the same Arc<str> buffer pointer"
        );
    }
}
