//! Geometry of the NF design system, v3 (`docs/DESIGN-SYSTEM.md` › "Density").
//! These numbers mirror the Figma variable collection `NF3 · Density` (source of the Figma side:
//! `interface/figma-plugin/v3/src/05-density.js`; `tests/density_parity.rs` keeps both in step).
//! All values are logical points (egui points), independent of display scale.
//!
//! **Density is a property of every atom.** Heights, inner paddings, gaps, radii, toggle sizes, label
//! columns and default panel / dialog widths all come from the functions below, which return the value
//! for the current [`Density`]: Normal (`Comfortable`), Compact or Classic Flash. The `pub const` values
//! are the Normal sizes for constant contexts and tests. The editor sets the profile once per frame with
//! [`set_density`]; it is kept per UI thread, so tests stay isolated. Type scale and colours never depend
//! on density.

use std::cell::Cell;

/// Editor-wide density profile: identical behaviour and information architecture, different sizes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Density {
    /// Normal: the reference profile. Serialized as `comfortable` for saved preferences.
    #[default]
    Comfortable,
    /// Denser chrome for laptops.
    Compact,
    /// Classic Flash: densest chrome, 20 pt controls.
    ClassicFlash,
}

impl Density {
    pub const ALL: [Self; 3] = [Self::Comfortable, Self::Compact, Self::ClassicFlash];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "Normal",
            Self::Compact => "Compact",
            Self::ClassicFlash => "Classic Flash",
        }
    }
    /// Normal / Compact / Classic Flash value.
    pub const fn pick(self, normal: f32, compact: f32, classic: f32) -> f32 {
        match self {
            Self::Comfortable => normal,
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
    ($($(#[$doc:meta])* $name:ident = $normal:expr, $compact:expr, $classic:expr;)*) => {
        $($(#[$doc])* pub fn $name() -> f32 {
            density().pick($normal, $compact, $classic)
        })*
        /// Every density metric as `(name, [normal, compact, classic])`, for parity tests and tooling.
        pub const DENSITY_TABLE: &[(&str, [f32; 3])] = &[$((stringify!($name), [$normal, $compact, $classic]),)*];
    };
}

density_metrics! {
    /// Fields, selects, buttons, icon buttons (`size/control`).
    control = CONTROL, 24.0, 22.0;
    /// Tree rows, menu items, table rows (`size/row`).
    row = ROW, 22.0, 20.0;
    /// Panel tabs (`size/tab`).
    panel_tab = PANEL_TAB, 30.0, 26.0;
    /// Docked panel header (`size/tab`).
    panel_header = PANEL_HEADER, 30.0, 26.0;
    /// Section header inside panels (`size/section`).
    section_header = SECTION_HEADER, 24.0, 22.0;
    /// Dock tabs (`size/tab`).
    dock_tab = DOCK_TAB, 30.0, 26.0;
    /// The main play button.
    icon_button_md = ICON_BUTTON_MD, 28.0, 24.0;
    /// Status bar (`size/status`).
    status_bar = STATUS_BAR, 22.0, 20.0;
    /// Menu section caption.
    menu_section = MENU_SECTION, 22.0, 20.0;
    /// Menu separator: a 1 px line with equal margins (`size/menu-separator`).
    menu_separator = MENU_SEPARATOR, 7.0, 7.0;
    /// Script tab height.
    script_tab = SCRIPT_TAB, 24.0, 22.0;
    /// Dialog header (`size/dialog-header`).
    dialog_header = DIALOG_HEADER, 40.0, 34.0;
    /// Minimum hit area of any control.
    min_hit = MIN_HIT, 24.0, 24.0;
    /// Tool glyph: the 20 px master in Normal, the 16 px master in Compact and Classic Flash.
    icon_tool = ICON_TOOL, 20.0, 16.0;
    /// Top bar (`size/top-bar`).
    top_bar = TOP_BAR, 34.0, 30.0;
    /// Tools panel, one column (`width/tools-1`).
    tools_rail = TOOLS_RAIL, 40.0, 34.0;
    /// Tools panel, two columns (`width/tools-2`).
    tools_two_columns = TOOLS_TWO_COLUMNS, 72.0, 61.0;
    /// Tool button side (`size/tool`).
    tool_button = TOOL_BUTTON, 28.0, 24.0;
    /// Inspector: default and minimum width of the right dock (`width/inspector`).
    inspector = INSPECTOR, 264.0, 248.0;
    /// Timeline controls row (`size/timeline-controls`).
    timeline_controls = TIMELINE_CONTROLS, 32.0, 28.0;
    /// Breadcrumb / scene pill on the Stage.
    breadcrumb = BREADCRUMB, 26.0, 24.0;
    /// Floating Stage toolbar (`size/stage-bar`).
    zoom_control = ZOOM_CONTROL, 30.0, 26.0;
    /// Stage ruler thickness (`size/stage-ruler`).
    stage_ruler = STAGE_RULER, 16.0, 16.0;
    /// Timeline frame ruler (`size/timeline-ruler`).
    timeline_ruler = TIMELINE_RULER, 22.0, 20.0;
    /// Layer column without the Mode and Solo cells.
    layer_column = LAYER_COLUMN, 208.0, 192.0;
    /// Layers header (`size/timeline-ruler`).
    layers_header = LAYERS_HEADER, 22.0, 20.0;
    /// Checkbox / radio / switch / slider row (`size/toggle`).
    toggle_row = TOGGLE_ROW, 18.0, 16.0;
    /// Checkbox box and radio ring (`size/check`).
    check_box = CHECK_BOX, 14.0, 14.0;
    /// Switch track width and height (`size/switch-w`, `size/switch-h`).
    switch_width = SWITCH_WIDTH, 26.0, 24.0;
    switch_height = SWITCH_HEIGHT, 14.0, 14.0;
    /// Slider thumb (`size/slider-thumb`).
    slider_thumb = SLIDER_THUMB, 12.0, 10.0;
    /// Badge and Kbd chip height (`size/badge`).
    badge = BADGE, 18.0, 16.0;
    kbd = KBD, 18.0, 16.0;
    /// Colour chip in lists (`size/swatch`).
    swatch = SWATCH, 16.0, 16.0;
    /// Inspector label column (`width/label`).
    label_width = LABEL_WIDTH, 72.0, 64.0;
    /// Dialog label column (`width/dialog-label`).
    dialog_label = DIALOG_LABEL, 108.0, 96.0;
    /// Natural widths of controls in dialogs (`width/num`, `width/field-s|m|l`).
    field_num = FIELD_NUM, 60.0, 56.0;
    field_s = FIELD_S, 92.0, 84.0;
    field_m = FIELD_M, 152.0, 140.0;
    field_l = FIELD_L, 208.0, 192.0;
    /// Category / target sidebars in dialogs (`width/sidebar`, `width/sidebar-wide`).
    sidebar = SIDEBAR, 152.0, 140.0;
    sidebar_wide = SIDEBAR_WIDE, 196.0, 180.0;
    /// Tree column of the Scripting Reference (`width/tree`).
    tree_width = TREE_WIDTH, 224.0, 208.0;
    /// Menu minimum width (`width/menu`).
    menu_width = MENU_WIDTH, 196.0, 180.0;
    /// Dialog widths (`width/dialog-s|m|l|xl`) and the two shared heights (`height/dialog-l|xl`).
    dialog_s = DIALOG_S, 344.0, 320.0;
    dialog_m = DIALOG_M, 456.0, 424.0;
    dialog_l = DIALOG_L, 680.0, 640.0;
    dialog_xl = DIALOG_XL, 840.0, 780.0;
    dialog_l_height = DIALOG_L_HEIGHT, 520.0, 470.0;
    dialog_xl_height = DIALOG_XL_HEIGHT, 610.0, 570.0;
    /// Scripting Reference window width (`width/reference`).
    reference_width = REFERENCE_WIDTH, 920.0, 880.0;
    /// Spacing scale `space::s1()…s6()`: tiny gap · control gap · field padding · panel padding ·
    /// dialog padding · block gap (`gap/*`, `pad/*`).
    space_1 = 4.0, 3.0, 3.0;
    space_2 = 8.0, 6.0, 6.0;
    space_3 = 10.0, 8.0, 8.0;
    space_4 = 14.0, 12.0, 10.0;
    space_5 = 24.0, 18.0, 14.0;
    space_6 = 32.0, 24.0, 18.0;
    /// Vertical gap between form rows (`gap/row`).
    row_gap = 8.0, 6.0, 5.0;
    /// Radii `radius::sm()…xl()`: marks · controls · panels and menus · dialogs (`radius/*`).
    radius_sm = 4.0, 3.0, 2.0;
    radius_md = 6.0, 5.0, 4.0;
    radius_lg = 8.0, 8.0, 6.0;
    radius_xl = 12.0, 10.0, 8.0;
}

/// Default layer-row height of the current density (Normal 22 / Short 18 / Compact 16 presets).
pub fn default_layer_row() -> f32 {
    density().pick(LAYER_ROW, LAYER_ROW_SHORT, LAYER_ROW_COMPACT)
}

/// Layer column with the Mode cell and the Solo toggle for the current density.
pub fn layer_column_mode_solo() -> f32 {
    layer_column() + LAYER_MODE + LAYER_TOGGLE + 8.0
}

// Normal-density reference values.
pub const CONTROL: f32 = 28.0;
pub const ROW: f32 = 26.0;
pub const PANEL_TAB: f32 = 34.0;
pub const PANEL_HEADER: f32 = 34.0;
pub const SECTION_HEADER: f32 = 28.0;
pub const DOCK_TAB: f32 = 34.0;
pub const ICON_BUTTON_MD: f32 = 32.0;
pub const STATUS_BAR: f32 = 24.0;
pub const MENU_SEPARATOR: f32 = 9.0;
pub const MENU_SECTION: f32 = 24.0;
pub const TOGGLE_ROW: f32 = 20.0;
pub const CHECK_BOX: f32 = 16.0;
pub const SWITCH_WIDTH: f32 = 28.0;
pub const SWITCH_HEIGHT: f32 = 16.0;
pub const SLIDER_THUMB: f32 = 12.0;
pub const BADGE: f32 = 20.0;
pub const KBD: f32 = 20.0;
pub const SWATCH: f32 = 18.0;
pub const SCRIPT_TAB: f32 = 28.0;
pub const DIALOG_HEADER: f32 = 48.0;
pub const MIN_HIT: f32 = 28.0;
pub const LABEL_WIDTH: f32 = 80.0;
pub const DIALOG_LABEL: f32 = 120.0;
pub const FIELD_NUM: f32 = 68.0;
pub const FIELD_S: f32 = 104.0;
pub const FIELD_M: f32 = 168.0;
pub const FIELD_L: f32 = 232.0;
pub const SIDEBAR: f32 = 168.0;
pub const SIDEBAR_WIDE: f32 = 216.0;
pub const TREE_WIDTH: f32 = 248.0;
pub const MENU_WIDTH: f32 = 216.0;
pub const DIALOG_S: f32 = 380.0;
pub const DIALOG_M: f32 = 500.0;
pub const DIALOG_L: f32 = 740.0;
pub const DIALOG_XL: f32 = 900.0;
pub const DIALOG_L_HEIGHT: f32 = 600.0;
pub const DIALOG_XL_HEIGHT: f32 = 660.0;
pub const REFERENCE_WIDTH: f32 = 980.0;
/// Keyboard focus ring: stroke width and distance outside the control radius.
pub const FOCUS_RING: f32 = 2.0;
pub const FOCUS_OFFSET: f32 = 2.0;

// Icons.
pub const ICON_UI: f32 = 16.0;
pub const ICON_TOOL: f32 = 20.0;

// Main window regions.
pub const TOP_BAR: f32 = 40.0;
pub const TOOLS_RAIL: f32 = 44.0;
/// Tools panel, two columns: 2 × tool button + gap + padding, with 2 pt of slack.
pub const TOOLS_TWO_COLUMNS: f32 = 80.0;
pub const TOOL_BUTTON: f32 = 32.0;
pub const INSPECTOR: f32 = 288.0;
pub const BOTTOM_DOCK: f32 = 300.0;
pub const TIMELINE_CONTROLS: f32 = 38.0;
/// Floating stage chrome: breadcrumb height, Stage toolbar height.
pub const BREADCRUMB: f32 = 30.0;
pub const ZOOM_CONTROL: f32 = 34.0;
pub const STAGE_RULER: f32 = 18.0;

// Timeline geometry.
/// Frame width in the Normal view mode. Frame `n` starts at `(n − 1) × FRAME`.
pub const FRAME: f32 = 12.0;
/// Layer row height presets: Normal 22 pt with `NF2/Body`, Short 18 pt with `NF2/Small` and Compact 16 pt
/// with `NF2/Caption`; Short and Compact use 12 pt layer icons. Toggle columns keep their 20 pt width
/// in every preset.
pub const LAYER_ROW: f32 = 24.0;
pub const LAYER_ROW_SHORT: f32 = 20.0;
pub const LAYER_ROW_COMPACT: f32 = 18.0;
/// Layer row height in Frame view ▸ Preview / Preview in context (outline `Mode / Preview`: three
/// rows in a 132 px frame area).
pub const LAYER_ROW_PREVIEW: f32 = 44.0;
/// Largest keyframe thumbnail in Preview rows (outline `Mode / Preview` › `Frame` 62 × 38); smaller
/// spans scale it down to fit.
pub const PREVIEW_THUMBNAIL_WIDTH: f32 = 62.0;
pub const PREVIEW_THUMBNAIL_HEIGHT: f32 = 38.0;
pub const TIMELINE_RULER: f32 = 26.0;
pub const LAYER_COLUMN: f32 = 232.0;
pub const LAYERS_HEADER: f32 = 26.0;
/// Width of one layer column toggle (eye / lock / outline).
pub const LAYER_TOGGLE: f32 = 20.0;
/// Width of the layer-row `Mode` cell (blend mode, R-040).
pub const LAYER_MODE: f32 = 64.0;
/// Layer column with the Mode cell and the Solo toggle (R-040, R-045) in Normal: 208 + 64 + 20 + 8.
pub const LAYER_COLUMN_MODE_SOLO: f32 = 324.0;
/// Below this layer-column width the `Mode` cell collapses to a 20 pt `mixer` glyph that appears only for
/// non-Normal blend modes (Narrow layer column, R-052).
pub const LAYER_MODE_NARROW_BELOW: f32 = LAYER_COLUMN;
/// Indentation of nested layers (`Indent 1`, `Indent 2`).
pub const LAYER_INDENT: f32 = 14.0;

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
