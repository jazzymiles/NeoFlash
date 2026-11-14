//! Interactive Visual UI & System Layout Studio (`Window ▸ Developer ▸ UI Layout Studio…`).
//!
//! Enables visual authoring, inspection, and live system-level tuning of NeoFlash panels,
//! containers, grids, and NF2 design tokens without leaving the editor:
//! - **Sizing in `%` or `px` (`pt`)**: every container (`Row`, `Column`, `Grid`, `Section`, `Panel`)
//!   and leaf control supports switching width and height between `Percent(0.0..=100.0)`,
//!   `Px(pt)`, `Auto`, and `Fill(fr)` with `min`/`max` constraints.
//! - **Paddings, Gaps & Typography**: 4-sided padding/margin, `[gap_x, gap_y]` item spacing
//!   (bound to NF2 `S0`..`S6` tokens or custom `px`), and typography (`System(TextStyle)` or
//!   custom family, size, weight, line height, tracking, alignment, and semantic color role).
//! - **Edit Systems as Systems**: live inspector for the NF2 spacing scale (`S1`..`S6`),
//!   typography scale (`Title`, `Body`, `Small`, `Caption`, `Mono`, `Heading`), density metrics
//!   (`Comfortable`, `Compact`, `Classic Flash`), shared layout systems (`TwoColumnFormSystem`,
//!   `SectionHeaderSystem`, `PanelHeaderSystem`, `ToolRailSystem`), and four-theme palette overrides.
//! - **Structural Container Operations**: wrap selected nodes into a `Row`, `Column`, or
//!   `Grid` (with 1-click 2-col, 3-col, 4-col, and mixed `[120px, 50%, 50%]` presets), unwrap
//!   containers, nest grids inside grid cells to arbitrary depth, create/duplicate/delete widgets,
//!   and reorder with multi-step Undo/Redo.
//! - **Live WYSIWYG Canvas & Build Export**: renders real NF2 components (`neoflash_ui::components`)
//!   with click-to-select outlines and exports both JSON overrides
//!   (`target/ui-studio/ui-layout-overrides.json`) and deterministic Rust layout code
//!   (`target/ui-studio/exported_layout.rs`).

#![allow(dead_code)]

use crate::theme::{ScrollTheme as _, ThemeMode};
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Default path for persisted JSON studio overrides.
pub const DEFAULT_STUDIO_JSON_PATH: &str = "target/ui-studio/ui-layout-overrides.json";
/// Default path for generated Rust layout & system token code.
pub const DEFAULT_STUDIO_RUST_EXPORT_PATH: &str = "target/ui-studio/exported_layout.rs";
/// Default path for exported SVG layout artboards.
pub const DEFAULT_STUDIO_SVG_EXPORT_PATH: &str = "target/ui-studio/exported-layout.svg";

/// Unique identifier for a node inside a [`UiStudioDocument`].
pub type UiNodeId = u64;

mod canvas;
mod document;
mod hierarchy;
mod inspector;
mod io_codegen_svg;
mod layout;
mod presets;
mod standalone;
#[cfg(test)]
mod tests;
mod tokens_and_systems;

use canvas::render_live_canvas_column;
pub use document::*;
use hierarchy::render_hierarchy_and_palette_column;
use inspector::render_inspector_and_systems_column;
pub use io_codegen_svg::*;
pub use layout::*;
pub use presets::*;
pub use standalone::*;
pub use tokens_and_systems::*;

/// Active tab in the right-hand Studio Inspector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioInspectorTab {
    /// Inspect & edit the selected node's `%`/`px` box sizing, paddings, gaps, typography, and grid tracks.
    #[default]
    NodeInspector,
    /// Edit NF2 Systems as Systems (`Spacing`, `Typography`, `Density`, `Shared Layouts`, `Palette`).
    SystemsTab,
    /// View & export generated Rust NF2 layout code and JSON overrides.
    CodeExport,
}

/// Sub-section inside [`StudioInspectorTab::SystemsTab`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemsSubTab {
    #[default]
    TimelineGeometry,
    WorkspaceAndStage,
    SpacingAndDensity,
    TypographyScale,
    SharedLayouts,
    PaletteTokens,
}

/// A free-positioned, live-resizable panel artboard on the Multi-Panel Studio Canvas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudioArtboard {
    pub id: u32,
    pub preset: StudioPanelPreset,
    pub document: UiStudioDocument,
    /// Position on the free artboard canvas (in canvas coordinates, px).
    pub canvas_pos: [f32; 2],
    /// Artboard width & height on the canvas (user can drag-resize width/height live to test % vs px!).
    pub canvas_size: [f32; 2],
    pub visible: bool,
    /// Optional per-artboard NF2 theme override (used by 4-theme side-by-side comparison).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_override: Option<ThemeMode>,
}

impl StudioArtboard {
    pub fn default_size_for(preset: StudioPanelPreset) -> [f32; 2] {
        match preset {
            StudioPanelPreset::FullWorkspaceShell => [1180.0, 680.0],
            StudioPanelPreset::TimelineStudio => [1310.0, 390.0],
            StudioPanelPreset::LibraryAndAssetsStudio => [290.0, 480.0],
            StudioPanelPreset::ColorAndGradientStudio => [280.0, 480.0],
            StudioPanelPreset::AlignAndTransform => [260.0, 480.0],
            StudioPanelPreset::PropertiesMovieClip => [300.0, 480.0],
            StudioPanelPreset::PropertiesText => [300.0, 480.0],
            StudioPanelPreset::ColorPanel => [280.0, 440.0],
            StudioPanelPreset::ToolsPanel => [84.0, 480.0],
            StudioPanelPreset::FiltersPanel => [290.0, 420.0],
            StudioPanelPreset::TimelineHeader => [620.0, 96.0],
        }
    }

    /// Default startup multi-panel board laying out 6 core NeoFlash panels + timeline without clipping.
    pub fn default_board() -> Vec<Self> {
        let specs = [
            (
                1,
                StudioPanelPreset::PropertiesMovieClip,
                [24.0, 28.0],
                [300.0, 480.0],
            ),
            (
                2,
                StudioPanelPreset::LibraryAndAssetsStudio,
                [348.0, 28.0],
                [290.0, 480.0],
            ),
            (
                3,
                StudioPanelPreset::ColorAndGradientStudio,
                [662.0, 28.0],
                [280.0, 480.0],
            ),
            (
                4,
                StudioPanelPreset::AlignAndTransform,
                [966.0, 28.0],
                [260.0, 480.0],
            ),
            (
                5,
                StudioPanelPreset::ToolsPanel,
                [1250.0, 28.0],
                [84.0, 480.0],
            ),
            (
                6,
                StudioPanelPreset::TimelineStudio,
                [24.0, 540.0],
                [1310.0, 390.0],
            ),
        ];
        specs
            .into_iter()
            .map(|(id, preset, canvas_pos, canvas_size)| Self {
                id,
                preset,
                document: preset.build_document(),
                canvas_pos,
                canvas_size,
                visible: true,
                theme_override: None,
            })
            .collect()
    }
}

fn default_true() -> bool {
    true
}

/// Persisted JSON bundle combining the studio document, multi-panel artboards, and system token scales.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudioPersistedBundle {
    pub document: UiStudioDocument,
    pub systems: UiStudioSystems,
    #[serde(default)]
    pub artboards: Vec<StudioArtboard>,
    #[serde(default)]
    pub active_artboard_idx: usize,
    #[serde(default)]
    pub active_preset: Option<StudioPanelPreset>,
    #[serde(default)]
    pub solo_active_artboard: bool,
    #[serde(default)]
    pub canvas_pan: [f32; 2],
    #[serde(default = "default_canvas_zoom")]
    pub canvas_zoom: f32,
    #[serde(default = "default_true")]
    pub show_wireframe_outlines: bool,
    #[serde(default = "default_true")]
    pub show_dimension_badges: bool,
    #[serde(default)]
    pub inspector_tab: StudioInspectorTab,
    #[serde(default)]
    pub systems_subtab: SystemsSubTab,
    #[serde(default)]
    pub preview_theme: Option<ThemeMode>,
    #[serde(default)]
    pub preview_theme_locked: bool,
}

fn deserialize_studio_theme<'de, D>(deserializer: D) -> Result<ThemeMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StoredTheme {
        Theme(ThemeMode),
        LegacyDarkMode(bool),
    }

    Ok(match StoredTheme::deserialize(deserializer)? {
        StoredTheme::Theme(theme) => theme,
        StoredTheme::LegacyDarkMode(true) => ThemeMode::Dark,
        StoredTheme::LegacyDarkMode(false) => ThemeMode::Light,
    })
}

/// Complete interactive state of the UI Layout Studio (`Window ▸ Developer ▸ UI Layout Studio…`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiLayoutStudioState {
    pub open: bool,
    pub active_preset: StudioPanelPreset,
    pub document: UiStudioDocument,
    pub systems: UiStudioSystems,
    #[serde(default = "StudioArtboard::default_board")]
    pub artboards: Vec<StudioArtboard>,
    #[serde(default)]
    pub active_artboard_idx: usize,
    #[serde(default)]
    pub canvas_pan: [f32; 2],
    #[serde(default = "default_canvas_zoom")]
    pub canvas_zoom: f32,
    #[serde(default)]
    pub solo_active_artboard: bool,
    #[serde(default)]
    pub svg_import_modal_open: bool,
    #[serde(default = "sample_svg_panel_snippet")]
    pub svg_buffer: String,
    #[serde(default)]
    pub preset_to_add: StudioPanelPreset,
    pub selected_nodes: Vec<UiNodeId>,
    pub hovered_node: Option<UiNodeId>,
    pub inspector_tab: StudioInspectorTab,
    pub systems_subtab: SystemsSubTab,
    pub show_wireframe_outlines: bool,
    pub show_dimension_badges: bool,
    pub preview_canvas_width: f32,
    #[serde(
        default,
        alias = "preview_dark_mode",
        deserialize_with = "deserialize_studio_theme"
    )]
    pub preview_theme: ThemeMode,
    /// When true, `preview_theme` was explicitly chosen inside UI Studio and is not overwritten by host frames.
    #[serde(default)]
    pub preview_theme_locked: bool,
    pub status_message: String,
    #[serde(skip)]
    pub history: UiStudioHistory,
    #[serde(skip)]
    pub drag_source_node: Option<UiNodeId>,
    #[serde(skip)]
    last_host_theme: Option<ThemeMode>,
}

fn default_canvas_zoom() -> f32 {
    1.0
}

impl Default for UiLayoutStudioState {
    fn default() -> Self {
        let artboards = StudioArtboard::default_board();
        let preset = artboards[0].preset;
        let document = artboards[0].document.clone();
        let root_id = document.root.id;
        Self {
            open: false,
            active_preset: preset,
            document,
            systems: UiStudioSystems::default(),
            artboards,
            active_artboard_idx: 0,
            canvas_pan: [0.0, 0.0],
            canvas_zoom: 1.0,
            solo_active_artboard: false,
            svg_import_modal_open: false,
            svg_buffer: sample_svg_panel_snippet(),
            preset_to_add: StudioPanelPreset::PropertiesText,
            selected_nodes: vec![root_id],
            hovered_node: None,
            inspector_tab: StudioInspectorTab::NodeInspector,
            systems_subtab: SystemsSubTab::TimelineGeometry,
            show_wireframe_outlines: true,
            show_dimension_badges: true,
            preview_canvas_width: 820.0,
            preview_theme: ThemeMode::Dark,
            preview_theme_locked: false,
            status_message: "Multi-Panel Artboard Canvas ready — drag headers to move, drag bottom-right grips to resize (% vs px), or Import/Export SVG."
                .to_owned(),
            history: UiStudioHistory::new(64),
            drag_source_node: None,
            last_host_theme: None,
        }
    }
}

impl UiLayoutStudioState {
    /// Synchronize `self.document` and `self.active_preset` into the currently active artboard.
    pub fn sync_active_to_artboard(&mut self) {
        if self.artboards.is_empty() {
            self.artboards = StudioArtboard::default_board();
            self.active_artboard_idx = 0;
        }
        let idx = self.active_artboard_idx.min(self.artboards.len() - 1);
        self.active_artboard_idx = idx;
        self.artboards[idx].document = self.document.clone();
        self.artboards[idx].preset = self.active_preset;
    }

    /// Select an artboard on the multi-panel canvas by index and make its document active.
    pub fn select_artboard(&mut self, idx: usize) {
        if idx >= self.artboards.len() {
            return;
        }
        self.sync_active_to_artboard();
        self.active_artboard_idx = idx;
        self.active_preset = self.artboards[idx].preset;
        self.document = self.artboards[idx].document.clone();
        self.selected_nodes = vec![self.document.root.id];
    }

    /// Add a new panel artboard to the multi-panel canvas and select it.
    pub fn add_artboard(&mut self, preset: StudioPanelPreset) -> usize {
        self.sync_active_to_artboard();
        self.commit_history(format!("Add '{}' artboard", preset.label()));
        let next_id = self
            .artboards
            .iter()
            .map(|a| a.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let size = StudioArtboard::default_size_for(preset);
        let count = self.artboards.len();
        let pos = [
            24.0 + ((count % 4) as f32) * 320.0,
            28.0 + ((count / 4) as f32) * 510.0,
        ];
        let doc = preset.build_document();
        self.artboards.push(StudioArtboard {
            id: next_id,
            preset,
            document: doc.clone(),
            canvas_pos: pos,
            canvas_size: size,
            visible: true,
            theme_override: None,
        });
        let new_idx = self.artboards.len() - 1;
        self.active_artboard_idx = new_idx;
        self.active_preset = preset;
        self.document = doc;
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = format!("Added '{}' artboard to canvas.", preset.label());
        new_idx
    }

    /// Duplicate an existing artboard on the canvas.
    pub fn duplicate_artboard(&mut self, idx: usize) -> usize {
        self.sync_active_to_artboard();
        let Some(src) = self.artboards.get(idx).cloned() else {
            return self.active_artboard_idx;
        };
        self.commit_history("Duplicate artboard");
        let next_id = self
            .artboards
            .iter()
            .map(|a| a.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let mut dup = src;
        dup.id = next_id;
        dup.canvas_pos[0] += 32.0;
        dup.canvas_pos[1] += 32.0;
        dup.document.name = format!("{} Copy", dup.document.name);
        self.artboards.push(dup.clone());
        let new_idx = self.artboards.len() - 1;
        self.active_artboard_idx = new_idx;
        self.active_preset = dup.preset;
        self.document = dup.document;
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = "Duplicated artboard on canvas.".to_owned();
        new_idx
    }

    /// Remove an artboard from the canvas (keeping at least one artboard).
    pub fn remove_artboard(&mut self, idx: usize) {
        if self.artboards.len() <= 1 || idx >= self.artboards.len() {
            return;
        }
        self.sync_active_to_artboard();
        self.commit_history("Close artboard");
        let removed_name = self.artboards[idx].document.name.clone();
        self.artboards.remove(idx);
        let next_idx = self.active_artboard_idx.min(self.artboards.len() - 1);
        self.active_artboard_idx = next_idx;
        self.active_preset = self.artboards[next_idx].preset;
        self.document = self.artboards[next_idx].document.clone();
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = format!("Closed artboard '{removed_name}'.");
    }

    /// Resize an artboard on the free canvas (clamped to sensible minimums so it never clips).
    pub fn resize_artboard(&mut self, idx: usize, new_width: f32, new_height: f32) {
        self.sync_active_to_artboard();
        if let Some(ab) = self.artboards.get_mut(idx) {
            ab.canvas_size[0] = new_width.clamp(72.0, 2400.0);
            ab.canvas_size[1] = new_height.clamp(80.0, 1800.0);
            ab.document.root.box_constraints.width = UiSizeUnit::Percent(100.0);
            if idx == self.active_artboard_idx {
                self.document.root.box_constraints.width = UiSizeUnit::Percent(100.0);
            }
        }
    }

    /// Reset to the 6 core NeoFlash panels laid out cleanly on the multi-panel board.
    pub fn open_all_core_artboards(&mut self) {
        self.commit_history("Open All Core Panels");
        self.artboards = StudioArtboard::default_board();
        self.active_artboard_idx = 0;
        self.active_preset = self.artboards[0].preset;
        self.document = self.artboards[0].document.clone();
        self.solo_active_artboard = false;
        self.canvas_pan = [0.0, 0.0];
        self.selected_nodes = vec![self.document.root.id];
        self.status_message =
            "Opened all 6 core NeoFlash panels + Timeline on the artboard canvas.".to_owned();
    }

    /// Explicitly switch the studio preview theme (`Dark`, `Light`, `Mid Neutral Gray`, `Dark Graphite`).
    pub fn set_preview_theme(&mut self, theme: ThemeMode) {
        self.preview_theme = theme;
        self.preview_theme_locked = true;
        self.status_message = format!("Switched UI Studio preview theme to '{}'.", theme.label());
    }

    /// Open a 4-artboard side-by-side comparison of the active preset across all 4 NF2 themes
    /// (`Dark`, `Light`, `Mid Neutral Gray`, and `Dark Graphite`).
    pub fn open_four_theme_comparison(&mut self) {
        self.sync_active_to_artboard();
        self.commit_history("Compare 4 Themes");
        let preset = self.active_preset;
        let base_doc = self.document.clone();
        let default_size = StudioArtboard::default_size_for(preset);
        let is_wide_surface = preset.is_full_shell() || preset.is_timeline_surface();
        let size = if is_wide_surface {
            [default_size[0].min(660.0), default_size[1].min(440.0)]
        } else {
            default_size
        };
        let gap_x = 24.0_f32;
        let gap_y = 40.0_f32;
        let mut artboards = Vec::with_capacity(ThemeMode::ALL.len());
        for (i, theme) in ThemeMode::ALL.into_iter().enumerate() {
            let mut doc = base_doc.clone();
            doc.name = format!("{} — {}", preset.label(), theme.label());
            let pos = if is_wide_surface {
                let col = (i % 2) as f32;
                let row = (i / 2) as f32;
                [
                    24.0 + col * (size[0] + gap_x),
                    28.0 + row * (size[1] + gap_y),
                ]
            } else {
                [24.0 + (i as f32) * (size[0] + gap_x), 28.0]
            };
            artboards.push(StudioArtboard {
                id: (i as u32) + 1,
                preset,
                document: doc,
                canvas_pos: pos,
                canvas_size: size,
                visible: true,
                theme_override: Some(theme),
            });
        }
        self.artboards = artboards;
        self.active_artboard_idx = 0;
        self.document = self.artboards[0].document.clone();
        self.solo_active_artboard = false;
        self.canvas_pan = [0.0, 0.0];
        self.canvas_zoom = 1.0;
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = format!(
            "Comparing '{}' across all 4 NF2 themes (Dark, Light, Mid Neutral Gray, Dark Graphite).",
            preset.label()
        );
    }

    /// Auto-arrange all visible artboards into a non-overlapping multi-row grid on the canvas.
    pub fn auto_arrange_artboards(&mut self) {
        self.sync_active_to_artboard();
        self.commit_history("Auto-arrange artboards");
        let gap_x = 24.0_f32;
        let gap_y = 32.0_f32;
        let max_row_w = 1360.0_f32;
        let mut cursor_x = 24.0_f32;
        let mut cursor_y = 28.0_f32;
        let mut row_max_h = 0.0_f32;

        for ab in &mut self.artboards {
            if !ab.visible {
                continue;
            }
            if cursor_x > 24.0 && cursor_x + ab.canvas_size[0] > max_row_w {
                cursor_x = 24.0;
                cursor_y += row_max_h + gap_y;
                row_max_h = 0.0;
            }
            ab.canvas_pos = [cursor_x, cursor_y];
            cursor_x += ab.canvas_size[0] + gap_x;
            row_max_h = row_max_h.max(ab.canvas_size[1]);
        }
        self.canvas_pan = [0.0, 0.0];
        self.status_message = "Auto-arranged all visible artboards on canvas.".to_owned();
    }

    /// Load one of the built-in NeoFlash panel presets (`Properties — Movie Clip`, `Color`, `Tools`, etc.).
    pub fn load_preset(&mut self, preset: StudioPanelPreset) {
        self.sync_active_to_artboard();
        self.commit_history(format!("Load preset {}", preset.label()));
        let doc = preset.build_document();
        if let Some(existing_idx) = self.artboards.iter().position(|a| a.preset == preset) {
            self.active_artboard_idx = existing_idx;
            self.artboards[existing_idx].visible = true;
            self.artboards[existing_idx].document = doc.clone();
        } else if !self.artboards.is_empty() {
            let idx = self.active_artboard_idx.min(self.artboards.len() - 1);
            self.active_artboard_idx = idx;
            self.artboards[idx].preset = preset;
            self.artboards[idx].document = doc.clone();
            self.artboards[idx].canvas_size = StudioArtboard::default_size_for(preset);
        }
        self.active_preset = preset;
        self.document = doc;
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = format!("Loaded preset '{}'.", preset.label());
    }

    /// Capture an atomic snapshot covering `document`, `systems`, `artboards`, and view state.
    pub fn capture_snapshot(&mut self, action_label: impl Into<String>) -> UiStudioSnapshot {
        self.sync_active_to_artboard();
        UiStudioSnapshot {
            label: action_label.into(),
            document: self.document.clone(),
            systems: self.systems.clone(),
            artboards: self.artboards.clone(),
            active_artboard_idx: self.active_artboard_idx,
            solo_active_artboard: self.solo_active_artboard,
            canvas_pan: self.canvas_pan,
            canvas_zoom: self.canvas_zoom,
        }
    }

    /// Push a previously captured snapshot onto the Undo history stack.
    pub fn push_history_snapshot(&mut self, snapshot: UiStudioSnapshot) {
        self.history.push_snapshot_entry(snapshot);
    }

    /// Record an undoable snapshot before mutating `document`, `systems`, or `artboards`.
    pub fn commit_history(&mut self, action_label: impl Into<String>) {
        let snapshot = self.capture_snapshot(action_label);
        self.history.push_snapshot_entry(snapshot);
    }

    pub fn undo(&mut self) -> bool {
        self.sync_active_to_artboard();
        if let Some(lbl) = self.history.undo_studio(
            &mut self.document,
            &mut self.systems,
            &mut self.artboards,
            &mut self.active_artboard_idx,
            &mut self.solo_active_artboard,
            &mut self.canvas_pan,
            &mut self.canvas_zoom,
        ) {
            if let Some(ab) = self.artboards.get(self.active_artboard_idx) {
                self.active_preset = ab.preset;
            }
            self.sync_active_to_artboard();
            self.selected_nodes = vec![self.document.root.id];
            self.status_message = format!("Undo: {lbl}");
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        self.sync_active_to_artboard();
        if let Some(lbl) = self.history.redo_studio(
            &mut self.document,
            &mut self.systems,
            &mut self.artboards,
            &mut self.active_artboard_idx,
            &mut self.solo_active_artboard,
            &mut self.canvas_pan,
            &mut self.canvas_zoom,
        ) {
            if let Some(ab) = self.artboards.get(self.active_artboard_idx) {
                self.active_preset = ab.preset;
            }
            self.sync_active_to_artboard();
            self.selected_nodes = vec![self.document.root.id];
            self.status_message = format!("Redo: {lbl}");
            true
        } else {
            false
        }
    }

    pub fn primary_selected_id(&self) -> UiNodeId {
        self.selected_nodes
            .first()
            .copied()
            .filter(|&id| self.document.find_node(id).is_some())
            .unwrap_or(self.document.root.id)
    }

    pub fn select_single(&mut self, id: UiNodeId) {
        self.selected_nodes.clear();
        self.selected_nodes.push(id);
    }

    pub fn toggle_multi_select(&mut self, id: UiNodeId) {
        if let Some(pos) = self.selected_nodes.iter().position(|&x| x == id) {
            if self.selected_nodes.len() > 1 {
                self.selected_nodes.remove(pos);
            }
        } else {
            self.selected_nodes.push(id);
        }
    }

    /// Export the active artboard to SVG XML.
    pub fn export_active_svg(&self) -> String {
        export_artboard_to_svg(&self.document, &self.systems, self.preview_theme)
    }

    /// Export all visible artboards on the multi-panel board into a combined SVG document.
    pub fn export_board_svg(&self) -> String {
        export_all_artboards_to_svg(self, self.preview_theme)
    }

    /// Write the active or multi-panel SVG export to disk (`target/ui-studio/exported_panel.svg`).
    pub fn save_svg_to_file(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        let svg = if self.solo_active_artboard || self.artboards.len() <= 1 {
            self.export_active_svg()
        } else {
            self.export_board_svg()
        };
        fs::write(path, svg).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        Ok(path.to_path_buf())
    }

    /// Parse an SVG string (either NeoFlash round-trip SVG — single or multi-artboard — or raw Figma/Illustrator SVG) and add it as live editable artboard(s) on the canvas.
    pub fn import_svg_as_artboard(&mut self, svg_text: &str) -> Result<usize, String> {
        self.sync_active_to_artboard();
        let trimmed = svg_text.trim();
        let docs = if trimmed.contains("<svg") && trimmed.contains("data-nf-kind=\"") {
            import_roundtrip_nf_svg_all(trimmed)?
        } else {
            vec![(import_svg_to_document(svg_text)?, None)]
        };
        let first_name = docs[0].0.name.clone();
        self.commit_history(format!("Import SVG '{first_name}'"));
        let mut first_new_idx = None;
        for (doc, theme_override) in docs {
            let next_id = self
                .artboards
                .iter()
                .map(|a| a.id)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let w = match doc.root.box_constraints.width {
                UiSizeUnit::Px(px) => px.clamp(160.0, 1400.0),
                _ => 320.0,
            };
            let h = match doc.root.box_constraints.height {
                UiSizeUnit::Px(px) => px.clamp(160.0, 1200.0),
                _ => 460.0,
            };
            let count = self.artboards.len();
            let pos = [
                48.0 + ((count % 4) as f32) * 310.0,
                48.0 + ((count / 4) as f32) * 460.0,
            ];
            let preset = StudioPanelPreset::PropertiesMovieClip;
            self.artboards.push(StudioArtboard {
                id: next_id,
                preset,
                document: doc.clone(),
                canvas_pos: pos,
                canvas_size: [w, h],
                visible: true,
                theme_override,
            });
            let idx = self.artboards.len() - 1;
            if first_new_idx.is_none() {
                first_new_idx = Some(idx);
            }
        }
        let new_idx = first_new_idx.unwrap_or(self.active_artboard_idx);
        self.active_artboard_idx = new_idx;
        self.active_preset = self.artboards[new_idx].preset;
        self.document = self.artboards[new_idx].document.clone();
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = format!(
            "Imported SVG as editable panel '{}' (#{} nodes).",
            self.document.name,
            self.document.all_node_ids().len()
        );
        Ok(new_idx)
    }

    /// Snapshot the full studio session state as a serializable bundle.
    pub fn to_persisted_bundle(&self) -> StudioPersistedBundle {
        let mut artboards = self.artboards.clone();
        if let Some(ab) = artboards.get_mut(self.active_artboard_idx) {
            ab.document = self.document.clone();
            ab.preset = self.active_preset;
        }
        StudioPersistedBundle {
            document: self.document.clone(),
            systems: self.systems.clone(),
            artboards,
            active_artboard_idx: self.active_artboard_idx,
            active_preset: Some(self.active_preset),
            solo_active_artboard: self.solo_active_artboard,
            canvas_pan: self.canvas_pan,
            canvas_zoom: self.canvas_zoom.clamp(0.35, 2.5),
            show_wireframe_outlines: self.show_wireframe_outlines,
            show_dimension_badges: self.show_dimension_badges,
            inspector_tab: self.inspector_tab,
            systems_subtab: self.systems_subtab,
            preview_theme: Some(self.preview_theme),
            preview_theme_locked: self.preview_theme_locked,
        }
    }

    /// Restore full studio session state (artboards, active selection, systems, view settings) from a bundle.
    pub fn restore_persisted_bundle(&mut self, bundle: StudioPersistedBundle) {
        self.document = bundle.document;
        self.systems = bundle.systems;
        if let Some(theme) = bundle.preview_theme {
            self.preview_theme = theme;
            self.preview_theme_locked = bundle.preview_theme_locked;
        }
        if !bundle.artboards.is_empty() {
            self.artboards = bundle.artboards;
            self.active_artboard_idx = bundle
                .active_artboard_idx
                .min(self.artboards.len().saturating_sub(1));
            self.artboards[self.active_artboard_idx].document = self.document.clone();
            self.active_preset = bundle
                .active_preset
                .unwrap_or(self.artboards[self.active_artboard_idx].preset);
            self.artboards[self.active_artboard_idx].preset = self.active_preset;
        } else {
            if let Some(preset) = bundle.active_preset {
                self.active_preset = preset;
            }
            self.sync_active_to_artboard();
        }
        self.solo_active_artboard = bundle.solo_active_artboard;
        self.canvas_pan = bundle.canvas_pan;
        self.canvas_zoom = bundle.canvas_zoom.clamp(0.35, 2.5);
        self.show_wireframe_outlines = bundle.show_wireframe_outlines;
        self.show_dimension_badges = bundle.show_dimension_badges;
        self.inspector_tab = bundle.inspector_tab;
        self.systems_subtab = bundle.systems_subtab;
        self.selected_nodes = vec![self.document.root.id];
    }

    /// Save studio document and system token overrides as pretty JSON (`target/ui-studio/ui-layout-overrides.json`).
    pub fn save_studio_json(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        let bundle = self.to_persisted_bundle();
        let json = serde_json::to_string_pretty(&bundle)
            .map_err(|e| format!("Failed to serialize studio JSON: {e}"))?;
        fs::write(path, json).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        Ok(path.to_path_buf())
    }

    /// Load studio session state without recording an undo step (used on session restore).
    pub fn load_studio_json_initial(&mut self, path: &Path) -> Result<(), String> {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let bundle: StudioPersistedBundle =
            serde_json::from_str(&raw).map_err(|e| format!("Failed to parse studio JSON: {e}"))?;
        self.restore_persisted_bundle(bundle);
        self.status_message = format!("Restored studio session from {}", path.display());
        Ok(())
    }

    /// Load studio document and system token overrides from JSON.
    pub fn load_studio_json(&mut self, path: &Path) -> Result<(), String> {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let bundle: StudioPersistedBundle =
            serde_json::from_str(&raw).map_err(|e| format!("Failed to parse studio JSON: {e}"))?;
        self.commit_history("Load JSON overrides");
        self.restore_persisted_bundle(bundle);
        self.status_message = format!("Loaded studio overrides from {}", path.display());
        Ok(())
    }

    /// Export generated Rust layout & system token code to disk (`target/ui-studio/exported_layout.rs`).
    pub fn export_rust_to_file(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        let code = export_rust_layout_with_systems(&self.document, &self.systems);
        fs::write(path, code).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        Ok(path.to_path_buf())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. Interactive UI Entrypoints, Hierarchy Tree, Palette & Live WYSIWYG Canvas
// ─────────────────────────────────────────────────────────────────────────────

fn studio_temp_id() -> egui::Id {
    egui::Id::new("nf2-ui-layout-studio-state")
}

pub(super) fn take_temp<T: Clone + Send + Sync + 'static>(
    ctx: &egui::Context,
    id: egui::Id,
) -> Option<T> {
    ctx.data_mut(|d| {
        let val = d.get_temp::<T>(id);
        if val.is_some() {
            d.remove::<T>(id);
        }
        val
    })
}

fn studio_persisted_id() -> egui::Id {
    egui::Id::new("nf2-ui-layout-studio-session-v1")
}

fn studio_disk_persistence_enabled() -> bool {
    !cfg!(test) && std::env::var_os("NEOFLASH_SCREENSHOT").is_none()
}

pub(crate) fn persist_studio_session(ctx: &egui::Context, state: &UiLayoutStudioState) {
    let bundle = state.to_persisted_bundle();
    let last_sig_id = egui::Id::new("nf2-ui-layout-studio-last-saved-bundle");
    let changed = ctx
        .data(|d| d.get_temp::<StudioPersistedBundle>(last_sig_id))
        .as_ref()
        != Some(&bundle);
    if !changed {
        return;
    }
    ctx.data_mut(|d| {
        d.insert_persisted(studio_persisted_id(), bundle.clone());
        d.insert_temp(last_sig_id, bundle);
    });
    if studio_disk_persistence_enabled() {
        let _ = state.save_studio_json(Path::new(DEFAULT_STUDIO_JSON_PATH));
    }
}

pub(crate) fn restore_studio_session(ctx: &egui::Context) -> UiLayoutStudioState {
    let mut state = UiLayoutStudioState::default();
    if let Some(bundle) =
        ctx.data_mut(|d| d.get_persisted::<StudioPersistedBundle>(studio_persisted_id()))
    {
        state.restore_persisted_bundle(bundle);
    } else if studio_disk_persistence_enabled() {
        let path = Path::new(DEFAULT_STUDIO_JSON_PATH);
        if path.exists() {
            let _ = state.load_studio_json_initial(path);
        }
    }
    state
}

/// Check whether the context-managed UI Layout Studio window is currently open.
pub fn is_studio_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| {
        d.get_temp::<UiLayoutStudioState>(studio_temp_id())
            .is_some_and(|s| s.open)
    })
}

/// Open or close the context-managed UI Layout Studio window.
pub fn set_studio_open(ctx: &egui::Context, open: bool) {
    let mut state = ctx
        .data(|d| d.get_temp::<UiLayoutStudioState>(studio_temp_id()))
        .unwrap_or_else(|| restore_studio_session(ctx));
    state.open = open;
    if !open {
        persist_studio_session(ctx, &state);
    }
    ctx.data_mut(|d| d.insert_temp(studio_temp_id(), state));
}

/// Show the context-managed UI Layout Studio floating window if open.
pub fn show_studio_window(ctx: &egui::Context) {
    let Some(mut state) = ctx.data(|d| d.get_temp::<UiLayoutStudioState>(studio_temp_id())) else {
        return;
    };
    if !state.open {
        return;
    }
    ui_layout_studio_window_ui(ctx, &mut state);
    persist_studio_session(ctx, &state);
    ctx.data_mut(|d| d.insert_temp(studio_temp_id(), state));
}

/// Floating window UI for the Interactive Visual UI & System Layout Studio.
pub fn ui_layout_studio_window_ui(ctx: &egui::Context, state: &mut UiLayoutStudioState) {
    if !state.open {
        return;
    }
    let nf = neoflash_ui::palette(ctx);
    let pos_id = egui::Id::new("nf2-ui-studio-win-pos");
    let drag_start_id = egui::Id::new("nf2-ui-studio-win-drag-start");
    let default_pos = egui::pos2(
        (ctx.content_rect().left() + 72.0).max(16.0),
        (ctx.content_rect().top() + 56.0).max(16.0),
    );
    let mut pos = ctx
        .data(|d| d.get_temp::<Pos2>(pos_id))
        .unwrap_or(default_pos);
    let mut close_requested = false;
    let mut drag_response: Option<egui::Response> = None;
    let win_width = (ctx.content_rect().width() - 96.0).clamp(880.0, 1360.0);

    egui::Area::new(egui::Id::new("nf2-ui-layout-studio-window"))
        .order(egui::Order::Foreground)
        .current_pos(pos)
        .movable(false)
        .constrain(true)
        .show(ctx, |ui| {
            c::Surface::Dialog.frame(ctx).show(ui, |ui| {
                ui.set_width(win_width);
                let top = ui.cursor().min;
                let header_rect = Rect::from_min_size(
                    top,
                    egui::vec2(win_width, neoflash_ui::metrics::dialog_header()),
                );
                let drag = ui.interact(
                    header_rect,
                    ui.id().with("studio-title-drag"),
                    Sense::drag(),
                );
                close_requested = c::DialogHeader {
                    title: "UI Layout & Design System Studio",
                }
                .show(ui);
                if drag.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                drag_response = Some(drag);
                ui.painter().hline(
                    header_rect.x_range(),
                    header_rect.bottom() - 0.5,
                    Stroke::new(1.0_f32, nf.border_subtle),
                );

                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S3 as i8))
                    .show(ui, |ui| {
                        ui.set_width(win_width - space::S3 * 2.0);
                        ui_layout_studio_panel_ui(ui, state);
                    });
            });
        });

    if let Some(drag) = drag_response {
        if drag.drag_started() {
            let start_ptr = ctx.input(|i| {
                i.pointer
                    .press_origin()
                    .or_else(|| i.pointer.interact_pos())
            });
            ctx.data_mut(|d| d.insert_temp(drag_start_id, (pos, start_ptr, Vec2::ZERO)));
        }
        if drag.dragged() {
            let cur_ptr = ctx.input(|i| i.pointer.interact_pos());
            if let Some((start_pos, start_ptr, mut accum)) =
                ctx.data(|d| d.get_temp::<(Pos2, Option<Pos2>, Vec2)>(drag_start_id))
            {
                let total_delta = match (start_ptr, cur_ptr) {
                    (Some(s), Some(c)) => c - s,
                    _ => {
                        accum += drag.drag_delta();
                        accum
                    }
                };
                ctx.data_mut(|d| d.insert_temp(drag_start_id, (start_pos, start_ptr, accum)));
                let bounds = ctx.content_rect();
                pos = egui::pos2(
                    (start_pos.x + total_delta.x).max(bounds.left() + 4.0),
                    (start_pos.y + total_delta.y).max(bounds.top() + 4.0),
                );
                ctx.data_mut(|d| d.insert_temp(pos_id, pos));
            }
        }
        if drag.drag_stopped() || !drag.dragged() {
            ctx.data_mut(|d| d.remove::<(Pos2, Option<Pos2>, Vec2)>(drag_start_id));
        }
    }
    if close_requested {
        state.open = false;
        persist_studio_session(ctx, state);
    }
}

/// Embeddable panel/body UI for the Interactive Visual UI & System Layout Studio.
pub fn ui_layout_studio_panel_ui(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    let nf = neoflash_ui::palette(ui.ctx());
    let host_theme = ThemeMode::current(ui.ctx());
    if state.last_host_theme != Some(host_theme) {
        state.last_host_theme = Some(host_theme);
        if !state.preview_theme_locked {
            state.preview_theme = host_theme;
        }
    }
    ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
    state.sync_active_to_artboard();

    // ── Top Toolbar: Preset Selector, Multi-Artboard Controls, SVG Import/Export, Wrap/Unwrap ──
    render_studio_top_toolbar(ui, state);
    c::divider(ui, false);

    // ── Main 3-Column Studio Workspace: [Tree + Palette | Multi-Panel Artboard Canvas | Inspector/Systems] ──
    let total_w = ui.available_width().max(760.0);
    let left_w = (total_w * 0.195).clamp(248.0, 275.0);
    let right_w = (total_w * 0.25).clamp(265.0, 345.0);
    let center_w = (total_w - left_w - right_w - space::S3 * 2.0).max(300.0);
    let body_h = (ui.available_height() - 48.0).clamp(420.0, 1400.0);

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::S3;

        // 1. Left Column: Hierarchy Tree + Structural Actions + Widget Creation Palette
        ui.allocate_ui_with_layout(
            egui::vec2(left_w, body_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                render_hierarchy_and_palette_column(ui, state, left_w, body_h);
            },
        );

        // 2. Center Column: Multi-Panel Free Artboard Canvas (unclipped pan/zoom + live resize grips)
        ui.allocate_ui_with_layout(
            egui::vec2(center_w, body_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                render_live_canvas_column(ui, state, center_w, body_h);
            },
        );

        // 3. Right Column: Node Inspector (% / px / Padding / Gap / Typography) & Systems Tab
        ui.allocate_ui_with_layout(
            egui::vec2(right_w, body_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                render_inspector_and_systems_column(ui, state, right_w, body_h);
            },
        );
    });

    state.sync_active_to_artboard();

    if state.svg_import_modal_open {
        render_svg_import_modal(ui.ctx(), state);
    }

    c::divider(ui, false);
    // ── Footer Status Bar ──
    ui.horizontal(|ui| {
        ui.add(c::Badge::new("NF2 Studio", c::BadgeType::Accent));
        ui.label(
            rich(
                format!(
                    "Artboards: {}   ·   Active: {}",
                    state.artboards.iter().filter(|a| a.visible).count(),
                    state.document.name
                ),
                TextStyle::MonoSmall,
            )
            .color(nf.text_tertiary),
        );
        ui.label(rich(&state.status_message, TextStyle::Small).color(nf.text_secondary));
    });
}

fn render_studio_top_toolbar(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;

        // Prominent 1-Click Mode Switcher: [Multi-Panel Board] | [Full Workspace Shell] | [Timeline Studio]
        let is_shell = state.solo_active_artboard
            && state.active_preset == StudioPanelPreset::FullWorkspaceShell;
        let is_timeline =
            state.solo_active_artboard && state.active_preset == StudioPanelPreset::TimelineStudio;
        let is_multi_board = !state.solo_active_artboard;

        let board_btn = if is_multi_board {
            c::Button::primary("Multi-Panel Board (6)").icon(i16::GRID)
        } else {
            c::Button::new("Multi-Panel Board (6)").icon(i16::GRID)
        };
        if ui.add(board_btn).clicked() {
            state.open_all_core_artboards();
        }

        let shell_btn = if is_shell {
            c::Button::primary("Full Workspace Shell").icon(i16::DESKTOP)
        } else {
            c::Button::new("Full Workspace Shell").icon(i16::DESKTOP)
        };
        if ui.add(shell_btn).clicked() {
            state.solo_active_artboard = true;
            state.load_preset(StudioPanelPreset::FullWorkspaceShell);
            state.systems_subtab = SystemsSubTab::WorkspaceAndStage;
        }

        let tl_btn = if is_timeline {
            c::Button::primary("Timeline & Tween Studio").icon(i16::LAYERS)
        } else {
            c::Button::new("Timeline & Tween Studio").icon(i16::LAYERS)
        };
        if ui.add(tl_btn).clicked() {
            state.load_preset(StudioPanelPreset::TimelineStudio);
            state.systems_subtab = SystemsSubTab::TimelineGeometry;
        }

        c::divider(ui, true);

        // Full Preset selector (all 11 workspace, timeline, library, color, align & panel presets)
        let mut preset = state.active_preset;
        let preset_options: [(StudioPanelPreset, &str); 11] = [
            (
                StudioPanelPreset::PropertiesMovieClip,
                "Panel: Properties — Movie Clip",
            ),
            (
                StudioPanelPreset::LibraryAndAssetsStudio,
                "Panel: Library Split (Preview + Tree)",
            ),
            (
                StudioPanelPreset::ColorAndGradientStudio,
                "Panel: Color Mixer, Gradient & Swatches",
            ),
            (
                StudioPanelPreset::AlignAndTransform,
                "Panel: Align & Transform",
            ),
            (StudioPanelPreset::ToolsPanel, "Panel: Tools Rail"),
            (
                StudioPanelPreset::TimelineStudio,
                "Surface: Timeline & Tween Geometry Studio",
            ),
            (
                StudioPanelPreset::FullWorkspaceShell,
                "Surface: Full Editor Workspace — Live Shell",
            ),
            (
                StudioPanelPreset::PropertiesText,
                "Panel: Properties — Text",
            ),
            (StudioPanelPreset::ColorPanel, "Panel: Color (Compact)"),
            (StudioPanelPreset::FiltersPanel, "Panel: Filters"),
            (
                StudioPanelPreset::TimelineHeader,
                "Panel: Timeline Header Bar",
            ),
        ];
        if c::Select::options(
            ui,
            "studio-preset-select",
            &mut preset,
            &preset_options,
            234.0,
        ) {
            state.load_preset(preset);
        }

        if ui
            .add(c::Button::new("Add to Canvas").icon(i16::PLUS))
            .clicked()
        {
            state.solo_active_artboard = false;
            state.add_artboard(preset);
        }
        if ui.add(c::Button::new("Auto-Arrange")).clicked() {
            state.auto_arrange_artboards();
        }

        c::divider(ui, true);

        // Explicit 4-Theme Switcher (`Dark`, `Light`, `Mid Neutral Gray`, `Dark Graphite`) + 4-Theme Comparison
        let mut preview_theme = state.preview_theme;
        let theme_options: [(ThemeMode, &str); 4] = [
            (ThemeMode::Dark, "Theme: Dark"),
            (ThemeMode::Light, "Theme: Light"),
            (ThemeMode::MidNeutralGray, "Theme: Mid Neutral Gray"),
            (ThemeMode::DarkGraphite, "Theme: Dark Graphite"),
        ];
        if c::Select::options(
            ui,
            "studio-top-theme-select",
            &mut preview_theme,
            &theme_options,
            192.0,
        ) {
            state.set_preview_theme(preview_theme);
        }
        if ui
            .add(c::Button::new("Compare 4 Themes").icon(i16::GRID))
            .clicked()
        {
            state.open_four_theme_comparison();
        }
        let mut top_density = state.systems.density.active_profile;
        if c::Select::options(
            ui,
            "studio-top-density-select",
            &mut top_density,
            &[
                (StudioDensityProfile::Comfortable, "Density: Comfortable"),
                (StudioDensityProfile::Compact, "Density: Compact"),
                (StudioDensityProfile::ClassicFlash, "Density: Classic Flash"),
            ],
            166.0,
        ) && top_density != state.systems.density.active_profile
        {
            state.commit_history("Change density profile");
            state.systems.density.active_profile = top_density;
            state.status_message = format!("Switched density profile to {}.", top_density.label());
        }

        // Undo / Redo
        if ui
            .add_enabled(
                state.history.can_undo(),
                c::IconButton::new(i16::UNDO, "Undo studio edit").shortcut("⌘Z"),
            )
            .clicked()
        {
            state.undo();
        }
        if ui
            .add_enabled(
                state.history.can_redo(),
                c::IconButton::new(i16::REDO, "Redo studio edit").shortcut("⇧⌘Z"),
            )
            .clicked()
        {
            state.redo();
        }

        c::divider(ui, true);

        // Quick 1-click Wrap buttons (push Undo step only if mutation changes the tree)
        if ui.add(c::Button::new("Wrap Row")).clicked() {
            let sel = state.selected_nodes.clone();
            let snap = state.capture_snapshot("Wrap in Row");
            if let Some(new_id) = state
                .document
                .wrap_selected_nodes(&sel, ContainerWrapPreset::Row)
            {
                state.push_history_snapshot(snap);
                state.select_single(new_id);
                state.sync_active_to_artboard();
                state.status_message = "Wrapped selection in Row.".to_owned();
            }
        }
        if ui.add(c::Button::new("Wrap Col")).clicked() {
            let sel = state.selected_nodes.clone();
            let snap = state.capture_snapshot("Wrap in Column");
            if let Some(new_id) = state
                .document
                .wrap_selected_nodes(&sel, ContainerWrapPreset::Column)
            {
                state.push_history_snapshot(snap);
                state.select_single(new_id);
                state.sync_active_to_artboard();
                state.status_message = "Wrapped selection in Column.".to_owned();
            }
        }
        for (btn_label, wrap_preset, desc) in [
            (
                "Wrap 2-Grid",
                ContainerWrapPreset::Grid2Equal,
                "2-column Grid (50%/50%)",
            ),
            (
                "Wrap 3-Grid",
                ContainerWrapPreset::Grid3Equal,
                "3-column Grid (33.3% × 3)",
            ),
            (
                "Wrap 4-Grid",
                ContainerWrapPreset::Grid4Equal,
                "4-column Grid (25% × 4)",
            ),
        ] {
            if ui.add(c::Button::new(btn_label)).clicked() {
                let sel = state.selected_nodes.clone();
                let snap = state.capture_snapshot(format!("Wrap in {desc}"));
                if let Some(new_id) = state.document.wrap_selected_nodes(&sel, wrap_preset) {
                    state.push_history_snapshot(snap);
                    state.select_single(new_id);
                    state.sync_active_to_artboard();
                    state.status_message = format!("Wrapped selection in {desc}.");
                }
            }
        }

        if ui.add(c::Button::new("Unwrap")).clicked() {
            let target = state.primary_selected_id();
            let snap = state.capture_snapshot("Unwrap container");
            if state.document.unwrap_selected_container(target) {
                state.push_history_snapshot(snap);
                state.sync_active_to_artboard();
                state.status_message = "Unwrapped container children into parent.".to_owned();
            } else if state.document.extract_node_to_parent(target) {
                state.push_history_snapshot(snap);
                state.sync_active_to_artboard();
                state.status_message = "Extracted node out of parent container.".to_owned();
            }
        }

        c::divider(ui, true);

        // SVG Export & Import + Save JSON + Export Rust Code
        if ui
            .add(c::Button::new("Export SVG").icon(i16::SAVE))
            .clicked()
        {
            let svg = state.export_active_svg();
            ui.ctx().copy_text(svg.clone());
            state.svg_buffer = svg;
            let svg_path = Path::new(DEFAULT_STUDIO_SVG_EXPORT_PATH);
            match state.save_svg_to_file(svg_path) {
                Ok(saved) => {
                    state.inspector_tab = StudioInspectorTab::CodeExport;
                    state.status_message = format!(
                        "Exported SVG to {} (and copied to clipboard).",
                        saved.display()
                    );
                }
                Err(err) => {
                    state.status_message = err;
                }
            }
        }
        if ui
            .add(c::Button::new("Import SVG → Panel…").icon(i16::UPLOAD))
            .clicked()
        {
            state.svg_import_modal_open = true;
        }
        if ui
            .add(c::Button::new("Save JSON").icon(i16::SAVE))
            .clicked()
        {
            let path = Path::new(DEFAULT_STUDIO_JSON_PATH);
            match state.save_studio_json(path) {
                Ok(saved) => {
                    state.status_message = format!("Saved overrides to {}", saved.display());
                }
                Err(err) => {
                    state.status_message = err;
                }
            }
        }
        if ui
            .add(c::Button::primary("Export Build Code").icon(i16::CODE))
            .clicked()
        {
            let json_path = Path::new(DEFAULT_STUDIO_JSON_PATH);
            let _ = state.save_studio_json(json_path);
            let rust_path = Path::new(DEFAULT_STUDIO_RUST_EXPORT_PATH);
            match state.export_rust_to_file(rust_path) {
                Ok(saved) => {
                    state.inspector_tab = StudioInspectorTab::CodeExport;
                    state.status_message =
                        format!("Exported Rust build code to {}", saved.display());
                }
                Err(err) => {
                    state.status_message = err;
                }
            }
        }
    });
}

fn render_svg_import_modal(ctx: &egui::Context, state: &mut UiLayoutStudioState) {
    let nf = neoflash_ui::palette(ctx);
    let screen = ctx.content_rect();
    let modal_w = 640.0_f32.min((screen.width() - 48.0).max(360.0));
    let modal_pos = egui::pos2(
        screen.center().x - modal_w * 0.5,
        (screen.top() + 92.0).max(24.0),
    );
    let mut close_modal = false;

    egui::Area::new(egui::Id::new("nf2-studio-svg-import-modal"))
        .order(egui::Order::Foreground)
        .current_pos(modal_pos)
        .constrain(true)
        .show(ctx, |ui| {
            c::Surface::Dialog.frame(ctx).show(ui, |ui| {
                ui.set_width(modal_w);
                if (c::DialogHeader {
                    title: "Import SVG → Live Editable NF2 Panel",
                })
                .show(ui)
                {
                    close_modal = true;
                }
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S3 as i8))
                    .show(ui, |ui| {
                        ui.set_width(modal_w - space::S3 * 2.0);
                        ui.spacing_mut().item_spacing.y = space::S2;
                        ui.label(
                            rich(
                                "Paste any SVG below (exported from NeoFlash UI Studio, Figma, or Illustrator) or load from target/ui-studio/exported_panel.svg. The importer reconstructs Panel, Sections, Grids/Rows, Fields, Selects, Swatches, Sliders, Buttons, and Labels as an interactive artboard on the canvas.",
                                TextStyle::Small,
                            )
                            .color(nf.text_secondary),
                        );

                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add(c::Button::new("Load Sample Figma Panel SVG"))
                                .clicked()
                            {
                                state.svg_buffer = sample_svg_panel_snippet();
                            }
                            if ui
                                .add(c::Button::new("Load Current Active Artboard SVG"))
                                .clicked()
                            {
                                state.svg_buffer = state.export_active_svg();
                            }
                            if ui
                                .add(c::Button::new("Read target/ui-studio/exported_panel.svg"))
                                .clicked()
                            {
                                match fs::read_to_string(DEFAULT_STUDIO_SVG_EXPORT_PATH) {
                                    Ok(raw) => {
                                        state.svg_buffer = raw;
                                        state.status_message = format!(
                                            "Loaded SVG text from {DEFAULT_STUDIO_SVG_EXPORT_PATH}"
                                        );
                                    }
                                    Err(err) => {
                                        state.status_message = format!(
                                            "Could not read {DEFAULT_STUDIO_SVG_EXPORT_PATH}: {err}"
                                        );
                                    }
                                }
                            }
                        });

                        ui.add(
                            c::TextArea::new(&mut state.svg_buffer, "SVG XML Input").rows(14),
                        );

                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    c::Button::primary("Convert SVG → Add Panel Artboard")
                                        .icon(i16::CHECK),
                                )
                                .clicked()
                            {
                                let buf = state.svg_buffer.clone();
                                match state.import_svg_as_artboard(&buf) {
                                    Ok(_) => {
                                        state.solo_active_artboard = false;
                                        close_modal = true;
                                    }
                                    Err(err) => {
                                        state.status_message = format!("SVG import error: {err}");
                                    }
                                }
                            }
                            if ui.add(c::Button::new("Cancel")).clicked() {
                                close_modal = true;
                            }
                        });
                    });
            });
        });

    if close_modal {
        state.svg_import_modal_open = false;
    }
}
