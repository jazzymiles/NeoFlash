#![allow(unused_imports)]
use super::*;
use crate::theme::{ScrollTheme as _, ThemeMode};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline::{
    self as tl, GridKind, LayerColumns, LayerKind, LayerRow, LayersHeaderColumns, ModeCell,
};
use neoflash_ui::icons::{self, Icon, i16};
use neoflash_ui::tokens::{Palette, TextStyle, radius, space};
use neoflash_ui::typography::rich;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────────────────────────────────────────
// 8b. Standalone Native Application (`NeoFlash UI Studio.app`)
// ─────────────────────────────────────────────────────────────────────────────

/// Standalone native `eframe::App` for `NeoFlash UI Studio.app`.
pub struct StandaloneUiStudioApp {
    pub state: UiLayoutStudioState,
    pub theme: ThemeMode,
    /// Density of the Studio's own chrome (`NEOFLASH_SCREENSHOT_DENSITY=compact|classic`).
    density: neoflash_ui::metrics::Density,
    frame_index: u64,
}

impl StandaloneUiStudioApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut defs = egui::FontDefinitions::default();
        neoflash_ui::typography::add_fonts(&mut defs);
        cc.egui_ctx.set_fonts(defs);
        let density = match std::env::var("NEOFLASH_SCREENSHOT_DENSITY").as_deref() {
            Ok("compact") => neoflash_ui::metrics::Density::Compact,
            Ok("classic") => neoflash_ui::metrics::Density::ClassicFlash,
            _ => neoflash_ui::metrics::Density::Comfortable,
        };
        neoflash_ui::metrics::set_density(density);
        crate::theme::initialize_editor_style(&cc.egui_ctx);
        let forced_theme = crate::shell::screenshot_theme();
        let mut theme = forced_theme.unwrap_or_default();
        let mut state = UiLayoutStudioState {
            open: true,
            preview_theme: theme,
            preview_theme_locked: forced_theme.is_some(),
            ..Default::default()
        };
        let json_path = Path::new(DEFAULT_STUDIO_JSON_PATH);
        if json_path.exists()
            && std::env::var_os("NEOFLASH_SCREENSHOT").is_none()
            && state.load_studio_json_initial(json_path).is_ok()
        {
            theme = state.preview_theme;
            state.preview_theme_locked = true;
        }
        if std::env::var_os("NEOFLASH_STUDIO_COMPARE_THEMES").is_some() {
            state.open_four_theme_comparison();
        }
        super::apply_screenshot_overrides(&mut state);
        theme.apply(&cc.egui_ctx);
        Self {
            state,
            theme,
            density,
            frame_index: 0,
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if crate::panels::dialog_helpers::blocks_local_dialog_enter(ctx) {
            return;
        }
        let mut do_undo = false;
        let mut do_redo = false;
        let mut do_dup = false;
        let mut do_del = false;
        let mut do_save = false;
        let mut do_export = false;
        ctx.input_mut(|i| {
            if i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) {
                do_redo = true;
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) {
                do_undo = true;
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::D) {
                do_dup = true;
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::S) {
                do_save = true;
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::E) {
                do_export = true;
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
            {
                do_del = true;
            }
        });
        if do_undo {
            self.state.undo();
        } else if do_redo {
            self.state.redo();
        } else if do_dup {
            let sel = self.state.primary_selected_id();
            if sel != self.state.document.root.id {
                self.state.commit_history("Duplicate node");
                if let Some(dup_id) = self.state.document.duplicate_node(sel) {
                    self.state.select_single(dup_id);
                    self.state.status_message = "Duplicated selected node.".to_owned();
                }
            }
        } else if do_del {
            let targets: Vec<UiNodeId> = self
                .state
                .selected_nodes
                .iter()
                .copied()
                .filter(|&id| id != self.state.document.root.id)
                .collect();
            if !targets.is_empty() {
                self.state.commit_history("Delete node(s)");
                let count = self.state.document.delete_nodes(&targets);
                self.state.select_single(self.state.document.root.id);
                self.state.status_message = format!("Deleted {count} node(s).");
            }
        } else if do_save {
            self.state.save_json_to_default_path();
        } else if do_export {
            self.state.export_code_to_default_paths();
        }
    }
}

impl eframe::App for StandaloneUiStudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_index = self.frame_index.saturating_add(1);
        neoflash_ui::metrics::set_density(self.density);
        if ctx.style().spacing.interact_size.y != neoflash_ui::metrics::control() {
            crate::theme::initialize_editor_style(ctx);
        }
        self.handle_shortcuts(ctx);
        let nf = neoflash_ui::palette(ctx);
        // The window title names the app; the Studio toolbar is the only place with theme controls.
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(nf.bg_panel))
            .show(ctx, |ui| {
                ui_layout_studio_panel_ui(ui, &mut self.state);
                if self.state.preview_theme != self.theme {
                    self.theme = self.state.preview_theme;
                    self.theme.apply(ctx);
                }
            });
        super::persist_studio_session(ctx, &self.state);
        crate::shell::capture(ctx, self.frame_index);
    }
}
