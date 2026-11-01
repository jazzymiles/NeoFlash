use crate::Project;
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide source of history revisions, so a replaced `History` never repeats one.
static REVISIONS: AtomicU64 = AtomicU64::new(1);

fn next_revision() -> u64 {
    REVISIONS.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug, PartialEq)]
struct HistoryEntry {
    project: Project,
    label: String,
}

/// Snapshot history with explicit transaction boundaries. A drag is one transaction.
#[derive(Debug)]
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
        let Some(entry) = self.pending.take() else {
            return false;
        };
        if entry.project == *project {
            return false;
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
        });
        self.revision = next_revision();
        true
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
