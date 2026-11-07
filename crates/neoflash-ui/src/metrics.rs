//! Fixed geometry from the NF2 reference (`interface/design-system/docs/NeoFlash-Design-System.codex.md`).
//! These numbers are not Figma variables, so they live here instead of `tokens.json`.
//! All values are logical points (egui points), independent of display scale.
//!
//! **Density profiles (T-101 / R-051, owner-approved 2026-11-07).** The `pub const` values are the
//! Comfortable reference sizes from Figma. Layout code calls the lower-case functions ([`control`],
//! [`row`], [`top_bar`] …), which return the size for the current [`Density`]. The editor sets the
//! profile once per frame with [`set_density`]; it is kept per UI thread, so tests stay isolated.

use std::cell::Cell;

/// Editor-wide density profile: identical behaviour and information architecture, different sizes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Density {
    /// The readable reference profile (NF2 v2.2 sizes).
    #[default]
    Comfortable,
    /// Denser chrome for laptops.
    Compact,
    /// Classic Flash: densest chrome, 24 pt minimum targets.
    ClassicFlash,
}

impl Density {
    pub const ALL: [Self; 3] = [Self::Comfortable, Self::Compact, Self::ClassicFlash];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "Comfortable",
            Self::Compact => "Compact",
            Self::ClassicFlash => "Classic Flash",
        }
    }
    /// Comfortable / Compact / Classic Flash value.
    pub const fn pick(self, comfortable: f32, compact: f32, classic: f32) -> f32 {
        match self {
            Self::Comfortable => comfortable,
            Self::Compact => compact,
            Self::ClassicFlash => classic,
        }
    }
}

thread_local! {
    static DENSITY: Cell<Density> = const { Cell::new(Density::Comfortable) };
}

/// Density used by the metric functions on this UI thread.
pub fn density() -> Density {
    DENSITY.with(Cell::get)
}

/// Set the density for this UI thread (the editor does it at the start of every frame).
pub fn set_density(density: Density) {
    DENSITY.with(|cell| cell.set(density));
}

macro_rules! density_metrics {
    ($($(#[$doc:meta])* $name:ident = $comfortable:expr, $compact:expr, $classic:expr;)*) => {
        $($(#[$doc])* pub fn $name() -> f32 {
            density().pick($comfortable, $compact, $classic)
        })*
    };
}

density_metrics! {
    /// [`CONTROL`] for the current density: fields, selects, buttons, icon buttons SM.
    control = CONTROL, 28.0, 24.0;
    /// [`ROW`]: tree rows, menu items, table rows.
    row = ROW, 24.0, 20.0;
    /// [`PANEL_TAB`].
    panel_tab = PANEL_TAB, 30.0, 26.0;
    /// [`PANEL_HEADER`].
    panel_header = PANEL_HEADER, 32.0, 28.0;
    /// [`SECTION_HEADER`].
    section_header = SECTION_HEADER, 28.0, 24.0;
    /// [`DOCK_TAB`].
    dock_tab = DOCK_TAB, 32.0, 28.0;
    /// [`ICON_BUTTON_MD`].
    icon_button_md = ICON_BUTTON_MD, 32.0, 28.0;
    /// [`STATUS_BAR`].
    status_bar = STATUS_BAR, 22.0, 20.0;
    /// [`MENU_SECTION`].
    menu_section = MENU_SECTION, 22.0, 20.0;
    /// [`SCRIPT_TAB`].
    script_tab = SCRIPT_TAB, 26.0, 22.0;
    /// [`DIALOG_HEADER`].
    dialog_header = DIALOG_HEADER, 52.0, 44.0;
    /// [`MIN_HIT`]: never below 24 pt (WCAG 2.2 target size).
    min_hit = MIN_HIT, 28.0, 24.0;
    /// [`ICON_TOOL`].
    icon_tool = ICON_TOOL, 20.0, 16.0;
    /// [`TOP_BAR`].
    top_bar = TOP_BAR, 40.0, 32.0;
    /// [`TOOLS_RAIL`].
    tools_rail = TOOLS_RAIL, 48.0, 40.0;
    /// [`TOOLS_TWO_COLUMNS`].
    tools_two_columns = TOOLS_TWO_COLUMNS, 68.0, 56.0;
    /// [`TOOL_BUTTON`].
    tool_button = TOOL_BUTTON, 28.0, 24.0;
    /// [`INSPECTOR`] default width.
    inspector = INSPECTOR, 288.0, 256.0;
    /// [`TIMELINE_CONTROLS`].
    timeline_controls = TIMELINE_CONTROLS, 40.0, 32.0;
    /// [`BREADCRUMB`].
    breadcrumb = BREADCRUMB, 30.0, 26.0;
    /// [`ZOOM_CONTROL`].
    zoom_control = ZOOM_CONTROL, 36.0, 30.0;
    /// [`STAGE_RULER`].
    stage_ruler = STAGE_RULER, 18.0, 16.0;
    /// [`TIMELINE_RULER`].
    timeline_ruler = TIMELINE_RULER, 26.0, 22.0;
    /// [`LAYER_COLUMN`] default width.
    layer_column = LAYER_COLUMN, 240.0, 200.0;
    /// [`LAYERS_HEADER`].
    layers_header = LAYERS_HEADER, 26.0, 22.0;
}

/// Default layer-row height of the current density (Normal 28 / Short 22 / Compact 16 presets).
pub fn default_layer_row() -> f32 {
    density().pick(LAYER_ROW, LAYER_ROW_SHORT, LAYER_ROW_COMPACT)
}

// §3.2 Control heights.
/// Fields, selects, buttons, icon buttons SM, stepper / combo fields.
pub const CONTROL: f32 = 32.0;
/// Tree rows, menu items, layer rows, breadcrumb items, segments, table rows.
pub const ROW: f32 = 28.0;
/// Panel tabs and the floating panel header.
pub const PANEL_TAB: f32 = 36.0;
/// Docked panel header (`NF2/Panel Header`).
pub const PANEL_HEADER: f32 = 40.0;
/// Section header inside panels (`NF2/Section Header`).
pub const SECTION_HEADER: f32 = 32.0;
/// Dock tabs and the main play button (Icon button MD).
pub const DOCK_TAB: f32 = 40.0;
/// Icon button MD (primary play).
pub const ICON_BUTTON_MD: f32 = 40.0;
/// Status bar.
pub const STATUS_BAR: f32 = 24.0;
/// Menu separator (1 px line with 4 px margins).
pub const MENU_SEPARATOR: f32 = 9.0;
/// Menu section caption.
pub const MENU_SECTION: f32 = 24.0;
/// Checkbox / switch / slider / radio row.
pub const TOGGLE_ROW: f32 = 20.0;
/// Badge height.
pub const BADGE: f32 = 22.0;
/// Kbd chip height.
pub const KBD: f32 = 20.0;
/// Script tab height.
pub const SCRIPT_TAB: f32 = 30.0;
/// Dialog header height.
pub const DIALOG_HEADER: f32 = 60.0;
/// Minimum hit area for any control (accessibility rule).
pub const MIN_HIT: f32 = 32.0;
/// Keyboard focus ring: stroke width and distance outside the control radius.
pub const FOCUS_RING: f32 = 2.0;
pub const FOCUS_OFFSET: f32 = 2.0;

// §3.4 Icons.
pub const ICON_UI: f32 = 16.0;
pub const ICON_TOOL: f32 = 20.0;

// §4 Main window regions (1920 × 1080 reference).
pub const TOP_BAR: f32 = 48.0;
pub const TOOLS_RAIL: f32 = 56.0;
/// Tools panel, 2 Columns layout — the base Tools layout (`NF2/Tools Panel` 80 wide: 2 × 32 tools,
/// 2 gap, 7 padding). The 1 Column rail ([`TOOLS_RAIL`]) is the narrow variant with a "…" overflow.
pub const TOOLS_TWO_COLUMNS: f32 = 80.0;
pub const TOOL_BUTTON: f32 = 32.0;
pub const INSPECTOR: f32 = 320.0;
pub const BOTTOM_DOCK: f32 = 420.0;
pub const TIMELINE_CONTROLS: f32 = 44.0;
/// Floating stage chrome: breadcrumb height, zoom control height.
pub const BREADCRUMB: f32 = 34.0;
pub const ZOOM_CONTROL: f32 = 42.0;
pub const STAGE_RULER: f32 = 20.0;

// §5.1 Timeline geometry.
/// Frame width in the Normal view mode. Frame `n` starts at `(n − 1) × FRAME`.
pub const FRAME: f32 = 12.0;
/// Layer row height presets (owner decision 2026-11-07, R-052): Normal 28 pt with `NF2/Body`, Short 22 pt with
/// `NF2/Small` and Compact 16 pt with `NF2/Caption`; Short and Compact use 12 pt layer icons. Toggle columns keep
/// their 20 pt width in every preset.
pub const LAYER_ROW: f32 = 28.0;
pub const LAYER_ROW_SHORT: f32 = 22.0;
pub const LAYER_ROW_COMPACT: f32 = 16.0;
/// Layer row height in Frame view ▸ Preview / Preview in context (outline `Mode / Preview`: three
/// rows in a 132 px frame area).
pub const LAYER_ROW_PREVIEW: f32 = 44.0;
/// Largest keyframe thumbnail in Preview rows (outline `Mode / Preview` › `Frame` 62 × 38); smaller
/// spans scale it down to fit.
pub const PREVIEW_THUMBNAIL_WIDTH: f32 = 62.0;
pub const PREVIEW_THUMBNAIL_HEIGHT: f32 = 38.0;
pub const TIMELINE_RULER: f32 = 32.0;
pub const LAYER_COLUMN: f32 = 280.0;
pub const LAYERS_HEADER: f32 = 32.0;
/// Width of one layer column toggle (eye / lock / outline).
pub const LAYER_TOGGLE: f32 = 20.0;
/// Width of the layer-row `Mode` cell (blend mode, R-040).
pub const LAYER_MODE: f32 = 64.0;
/// Layer column with the Mode cell and the Solo toggle (R-040, R-045): 280 + 64 + 20 + 8.
pub const LAYER_COLUMN_MODE_SOLO: f32 = 372.0;
/// Below this layer-column width the `Mode` cell collapses to a 20 pt `mixer` glyph that appears only for
/// non-Normal blend modes (Narrow layer column, R-052, owner-approved 2026-11-07).
pub const LAYER_MODE_NARROW_BELOW: f32 = LAYER_COLUMN;
/// Indentation of nested layers (`Indent 1`, `Indent 2`).
pub const LAYER_INDENT: f32 = 16.0;

/// Frame view modes of the timeline (§5.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FrameView {
    Tiny,
    Small,
    #[default]
    Normal,
    Medium,
    Large,
}

impl FrameView {
    pub const ALL: [Self; 5] = [
        Self::Tiny,
        Self::Small,
        Self::Normal,
        Self::Medium,
        Self::Large,
    ];
    /// Pixels per frame: Tiny 6 · Small 9 · Normal 12 · Medium 16 · Large 22.
    pub const fn frame_width(self) -> f32 {
        match self {
            Self::Tiny => 6.0,
            Self::Small => 9.0,
            Self::Normal => FRAME,
            Self::Medium => 16.0,
            Self::Large => 22.0,
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tiny => "Tiny",
            Self::Small => "Small",
            Self::Normal => "Normal",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }
}

// §3.6 Elevation.
/// Floating panels and popovers: y 12, blur 32, black 35 %.
pub const SHADOW_POPOVER: Shadow = Shadow {
    y: 12.0,
    blur: 32.0,
    alpha: 0.35,
};
/// Dialogs: y 16, blur 48, black 45 %.
pub const SHADOW_DIALOG: Shadow = Shadow {
    y: 16.0,
    blur: 48.0,
    alpha: 0.45,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub y: f32,
    pub blur: f32,
    pub alpha: f32,
}

impl Shadow {
    pub fn egui(self) -> eframe::egui::epaint::Shadow {
        eframe::egui::epaint::Shadow {
            offset: [0, self.y as i8],
            blur: self.blur as u8,
            spread: 0,
            color: eframe::egui::Color32::from_black_alpha((self.alpha * 255.0) as u8),
        }
    }
}
