//! NF2 components: one Rust type (or function) per Figma component set on `v2 · System & Components`.
//!
//! Naming follows Figma: `NF2/Icon Button` → [`IconButton`], variant `Type=Ghost` → [`IconButtonType::Ghost`].
//! Rules (§13.2 of the reference):
//! * Variant properties become enums with the same option names; booleans become builder flags.
//! * Hover / focus / disabled / pressed are runtime **states**, never props. Disabled comes from
//!   `ui.add_enabled(false, …)` / `ui.disable()`.
//! * `Selected` / `Checked` / `Active` / `On` are real props (they describe data, not pointer state).
//! * Heights are fixed ([`crate::metrics`]); widths hug content unless the caller passes a width.
//! * Colours come only from [`crate::palette`]. Components never branch on theme.
//!
//! | Figma shelf | Module |
//! |---|---|
//! | Button, Icon button, Tool button | [`button`] |
//! | Segmented control | [`segmented`] |
//! | Field · Search · Select, Stepper field (`Field::stepper`), Combo field (`Field::presets`), Colour field, Line style select | [`field`] |
//! | Checkbox · Slider · Switch · Radio | [`toggle`] |
//! | Badge, Kbd, Tooltip, Swatch, Divider, Logo, Spinner, Progress, Toast, Empty state | [`display`] |
//! | Dock tab · Panel tab · Script tab · Section header · Panel header · Floating panel header · Dialog header | [`headers`] |
//! | Tree item, Table row | [`tree`] |
//! | Menu item · Menu section · Menu separator · Menu bar item | [`menu`] |
//! | Breadcrumb item / separator / overflow | [`breadcrumb`] |
//! | Colour well, Fill stroke chip, Swatch cell, Registration picker, Icon group | [`color`] |
//! | Panel / popover / dialog / status bar surfaces, Zoom control, Dock zone, Drop indicator, Splitter | [`surface`] |
//! | Timeline 2.0 — layers and frames | [`timeline`] |
//! | Rulers, guides, grid, canvas editing chrome (selection, transform, anchors, marquee, gradient transform, snap ring) | [`canvas`] |
//! | Inspector numeric field (drag-to-scrub value), Properties type header | [`inspector`] |
//! | Ease preset, Ease graph, ease preview track | [`ease`] |
//! | Motion editor lanes (frame scale, curve lane, playhead) | [`motion`] |
//! | Swatches popover, gradient swatch | [`swatches`] |
//! | View settings (rulers, guides & grid) | [`view_settings`] |
//! | Color picker (saturation/brightness area, hue and alpha strips) | [`color_picker`] |
//! | Gradient stop, Gradient bar | [`gradient`] |
//! | Anchor point, Edit envelope channel lane | [`envelope`] |

pub mod breadcrumb;
pub mod button;
pub mod canvas;
pub mod code;
pub mod color;
pub mod color_picker;
pub mod display;
pub mod docking;
pub mod ease;
pub mod envelope;
pub mod field;
pub mod gradient;
pub mod headers;
pub mod icon_group_items;
pub mod inspector;
pub mod menu;
pub mod motion;
pub mod segmented;
pub mod surface;
pub mod swatches;
pub mod text_area;
pub mod timeline;
pub mod toggle;
pub mod tree;
pub mod unavailable;
pub mod view_settings;

pub use breadcrumb::{BreadcrumbItem, BreadcrumbType, breadcrumb_overflow, breadcrumb_separator};
pub use button::{Button, ButtonType, IconButton, IconButtonSize, IconButtonType, ToolButton};
pub use code::{CompilerMessage, Gutter, LogLine, Severity};
pub use color::{
    ColorWell, ColorWellTarget, FillStrokeChip, IconGroup, RegistrationPicker, RegistrationPoint,
    SwatchCell,
};
pub use color_picker::ColorPicker;
pub use display::{
    Badge, BadgeType, EmptyState, Toast, ToastType, divider, kbd, logo, progress, spinner, swatch,
    tooltip,
};
pub use docking::{dock_target, drag_ghost_at};
pub use ease::{EaseGraph, EasePreset, EasePresetKind, EaseRange, ease_preview_track};
pub use envelope::{AnchorPoint, EnvelopeLane};
pub use field::{ColorField, Field, LineStyle, LineStyleSelect, Search, Select, TextField};
pub use gradient::{GradientBar, GradientStop};
pub use headers::{
    DialogHeader, DockTab, FloatingPanelHeader, PanelHeader, PanelTab, ScriptTab, SectionHeader,
};
pub use inspector::{DragField, TypeHeader};
pub use menu::{
    MenuBarItem, MenuItem, context_menu, menu_bar, menu_section, menu_separator, popover_frame,
    popup_below, submenu,
};
pub use motion::{LaneKey, paint_curve_lane, paint_frame_scale, paint_lane_playhead};
pub use segmented::Segmented;
pub use surface::{Surface, ZoomControl};
pub use swatches::{GradientSwatch, SwatchesPopover, SwatchesResponse, default_palette};
pub use text_area::TextArea;
pub use toggle::{Checkbox, Radio, Slider, Switch};
pub use tree::{TableRow, TableRowKind, TreeItem, TreeItemType};
pub use unavailable::{NOT_AVAILABLE, not_available};
pub use view_settings::{ViewSettings, ViewSettingsResponse, ViewSettingsState};

use crate::theme::{cr, focus_ring, keyboard_focus};
use eframe::egui::{self, Color32, Rect, Response, Stroke, StrokeKind, Ui};

/// Pointer / keyboard state of an interactive component, derived from a `Response`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub enabled: bool,
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
}

impl State {
    pub fn of(ui: &Ui, response: &Response) -> Self {
        let enabled = ui.is_enabled() && response.enabled();
        Self {
            enabled,
            hovered: enabled && response.hovered(),
            pressed: enabled && response.is_pointer_button_down_on(),
            focused: keyboard_focus(ui, response),
        }
    }
}

/// Fill + optional 1 px inside border with NF2 radius.
pub(crate) fn fill(ui: &Ui, rect: Rect, radius: f32, fill: Color32, border: Option<Color32>) {
    let p = ui.painter();
    p.rect_filled(rect, cr(radius), fill);
    if let Some(b) = border {
        p.rect_stroke(
            rect,
            cr(radius),
            Stroke::new(1.0_f32, b),
            StrokeKind::Inside,
        );
    }
}

pub(crate) fn focus(ui: &Ui, state: State, rect: Rect, radius: f32) {
    if state.focused {
        focus_ring(ui, rect, radius);
    }
}

/// Lay out a single-line label in an NF2 text style.
pub(crate) fn galley(
    ui: &Ui,
    text: &str,
    style: crate::TextStyle,
    color: Color32,
) -> std::sync::Arc<egui::Galley> {
    let spec = style.spec();
    let text = if spec.uppercase {
        text.to_uppercase()
    } else {
        text.to_owned()
    };
    let mut job =
        egui::text::LayoutJob::single_section(text, crate::typography::format_line(style, color));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// Paint a galley vertically centred at `x`.
pub(crate) fn text_at(
    ui: &Ui,
    x: f32,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    color: Color32,
) {
    let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
}

/// Accessible name for icon-only and painted widgets.
pub(crate) fn a11y(
    response: &Response,
    kind: egui::WidgetType,
    label: &str,
    selected: Option<bool>,
) {
    let enabled = response.enabled();
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(kind, enabled, label);
        info.selected = selected;
        info
    });
}
