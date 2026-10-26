//! Application preferences, deliberately separate from Project and its history.
use crate::{app::BottomTab, theme::ThemeMode};
use serde::{Deserialize, Serialize};

pub const PREFS_KEY: &str = "neoflash.workspace.v1";
pub const MAX_PREFS_BYTES: usize = 16 * 1024;
const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PanelId {
    Timeline,
    Tools,
    Library,
    Properties,
    Actions,
    Accessibility,
    Output,
}
impl PanelId {
    pub const DETACHABLE: [Self; 3] = [Self::Tools, Self::Timeline, Self::Actions];
    pub fn detachable(self) -> bool {
        Self::DETACHABLE.contains(&self)
    }
    /// Stable identifiers are not serialized egui memory or document IDs.
    pub fn viewport_key(self) -> Option<&'static str> {
        match self {
            Self::Tools => Some("neoflash.panel.tools"),
            Self::Timeline => Some("neoflash.panel.timeline"),
            Self::Actions => Some("neoflash.panel.actions"),
            _ => None,
        }
    }
    #[cfg(test)]
    pub const ALL: [Self; 7] = [
        Self::Timeline,
        Self::Tools,
        Self::Library,
        Self::Properties,
        Self::Actions,
        Self::Accessibility,
        Self::Output,
    ];
    pub const BOTTOM: [Self; 4] = [
        Self::Properties,
        Self::Actions,
        Self::Accessibility,
        Self::Output,
    ];
    pub fn bottom_tab(self) -> Option<BottomTab> {
        match self {
            Self::Properties => Some(BottomTab::Properties),
            Self::Actions => Some(BottomTab::Actions),
            Self::Accessibility => Some(BottomTab::Accessibility),
            Self::Output => Some(BottomTab::Output),
            _ => None,
        }
    }
    pub fn from_bottom_tab(tab: BottomTab) -> Self {
        match tab {
            BottomTab::Properties => Self::Properties,
            BottomTab::Actions => Self::Actions,
            BottomTab::Accessibility => Self::Accessibility,
            BottomTab::Output => Self::Output,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelVisibility {
    pub timeline: bool,
    pub tools: bool,
    pub library: bool,
    pub properties: bool,
    pub actions: bool,
    pub accessibility: bool,
    pub output: bool,
}
impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            timeline: true,
            tools: true,
            library: true,
            properties: true,
            actions: true,
            accessibility: true,
            output: true,
        }
    }
}
impl PanelVisibility {
    fn slot(&mut self, panel: PanelId) -> &mut bool {
        match panel {
            PanelId::Timeline => &mut self.timeline,
            PanelId::Tools => &mut self.tools,
            PanelId::Library => &mut self.library,
            PanelId::Properties => &mut self.properties,
            PanelId::Actions => &mut self.actions,
            PanelId::Accessibility => &mut self.accessibility,
            PanelId::Output => &mut self.output,
        }
    }
    pub fn is_visible(self, panel: PanelId) -> bool {
        match panel {
            PanelId::Timeline => self.timeline,
            PanelId::Tools => self.tools,
            PanelId::Library => self.library,
            PanelId::Properties => self.properties,
            PanelId::Actions => self.actions,
            PanelId::Accessibility => self.accessibility,
            PanelId::Output => self.output,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelSizes {
    pub timeline_height: f32,
    pub bottom_height: f32,
    pub library_width: f32,
}
impl Default for PanelSizes {
    fn default() -> Self {
        Self {
            timeline_height: 207.,
            bottom_height: 190.,
            library_width: 228.,
        }
    }
}
impl PanelSizes {
    /// Stored preferences have static safety limits. Rendering additionally
    /// clamps these against the current viewport and space reserved for Stage.
    fn sanitize(&mut self) {
        let default = Self::default();
        let size = |value: f32, fallback: f32, min: f32, max: f32| {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                fallback
            }
        };
        self.timeline_height = size(self.timeline_height, default.timeline_height, 130., 380.);
        self.bottom_height = size(self.bottom_height, default.bottom_height, 145., 420.);
        self.library_width = size(self.library_width, default.library_width, 190., 480.);
    }
}

/// Window placement belongs to the workspace, never to the authored Project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelPlacement {
    #[default]
    Docked,
    Detached,
}

/// OS logical points in a global top-left-origin desktop coordinate system.
/// On macOS these are Cocoa points with Y flipped around the primary screen.
/// Multiply egui ViewportInfo coordinates by ctx.zoom_factor() when capturing;
/// divide by the same factor when supplying a ViewportBuilder. Do not multiply
/// by the Retina backing scale. Position is outer; size excludes window chrome.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub outer_position: [f32; 2],
    pub inner_size: [f32; 2],
}
impl WindowGeometry {
    fn sanitized(self, panel: PanelId) -> Option<Self> {
        if !self
            .outer_position
            .into_iter()
            .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
            || !self.inner_size.into_iter().all(|v| v.is_finite() && v > 0.)
        {
            return None;
        }
        let min = minimum_window_size(panel);
        Some(Self {
            outer_position: self.outer_position,
            inner_size: [
                self.inner_size[0].clamp(min[0], 4096.),
                self.inner_size[1].clamp(min[1], 2160.),
            ],
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelWindowPrefs {
    pub placement: PanelPlacement,
    pub geometry: Option<WindowGeometry>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetachedPanels {
    pub tools: PanelWindowPrefs,
    pub timeline: PanelWindowPrefs,
    pub actions: PanelWindowPrefs,
}
impl DetachedPanels {
    fn get(&self, panel: PanelId) -> Option<&PanelWindowPrefs> {
        match panel {
            PanelId::Tools => Some(&self.tools),
            PanelId::Timeline => Some(&self.timeline),
            PanelId::Actions => Some(&self.actions),
            _ => None,
        }
    }
    fn get_mut(&mut self, panel: PanelId) -> Option<&mut PanelWindowPrefs> {
        match panel {
            PanelId::Tools => Some(&mut self.tools),
            PanelId::Timeline => Some(&mut self.timeline),
            PanelId::Actions => Some(&mut self.actions),
            _ => None,
        }
    }
    fn sanitize(&mut self) {
        for panel in PanelId::DETACHABLE {
            let window = self.get_mut(panel).expect("supported panel");
            window.geometry = window
                .geometry
                .and_then(|geometry| geometry.sanitized(panel));
        }
    }
}

/// A native monitor's usable area (excluding menu bar/Dock), in OS logical
/// points. A root-window rectangle can be supplied as fallback when enumeration
/// is unavailable; it is only a recovery target, never a window to be moved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenRect {
    pub origin: [f32; 2],
    pub size: [f32; 2],
}
impl ScreenRect {
    pub fn is_valid(self) -> bool {
        self.origin
            .into_iter()
            .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
            && self
                .size
                .into_iter()
                .all(|v| v.is_finite() && (120. ..=32768.).contains(&v))
    }
    fn center(self) -> [f32; 2] {
        [
            self.origin[0] + self.size[0] * 0.5,
            self.origin[1] + self.size[1] * 0.5,
        ]
    }
    fn distance_squared(self, point: [f32; 2]) -> f64 {
        (0..2)
            .map(|axis| {
                let distance = f64::from(
                    point[axis].clamp(self.origin[axis], self.origin[axis] + self.size[axis])
                        - point[axis],
                );
                distance * distance
            })
            .sum()
    }
}

pub fn minimum_window_size(panel: PanelId) -> [f32; 2] {
    match panel {
        PanelId::Tools => [80., 300.],
        PanelId::Timeline => [320., 180.],
        _ => [320., 220.],
    }
}
pub fn default_window_size(panel: PanelId) -> [f32; 2] {
    match panel {
        PanelId::Tools => [96., 600.],
        PanelId::Timeline => [900., 280.],
        _ => [680., 440.],
    }
}

/// Recover only a detached panel, preserving partially offscreen windows whose
/// titlebar still has an 80x22-point draggable area on ANY connected monitor.
/// Pure bounded calculation; the caller applies the result once on creation or
/// topology changes, not every paint. Retain live user movement after that.
pub fn recover_geometry(
    panel: PanelId,
    saved: Option<WindowGeometry>,
    monitors: &[ScreenRect],
    fallback: ScreenRect,
) -> WindowGeometry {
    const CHROME: [f32; 2] = [16., 40.];
    let fallback = if fallback.is_valid() {
        fallback
    } else {
        ScreenRect {
            origin: [40., 40.],
            size: [1280., 800.],
        }
    };
    let valid: Vec<_> = monitors
        .iter()
        .copied()
        .take(64)
        .filter(|screen| screen.is_valid())
        .collect();
    let screens = if valid.is_empty() {
        std::slice::from_ref(&fallback)
    } else {
        valid.as_slice()
    };
    let saved = saved.and_then(|geometry| geometry.sanitized(panel));
    let mut geometry = saved.unwrap_or(WindowGeometry {
        outer_position: [fallback.origin[0] + 40., fallback.origin[1] + 40.],
        inner_size: default_window_size(panel),
    });
    let reachable = screens.iter().copied().find(|screen| {
        let overlap = |axis: usize, length: f32| {
            (geometry.outer_position[axis] + length).min(screen.origin[axis] + screen.size[axis])
                - geometry.outer_position[axis].max(screen.origin[axis])
        };
        overlap(0, geometry.inner_size[0] + CHROME[0]) >= 80. && overlap(1, 28.) >= 22.
    });
    let reference = saved.map_or(fallback.center(), |g| g.outer_position);
    let screen = reachable.unwrap_or_else(|| {
        *screens
            .iter()
            .min_by(|a, b| {
                a.distance_squared(reference)
                    .total_cmp(&b.distance_squared(reference))
            })
            .expect("fallback screen")
    });
    let old_size = geometry.inner_size;
    for (axis, chrome) in CHROME.into_iter().enumerate() {
        geometry.inner_size[axis] = geometry.inner_size[axis].min(screen.size[axis] - chrome);
    }
    if reachable.is_none() || saved.is_none() || geometry.inner_size != old_size {
        for (axis, chrome) in CHROME.into_iter().enumerate() {
            geometry.outer_position[axis] = geometry.outer_position[axis].clamp(
                screen.origin[axis],
                screen.origin[axis]
                    + (screen.size[axis] - geometry.inner_size[axis] - chrome).max(0.),
            );
        }
    }
    geometry
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePrefs {
    pub version: u32,
    pub theme: ThemeMode,
    pub panels: PanelVisibility,
    pub active_bottom: BottomTab,
    pub sizes: PanelSizes,
    pub detached: DetachedPanels,
}
impl Default for WorkspacePrefs {
    fn default() -> Self {
        Self {
            version: VERSION,
            theme: ThemeMode::Light,
            panels: PanelVisibility::default(),
            active_bottom: BottomTab::Properties,
            sizes: PanelSizes::default(),
            detached: DetachedPanels::default(),
        }
    }
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
        if prefs.version != VERSION {
            return Self::default();
        }
        prefs.sanitize();
        prefs
    }
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        storage
            .and_then(|s| s.get_string(PREFS_KEY))
            .map_or_else(Self::default, |json| Self::from_json(&json))
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
        self.version = VERSION;
        self.sizes.sanitize();
        self.detached.sanitize();
        self.normalize_active_bottom();
    }
    pub fn is_panel_visible(&self, panel: PanelId) -> bool {
        self.panels.is_visible(panel)
    }
    pub fn bottom_visible(&self) -> bool {
        PanelId::BOTTOM
            .into_iter()
            .any(|panel| self.is_panel_visible(panel) && self.is_panel_docked(panel))
    }
    /// Revealing a bottom panel also selects it. F9 and error-reveal commands
    /// must use this even when a dock/tab was previously hidden.
    pub fn show_panel(&mut self, panel: PanelId) -> bool {
        let mut changed = !*self.panels.slot(panel);
        *self.panels.slot(panel) = true;
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
        let changed = *self.panels.slot(panel);
        *self.panels.slot(panel) = false;
        let before = self.active_bottom;
        self.normalize_active_bottom();
        changed || before != self.active_bottom
    }
    /// Reset layout only. Theme is a separate user preference. The caller may
    /// remove known egui PanelState IDs, but must preserve authoring/input state.
    pub fn restore_layout(&mut self) {
        *self = Self {
            theme: self.theme,
            ..Self::default()
        };
    }
    pub fn placement(&self, panel: PanelId) -> PanelPlacement {
        self.detached
            .get(panel)
            .map_or(PanelPlacement::Docked, |p| p.placement)
    }
    pub fn is_panel_docked(&self, panel: PanelId) -> bool {
        self.placement(panel) == PanelPlacement::Docked
    }
    /// A placement command reveals the panel. Native close should only hide it,
    /// retaining placement and geometry so Window/F9 can reopen the same panel.
    pub fn set_placement(&mut self, panel: PanelId, placement: PanelPlacement) -> bool {
        let Some(window) = self.detached.get_mut(panel) else {
            return false;
        };
        let changed = window.placement != placement;
        window.placement = placement;
        let shown = self.show_panel(panel);
        self.normalize_active_bottom();
        changed || shown
    }
    pub fn detached_geometry(&self, panel: PanelId) -> Option<WindowGeometry> {
        self.detached.get(panel).and_then(|p| p.geometry)
    }
    /// Minimized/missing/nonfinite native snapshots must not erase good geometry.
    pub fn remember_geometry(&mut self, panel: PanelId, geometry: WindowGeometry) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Storage {
        values: BTreeMap<String, String>,
        flushes: usize,
    }
    impl eframe::Storage for Storage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.values.insert(key.into(), value);
        }
        fn flush(&mut self) {
            self.flushes += 1;
        }
    }
    #[test]
    fn workspace_roundtrip_writes_only_its_own_bounded_key() {
        let mut prefs = WorkspacePrefs {
            theme: ThemeMode::Dark,
            ..Default::default()
        };
        prefs.set_panel_visible(PanelId::Timeline, false);
        prefs.set_panel_visible(PanelId::Properties, false);
        prefs.show_panel(PanelId::Accessibility);
        prefs.sizes.library_width = 310.;
        let mut storage = Storage::default();
        storage.values.insert("unrelated".into(), "keep me".into());
        prefs.save(&mut storage);
        assert_eq!(WorkspacePrefs::load(Some(&storage)), prefs);
        assert_eq!(storage.values.len(), 2);
        assert_eq!(storage.values["unrelated"], "keep me");
        assert_eq!(storage.flushes, 0);
        assert!(storage.values[PREFS_KEY].len() < MAX_PREFS_BYTES);
        assert!(!storage.values[PREFS_KEY].contains("project"));
    }
    #[test]
    fn malformed_large_unknown_version_and_unknown_theme_preferences_fail_soft() {
        let default = WorkspacePrefs::default();
        assert_eq!(WorkspacePrefs::load(None), default);
        for input in [
            "",
            "{",
            "null",
            r#"{"version":2,"theme":"dark"}"#,
            r#"{"theme":"system"}"#,
            r#"{"sizes":{"timeline_height":null}}"#,
        ] {
            assert_eq!(WorkspacePrefs::from_json(input), default, "{input}");
        }
        assert_eq!(
            WorkspacePrefs::from_json(&" ".repeat(MAX_PREFS_BYTES + 1)),
            default
        );
        let partial = WorkspacePrefs::from_json(
            r#"{"theme":"dark","panels":{"library":false},"unknown":"ignored"}"#,
        );
        assert_eq!(partial.theme, ThemeMode::Dark);
        assert!(!partial.panels.library);
        assert!(partial.panels.tools);
        assert_eq!(partial.sizes, PanelSizes::default());
    }
    #[test]
    fn hidden_tabs_reopen_and_active_tab_falls_back_without_losing_dock_state() {
        let mut prefs = WorkspacePrefs::default();
        prefs.show_panel(PanelId::Actions);
        assert_eq!(prefs.active_bottom, BottomTab::Actions);
        prefs.set_panel_visible(PanelId::Actions, false);
        assert_eq!(prefs.active_bottom, BottomTab::Properties);
        for panel in PanelId::ALL {
            prefs.set_panel_visible(panel, false);
        }
        assert!(!prefs.bottom_visible());
        assert!(PanelId::ALL.into_iter().all(|p| !prefs.is_panel_visible(p)));
        assert!(prefs.show_panel(PanelId::Actions));
        assert!(prefs.bottom_visible());
        assert_eq!(prefs.active_bottom, BottomTab::Actions);
        assert!(!prefs.panels.properties);
        assert!(!prefs.show_panel(PanelId::Actions));
        for panel in PanelId::ALL {
            prefs.show_panel(panel);
            assert!(prefs.is_panel_visible(panel));
        }
    }
    #[test]
    fn restore_layout_preserves_theme_and_resets_visibility_sizes_and_active_tab() {
        let mut prefs = WorkspacePrefs {
            theme: ThemeMode::Dark,
            ..Default::default()
        };
        for panel in PanelId::ALL {
            prefs.set_panel_visible(panel, false);
        }
        prefs.sizes = PanelSizes {
            timeline_height: 380.,
            bottom_height: 420.,
            library_width: 480.,
        };
        prefs.restore_layout();
        assert_eq!(
            prefs,
            WorkspacePrefs {
                theme: ThemeMode::Dark,
                ..Default::default()
            }
        );
    }
    #[test]
    fn dimensions_are_normalized_before_persistence_without_mutating_live_preferences() {
        let prefs = WorkspacePrefs {
            sizes: PanelSizes {
                timeline_height: f32::NAN,
                bottom_height: f32::INFINITY,
                library_width: -100.,
            },
            ..Default::default()
        };
        let mut storage = Storage::default();
        prefs.save(&mut storage);
        let loaded = WorkspacePrefs::load(Some(&storage));
        assert_eq!(
            loaded.sizes,
            PanelSizes {
                timeline_height: 207.,
                bottom_height: 190.,
                library_width: 190.
            }
        );
        assert!(prefs.sizes.timeline_height.is_nan());
        let large = WorkspacePrefs::from_json(
            r#"{"sizes":{"timeline_height":999999,"bottom_height":-1,"library_width":999999}}"#,
        );
        assert_eq!(
            large.sizes,
            PanelSizes {
                timeline_height: 380.,
                bottom_height: 145.,
                library_width: 480.
            }
        );
    }
    #[test]
    fn restoring_preferences_cannot_select_a_hidden_tab_unless_all_tabs_are_hidden() {
        let prefs = WorkspacePrefs::from_json(
            r#"{"active_bottom":"Output","panels":{"output":false,"properties":false}}"#,
        );
        assert_eq!(prefs.active_bottom, BottomTab::Actions);
        let prefs = WorkspacePrefs::from_json(
            r#"{"active_bottom":"Output","panels":{"properties":false,"actions":false,"accessibility":false,"output":false}}"#,
        );
        assert_eq!(prefs.active_bottom, BottomTab::Output);
        assert!(!prefs.bottom_visible());
    }
    fn screen(origin: [f32; 2], size: [f32; 2]) -> ScreenRect {
        ScreenRect { origin, size }
    }
    fn window(position: [f32; 2], size: [f32; 2]) -> WindowGeometry {
        WindowGeometry {
            outer_position: position,
            inner_size: size,
        }
    }
    #[test]
    fn detached_actions_is_one_panel_and_reopens_in_the_same_placement() {
        let mut prefs = WorkspacePrefs::default();
        assert!(prefs.set_placement(PanelId::Actions, PanelPlacement::Detached));
        assert!(prefs.panels.actions);
        assert_eq!(prefs.active_bottom, BottomTab::Properties);
        assert!(!prefs.is_panel_docked(PanelId::Actions));
        assert!(!prefs.set_placement(PanelId::Library, PanelPlacement::Detached));
        assert!(prefs.is_panel_docked(PanelId::Library));
        for panel in [PanelId::Properties, PanelId::Accessibility, PanelId::Output] {
            prefs.set_panel_visible(panel, false);
        }
        assert!(!prefs.bottom_visible());
        let geometry = window([-600., 70.], [640., 430.]);
        assert!(prefs.remember_geometry(PanelId::Actions, geometry));
        prefs.set_panel_visible(PanelId::Actions, false);
        assert!(prefs.show_panel(PanelId::Actions));
        assert_eq!(prefs.placement(PanelId::Actions), PanelPlacement::Detached);
        assert_eq!(prefs.detached_geometry(PanelId::Actions), Some(geometry));
        assert!(!prefs.bottom_visible());
        prefs.set_placement(PanelId::Actions, PanelPlacement::Docked);
        assert_eq!(prefs.active_bottom, BottomTab::Actions);
        assert!(prefs.bottom_visible());
    }
    #[test]
    fn detached_preferences_roundtrip_migrate_and_restore_without_changing_theme() {
        let old = WorkspacePrefs::from_json(r#"{"version":1,"theme":"dark"}"#);
        assert_eq!(old.detached, DetachedPanels::default());
        let mut prefs = old;
        for panel in PanelId::DETACHABLE {
            prefs.set_placement(panel, PanelPlacement::Detached);
            prefs.remember_geometry(panel, window([1800., -200.], default_window_size(panel)));
        }
        let mut storage = Storage::default();
        prefs.save(&mut storage);
        assert_eq!(WorkspacePrefs::load(Some(&storage)), prefs);
        assert!(storage.values[PREFS_KEY].len() < MAX_PREFS_BYTES);
        assert!(
            PanelId::DETACHABLE
                .into_iter()
                .all(|p| p.viewport_key().is_some())
        );
        assert_ne!(
            PanelId::Tools.viewport_key(),
            PanelId::Timeline.viewport_key()
        );
        prefs.restore_layout();
        assert_eq!(prefs.theme, ThemeMode::Dark);
        assert_eq!(prefs.detached, DetachedPanels::default());
    }
    #[test]
    fn invalid_geometry_cannot_overwrite_last_good_snapshot() {
        let mut prefs = WorkspacePrefs::default();
        let good = window([70., 80.], [680., 440.]);
        prefs.remember_geometry(PanelId::Actions, good);
        for bad in [
            window([f32::NAN, 0.], [680., 440.]),
            window([0., 0.], [0., 440.]),
            window([0., 0.], [f32::INFINITY, 440.]),
        ] {
            assert!(!prefs.remember_geometry(PanelId::Actions, bad));
            assert_eq!(prefs.detached_geometry(PanelId::Actions), Some(good));
        }
        assert!(!prefs.remember_geometry(PanelId::Library, good));
        let parsed = WorkspacePrefs::from_json(
            r#"{"theme":"dark","detached":{"actions":{"placement":"detached","geometry":{"outer_position":[1e30,0],"inner_size":[600,400]}}}}"#,
        );
        assert_eq!(parsed.theme, ThemeMode::Dark);
        assert_eq!(parsed.placement(PanelId::Actions), PanelPlacement::Detached);
        assert_eq!(parsed.detached_geometry(PanelId::Actions), None);
    }
    #[test]
    fn recovery_preserves_connected_secondary_and_partial_windows() {
        let main = screen([0., 24.], [1440., 826.]);
        let left = screen([-1920., -180.], [1920., 1080.]);
        for geometry in [
            window([-1800., -100.], [680., 440.]),
            window([1400., 60.], [680., 440.]),
            window([-500., 60.], [680., 440.]),
        ] {
            let recovered = recover_geometry(
                PanelId::Actions,
                Some(geometry),
                &[main, left, screen([1440., 0.], [1920., 1080.])],
                main,
            );
            assert_eq!(recovered, geometry);
        }
    }
    #[test]
    fn recovery_requires_a_reachable_titlebar_and_fits_removed_monitor_geometry() {
        let main = screen([0., 24.], [1440., 826.]);
        for geometry in [
            window([-1800., -100.], [680., 440.]),
            window([100., -300.], [680., 440.]),
            window([1430., 50.], [680., 440.]),
        ] {
            let recovered = recover_geometry(PanelId::Actions, Some(geometry), &[main], main);
            assert!(recovered.outer_position[0] >= 0.);
            assert!(recovered.outer_position[1] >= 24.);
            assert!(recovered.outer_position[0] + recovered.inner_size[0] + 16. <= 1440.);
            assert!(recovered.outer_position[1] + recovered.inner_size[1] + 40. <= 850.);
        }
        // A sliver of content below the menu bar cannot make its titlebar usable.
        let titlebar_hidden = window([100., 0.], [680., 440.]);
        assert_eq!(
            recover_geometry(PanelId::Actions, Some(titlebar_hidden), &[main], main).outer_position
                [1],
            24.
        );
    }
    #[test]
    fn recovery_bounds_huge_sizes_and_invalid_screen_input() {
        let small = screen([20., 30.], [500., 400.]);
        let recovered = recover_geometry(
            PanelId::Actions,
            Some(window([50., 50.], [1e20, 1e20])),
            &[small],
            small,
        );
        assert_eq!(recovered, window([20., 30.], [484., 360.]));
        let invalid = screen([f32::NAN, 0.], [0., f32::INFINITY]);
        let recovered = recover_geometry(PanelId::Tools, None, &[invalid], invalid);
        assert!(recovered.outer_position.into_iter().all(f32::is_finite));
        assert!(
            recovered
                .inner_size
                .into_iter()
                .all(|v| v.is_finite() && v > 0.)
        );
        let tiny = screen([999_999., 0.], [120.1, 120.1]);
        let recovered = recover_geometry(
            PanelId::Actions,
            Some(window([999_999., 0.], [680., 440.])),
            &[tiny],
            tiny,
        );
        assert!(recovered.inner_size[0] < 120.1 && recovered.inner_size[1] < 120.1);
    }
}
