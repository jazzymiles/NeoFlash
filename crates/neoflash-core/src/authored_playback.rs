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
}

impl AuthoredPlayback {
    pub fn new(project: &Project, symbol: &str, start_frame: u32) -> Self {
        // A scoped player shares the runtime's placement clocks but does not
        // prepare bitmap pixels. Native textures consume immutable resources.
        let mut player = Player::with_source_root(project, Some(symbol.to_owned()));
        if start_frame != 0 {
            player.seek(project, start_frame);
        }
        let mut preview = Self {
            symbol: symbol.to_owned(),
            player,
            button_root: project
                .symbol(symbol)
                .is_some_and(|symbol| symbol.kind == SymbolKind::Button),
        };
        preview.discard_scripts();
        preview
    }

    pub fn tick(&mut self, project: &Project) {
        // Button Up/Over/Down/Hit are selected authoring states, not a timeline
        // animation that should advance automatically through Hit.
        if !self.button_root {
            self.player.tick(project);
        }
        self.discard_scripts();
    }

    pub fn frame(&self) -> u32 {
        self.player.frame()
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
