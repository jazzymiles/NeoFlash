use crate::Project;
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide source of history revisions, so a replaced `History` never repeats one.
static REVISIONS: AtomicU64 = AtomicU64::new(1);

fn next_revision() -> u64 {
    REVISIONS.fetch_add(1, Ordering::Relaxed)
}

/// Snapshot history with explicit transaction boundaries. A drag is one transaction.
#[derive(Debug)]
pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
    pending: Option<Project>,
    capacity: usize,
    revision: u64,
}

impl Default for History {
    fn default() -> Self {
        Self::new(100)
    }
}

impl History {
    pub fn new(capacity: usize) -> Self {
        Self {
            undo: vec![],
            redo: vec![],
            pending: None,
            capacity,
            revision: next_revision(),
        }
    }

    /// Repeated begin calls preserve the original snapshot until commit or cancel.
    pub fn begin(&mut self, project: &Project) {
        if self.pending.is_none() {
            self.pending = Some(project.clone());
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
            normalize(before, project);
        }
        self.commit(project)
    }

    pub fn commit(&mut self, project: &Project) -> bool {
        let Some(before) = self.pending.take() else {
            return false;
        };
        if before == *project {
            return false;
        }
        self.redo.clear();
        self.revision = next_revision();
        if self.capacity > 0 {
            self.undo.push(before);
            if self.undo.len() > self.capacity {
                self.undo.remove(0);
            }
        }
        true
    }

    pub fn cancel(&mut self, project: &mut Project) -> bool {
        if let Some(before) = self.pending.take() {
            *project = before;
            self.revision = next_revision();
            true
        } else {
            false
        }
    }

    pub fn undo(&mut self, project: &mut Project) -> bool {
        self.commit(project);
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(project, previous));
        self.revision = next_revision();
        true
    }

    pub fn redo(&mut self, project: &mut Project) -> bool {
        self.commit(project);
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(project, next));
        self.revision = next_revision();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.pending.is_some()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
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
