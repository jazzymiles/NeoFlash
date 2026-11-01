//! Fixed geometry from the NF2 reference (`interface/design-system/docs/NeoFlash-Design-System.codex.md`).
//! These numbers are not Figma variables, so they live here instead of `tokens.json`.
//! All values are logical points (egui points), independent of display scale.

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
/// Layer row height (Short mode: [`LAYER_ROW_SHORT`]).
pub const LAYER_ROW: f32 = 28.0;
pub const LAYER_ROW_SHORT: f32 = 18.0;
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
