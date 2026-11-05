//! Ephemeral transport state. The serialized document stays authored and pure.
use crate::app::Editor;
use neoflash_core::{
    AuthoredDraw, AuthoredPlayback, AuthoredScene, Id, Project, SoundCommand, evaluate_scene,
    evaluate_scene_with_buttons,
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
        if self.playing {
            self.clear_simple_button_state();
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
        let eval_project = self.project_for_stage_eval();
        for f in frames {
            let ghost = evaluate_scene(&eval_project, symbol_id, f, true, show_masking);
            let mut added_this_frame = HashSet::new();
            let prefix = |id: &str| format!("@emf-{f}:{id}");
            for mut draw in ghost.draws {
                let Some(top) = draw.item.instance_path.first() else {
                    continue;
                };
                if seen_top.contains(top) {
                    continue;
                }
                added_this_frame.insert(top.clone());
                draw.clip_ids = draw.clip_ids.iter().map(|id| prefix(id)).collect();
                draw.mask_group = draw.mask_group.as_deref().map(prefix);
                merged.draws.push(AuthoredDraw { ..draw });
            }
            if !added_this_frame.is_empty() {
                // Keep the transitive mask/group graph: an outer source may depend on a mask
                // that does not appear directly in any visible draw's clip list.
                for mut clip in ghost.clips {
                    clip.id = prefix(&clip.id);
                    for component in &mut clip.components {
                        component.clip_ids =
                            component.clip_ids.iter().map(|id| prefix(id)).collect();
                    }
                    for component in clip.alpha_components.iter_mut().flatten() {
                        component.clip_ids =
                            component.clip_ids.iter().map(|id| prefix(id)).collect();
                    }
                    merged.clips.push(clip);
                }
                for mut group in ghost.mask_groups {
                    group.id = prefix(&group.id);
                    group.parent_id = group.parent_id.as_deref().map(prefix);
                    group.clip_ids = group.clip_ids.iter().map(|id| prefix(id)).collect();
                    merged.mask_groups.push(group);
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

    /// Active `SymbolKind::Button` frame (`0` = Up, `1` = Over, `2` = Down) for top-level button
    /// instances on the authoring stage when `Control ▸ Enable simple buttons` (`⌥⌘B`) is active.
    pub(crate) fn simple_button_frame(&self, path: &[Id]) -> Option<u32> {
        if !self.enable_simple_buttons || self.playing || path.len() != 1 {
            return None;
        }
        let id = &path[0];
        if let Some(pressed) = &self.pressed_simple_button {
            if pressed == id {
                return Some(if self.hovered_simple_button.as_ref() == Some(id) {
                    2
                } else {
                    1
                });
            }
            return Some(0);
        }
        if self.hovered_simple_button.as_ref() == Some(id) {
            Some(1)
        } else {
            Some(0)
        }
    }

    fn authored_scene_from_preview(
        &self,
        preview: Option<&AuthoringPreview>,
        show_masking: bool,
    ) -> AuthoredScene {
        let eval_project = self.project_for_stage_eval();
        let simple_buttons_active = self.enable_simple_buttons
            && !self.playing
            && (preview.is_none()
                || self.hovered_simple_button.is_some()
                || self.pressed_simple_button.is_some());
        if simple_buttons_active {
            return evaluate_scene_with_buttons(
                &eval_project,
                self.symbol_id(),
                self.frame,
                true,
                show_masking,
                &|path| self.simple_button_frame(path),
            );
        }
        match preview {
            Some(preview) => preview.playback.scene(&eval_project, true, show_masking),
            None => evaluate_scene(
                &eval_project,
                self.symbol_id(),
                self.frame,
                true,
                show_masking,
            ),
        }
    }
}
