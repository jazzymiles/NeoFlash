//! Ephemeral transport state. The serialized document stays authored and pure.
use crate::app::Editor;
use neoflash_core::{AuthoredPlayback, AuthoredScene, Id, Project, evaluate_scene};

pub(crate) struct AuthoringPreview {
    project: Project,
    symbol: Id,
    playback: AuthoredPlayback,
}

impl Editor {
    fn valid_authored_preview(&self) -> Option<&AuthoringPreview> {
        self.authored_preview.as_ref().filter(|preview| {
            preview.symbol == self.symbol_id()
                && preview.playback.frame() == self.frame
                && preview.project == self.project
        })
    }

    pub(crate) fn reset_authored_preview(&mut self) {
        self.authored_preview = None;
        self.accumulator = 0.;
    }

    pub(crate) fn sync_authored_preview(&mut self) {
        // Also catches direct inspector edits, undo and document replacement.
        // Resource payloads share their immutable storage with this snapshot.
        if self.authored_preview.is_some() && self.valid_authored_preview().is_none() {
            self.reset_authored_preview();
        }
        if self.playing && self.authored_preview.is_none() {
            let symbol = self.symbol_id().to_owned();
            self.authored_preview = Some(AuthoringPreview {
                playback: AuthoredPlayback::new(&self.project, &symbol, self.frame),
                project: self.project.clone(),
                symbol,
            });
        }
    }

    pub(crate) fn tick_authored_preview(&mut self) {
        if !self.playing {
            return;
        }
        self.sync_authored_preview();
        if let Some(preview) = &mut self.authored_preview {
            preview.playback.tick(&self.project);
            self.frame = preview.playback.frame();
        }
    }

    pub(crate) fn current_authored_scene(&mut self, show_masking: bool) -> AuthoredScene {
        self.sync_authored_preview();
        self.authored_scene_from_preview(self.authored_preview.as_ref(), show_masking)
    }

    /// Picking uses the same clocks and masks as painting, including a paused
    /// preview. A direct edit must not expose a stale presentation to callers.
    pub(crate) fn presented_authored_scene(&self, show_masking: bool) -> AuthoredScene {
        self.authored_scene_from_preview(self.valid_authored_preview(), show_masking)
    }

    fn authored_scene_from_preview(
        &self,
        preview: Option<&AuthoringPreview>,
        show_masking: bool,
    ) -> AuthoredScene {
        match preview {
            Some(preview) => preview.playback.scene(&self.project, true, show_masking),
            None => evaluate_scene(
                &self.project,
                self.symbol_id(),
                self.frame,
                true,
                show_masking,
            ),
        }
    }
}
