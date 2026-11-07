//! Stateful authoring transport using the shared timeline clocks. The wrapper
//! deliberately exposes neither script dispatch nor mutable runtime objects.
use crate::{AuthoredScene, Id, Player, Project, SymbolKind};

/// Independent MovieClips advance even when their parent has only one frame.
/// Pausing means retaining this value without calling `tick`; recreating it after
/// a scrub/edit starts a fresh placement lifetime. Artwork and source stay pure.
pub struct AuthoredPlayback {
    symbol: Id,
    player: Player,
    button_root: bool,
    looping: bool,
}

impl AuthoredPlayback {
    pub fn new(project: &Project, symbol: &str, start_frame: u32) -> Self {
        // A scoped player shares the runtime's placement clocks but does not
        // prepare bitmap pixels. Native textures consume immutable resources.
        let mut player = Player::with_source_root(project, Some(symbol.to_owned()));
        if start_frame != 0 {
            // Sounds of frame 0 belong to the discarded start, not to this preview.
            player.drain_sound_commands();
            player.seek(project, start_frame);
        }
        let mut preview = Self {
            symbol: symbol.to_owned(),
            player,
            button_root: project
                .symbol(symbol)
                .is_some_and(|symbol| symbol.kind == SymbolKind::Button),
            looping: true,
        };
        preview.discard_scripts();
        preview
    }

    pub fn set_looping(&mut self, looping: bool) {
        // Re-enabling loop after a non-looping preview reached its last frame must also resume
        // the transport that the terminal-frame guard paused.
        if looping && !self.looping {
            self.player.set_playing(true);
        }
        self.looping = looping;
    }

    pub fn is_looping(&self) -> bool {
        self.looping
    }

    pub fn tick(&mut self, project: &Project) {
        // Button Up/Over/Down/Hit are selected authoring states, not a timeline
        // animation that should advance automatically through Hit.
        if !self.button_root {
            if !self.looping {
                let total_frames = project
                    .symbol(&self.symbol)
                    .map_or(1, |s| s.timeline.frame_count.max(1));
                if self.frame() + 1 >= total_frames {
                    self.player.set_playing(false);
                    self.discard_scripts();
                    return;
                }
            }
            self.player.tick(project);
        }
        self.discard_scripts();
    }

    pub fn frame(&self) -> u32 {
        self.player.frame()
    }

    /// Timeline sound commands for the editor's audio preview (see `SoundCommand`).
    pub fn drain_sound_commands(&mut self) -> Vec<crate::SoundCommand> {
        self.player.drain_sound_commands()
    }

    pub fn scene(
        &self,
        project: &Project,
        include_guides: bool,
        show_masking: bool,
    ) -> AuthoredScene {
        let evaluation = crate::evaluate::snapshot_mode(
            project,
            &self.symbol,
            self.frame(),
            &|path| self.player.clip_frame(path),
            &|_| None,
            include_guides,
        );
        // Outline is an editor display preference. Playback shows the authored
        // fills/effects while leaving both the document and static scrub intact.
        crate::authored_layers::scene_from_evaluation(project, evaluation, show_masking, None)
    }

    fn discard_scripts(&mut self) {
        // Player only queues source; it never executes it. Drain every step so
        // arbitrary frame/object handlers cannot accumulate in an editor preview.
        self.player.drain_scripts();
        self.player.runtime_scripts.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_looping_preview_stops_at_last_frame_and_can_resume_looping() {
        let mut project = Project::blank();
        project.symbols[0].timeline.frame_count = 3;
        let root = project.root.clone();
        let mut preview = AuthoredPlayback::new(&project, &root, 0);

        preview.set_looping(false);
        preview.tick(&project);
        preview.tick(&project);
        preview.tick(&project);
        assert_eq!(preview.frame(), 2);

        preview.set_looping(true);
        preview.tick(&project);
        assert_eq!(preview.frame(), 0);
    }
}
