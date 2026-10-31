//! `NF2/View Settings` popover — rulers, guides & grid (spec ST-06 / PP-12). Opened by ⚙ in the
//! zoom control, View ▸ Rulers, guides & grid…, View ▸ Grid ▸ Edit grid… (⌥⌘G) and View ▸ Guides ▸
//! Edit guides…. Edits apply live to `StageView::prefs` (stage_view.rs); Clear guides is one Undo step;
//! Save as default stores the settings in the editor preferences (`WorkspacePrefs::view_defaults`).
use crate::app::Editor;
use eframe::egui::{self, Rect, Vec2};
use neoflash_ui::components::ViewSettings;

impl Editor {
    /// `NF2/View Settings` popover (⚙ in the zoom control, View ▸ Rulers, guides & grid…).
    pub(crate) fn view_settings_popover(&mut self, ctx: &egui::Context, canvas: Rect) {
        if !self.stage_view.settings_open {
            return;
        }
        let p = neoflash_ui::palette(ctx);
        let button = self.stage_view.settings_button.unwrap_or_else(|| {
            Rect::from_center_size(
                canvas.center_bottom() - Vec2::new(0., 40.),
                Vec2::splat(32.),
            )
        });
        let mut state = self.stage_view.prefs.to_state(p);
        let response = ViewSettings { state: &mut state }.popover(
            ctx,
            egui::Id::new("nf2-view-settings"),
            button,
            ctx.content_rect(),
        );
        self.stage_view.prefs.apply_state(&state, p);
        if response.clear_guides {
            self.clear_guides();
        }
        if response.save_default {
            self.workspace.view_defaults = Some(self.stage_view.prefs.clone());
            self.message("Rulers, guides and grid settings saved as the default");
        }
        if response.close {
            self.stage_view.settings_open = false;
        }
    }
}
