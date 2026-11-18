//! UI Layout Studio (`Window ▸ UI Layout Studio…`, standalone with `NEOFLASH_UI_STUDIO`).
//!
//! Layout of the Studio itself (v3): one toolbar row of `metrics::timeline_controls()` height whose
//! lower-priority groups collapse into a ⋯ menu, three fixed columns (Hierarchy `tree_width()`,
//! canvas, inspector `inspector()`) and a status row. The canvas previews are laid out in their own
//! density ([`PreviewDensity`]); "All" shows the active panel in Normal, Compact and Classic Flash.
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

/// Tab of the left Studio column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioHierarchyTab {
    /// Node tree of the active panel.
    #[default]
    Hierarchy,
    /// Containers and widgets to insert into the selected node.
    Add,
}

/// Board shown on the canvas (toolbar segmented control).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StudioBoardMode {
    Panels,
    Workspace,
    Timeline,
}

/// Density choice of the toolbar: one profile, or all three side by side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StudioDensityChoice {
    One(StudioDensityProfile),
    All,
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
    /// Artboard size of a preset in the Normal density: dock panels are one inspector wide.
    pub fn default_size_for(preset: StudioPanelPreset) -> [f32; 2] {
        let width = preset.natural_width(neoflash_ui::metrics::Density::Comfortable);
        let height = match preset {
            StudioPanelPreset::FullWorkspaceShell => 680.0,
            StudioPanelPreset::TimelineStudio => 390.0,
            StudioPanelPreset::ColorPanel => 440.0,
            StudioPanelPreset::FiltersPanel => 420.0,
            StudioPanelPreset::TimelineHeader => 96.0,
            _ => 480.0,
        };
        [width, height]
    }

    /// Default startup multi-panel board laying out 6 core NeoFlash panels + timeline without clipping.
    pub fn default_board() -> Vec<Self> {
        let top_row = [
            StudioPanelPreset::PropertiesMovieClip,
            StudioPanelPreset::LibraryAndAssetsStudio,
            StudioPanelPreset::ColorAndGradientStudio,
            StudioPanelPreset::AlignAndTransform,
            StudioPanelPreset::ToolsPanel,
        ];
        let mut boards = Vec::with_capacity(top_row.len() + 1);
        let mut x = BOARD_MARGIN_X;
        let mut row_h = 0.0_f32;
        for (i, preset) in top_row.into_iter().enumerate() {
            let canvas_size = Self::default_size_for(preset);
            boards.push(Self {
                id: i as u32 + 1,
                preset,
                document: preset.build_document(),
                canvas_pos: [x, BOARD_MARGIN_Y],
                canvas_size,
                visible: true,
                theme_override: None,
            });
            x += canvas_size[0] + BOARD_GAP_X;
            row_h = row_h.max(canvas_size[1]);
        }
        let timeline = StudioPanelPreset::TimelineStudio;
        boards.push(Self {
            id: boards.len() as u32 + 1,
            preset: timeline,
            document: timeline.build_document(),
            canvas_pos: [BOARD_MARGIN_X, BOARD_MARGIN_Y + row_h + BOARD_GAP_Y],
            canvas_size: Self::default_size_for(timeline),
            visible: true,
            theme_override: None,
        });
        boards
    }
}

/// Pasteboard geometry of the artboard canvas (canvas coordinates, not chrome).
pub(super) const BOARD_MARGIN_X: f32 = 24.0;
pub(super) const BOARD_MARGIN_Y: f32 = 32.0;
pub(super) const BOARD_GAP_X: f32 = 24.0;
pub(super) const BOARD_GAP_Y: f32 = 40.0;
/// Smallest column / row step when artboards are appended to the board.
const BOARD_STEP_X: f32 = 296.0;
const BOARD_STEP_Y: f32 = 470.0;

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
    #[serde(default)]
    pub preview_all_densities: bool,
    #[serde(default)]
    pub hierarchy_tab: StudioHierarchyTab,
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
    /// The canvas shows the active panel in Normal, Compact and Classic Flash side by side.
    #[serde(default)]
    pub preview_all_densities: bool,
    #[serde(default)]
    pub hierarchy_tab: StudioHierarchyTab,
    pub status_message: String,
    /// Last manual save or export, shown at the right of the status row.
    #[serde(default)]
    pub last_output: String,
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
            preview_all_densities: false,
            hierarchy_tab: StudioHierarchyTab::Hierarchy,
            status_message: "Drag a preview header to move it, the corner grip to resize it."
                .to_owned(),
            last_output: String::new(),
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
            BOARD_MARGIN_X + ((count % 4) as f32) * (size[0].max(BOARD_STEP_X) + BOARD_GAP_X),
            BOARD_MARGIN_Y + ((count / 4) as f32) * (size[1].max(BOARD_STEP_Y) + BOARD_GAP_Y),
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

    /// Board the toolbar segmented control shows as selected.
    pub fn board_mode(&self) -> StudioBoardMode {
        match (self.solo_active_artboard, self.active_preset) {
            (true, StudioPanelPreset::FullWorkspaceShell) => StudioBoardMode::Workspace,
            (true, StudioPanelPreset::TimelineStudio) => StudioBoardMode::Timeline,
            _ => StudioBoardMode::Panels,
        }
    }

    /// Switch the board: the six core panels, the workspace shell alone, or the timeline alone.
    pub fn set_board_mode(&mut self, mode: StudioBoardMode) {
        match mode {
            StudioBoardMode::Panels => self.open_all_core_artboards(),
            StudioBoardMode::Workspace => {
                self.solo_active_artboard = true;
                self.load_preset(StudioPanelPreset::FullWorkspaceShell);
                self.systems_subtab = SystemsSubTab::WorkspaceAndStage;
            }
            StudioBoardMode::Timeline => {
                self.solo_active_artboard = true;
                self.load_preset(StudioPanelPreset::TimelineStudio);
                self.systems_subtab = SystemsSubTab::TimelineGeometry;
            }
        }
    }

    pub fn preview_density_choice(&self) -> StudioDensityChoice {
        if self.preview_all_densities {
            StudioDensityChoice::All
        } else {
            StudioDensityChoice::One(self.systems.density.active_profile)
        }
    }

    /// Density the canvas previews are laid out with. A single profile is an undoable system
    /// change; "All" is a view of the active panel in the three densities.
    pub fn set_preview_density_choice(&mut self, choice: StudioDensityChoice) {
        match choice {
            StudioDensityChoice::All => {
                self.preview_all_densities = true;
                self.status_message =
                    "Showing the active panel in Normal, Compact and Classic Flash.".to_owned();
            }
            StudioDensityChoice::One(profile) => {
                self.preview_all_densities = false;
                if profile != self.systems.density.active_profile {
                    self.commit_history("Change density profile");
                    self.systems.density.active_profile = profile;
                }
                self.status_message = format!("Preview density: {}.", profile.label());
            }
        }
    }

    /// Densities the canvas lays the active panel out in: one, or all three.
    pub fn preview_densities(&self) -> Vec<neoflash_ui::metrics::Density> {
        if self.preview_all_densities {
            neoflash_ui::metrics::Density::ALL.to_vec()
        } else {
            vec![self.systems.density.active_profile.ui()]
        }
    }

    /// Wrap the selected nodes in a container; one undo step when the tree changed.
    pub fn wrap_selection(&mut self, preset: ContainerWrapPreset, what: &str) {
        let sel = self.selected_nodes.clone();
        let snap = self.capture_snapshot(format!("Wrap in {what}"));
        if let Some(new_id) = self.document.wrap_selected_nodes(&sel, preset) {
            self.push_history_snapshot(snap);
            self.select_single(new_id);
            self.sync_active_to_artboard();
            self.status_message = format!("Wrapped selection in {what}.");
        }
    }

    /// Unwrap the selected container, or lift the selected node out of its parent.
    pub fn unwrap_selection(&mut self) {
        let target = self.primary_selected_id();
        let snap = self.capture_snapshot("Unwrap container");
        if self.document.unwrap_selected_container(target) {
            self.push_history_snapshot(snap);
            self.sync_active_to_artboard();
            self.status_message = "Unwrapped container children into parent.".to_owned();
        } else if self.document.extract_node_to_parent(target) {
            self.push_history_snapshot(snap);
            self.sync_active_to_artboard();
            self.status_message = "Extracted node out of parent container.".to_owned();
        }
    }

    /// File ▸ Export SVG…: active artboard to the clipboard and `DEFAULT_STUDIO_SVG_EXPORT_PATH`.
    pub fn export_svg_to_default_path(&mut self, ctx: &egui::Context) {
        let svg = self.export_active_svg();
        ctx.copy_text(svg.clone());
        self.svg_buffer = svg;
        match self.save_svg_to_file(Path::new(DEFAULT_STUDIO_SVG_EXPORT_PATH)) {
            Ok(saved) => {
                self.inspector_tab = StudioInspectorTab::CodeExport;
                self.last_output = format!("Exported SVG to {}", saved.display());
                self.status_message = "SVG exported and copied to the clipboard.".to_owned();
            }
            Err(err) => self.status_message = err,
        }
    }

    /// File ▸ Save JSON.
    pub fn save_json_to_default_path(&mut self) {
        match self.save_studio_json(Path::new(DEFAULT_STUDIO_JSON_PATH)) {
            Ok(saved) => {
                self.last_output = format!("Saved to {}", saved.display());
                self.status_message = "Saved studio JSON.".to_owned();
            }
            Err(err) => self.status_message = err,
        }
    }

    /// Export code: the JSON overrides and the generated Rust layout.
    pub fn export_code_to_default_paths(&mut self) {
        let _ = self.save_studio_json(Path::new(DEFAULT_STUDIO_JSON_PATH));
        match self.export_rust_to_file(Path::new(DEFAULT_STUDIO_RUST_EXPORT_PATH)) {
            Ok(saved) => {
                self.inspector_tab = StudioInspectorTab::CodeExport;
                self.last_output = format!("Exported code to {}", saved.display());
                self.status_message = "Exported Rust layout code and JSON overrides.".to_owned();
            }
            Err(err) => self.status_message = err,
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
            preview_all_densities: self.preview_all_densities,
            hierarchy_tab: self.hierarchy_tab,
        }
    }

    /// Restore full studio session state (artboards, active selection, systems, view settings) from a bundle.
    pub fn restore_persisted_bundle(&mut self, bundle: StudioPersistedBundle) {
        self.document = bundle.document;
        self.systems = bundle.systems;
        self.systems.migrate_legacy_defaults();
        self.preview_all_densities = bundle.preview_all_densities;
        self.hierarchy_tab = bundle.hierarchy_tab;
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
    apply_screenshot_overrides(&mut state);
    state
}

/// Screenshot aids (only with `NEOFLASH_SCREENSHOT`): `NEOFLASH_STUDIO_DENSITY=all|normal|compact|classic`
/// picks the preview density, `NEOFLASH_STUDIO_BOARD=workspace|timeline` the board and
/// `NEOFLASH_STUDIO_TAB=systems|code` the inspector tab.
pub(super) fn apply_screenshot_overrides(state: &mut UiLayoutStudioState) {
    if cfg!(test) || std::env::var_os("NEOFLASH_SCREENSHOT").is_none() {
        return;
    }
    let choice = match std::env::var("NEOFLASH_STUDIO_DENSITY").as_deref() {
        Ok("all") => Some(StudioDensityChoice::All),
        Ok("normal") => Some(StudioDensityChoice::One(StudioDensityProfile::Comfortable)),
        Ok("compact") => Some(StudioDensityChoice::One(StudioDensityProfile::Compact)),
        Ok("classic") => Some(StudioDensityChoice::One(StudioDensityProfile::ClassicFlash)),
        _ => None,
    };
    if let Some(choice) = choice {
        state.preview_all_densities = choice == StudioDensityChoice::All;
        if let StudioDensityChoice::One(profile) = choice {
            state.systems.density.active_profile = profile;
        }
    }
    match std::env::var("NEOFLASH_STUDIO_BOARD").as_deref() {
        Ok("workspace") => state.set_board_mode(StudioBoardMode::Workspace),
        Ok("timeline") => state.set_board_mode(StudioBoardMode::Timeline),
        _ => {}
    }
    match std::env::var("NEOFLASH_STUDIO_TAB").as_deref() {
        Ok("systems") => state.inspector_tab = StudioInspectorTab::SystemsTab,
        Ok("code") => state.inspector_tab = StudioInspectorTab::CodeExport,
        _ => {}
    }
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

/// Width of the Studio window for a host content width: 880–1360 pt.
pub(super) fn studio_window_width(content_width: f32) -> f32 {
    (content_width - 96.0).clamp(STUDIO_MIN_WIDTH, STUDIO_MAX_WIDTH)
}

/// Height of the Studio window (header included) for a host content height.
pub(super) fn studio_window_height(content_height: f32) -> f32 {
    let preferred = neoflash_ui::metrics::density().pick(760.0, 700.0, 640.0)
        + neoflash_ui::metrics::dialog_header();
    (content_height - 96.0).clamp(STUDIO_MIN_HEIGHT.min(preferred), preferred)
}

/// Supported widths of the Studio window; the toolbar stays one row across the whole range.
pub(super) const STUDIO_MIN_WIDTH: f32 = 880.0;
pub(super) const STUDIO_MAX_WIDTH: f32 = 1360.0;
const STUDIO_MIN_HEIGHT: f32 = 480.0;

/// Floating window of the UI Layout Studio.
pub fn ui_layout_studio_window_ui(ctx: &egui::Context, state: &mut UiLayoutStudioState) {
    if !state.open {
        return;
    }
    let nf = neoflash_ui::palette(ctx);
    let pos_id = egui::Id::new("nf2-ui-studio-win-pos");
    let drag_start_id = egui::Id::new("nf2-ui-studio-win-drag-start");
    let screen = ctx.content_rect();
    let win_width = studio_window_width(screen.width());
    let win_height = studio_window_height(screen.height());
    let default_pos = egui::pos2(
        (screen.center().x - win_width / 2.0).max(screen.left()),
        (screen.center().y - win_height / 2.0).max(screen.top()),
    );
    let mut pos = ctx
        .data(|d| d.get_temp::<Pos2>(pos_id))
        .unwrap_or(default_pos);
    let mut close_requested = false;
    let mut drag_response: Option<egui::Response> = None;

    egui::Area::new(egui::Id::new("nf2-ui-layout-studio-window"))
        .order(egui::Order::Foreground)
        .current_pos(pos)
        .movable(false)
        .constrain(true)
        .show(ctx, |ui| {
            c::Surface::Dialog.frame(ctx).show(ui, |ui| {
                ui.set_width(win_width);
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let header_h = neoflash_ui::metrics::dialog_header();
                let header_rect =
                    Rect::from_min_size(ui.cursor().min, egui::vec2(win_width, header_h));
                let drag = ui.interact(
                    header_rect,
                    ui.id().with("studio-title-drag"),
                    Sense::drag(),
                );
                close_requested = c::DialogHeader {
                    title: "UI Layout Studio",
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

                let body = Rect::from_min_size(
                    header_rect.left_bottom(),
                    egui::vec2(win_width, win_height - header_h),
                );
                ui.allocate_rect(body, Sense::hover());
                let mut body_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(body)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
                ui_layout_studio_panel_ui(&mut body_ui, state);
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

/// Rectangles of the Studio body: toolbar row, three fixed columns, status row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct StudioBodyRects {
    pub toolbar: Rect,
    pub hierarchy: Rect,
    pub canvas: Rect,
    pub inspector: Rect,
    pub status: Rect,
}

/// Split `full` into the Studio regions. The side columns have the fixed metric widths
/// (`tree_width()`, `inspector()`); the canvas takes the rest.
pub(super) fn studio_body_rects(full: Rect) -> StudioBodyRects {
    use neoflash_ui::metrics;
    let toolbar = Rect::from_min_size(
        full.min,
        egui::vec2(full.width(), metrics::timeline_controls()),
    );
    let status = Rect::from_min_max(
        egui::pos2(full.left(), full.bottom() - metrics::status_bar()),
        full.max,
    );
    let top = toolbar.bottom();
    let bottom = status.top().max(top);
    let left_w = metrics::tree_width().min(full.width());
    let right_w = metrics::inspector().min((full.width() - left_w).max(0.0));
    let hierarchy = Rect::from_min_max(
        egui::pos2(full.left(), top),
        egui::pos2(full.left() + left_w, bottom),
    );
    let inspector = Rect::from_min_max(
        egui::pos2(full.right() - right_w, top),
        egui::pos2(full.right(), bottom),
    );
    let canvas = Rect::from_min_max(hierarchy.right_top(), inspector.left_bottom());
    StudioBodyRects {
        toolbar,
        hierarchy,
        canvas,
        inspector,
        status,
    }
}

/// Child `Ui` clipped to one Studio region.
fn region_ui(ui: &mut Ui, rect: Rect, layout: egui::Layout) -> Ui {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child
}

/// Embeddable body of the UI Layout Studio; fills the available rectangle of `ui`.
pub fn ui_layout_studio_panel_ui(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    let nf = neoflash_ui::palette(ui.ctx());
    let host_theme = ThemeMode::current(ui.ctx());
    if state.last_host_theme != Some(host_theme) {
        state.last_host_theme = Some(host_theme);
        if !state.preview_theme_locked {
            state.preview_theme = host_theme;
        }
    }
    state.sync_active_to_artboard();

    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    let rects = studio_body_rects(full);
    let line = Stroke::new(1.0_f32, nf.border_subtle);
    let top_down = egui::Layout::top_down(egui::Align::Min);

    ui.painter().rect_filled(rects.toolbar, 0.0, nf.bg_panel);
    ui.painter().rect_filled(rects.hierarchy, 0.0, nf.bg_app);
    ui.painter().rect_filled(rects.inspector, 0.0, nf.bg_panel);
    ui.painter().rect_filled(rects.status, 0.0, nf.bg_app);

    {
        let mut bar = region_ui(
            ui,
            rects.toolbar,
            egui::Layout::left_to_right(egui::Align::Center),
        );
        render_studio_top_toolbar(&mut bar, state);
    }
    {
        let mut col = region_ui(ui, rects.hierarchy, top_down);
        render_hierarchy_and_palette_column(
            &mut col,
            state,
            rects.hierarchy.width(),
            rects.hierarchy.height(),
        );
    }
    {
        let mut col = region_ui(ui, rects.canvas, top_down);
        render_live_canvas_column(&mut col, state, rects.canvas.width(), rects.canvas.height());
    }
    {
        let mut col = region_ui(ui, rects.inspector, top_down);
        render_inspector_and_systems_column(
            &mut col,
            state,
            rects.inspector.width(),
            rects.inspector.height(),
        );
    }
    {
        let mut row = region_ui(
            ui,
            rects.status,
            egui::Layout::left_to_right(egui::Align::Center),
        );
        render_studio_status_row(&mut row, state);
    }

    ui.painter()
        .hline(full.x_range(), rects.toolbar.bottom() - 0.5, line);
    ui.painter()
        .hline(full.x_range(), rects.status.top() + 0.5, line);
    ui.painter().vline(
        rects.hierarchy.right() - 0.5,
        rects.hierarchy.y_range(),
        line,
    );
    ui.painter().vline(
        rects.inspector.left() + 0.5,
        rects.inspector.y_range(),
        line,
    );

    state.sync_active_to_artboard();

    if state.svg_import_modal_open {
        render_svg_import_modal(ui.ctx(), state);
    }
}

/// Width of `text` in a text style, for row budgets.
pub(super) fn text_width(ui: &Ui, text: &str, style: TextStyle) -> f32 {
    ui.fonts_mut(|f| {
        f.layout_no_wrap(
            text.to_owned(),
            neoflash_ui::typography::font(style),
            egui::Color32::WHITE,
        )
        .size()
        .x
    })
}

/// `text` shortened with "…" to at most `max_width`.
pub(super) fn elided(ui: &Ui, text: &str, style: TextStyle, max_width: f32) -> String {
    canvas::truncate_galley_with_ellipsis(
        ui.painter(),
        text,
        neoflash_ui::typography::font(style),
        max_width.max(0.0),
        egui::Color32::WHITE,
    )
    .0
    .text()
    .to_owned()
}

/// Status row: active board and node count on the left, the last save / export on the right.
fn render_studio_status_row(ui: &mut Ui, state: &UiLayoutStudioState) {
    let nf = neoflash_ui::palette(ui.ctx());
    let pad = space::s5();
    let gap = space::s4();
    let rect = ui.max_rect().shrink2(egui::vec2(pad, 0.0));
    let style = TextStyle::Small;
    let font = neoflash_ui::typography::font(style);
    let left = format!(
        "{} · {} nodes",
        state.document.name,
        state.document.all_node_ids().len()
    );
    let output = if state.last_output.is_empty() {
        format!("Session: {DEFAULT_STUDIO_JSON_PATH}")
    } else {
        state.last_output.clone()
    };
    // The right text keeps its place; the left gives way, then the message in between.
    let right = elided(ui, &output, style, rect.width() * 0.5);
    let right_w = text_width(ui, &right, style);
    let left = elided(ui, &left, style, rect.width() - right_w - gap);
    let left_w = text_width(ui, &left, style);
    let painter = ui.painter();
    painter.text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        &left,
        font.clone(),
        nf.text_secondary,
    );
    painter.text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        &right,
        font.clone(),
        nf.text_tertiary,
    );
    let middle_w = rect.width() - left_w - right_w - 2.0 * gap;
    if middle_w > neoflash_ui::metrics::field_s() && !state.status_message.is_empty() {
        let message = elided(ui, &state.status_message, style, middle_w);
        ui.painter().text(
            egui::pos2(rect.left() + left_w + gap, rect.center().y),
            egui::Align2::LEFT_CENTER,
            message,
            font,
            nf.text_tertiary,
        );
    }
}

/// Short names of the presets for the toolbar Select (they fit `field_l()` in every density).
const PRESET_OPTIONS: [(StudioPanelPreset, &str); 11] = [
    (
        StudioPanelPreset::PropertiesMovieClip,
        "Properties — Movie clip",
    ),
    (StudioPanelPreset::PropertiesText, "Properties — Text"),
    (StudioPanelPreset::LibraryAndAssetsStudio, "Library"),
    (
        StudioPanelPreset::ColorAndGradientStudio,
        "Color and gradient",
    ),
    (StudioPanelPreset::ColorPanel, "Color"),
    (StudioPanelPreset::AlignAndTransform, "Align and transform"),
    (StudioPanelPreset::ToolsPanel, "Tools"),
    (StudioPanelPreset::FiltersPanel, "Filters"),
    (StudioPanelPreset::TimelineHeader, "Timeline header"),
    (StudioPanelPreset::TimelineStudio, "Timeline"),
    (StudioPanelPreset::FullWorkspaceShell, "Workspace shell"),
];

/// Theme name short enough for the closed theme Select (`field_s()` in every density).
const fn theme_short_label(theme: ThemeMode) -> &'static str {
    match theme {
        ThemeMode::Dark => "Dark",
        ThemeMode::Light => "Light",
        ThemeMode::MidNeutralGray => "Mid gray",
        ThemeMode::DarkGraphite => "Graphite",
    }
}

const BOARD_OPTIONS: [(StudioBoardMode, &str); 3] = [
    (StudioBoardMode::Panels, "Panels"),
    (StudioBoardMode::Workspace, "Workspace"),
    (StudioBoardMode::Timeline, "Timeline"),
];

const DENSITY_OPTIONS: [(StudioDensityChoice, &str); 4] = [
    (
        StudioDensityChoice::One(StudioDensityProfile::Comfortable),
        "Normal",
    ),
    (
        StudioDensityChoice::One(StudioDensityProfile::Compact),
        "Compact",
    ),
    (
        StudioDensityChoice::One(StudioDensityProfile::ClassicFlash),
        "Classic",
    ),
    (StudioDensityChoice::All, "All"),
];

/// Natural widths of the toolbar items in the current density.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ToolbarWidths {
    pub board: f32,
    pub preset: f32,
    pub add: f32,
    pub theme: f32,
    pub density: f32,
    pub file: f32,
    pub export: f32,
    /// Icon button (Auto-arrange, Undo, Redo, ⋯).
    pub icon: f32,
    pub divider: f32,
    pub gap: f32,
    /// Side padding of the row.
    pub pad: f32,
}

impl ToolbarWidths {
    fn measure(ui: &Ui) -> Self {
        use neoflash_ui::metrics;
        let pad_x = c::button::button_padding();
        let label = |text: &str| text_width(ui, text, TextStyle::Body);
        // Segmented: every segment hugs its label with the button padding; 2 pt inset each side
        // plus 2 pt of slack for rounding.
        let segmented =
            |labels: &[&str]| labels.iter().map(|l| label(l) + 2.0 * pad_x).sum::<f32>() + 6.0;
        let button =
            |text: &str| 2.0 * pad_x + neoflash_ui::metrics::ICON_UI + space::s2() + label(text);
        Self {
            board: segmented(&BOARD_OPTIONS.map(|(_, l)| l)),
            preset: metrics::field_l(),
            add: button("Add"),
            theme: metrics::field_s(),
            density: segmented(&DENSITY_OPTIONS.map(|(_, l)| l)),
            file: metrics::field_s(),
            export: button("Export code"),
            icon: metrics::control(),
            divider: 1.0 + space::s2(),
            gap: space::s2(),
            pad: space::s4(),
        }
    }
}

/// Which toolbar groups are on the row; the others are items of the trailing ⋯ menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ToolbarPlan {
    pub auto_arrange: bool,
    pub history: bool,
    pub density: bool,
    pub theme: bool,
    /// Width of the preset Select (`field_l()` unless even the smallest row does not fit).
    pub preset_width: f32,
    /// Width the row needs, side padding included.
    pub natural: f32,
}

impl ToolbarPlan {
    pub fn overflow(&self) -> bool {
        !(self.auto_arrange && self.history && self.density && self.theme)
    }

    fn natural_width(&self, w: &ToolbarWidths) -> f32 {
        let mut items = vec![w.board, w.divider, self.preset_width, w.add];
        if self.auto_arrange {
            items.push(w.icon);
        }
        if self.theme || self.density {
            items.push(w.divider);
        }
        if self.theme {
            items.push(w.theme);
        }
        if self.density {
            items.push(w.density);
        }
        if self.history {
            items.extend([w.divider, w.icon, w.icon]);
        }
        if self.overflow() {
            items.push(w.icon);
        }
        // Flexible space (at least one gap), then File and Export code.
        items.extend([0.0, w.file, w.export]);
        items.iter().sum::<f32>() + w.gap * (items.len() - 1) as f32 + 2.0 * w.pad
    }
}

/// Fit the toolbar into `available`: groups leave the row for the ⋯ menu in the order
/// Auto-arrange, Undo / Redo, density, theme. The row never wraps.
pub(super) fn toolbar_plan(available: f32, w: &ToolbarWidths) -> ToolbarPlan {
    let mut plan = ToolbarPlan {
        auto_arrange: true,
        history: true,
        density: true,
        theme: true,
        preset_width: w.preset,
        natural: 0.0,
    };
    let steps: [fn(&mut ToolbarPlan); 4] = [
        |p| p.auto_arrange = false,
        |p| p.history = false,
        |p| p.density = false,
        |p| p.theme = false,
    ];
    for step in steps {
        if plan.natural_width(w) <= available {
            break;
        }
        step(&mut plan);
    }
    let over = plan.natural_width(w) - available;
    if over > 0.0 {
        plan.preset_width = (w.preset - over).max(w.theme);
    }
    plan.natural = plan.natural_width(w);
    plan
}

/// What the last toolbar pass used, for layout tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ToolbarProbe {
    pub rect: Rect,
    /// Right edge of the left groups and left edge of File / Export code.
    pub left_end: f32,
    pub right_start: f32,
    pub plan: ToolbarPlan,
}

#[cfg(test)]
pub(super) fn toolbar_probe_id() -> egui::Id {
    egui::Id::new("nf-ui-studio-toolbar-probe")
}

fn theme_menu_items(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    for theme in ThemeMode::ALL {
        if ui
            .add(c::MenuItem::new(theme.label()).checked(state.preview_theme == theme))
            .clicked()
        {
            state.set_preview_theme(theme);
        }
    }
    c::menu_separator(ui);
    if ui.add(c::MenuItem::new("Compare four themes")).clicked() {
        state.open_four_theme_comparison();
    }
}

fn density_menu_items(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    let current = state.preview_density_choice();
    for (choice, label) in [
        (DENSITY_OPTIONS[0].0, "Normal"),
        (DENSITY_OPTIONS[1].0, "Compact"),
        (DENSITY_OPTIONS[2].0, "Classic Flash"),
        (DENSITY_OPTIONS[3].0, "All three"),
    ] {
        if ui
            .add(c::MenuItem::new(label).checked(current == choice))
            .clicked()
        {
            state.set_preview_density_choice(choice);
        }
    }
}

fn render_studio_top_toolbar(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    let bar = ui.max_rect();
    let widths = ToolbarWidths::measure(ui);
    let plan = toolbar_plan(bar.width(), &widths);
    let inner = bar.shrink2(egui::vec2(widths.pad, 0.0));
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ui = &mut row;
    ui.spacing_mut().item_spacing.x = widths.gap;

    // Board
    let mut board = state.board_mode();
    if c::Segmented::show(ui, &mut board, &BOARD_OPTIONS, false).is_some() {
        state.set_board_mode(board);
    }
    c::divider(ui, true);

    // Panel preset
    let mut preset = state.active_preset;
    if c::Select::options(
        ui,
        "studio-preset-select",
        &mut preset,
        &PRESET_OPTIONS,
        plan.preset_width,
    ) {
        state.load_preset(preset);
    }
    if ui.add(c::Button::new("Add").icon(i16::PLUS)).clicked() {
        state.solo_active_artboard = false;
        state.add_artboard(preset);
    }
    if plan.auto_arrange
        && ui
            .add(c::IconButton::new(i16::GRID, "Auto-arrange previews"))
            .clicked()
    {
        state.auto_arrange_artboards();
    }

    // Theme and density of the previews
    if plan.theme || plan.density {
        c::divider(ui, true);
    }
    if plan.theme {
        c::Select::new(
            "studio-theme-select",
            theme_short_label(state.preview_theme),
        )
        .width(widths.theme)
        .show(ui, |ui| {
            ui.set_min_width(neoflash_ui::metrics::menu_width());
            theme_menu_items(ui, state);
        });
    }
    if plan.density {
        let mut choice = state.preview_density_choice();
        if c::Segmented::show(ui, &mut choice, &DENSITY_OPTIONS, false).is_some() {
            state.set_preview_density_choice(choice);
        }
    }

    // History
    if plan.history {
        c::divider(ui, true);
        if ui
            .add_enabled(
                state.history.can_undo(),
                c::IconButton::new(i16::UNDO, "Undo").shortcut("⌘Z"),
            )
            .clicked()
        {
            state.undo();
        }
        if ui
            .add_enabled(
                state.history.can_redo(),
                c::IconButton::new(i16::REDO, "Redo").shortcut("⇧⌘Z"),
            )
            .clicked()
        {
            state.redo();
        }
    }

    // Groups that did not fit
    if plan.overflow() {
        let more = ui.add(c::IconButton::new(i16::MORE, "More"));
        c::popup_below(&more, neoflash_ui::metrics::menu_width(), |ui| {
            if !plan.auto_arrange && ui.add(c::MenuItem::new("Auto-arrange previews")).clicked() {
                state.auto_arrange_artboards();
            }
            if !plan.history {
                if ui
                    .add_enabled(
                        state.history.can_undo(),
                        c::MenuItem::new("Undo").shortcut("⌘Z"),
                    )
                    .clicked()
                {
                    state.undo();
                }
                if ui
                    .add_enabled(
                        state.history.can_redo(),
                        c::MenuItem::new("Redo").shortcut("⇧⌘Z"),
                    )
                    .clicked()
                {
                    state.redo();
                }
            }
            if !plan.density {
                c::menu_section(ui, "Preview density");
                density_menu_items(ui, state);
            }
            if !plan.theme {
                c::menu_section(ui, "Preview theme");
                theme_menu_items(ui, state);
            }
        });
    }
    let left_end = ui.min_rect().right();

    // File and the one Primary action, at the right edge
    let right_start = ui
        .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(c::Button::primary("Export code").icon(i16::CODE))
                .clicked()
            {
                state.export_code_to_default_paths();
            }
            c::Select::new("studio-file-menu", "File")
                .width(widths.file)
                .show(ui, |ui| {
                    ui.set_min_width(neoflash_ui::metrics::menu_width());
                    if ui.add(c::MenuItem::new("Export SVG…")).clicked() {
                        state.export_svg_to_default_path(ui.ctx());
                    }
                    if ui.add(c::MenuItem::new("Import SVG → Panel…")).clicked() {
                        state.svg_import_modal_open = true;
                    }
                    c::menu_separator(ui);
                    if ui
                        .add(c::MenuItem::new("Save JSON").shortcut("⌘S"))
                        .clicked()
                    {
                        state.save_json_to_default_path();
                    }
                    if ui
                        .add_enabled(
                            Path::new(DEFAULT_STUDIO_JSON_PATH).exists(),
                            c::MenuItem::new("Open JSON"),
                        )
                        .clicked()
                        && let Err(err) =
                            state.load_studio_json(Path::new(DEFAULT_STUDIO_JSON_PATH))
                    {
                        state.status_message = err;
                    }
                    c::menu_separator(ui);
                    if ui
                        .add(c::MenuItem::new("Reset board to core panels"))
                        .clicked()
                    {
                        state.open_all_core_artboards();
                    }
                });
            ui.min_rect().left()
        })
        .inner;

    #[cfg(test)]
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            toolbar_probe_id(),
            ToolbarProbe {
                rect: bar,
                left_end,
                right_start,
                plan,
            },
        )
    });
    #[cfg(not(test))]
    let _ = (left_end, right_start);
}

fn render_svg_import_modal(ctx: &egui::Context, state: &mut UiLayoutStudioState) {
    let nf = neoflash_ui::palette(ctx);
    let screen = ctx.content_rect();
    let modal_w = neoflash_ui::metrics::dialog_l()
        .min((screen.width() - 2.0 * space::s6()).max(neoflash_ui::metrics::dialog_s()));
    let modal_pos = egui::pos2(
        screen.center().x - modal_w * 0.5,
        screen.top() + neoflash_ui::metrics::dialog_header() * 2.0,
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
                    title: "Import SVG as panel",
                })
                .show(ui)
                {
                    close_modal = true;
                }
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::s5() as i8))
                    .show(ui, |ui| {
                        ui.set_width(modal_w - space::s5() * 2.0);
                        ui.spacing_mut().item_spacing = egui::vec2(space::s2(), space::s2());
                        ui.label(
                            rich(
                                "Paste an SVG exported from the Studio, Figma or Illustrator. Panels, sections, rows, fields, selects, swatches, sliders, buttons and labels become an editable panel on the canvas.",
                                TextStyle::Small,
                            )
                            .color(nf.text_secondary),
                        );

                        ui.horizontal(|ui| {
                            if ui.add(c::Button::new("Sample SVG")).clicked() {
                                state.svg_buffer = sample_svg_panel_snippet();
                            }
                            if ui.add(c::Button::new("Active panel")).clicked() {
                                state.svg_buffer = state.export_active_svg();
                            }
                            if ui.add(c::Button::new("Last exported file")).clicked() {
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
                                    c::Button::primary("Add panel").icon(i16::CHECK),
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
