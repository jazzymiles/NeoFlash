//! Ephemeral transport state. The serialized document stays authored and pure.
use crate::app::Editor;
use neoflash_core::{
    AuthoredDraw, AuthoredPlayback, AuthoredScene, Id, Project, RuntimeClip, SoundCommand,
    evaluate_scene,
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct AuthoringPreview {
    project: Project,
    symbol: Id,
    playback: AuthoredPlayback,
    /// Distinguishes playback sessions; a new session silences the previous one's sounds.
    epoch: u64,
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
            static EPOCH: AtomicU64 = AtomicU64::new(1);
            self.authored_preview = Some(AuthoringPreview {
                playback: AuthoredPlayback::new(&self.project, &symbol, self.frame),
                project: self.project.clone(),
                symbol,
                epoch: EPOCH.fetch_add(1, Ordering::Relaxed),
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

    /// The playing preview session, if any.
    pub(crate) fn preview_epoch(&self) -> Option<u64> {
        self.authored_preview
            .as_ref()
            .filter(|_| self.playing)
            .map(|preview| preview.epoch)
    }

    /// Timeline sound commands of the playing preview since the last call.
    pub(crate) fn drain_preview_sounds(&mut self) -> Vec<SoundCommand> {
        match &mut self.authored_preview {
            Some(preview) if self.playing => preview.playback.drain_sound_commands(),
            _ => vec![],
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

    /// When `Edit multiple frames` is active, merge placements that exist only on other keyframes
    /// inside `onion_range()` (nearest keyframe first) so they can be hit-tested, marquee-selected,
    /// and bounded without duplicating objects already present on `self.frame`.
    pub(crate) fn interactive_authored_scene(
        &self,
        base: AuthoredScene,
        show_masking: bool,
    ) -> AuthoredScene {
        if !self.timeline_edit.edit_multiple {
            return base;
        }
        let mut merged = base;
        let mut seen_top: HashSet<Id> = merged
            .draws
            .iter()
            .filter_map(|d| d.item.instance_path.first().cloned())
            .collect();
        let mut frames = self.edit_multiple_key_frames();
        frames.retain(|&f| f != self.frame);
        frames.sort_by_key(|&f| (f.abs_diff(self.frame), f));
        let symbol_id = self.symbol_id();
        for f in frames {
            let ghost = evaluate_scene(&self.project, symbol_id, f, true, show_masking);
            let mut added_this_frame = HashSet::new();
            let mut used_clips = HashSet::new();
            for mut draw in ghost.draws {
                let Some(top) = draw.item.instance_path.first() else {
                    continue;
                };
                if seen_top.contains(top) {
                    continue;
                }
                added_this_frame.insert(top.clone());
                draw.clip_ids = draw
                    .clip_ids
                    .into_iter()
                    .map(|cid| {
                        let prefixed = format!("@emf-{f}:{cid}");
                        used_clips.insert((cid, prefixed.clone()));
                        prefixed
                    })
                    .collect();
                merged.draws.push(AuthoredDraw { ..draw });
            }
            if !used_clips.is_empty() {
                for clip in ghost.clips {
                    if let Some((_, prefixed)) =
                        used_clips.iter().find(|(orig, _)| *orig == clip.id)
                    {
                        merged.clips.push(RuntimeClip {
                            id: prefixed.clone(),
                            ..clip
                        });
                    }
                }
            }
            seen_top.extend(added_this_frame);
        }
        merged
    }

    pub(crate) fn presented_interactive_authored_scene(&self, show_masking: bool) -> AuthoredScene {
        let base = self.presented_authored_scene(show_masking);
        self.interactive_authored_scene(base, show_masking)
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
