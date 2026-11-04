//! Workspace layout transitions. Naming and selection controls await NF2 approval.
use crate::app::Editor;
use eframe::egui;

impl Editor {
    /// Capture the live tab along with the persisted window/dock state; errors are atomic.
    #[allow(dead_code)]
    pub(crate) fn save_named_workspace(
        &mut self,
        name: &str,
        overwrite: bool,
    ) -> Result<(), crate::workspace::WorkspacePresetError> {
        let mut workspace = self.workspace.clone();
        workspace.active_bottom = self.bottom;
        workspace.store_named_preset(name, overwrite)?;
        self.workspace = workspace;
        Ok(())
    }

    /// Apply a saved layout without finishing edits or changing document/Undo state.
    #[allow(dead_code)]
    pub(crate) fn apply_named_workspace(&mut self, ctx: &egui::Context, name: &str) -> bool {
        if !self.workspace.apply_named_preset(name) {
            return false;
        }
        self.panels_hidden = false;
        self.reset_workspace_layout_state(ctx);
        true
    }

    /// Recreate native windows with the newly selected geometry and clear only owned dock sizes.
    pub(crate) fn reset_workspace_layout_state(&mut self, ctx: &egui::Context) {
        self.detached = crate::detached::DetachedState::default();
        self.bottom = self.workspace.active_bottom;
        // Drafts, focus and arbitrary egui memory belong to the authoring session, not its layout.
        ctx.data_mut(|data| {
            for id in [
                "timeline",
                "tools",
                "library",
                "properties",
                "dock-edge-top",
                "dock-edge-bottom",
                "dock-edge-left",
                "dock-edge-right",
            ] {
                data.remove::<egui::containers::panel::PanelState>(egui::Id::new(id));
            }
        });
        ctx.request_repaint();
    }
}
