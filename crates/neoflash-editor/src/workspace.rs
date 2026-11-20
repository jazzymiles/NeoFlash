//! Application preferences, deliberately separate from Project and its history.
use crate::{app::BottomTab, theme::ThemeMode};
use serde::{Deserialize, Serialize};

mod dock;
mod panel_id;
mod prefs;
mod presets;
#[cfg(test)]
mod tests;

pub(crate) use dock::recover_geometry_with_bounds;
#[allow(unused_imports)]
pub use dock::{
    DetachedPanels, DockArea, DockDestination, DockEdge, DockGroup, DockLayout, DockLocation,
    DockSurface, FloatingDock, FloatingNode, PanelPlacement, PanelWindowPrefs, ScreenRect,
    SplitAxis, ToolsStageAnchor, WindowGeometry, default_window_size, maximum_window_size,
    minimum_tools_window_width, minimum_window_size, recover_geometry,
};
pub use panel_id::{PanelId, PanelSizes, PanelVisibility};
#[allow(unused_imports)]
pub use prefs::{
    ClipboardPrefs, CodeEditorPrefs, DrawingPrefs, GeneralPrefs, OnLaunch, PublishPrefs,
    PublishProfilePreset, PublishTarget, RecentFilesCache, ShortcutPrefs, TextAntiAlias, TextPrefs,
    TimelinePrefs, Tolerance, TweenOnDrag, UpdateChannel, UpdatePrefs,
};
pub use presets::{WorkspacePreset, WorkspacePresetError, WorkspaceTemplate};

use dock::density_metric;
use prefs::{default_sequence_import_options, default_true};
use presets::{deserialize_named_presets, normalized_preset_name};

/// The storage key stays stable so non-layout preferences survive layout migrations.
pub const PREFS_KEY: &str = "neoflash.workspace.v2";
// Includes up to 32 complete user layouts, each with its own floating windows.
pub const MAX_PREFS_BYTES: usize = 1024 * 1024;
const VERSION: u32 = 7;

const fn default_document_script_language() -> neoflash_core::ScriptLanguage {
    neoflash_core::ScriptLanguage::ActionScript3
}

/// Default document properties saved from Document settings ▸ Make default (`DL-02`, `R-102`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DocumentDefaults {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub background: neoflash_core::Color,
    pub units: crate::stage_view::RulerUnits,
    #[serde(default = "default_document_script_language")]
    pub script_language: neoflash_core::ScriptLanguage,
    #[serde(default = "default_true")]
    pub also_compile_compatible_scripts: bool,
}

impl Default for DocumentDefaults {
    fn default() -> Self {
        let blank = neoflash_core::Project::blank();
        Self {
            width: blank.width,
            height: blank.height,
            fps: blank.fps,
            background: blank.background,
            units: crate::stage_view::RulerUnits::Pixels,
            script_language: default_document_script_language(),
            also_compile_compatible_scripts: true,
        }
    }
}

impl DocumentDefaults {
    pub fn sanitize(&mut self) {
        self.width = self.width.clamp(1, 65535);
        self.height = self.height.clamp(1, 65535);
        self.fps = self.fps.clamp(1, 120);
    }
}

/// Stage toolbar state (View ▸ Stage toolbar; T-101 / R-053): zoom, fit and view toggles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageToolbarMode {
    /// Floating at the bottom centre of the Stage.
    Expanded,
    /// A small pill at the bottom-right corner that expands in place on hover (default of the
    /// Comfortable and Compact profiles).
    #[default]
    Collapsed,
    /// Classic Flash edit bar along the top of the Stage: never covers artwork (default of the
    /// Classic Flash profile).
    EditBar,
}

impl StageToolbarMode {
    /// Owner decision 2026-11-07: the profile chooses the toolbar state like it chooses row height.
    pub fn for_density(density: DensityPref) -> Self {
        match density {
            DensityPref::ClassicFlash => Self::EditBar,
            DensityPref::Comfortable | DensityPref::Compact => Self::Collapsed,
        }
    }
}

impl StageToolbarMode {
    pub const ALL: [Self; 3] = [Self::Expanded, Self::Collapsed, Self::EditBar];
    pub fn label(self) -> &'static str {
        match self {
            Self::Expanded => "Floating",
            Self::Collapsed => "Collapsed",
            Self::EditBar => "Edit bar",
        }
    }
}

/// Editor density profile (Preferences ▸ General ▸ Density, Window ▸ Density; T-101 / R-051).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DensityPref {
    #[default]
    Comfortable,
    Compact,
    ClassicFlash,
}

impl DensityPref {
    pub const ALL: [Self; 3] = [Self::Comfortable, Self::Compact, Self::ClassicFlash];
    pub fn ui(self) -> neoflash_ui::metrics::Density {
        match self {
            Self::Comfortable => neoflash_ui::metrics::Density::Comfortable,
            Self::Compact => neoflash_ui::metrics::Density::Compact,
            Self::ClassicFlash => neoflash_ui::metrics::Density::ClassicFlash,
        }
    }
    pub fn label(self) -> &'static str {
        self.ui().label()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePrefs {
    pub version: u32,
    pub theme: ThemeMode,
    /// Density profile: sizes of the editor chrome; behaviour is identical in every profile.
    pub density: DensityPref,
    /// Stage toolbar state (R-053).
    pub stage_toolbar: StageToolbarMode,
    /// Color panel shows the full SV / hue / channel picker (R-055); off = compact hex · alpha row
    /// with recent colours, the picker opens on demand.
    pub color_picker_expanded: bool,
    pub panels: PanelVisibility,
    pub active_bottom: BottomTab,
    pub sizes: PanelSizes,
    pub detached: DetachedPanels,
    pub dock: DockLayout,
    pub floating: Vec<FloatingDock>,
    /// Floating panel windows collapsed to their title header (`T-374`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub collapsed_floating: std::collections::BTreeSet<PanelId>,
    /// Template the current layout started from (shown in the top-bar workspace select).
    pub template: WorkspaceTemplate,
    /// Timeline frame view and playback options (Frame view ⋯, Control menu).
    pub timeline: TimelineView,
    /// Rulers, guides & grid settings saved with "Save as default" (View settings popover); every
    /// session starts from them. `None` = NF2 defaults.
    pub view_defaults: Option<crate::stage_view::ViewPrefs>,
    /// Last picked colours in Swatches (`PP-10`), most recent first (bounded to 12).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_colors: Vec<neoflash_core::Color>,
    /// User-added swatches (`Add swatch`, `PP-10`, bounded to 64).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_swatches: Vec<neoflash_core::Color>,
    /// Swatches / Properties ▸ Used colours ▸ Only on stage (`T-386`, default `true`).
    #[serde(default = "default_true")]
    pub used_colors_only_on_stage: bool,
    /// Default document settings saved with "Make default" (`DL-02`); `None` = `Project::blank()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_defaults: Option<DocumentDefaults>,
    /// Preferences ▸ General ▸ UI scale (egui zoom factor, 0.8–1.5).
    pub ui_scale: f32,
    /// Preferences ▸ General ▸ Undo levels (history capacity of each document).
    pub undo_levels: u32,
    /// Preferences ▸ General ▸ Shift-select: on = ⇧ adds to the selection, off = every click adds.
    pub shift_select: bool,
    /// Preferences ▸ Timeline ▸ Onion skin range (frames before, after) every session starts with.
    pub onion_range: [u32; 2],
    /// Preferences ▸ Timeline ▸ Default ease of new classic tweens.
    pub default_ease: neoflash_core::Easing,
    /// Preferences ▸ Text ▸ Default font family of new text (empty = Arial, else Helvetica).
    pub text_font: String,
    /// Preferences ▸ Text ▸ Default size of new text, in points.
    pub text_size: f64,
    /// Preferences ▸ Clipboard ▸ Default paste: plain Paste keeps the copied position.
    pub paste_in_place: bool,
    /// Preferences pages, one group each (`panels/preferences.rs`). Never document data.
    pub general: GeneralPrefs,
    pub code_editor: CodeEditorPrefs,
    pub timeline_prefs: TimelinePrefs,
    pub drawing: DrawingPrefs,
    pub tools: crate::tool_preferences::ToolPreferences,
    pub text_prefs: TextPrefs,
    pub clipboard_prefs: ClipboardPrefs,
    pub publish: PublishPrefs,
    pub shortcuts: ShortcutPrefs,
    #[serde(default)]
    pub ai: crate::ai_connections::AiPrefs,
    #[serde(default)]
    pub updates: UpdatePrefs,
    /// Complete named layouts, managed through the workspace selector.
    #[serde(
        default,
        deserialize_with = "deserialize_named_presets",
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub named_presets: std::collections::BTreeMap<String, WorkspacePreset>,
    /// The saved layout the current arrangement started from; edits do not change the saved copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_named_preset: Option<String>,
    /// The active saved preset was deleted/replaced; retain its live layout without claiming a built-in.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsaved_layout: bool,
    /// Last confirmed `Export image…` options (`DL-04`); Cancel leaves this unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_image_options: Option<neoflash_export::ImageExportOptions>,
    /// Last confirmed `Export video…` options (`DL-05`); Cancel leaves this unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_video_options: Option<neoflash_export::VideoOptions>,
    /// Last confirmed `Export audio…` options (`DL-06`); Cancel leaves this unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_audio_options: Option<neoflash_export::AudioExportOptions>,
    /// Last confirmed `Export Lottie…` options (`R-043`); Cancel leaves this unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_lottie_options: Option<neoflash_export::LottieOptions>,
    /// Authoring placement and frame step for numbered image-sequence imports (`T-218`).
    #[serde(default = "default_sequence_import_options")]
    pub sequence_import_options: neoflash_import::ImageSequenceImportOptions,
}

impl WorkspacePrefs {
    /// Clamp every Preferences group to the ranges its controls offer.
    pub(crate) fn sanitize_preference_groups(&mut self) {
        let g = &mut self.general;
        g.language = "English".into();
        g.recent_files.retain(|p| !p.is_empty() && p.len() <= 4096);
        g.recent_files.truncate(10);
        g.invalidate_recent_files_cache();
        g.session_files
            .retain(|p| !p.trim().is_empty() && p.len() <= 4096);
        g.session_files.truncate(GeneralPrefs::MAX_SESSION_FILES);
        if g.active_session_file
            .as_ref()
            .is_some_and(|p| p.trim().is_empty() || p.len() > 4096)
        {
            g.active_session_file = None;
        }
        g.autosave_minutes = g.autosave_minutes.clamp(1, 120);
        let c = &mut self.code_editor;
        c.font.truncate(256);
        c.font_size = finite_or(c.font_size, 13.0).clamp(8.0, 32.0);
        c.line_height = finite_or(c.line_height, 1.6).clamp(1.0, 3.0);
        c.hint_delay_ms = c.hint_delay_ms.min(2000);
        c.tab_size = c.tab_size.clamp(1, 8);
        let t = &mut self.timeline_prefs;
        t.onion_falloff = finite_or(t.onion_falloff, 70.0).clamp(15.0, 100.0);
        let d = &mut self.drawing;
        d.snap_tolerance = finite_or(d.snap_tolerance, 8.0).clamp(1.0, 32.0);
        self.clipboard_prefs.bitmap_ppi = self.clipboard_prefs.bitmap_ppi.clamp(72, 600);
        let p = &mut self.publish;
        if !p.has_any_enabled_target() {
            p.enable_target(p.default_target);
        }
        p.jpeg_quality = p.jpeg_quality.clamp(1, 100);
        p.atlas_extrude = p.atlas_extrude.min(8);
        p.banner_max_gzip_kb = p.banner_max_gzip_kb.clamp(10, 5000);
        if !neoflash_export::ImageExportFormat::SEQUENCE_FORMATS.contains(&p.image_sequence_format)
        {
            p.image_sequence_format = neoflash_export::ImageExportFormat::Png;
        }
        self.sequence_import_options.frame_step =
            self.sequence_import_options.frame_step.clamp(1, 60);
        if let Some(img) = &mut self.export_image_options {
            img.jpeg_quality = img.jpeg_quality.clamp(1, 100);
            img.width = img.width.clamp(1, 16_384);
            img.height = img.height.clamp(1, 16_384);
        }
        if let Some(vid) = &mut self.export_video_options {
            vid.quality = vid.quality.clamp(1, 100);
            if !vid.scale.is_finite() || vid.scale <= 0.0 {
                vid.scale = 1.0;
            } else {
                vid.scale = vid.scale.clamp(0.1, 8.0);
            }
            if !vid.fps.is_finite() || vid.fps < 0.0 {
                vid.fps = 0.0;
            } else if vid.fps > 120.0 {
                vid.fps = 120.0;
            }
        }
        p.output_folder = p
            .output_folder
            .take()
            .map(|f| f.trim().to_owned())
            .filter(|f| !f.is_empty() && f.len() <= 4096);
        p.desktop_app_folder = p
            .desktop_app_folder
            .take()
            .map(|f| f.trim().to_owned())
            .filter(|f| !f.is_empty() && f.len() <= 4096);
        p.image_sequence_folder = p
            .image_sequence_folder
            .take()
            .map(|f| f.trim().to_owned())
            .filter(|f| !f.is_empty() && f.len() <= 4096);
        p.svg_file = p
            .svg_file
            .take()
            .map(|f| f.trim().to_owned())
            .filter(|f| !f.is_empty() && f.len() <= 4096);
        p.video_file = p
            .video_file
            .take()
            .map(|f| f.trim().to_owned())
            .filter(|f| !f.is_empty() && f.len() <= 4096);
        p.video_options.quality = p.video_options.quality.clamp(1, 100);
        if !p.video_options.scale.is_finite() || p.video_options.scale <= 0.0 {
            p.video_options.scale = 1.0;
        } else {
            p.video_options.scale = p.video_options.scale.clamp(0.1, 8.0);
        }
        if !p.video_options.fps.is_finite() || p.video_options.fps < 0.0 {
            p.video_options.fps = 0.0;
        } else if p.video_options.fps > 120.0 {
            p.video_options.fps = 120.0;
        }
        self.shortcuts.sanitize();
    }

    pub const UI_SCALES: [(f32, &'static str); 6] = [
        (0.8, "80 %"),
        (0.9, "90 %"),
        (1.0, "100 %"),
        (1.1, "110 %"),
        (1.25, "125 %"),
        (1.5, "150 %"),
    ];
    pub const DEFAULT_UNDO_LEVELS: u32 = 150;
    pub const UNDO_LEVELS: std::ops::RangeInclusive<u32> = 10..=1000;
    pub const ONION_MAX: u32 = 60;
    pub const EASES: [(neoflash_core::Easing, &'static str); 4] = [
        (neoflash_core::Easing::Linear, "Linear"),
        (neoflash_core::Easing::EaseIn, "Ease In"),
        (neoflash_core::Easing::EaseOut, "Ease Out"),
        (neoflash_core::Easing::EaseInOut, "Ease In-Out"),
    ];
    pub const TEXT_SIZES: std::ops::RangeInclusive<f64> = 4.0..=200.0;
    pub const DEFAULT_TEXT_SIZE: f64 = 32.0;
}

/// Presentation and playback options of the Timeline. Workspace-only, never in the document.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineView {
    /// Off = spans are not colour-coded, only arrows (§5.4 "Tinted frames").
    pub tinted: bool,
    pub show_seconds: bool,
    /// 22 pt layer rows (Frame view ▸ Short).
    pub short_rows: bool,
    /// 16 pt compact layer rows (Frame view ▸ Compact).
    #[serde(default)]
    pub compact_rows: bool,
    /// Ruler scrubbing lands on the current layer's keyframes (⇧ suspends it).
    pub snap_playhead: bool,
    /// Off = editor playback stops on the last frame instead of wrapping.
    pub loop_playback: bool,
    /// Frame view ▸ Preview / Preview in context: keyframe thumbnails in taller rows.
    pub preview: FramePreview,
    /// Properties — Motion tween ▸ Show path (stage motion path overlay).
    pub show_motion_path: bool,
    /// Properties — Motion tween ▸ Show frame dots (per-frame dots along the stage motion path).
    pub show_motion_frame_dots: bool,
}

/// Keyframe thumbnails in the Timeline (Frame view ▸ Preview / Preview in context, TL-10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FramePreview {
    #[default]
    Off,
    /// Each span shows its layer's content at the keyframe, scaled to fit.
    Layer,
    /// Each span shows the whole stage at the keyframe.
    InContext,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            tinted: true,
            show_seconds: true,
            short_rows: false,
            compact_rows: false,
            snap_playhead: false,
            loop_playback: true,
            preview: FramePreview::Off,
            show_motion_path: true,
            show_motion_frame_dots: true,
        }
    }
}

impl TimelineView {
    /// Compact layer row height (`Frame view ▸ Compact`, 16 pt; R-052).
    pub const LAYER_ROW_COMPACT: f32 = neoflash_ui::metrics::LAYER_ROW_COMPACT;

    /// `NF2/TL Layer` row height for the current frame view.
    pub fn row_height(&self) -> f32 {
        if self.preview != FramePreview::Off {
            neoflash_ui::metrics::LAYER_ROW_PREVIEW
        } else if self.compact_rows {
            Self::LAYER_ROW_COMPACT
        } else if self.short_rows {
            neoflash_ui::metrics::LAYER_ROW_SHORT
        } else {
            neoflash_ui::metrics::LAYER_ROW
        }
    }
}
impl Default for WorkspacePrefs {
    fn default() -> Self {
        Self {
            version: VERSION,
            theme: ThemeMode::default(),
            panels: PanelVisibility::default(),
            active_bottom: BottomTab::Properties,
            sizes: PanelSizes::default(),
            detached: DetachedPanels::default(),
            dock: DockLayout::default(),
            floating: Vec::new(),
            collapsed_floating: std::collections::BTreeSet::new(),
            template: WorkspaceTemplate::Classic,
            density: DensityPref::default(),
            stage_toolbar: StageToolbarMode::default(),
            color_picker_expanded: false,
            timeline: TimelineView::default(),
            view_defaults: None,
            recent_colors: Vec::new(),
            custom_swatches: Vec::new(),
            used_colors_only_on_stage: true,
            document_defaults: None,
            ui_scale: 1.0,
            undo_levels: Self::DEFAULT_UNDO_LEVELS,
            shift_select: true,
            onion_range: [3, 3],
            default_ease: neoflash_core::Easing::EaseInOut,
            text_font: String::new(),
            text_size: Self::DEFAULT_TEXT_SIZE,
            paste_in_place: false,
            general: GeneralPrefs::default(),
            code_editor: CodeEditorPrefs::default(),
            timeline_prefs: TimelinePrefs::default(),
            drawing: DrawingPrefs::default(),
            tools: crate::tool_preferences::ToolPreferences::default(),
            text_prefs: TextPrefs::default(),
            clipboard_prefs: ClipboardPrefs::default(),
            publish: PublishPrefs::default(),
            shortcuts: ShortcutPrefs::default(),
            ai: crate::ai_connections::AiPrefs::default(),
            updates: UpdatePrefs::default(),
            named_presets: std::collections::BTreeMap::new(),
            active_named_preset: None,
            unsaved_layout: false,
            export_image_options: None,
            export_video_options: None,
            export_audio_options: None,
            export_lottie_options: None,
            sequence_import_options: default_sequence_import_options(),
        }
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

impl WorkspacePrefs {
    /// Missing, malformed, oversized or future-version preferences fail soft.
    /// Unknown fields within this version are ignored and never written back.
    pub fn from_json(json: &str) -> Self {
        if json.len() > MAX_PREFS_BYTES {
            return Self::default();
        }
        let Ok(mut prefs) = serde_json::from_str::<Self>(json) else {
            return Self::default();
        };
        if !(1..=VERSION).contains(&prefs.version) {
            return Self::default();
        }
        let saved_version = prefs.version;
        // Span-based selection was briefly persisted as an always-on default, and workspaces
        // saved after that release can still carry the accidental value even though the default
        // was corrected later. Reset it once for every pre-v7 workspace so an ordinary click
        // reliably targets one frame after updating. An explicit opt-in made on v7+ persists.
        if saved_version < 7 {
            prefs.timeline_prefs.span_selection = false;
        }
        // Earlier preferences stored fixed panel sizes and detached flags.
        // Migrate once; thereafter the grid is the authoritative dock layout.
        let has_grid = serde_json::from_str::<serde_json::Value>(json)
            .ok()
            .is_some_and(|value| value.get("dock").is_some());
        if !has_grid {
            prefs.dock.top.size = prefs.sizes.timeline_height;
            prefs.dock.bottom.size = prefs.sizes.bottom_height;
            prefs.dock.right.size = prefs.sizes.library_width;
            prefs
                .dock
                .select(PanelId::from_bottom_tab(prefs.active_bottom));
        }
        // NF2 v2.2 changed the canonical built-in compositions. A version-2 live layout can keep
        // stale visibility (for example Color as the only right inspector) even though the current
        // template is correct. Refresh that live composition once while preserving editor, timeline,
        // publish and tool preferences plus every explicitly saved named layout.
        if saved_version < 3 {
            let (dock, panels, active_bottom) = prefs.template.layout();
            prefs.dock = dock;
            prefs.panels = panels;
            prefs.active_bottom = active_bottom;
            prefs.detached = DetachedPanels::default();
            prefs.floating.clear();
            prefs.active_named_preset = None;
            prefs.unsaved_layout = false;
        }
        // Versions 1–3 shipped Create keyframe on transform (`auto_keyframe`) enabled by default,
        // which split spans whenever an object was edited on a held frame. Restore the classic
        // Flash default (`false`) for existing workspaces while preserving explicit opt-ins from v4+.
        if saved_version < 4 {
            prefs.timeline_prefs.auto_keyframe = false;
        }
        if saved_version < 5 {
            prefs.panels.accessibility = false;
        }
        if saved_version < 6 {
            prefs.panels.debugger = false;
        }
        prefs.version = VERSION;
        prefs.sanitize();
        prefs
    }
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        storage
            .and_then(|s| s.get_string(PREFS_KEY))
            .map_or_else(Self::default, |json| Self::from_json(&json))
    }
    /// Choose a compact default once, never rewrite a saved user layout on resize.
    pub fn load_for_viewport(storage: Option<&dyn eframe::Storage>, size: [f32; 2]) -> Self {
        let mut prefs = Self::load(storage);
        if storage.and_then(|s| s.get_string(PREFS_KEY)).is_none() {
            prefs.compact_default(size);
        }
        prefs
    }
    pub fn compact_default(&mut self, size: [f32; 2]) {
        if size[0] > 0. && size[1] > 0. && (size[0] <= 1120. || size[1] <= 800.) {
            let tabs = self
                .dock
                .right
                .groups
                .iter()
                .flat_map(|g| g.tabs.iter().copied())
                .collect();
            self.dock.right.groups = vec![DockGroup {
                id: 4,
                tabs,
                active: PanelId::Properties,
                weight: 1.,
            }];
            self.dock.bottom.size = neoflash_ui::metrics::BOTTOM_DOCK;
        }
    }
    /// Only this small JSON key is written. Eframe owns flushing the storage;
    /// this never serializes Editor, Project, clipboard or arbitrary egui memory.
    pub fn save(&self, storage: &mut dyn eframe::Storage) {
        let mut normalized = self.clone();
        normalized.sanitize();
        if let Ok(json) = serde_json::to_string(&normalized) {
            storage.set_string(PREFS_KEY, json);
        }
    }
    pub fn sanitize(&mut self) {
        self.tools.sanitize();
        self.version = VERSION;
        if !Self::UI_SCALES
            .iter()
            .any(|(scale, _)| *scale == self.ui_scale)
        {
            self.ui_scale = 1.0;
        }
        self.undo_levels = self
            .undo_levels
            .clamp(*Self::UNDO_LEVELS.start(), *Self::UNDO_LEVELS.end());
        self.onion_range = self.onion_range.map(|n| n.min(Self::ONION_MAX));
        if !Self::EASES
            .iter()
            .any(|(ease, _)| *ease == self.default_ease)
        {
            self.default_ease = neoflash_core::Easing::EaseInOut;
        }
        if !self.text_size.is_finite() {
            self.text_size = Self::DEFAULT_TEXT_SIZE;
        }
        self.text_size = self
            .text_size
            .clamp(*Self::TEXT_SIZES.start(), *Self::TEXT_SIZES.end());
        self.text_font.truncate(256);
        self.sanitize_preference_groups();
        self.ai.sanitize();
        if let Some(view) = &mut self.view_defaults {
            view.sanitize();
        }
        if let Some(doc) = &mut self.document_defaults {
            doc.sanitize();
        }
        self.recent_colors
            .truncate(neoflash_ui::components::swatches::PALETTE_COLUMNS);
        self.custom_swatches.truncate(64);
        self.sanitize_layout();
        let mut presets = std::collections::BTreeMap::new();
        for (name, mut preset) in std::mem::take(&mut self.named_presets) {
            // Older builds allowed built-in names; keep those saved layouts loadable/renameable.
            if let Ok(name) = normalized_preset_name(&name)
                && presets.len() < Self::MAX_NAMED_PRESETS
            {
                preset.sanitize();
                presets.entry(name.to_owned()).or_insert(preset);
            }
        }
        self.named_presets = presets;
        let had_named_preset = self.active_named_preset.is_some();
        self.active_named_preset = self
            .active_named_preset
            .take()
            .map(|name| name.trim().to_owned())
            .filter(|name| self.named_presets.contains_key(name));
        if self.active_named_preset.is_some() {
            self.unsaved_layout = false;
        } else if had_named_preset {
            self.unsaved_layout = true;
        }
    }

    fn sanitize_layout(&mut self) {
        // Window size limits follow density: check saved geometry against the saved profile, not
        // against whatever profile the calling thread happens to use (Normal while loading).
        let caller = neoflash_ui::metrics::density();
        neoflash_ui::metrics::set_density(self.density.ui());
        self.sanitize_layout_in_saved_density();
        neoflash_ui::metrics::set_density(caller);
    }

    fn sanitize_layout_in_saved_density(&mut self) {
        self.sizes.sanitize();
        self.detached.sanitize();
        self.dock.sanitize(self.detached);
        let mut seen = std::collections::HashSet::new();
        let mut windows = std::collections::HashSet::new();
        self.floating.retain_mut(|window| {
            if !windows.insert(window.id) {
                return false;
            }
            let mut ids = std::collections::HashSet::new();
            let mut next = 1;
            window.root = window
                .root
                .take()
                .and_then(|root| root.sanitize(&mut seen, &mut ids, &mut next, self.detached));
            if window.panels() != [PanelId::Tools] {
                window.tools_anchor = None;
            }
            let minimum = ids.into_iter().max().unwrap_or(0) + 1;
            window.next_group = window.next_group.clamp(minimum, 1_000_008);
            if let Some(panel) = window.panels().first() {
                window.geometry = window
                    .geometry
                    .and_then(|geometry| geometry.sanitized(*panel));
            }
            window.root.is_some()
        });
        // Old single-window preferences migrate into one floating tab group.
        for panel in PanelId::DETACHABLE {
            if self.placement(panel) == PanelPlacement::Detached
                && self.floating_window(panel).is_none()
            {
                let geometry = self.detached.get(panel).and_then(|p| p.geometry);
                let visible = self.is_panel_visible(panel);
                self.float_panel(panel, geometry);
                *self.panels.slot(panel) = visible;
            }
        }
        self.collapsed_floating
            .retain(|id| id.detachable() && self.floating.iter().any(|w| w.id == *id));
        self.normalize_active_bottom();
    }
    pub fn is_panel_visible(&self, panel: PanelId) -> bool {
        self.panels.is_visible(panel)
    }
    /// Whether `panel` is visible and is the active tab of its dock or floating group.
    pub fn is_panel_active(&self, panel: PanelId) -> bool {
        if !self.is_panel_visible(panel) {
            return false;
        }
        if let Some(location) = self.dock.location(panel) {
            return self
                .dock
                .area(location.edge)
                .groups
                .iter()
                .find(|g| g.id == location.group)
                .and_then(|g| g.active_visible(self.panels))
                == Some(panel);
        }
        if let Some(window) = self.floating_window(panel)
            && let Some(root) = &window.root
            && let Some((group_id, _)) = root.location(panel)
        {
            return root
                .group(group_id)
                .and_then(|g| g.active_visible(self.panels))
                == Some(panel);
        }
        true
    }
    #[cfg(test)]
    pub fn bottom_visible(&self) -> bool {
        self.dock
            .bottom
            .groups
            .iter()
            .any(|group| group.tabs.iter().any(|panel| self.is_panel_visible(*panel)))
    }
    /// Revealing a bottom panel also selects it. F9 and error-reveal commands
    /// must use this even when a dock/tab was previously hidden.
    pub fn show_panel(&mut self, panel: PanelId) -> bool {
        if self.dock.location(panel).is_none() && self.floating_window(panel).is_none() {
            self.dock.insert_at_default(panel);
        }
        let mut changed = !*self.panels.slot(panel);
        *self.panels.slot(panel) = true;
        changed |= self.dock.select(panel);
        for window in &mut self.floating {
            changed |= window.select(panel);
        }
        if let Some(tab) = panel.bottom_tab().filter(|_| self.is_panel_docked(panel)) {
            changed |= self.active_bottom != tab;
            self.active_bottom = tab;
        }
        changed
    }
    pub fn set_panel_visible(&mut self, panel: PanelId, visible: bool) -> bool {
        if visible {
            return self.show_panel(panel);
        }
        let mut changed = *self.panels.slot(panel);
        *self.panels.slot(panel) = false;
        changed |= self.normalize_active_groups();
        let before = self.active_bottom;
        self.normalize_active_bottom();
        changed || before != self.active_bottom
    }
    fn normalize_active_groups(&mut self) -> bool {
        let mut changed = false;
        for edge in DockEdge::ALL {
            for group in &mut self.dock.area_mut(edge).groups {
                changed |= group.normalize_active(self.panels);
            }
        }
        for window in &mut self.floating {
            if let Some(root) = &mut window.root {
                changed |= root.normalize_active(self.panels);
            }
        }
        changed
    }

    #[cfg(test)]
    pub fn record_recent_file(&mut self, path: impl Into<std::path::PathBuf>) {
        let path = path.into();
        self.general.push_recent(&path);
    }

    pub fn clear_recent_files(&mut self) {
        self.general.recent_files.clear();
        self.general.invalidate_recent_files_cache();
    }

    /// Switch the density profile. Its layer-row preset (Normal / Short / Compact), default
    /// layer-column width and Stage toolbar state become current; the user can change them afterwards.
    pub fn set_density(&mut self, density: DensityPref) {
        // A side dock that still sits at the previous profile's default width (inspector, tools
        // rail or two tool columns, plus the 6 pt resize strip) follows the new profile; a width
        // the user chose is kept and only clamped to the new minimum when drawn.
        let defaults = |density: DensityPref| {
            ["inspector", "tools_two_columns", "tools_rail"]
                .map(|name| density_metric(name, density.ui()) + 6.)
        };
        let (old, new) = (defaults(self.density), defaults(density));
        for area in [&mut self.dock.left, &mut self.dock.right] {
            if let Some(index) = old.iter().position(|size| (area.size - size).abs() < 0.5) {
                area.size = new[index];
            }
        }
        self.density = density;
        self.stage_toolbar = StageToolbarMode::for_density(density);
        let d = density.ui();
        self.timeline.short_rows = d == neoflash_ui::metrics::Density::Compact;
        self.timeline.compact_rows = d == neoflash_ui::metrics::Density::ClassicFlash;
        self.sizes.layer_column_width = d.pick(
            neoflash_ui::metrics::LAYER_COLUMN_MODE_SOLO,
            neoflash_ui::metrics::LAYER_COLUMN_MODE_SOLO - 24.,
            neoflash_ui::metrics::LAYER_COLUMN_MODE_SOLO - 40.,
        );
    }
    pub fn placement(&self, panel: PanelId) -> PanelPlacement {
        if self.dock.location(panel).is_some() {
            PanelPlacement::Docked
        } else {
            PanelPlacement::Detached
        }
    }
    pub fn is_panel_docked(&self, panel: PanelId) -> bool {
        self.placement(panel) == PanelPlacement::Docked
    }
    pub(crate) fn shares_dock_group(&self, a: PanelId, b: PanelId) -> bool {
        matches!(
            (self.dock.location(a), self.dock.location(b)),
            (Some(la), Some(lb)) if la.edge == lb.edge && la.group == lb.group
        )
    }
    /// A placement command reveals the panel. Native close should only hide it,
    /// retaining placement and geometry so Window/F9 can reopen the same panel.
    pub fn set_placement(&mut self, panel: PanelId, placement: PanelPlacement) -> bool {
        let old = self.placement(panel);
        let grouped = self
            .floating_window(panel)
            .is_some_and(|window| window.panels().len() > 1);
        if old != placement || (grouped && placement == PanelPlacement::Detached) {
            if placement == PanelPlacement::Detached {
                let geometry = self.detached_geometry(panel);
                self.float_panel(panel, geometry);
            } else {
                let destination = self.detached.get(panel).and_then(|w| w.last_dock).map_or(
                    DockDestination::Edge(DockEdge::default_for(panel)),
                    DockDestination::Tabs,
                );
                self.dock_panel(panel, destination);
            }
        }
        let shown = self.show_panel(panel);
        self.normalize_active_bottom();
        old != placement || grouped || shown
    }
    pub fn dock_panel(&mut self, panel: PanelId, destination: DockDestination) {
        match destination {
            DockDestination::FloatingTabs {
                window,
                group,
                mut index,
            } => {
                let Some(target) = self.floating.iter().find(|w| w.id == window) else {
                    return;
                };
                if target.group(group).is_none() {
                    return;
                }
                if let Some((old_group, old_index)) =
                    target.root.as_ref().and_then(|root| root.location(panel))
                    && old_group == group
                    && old_index < index
                {
                    index = index.saturating_sub(1);
                }
                self.leave_root_dock(panel);
                self.remove_floating_panel(panel, Some((window, group)));
                let target = self
                    .floating
                    .iter_mut()
                    .find(|w| w.id == window)
                    .expect("retained target window");
                let group = target
                    .root
                    .as_mut()
                    .and_then(|root| root.group_mut(group))
                    .expect("retained target group");
                group.tabs.insert(index.min(group.tabs.len()), panel);
                group.active = panel;
                self.detached.get_mut(panel).expect("known panel").placement =
                    PanelPlacement::Detached;
                self.show_panel(panel);
                self.normalize_active_bottom();
                return;
            }
            DockDestination::FloatingSplit {
                window,
                group,
                edge,
            } => {
                let Some(target) = self.floating.iter().find(|w| w.id == window) else {
                    return;
                };
                if group.is_some_and(|id| target.group(id).is_none()) {
                    return;
                }
                if target.panels() == [panel]
                    || group.is_some_and(|id| target.group(id).is_some_and(|g| g.tabs == [panel]))
                {
                    return;
                }
                self.leave_root_dock(panel);
                self.remove_floating_panel(panel, None);
                let target = self
                    .floating
                    .iter_mut()
                    .find(|w| w.id == window)
                    .expect("nonempty target window");
                let new_group = target.fresh_group(panel);
                target.root = target
                    .root
                    .take()
                    .map(|root| root.split(group, edge, new_group));
                self.detached.get_mut(panel).expect("known panel").placement =
                    PanelPlacement::Detached;
                self.show_panel(panel);
                self.normalize_active_bottom();
                return;
            }
            _ => {}
        }
        self.remove_floating_panel(panel, None);
        self.dock.insert(panel, destination);
        let location = self.dock.location(panel);
        let window = self.detached.get_mut(panel).expect("known panel");
        window.placement = PanelPlacement::Docked;
        window.last_dock = location;
        self.show_panel(panel);
        self.normalize_active_bottom();
    }
    /// Move a tab group as one unit. Hidden tabs keep their visibility, and the
    /// selected tab survives moves between the main and native-window layouts.
    pub fn dock_group(
        &mut self,
        panels: &[PanelId],
        active: PanelId,
        destination: DockDestination,
    ) {
        let Some(&first) = panels.first() else { return };
        let same_group = match destination {
            DockDestination::Tabs(location) => self
                .dock
                .area(location.edge)
                .groups
                .iter()
                .find(|g| g.id == location.group)
                .map(|g| &g.tabs),
            DockDestination::Split { edge, group, .. } => self
                .dock
                .area(edge)
                .groups
                .iter()
                .find(|g| g.id == group)
                .map(|g| &g.tabs),
            DockDestination::FloatingTabs { window, group, .. }
            | DockDestination::FloatingSplit {
                window,
                group: Some(group),
                ..
            } => self
                .floating
                .iter()
                .find(|w| w.id == window)
                .and_then(|w| w.group(group))
                .map(|g| &g.tabs),
            _ => None,
        }
        .is_some_and(|tabs| tabs.len() == panels.len() && panels.iter().all(|p| tabs.contains(p)));
        let same_window = match destination {
            DockDestination::FloatingSplit {
                window,
                group: None,
                ..
            } => self
                .floating
                .iter()
                .find(|w| w.id == window)
                .is_some_and(|w| {
                    let tabs = w.panels();
                    tabs.len() == panels.len() && panels.iter().all(|p| tabs.contains(p))
                }),
            _ => false,
        };
        if same_group || same_window {
            return;
        }
        let visibility: Vec<_> = panels
            .iter()
            .map(|p| (*p, self.is_panel_visible(*p)))
            .collect();
        self.dock_panel(first, destination);
        for (offset, panel) in panels.iter().enumerate().skip(1) {
            let destination = if let Some(location) = self.dock.location(first) {
                DockDestination::Tabs(DockLocation {
                    index: location.index + offset,
                    ..location
                })
            } else {
                let window = self.floating_window(first).expect("moved group window");
                let (group, index) = window
                    .root
                    .as_ref()
                    .and_then(|root| root.location(first))
                    .expect("moved group tab");
                DockDestination::FloatingTabs {
                    window: window.id,
                    group,
                    index: index + offset,
                }
            };
            self.dock_panel(*panel, destination);
        }
        for (panel, visible) in visibility {
            *self.panels.slot(panel) = visible;
        }
        self.show_panel(active);
    }

    pub fn float_group(
        &mut self,
        panels: &[PanelId],
        active: PanelId,
        geometry: WindowGeometry,
    ) -> PanelId {
        let first = panels[0];
        if let Some(window) = self.floating.iter().find(|w| {
            let tabs = w.panels();
            tabs.len() == panels.len() && panels.iter().all(|p| tabs.contains(p))
        }) {
            let id = window.id;
            self.remember_floating_geometry(id, geometry);
            return id;
        }
        let visible = self.is_panel_visible(first);
        let window = self.float_panel(first, Some(geometry));
        *self.panels.slot(first) = visible;
        let group = self
            .floating_window(first)
            .and_then(|w| w.root.as_ref())
            .and_then(|root| root.location(first))
            .expect("new floating group")
            .0;
        self.dock_group(
            panels,
            active,
            DockDestination::FloatingTabs {
                window,
                group,
                index: 0,
            },
        );
        window
    }
    pub fn detached_geometry(&self, panel: PanelId) -> Option<WindowGeometry> {
        self.floating_window(panel)
            .and_then(|window| window.geometry)
            .or_else(|| self.detached.get(panel).and_then(|p| p.geometry))
    }
    pub fn floating_window(&self, panel: PanelId) -> Option<&FloatingDock> {
        self.floating.iter().find(|window| window.contains(panel))
    }
    pub fn tools_stage_anchor(&self, panel: PanelId) -> Option<ToolsStageAnchor> {
        self.floating_window(panel)
            .filter(|window| window.panels() == [PanelId::Tools])
            .and_then(|window| window.tools_anchor)
    }
    pub fn tools_popover_prefers_left(&self) -> bool {
        self.tools_stage_anchor(PanelId::Tools)
            .is_some_and(|anchor| !anchor.expands_right())
    }
    pub fn set_tools_stage_anchor(
        &mut self,
        window_id: PanelId,
        anchor: Option<ToolsStageAnchor>,
    ) -> bool {
        let Some(window) = self
            .floating
            .iter_mut()
            .find(|window| window.id == window_id)
        else {
            return false;
        };
        let anchor = if window.panels() == [PanelId::Tools] {
            anchor
        } else {
            None
        };
        let changed = window.tools_anchor != anchor;
        window.tools_anchor = anchor;
        changed
    }
    pub fn panel_surface(&self, panel: PanelId) -> DockSurface {
        self.floating_window(panel)
            .map_or(DockSurface::Main, |window| DockSurface::Floating(window.id))
    }
    fn leave_root_dock(&mut self, panel: PanelId) {
        if let Some(location) = self.dock.location(panel) {
            self.detached.get_mut(panel).expect("known panel").last_dock = Some(location);
            self.dock.remove(panel, None);
        }
    }
    fn remove_floating_panel(&mut self, panel: PanelId, keep: Option<(PanelId, u32)>) {
        for window in &mut self.floating {
            let keep_group = keep
                .filter(|(id, _)| *id == window.id)
                .map(|(_, group)| group);
            window.root = window
                .root
                .take()
                .and_then(|root| root.remove(panel, keep_group));
        }
        self.floating.retain(|window| window.root.is_some());
        self.collapsed_floating
            .retain(|id| self.floating.iter().any(|w| w.id == *id));
    }
    /// Tear one tab into its own native window. Existing standalone windows keep
    /// identity and size; group membership never determines viewport identity.
    pub fn float_panel(&mut self, panel: PanelId, geometry: Option<WindowGeometry>) -> PanelId {
        if let Some(window) = self
            .floating
            .iter_mut()
            .find(|window| window.panels() == [panel])
        {
            if let Some(geometry) = geometry.and_then(|geometry| geometry.sanitized(panel)) {
                window.geometry = Some(geometry);
            }
            return window.id;
        }
        self.leave_root_dock(panel);
        self.remove_floating_panel(panel, None);
        let id = if self.floating.iter().all(|window| window.id != panel) {
            panel
        } else {
            PanelId::DETACHABLE
                .into_iter()
                .find(|id| self.floating.iter().all(|window| window.id != *id))
                .expect("at most ten windows")
        };
        let geometry = geometry.and_then(|geometry| geometry.sanitized(panel));
        self.floating.push(FloatingDock::new(id, panel, geometry));
        self.detached.get_mut(panel).expect("known panel").placement = PanelPlacement::Detached;
        self.show_panel(panel);
        self.normalize_active_bottom();
        id
    }
    pub fn remember_floating_geometry(&mut self, id: PanelId, geometry: WindowGeometry) -> bool {
        let Some(window) = self.floating.iter_mut().find(|window| window.id == id) else {
            return false;
        };
        let panels = window.panels();
        let Some(panel) = panels.first() else {
            return false;
        };
        let Some(geometry) = geometry.sanitized(*panel) else {
            return false;
        };
        let changed = window.geometry != Some(geometry);
        window.geometry = Some(geometry);
        if panels != [PanelId::Tools] {
            window.tools_anchor = None;
        }
        for panel in panels {
            self.detached.get_mut(panel).expect("known panel").geometry = Some(geometry);
        }
        changed
    }
    /// Minimized/missing/nonfinite native snapshots must not erase good geometry.
    #[cfg(test)]
    pub fn remember_geometry(&mut self, panel: PanelId, geometry: WindowGeometry) -> bool {
        if let Some(id) = self.floating_window(panel).map(|window| window.id) {
            return self.remember_floating_geometry(id, geometry);
        }
        let Some(geometry) = geometry.sanitized(panel) else {
            return false;
        };
        let Some(window) = self.detached.get_mut(panel) else {
            return false;
        };
        let changed = window.geometry != Some(geometry);
        window.geometry = Some(geometry);
        changed
    }
    pub fn is_floating_window_collapsed(&self, window_id: PanelId) -> bool {
        self.collapsed_floating.contains(&window_id)
    }
    pub fn set_floating_window_collapsed(&mut self, window_id: PanelId, collapsed: bool) -> bool {
        if collapsed {
            if window_id.detachable() && self.floating.iter().any(|w| w.id == window_id) {
                self.collapsed_floating.insert(window_id)
            } else {
                false
            }
        } else {
            self.collapsed_floating.remove(&window_id)
        }
    }
    fn normalize_active_bottom(&mut self) {
        let active = PanelId::from_bottom_tab(self.active_bottom);
        if (!self.is_panel_visible(active) || !self.is_panel_docked(active))
            && let Some(tab) = PanelId::BOTTOM
                .into_iter()
                .find(|&panel| self.is_panel_visible(panel) && self.is_panel_docked(panel))
                .and_then(PanelId::bottom_tab)
        {
            self.active_bottom = tab;
        }
        // With every tab hidden, keep the last selection until an explicit show.
    }
}
