//! Workspace colors only. Authored fill/stroke/background colors stay in Project.
use eframe::egui::{self, Color32, Stroke};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

pub(crate) const PANEL_FONT: f32 = 12.;
pub(crate) const CONTROL_HEIGHT: f32 = 24.;
pub(crate) const LABEL_WIDTH: f32 = 76.;
pub(crate) const SECONDARY_FONT: f32 = 11.;
pub(crate) const ROW_HEIGHT: f32 = 24.;
pub(crate) const PROPERTY_ROW_HEIGHT: f32 = 28.;
pub(crate) const HEADER_HEIGHT: f32 = 28.;
pub(crate) const FOOTER_HEIGHT: f32 = 28.;
pub(crate) const PANEL_PADDING: f32 = 8.;
pub(crate) const ICON_SIZE: f32 = 16.;
pub(crate) const ICON_STROKE: f32 = 1.5;
pub(crate) const TOOLS_WIDTH: f32 = 64.;
pub(crate) const TIMELINE_HEIGHT: f32 = 200.;
pub(crate) const TIMELINE_MIN_HEIGHT: f32 = 160.;
pub(crate) const TRANSFORM_HANDLE: f32 = 6.;

/// Give small timeline controls clear rails, handles and check marks without
/// changing the rest of the panel or adding any layout padding.
pub(crate) fn timeline_controls<R>(
    ui: &mut egui::Ui,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.scope(|ui| {
        let p = UiPalette::current(ui.ctx());
        let rail = p.control_border;
        ui.spacing_mut().slider_rail_height = 6.;
        let visuals = ui.visuals_mut();
        visuals.slider_trailing_fill = true;
        visuals.selection.bg_fill = p.accent;
        visuals.widgets.inactive.bg_fill = rail;
        visuals.widgets.inactive.bg_stroke = Stroke::new(ICON_STROKE, p.text);
        // Slider handles use fg_stroke for their outline; checkboxes use it
        // for the check. Keep both readable even before hover or focus.
        visuals.widgets.inactive.fg_stroke = Stroke::new(ICON_STROKE, p.text);
        for widget in [
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.bg_fill = p.input_fill;
            widget.bg_stroke = Stroke::new(1.5_f32, p.accent);
            widget.fg_stroke = Stroke::new(2.0_f32, p.text);
        }
        content(ui)
    })
}

/// Install the editor's initial layout once. Later theme changes only update
/// colors and preserve any customized typography and widget state.
pub(crate) fn initialize_editor_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8., 4.);
        style.spacing.button_padding = egui::vec2(8., 4.);
        style.spacing.interact_size = egui::vec2(CONTROL_HEIGHT, CONTROL_HEIGHT);
        style.spacing.combo_width = 120.;
        style.spacing.icon_width = ICON_SIZE;
        style.spacing.icon_spacing = 8.;
        style.spacing.menu_margin = egui::Margin::same(8);
        style.spacing.window_margin = egui::Margin::same(8);
        style.spacing.scroll.bar_width = 8.;
        style.spacing.scroll.bar_inner_margin = 2.;
        style.spacing.scroll.bar_outer_margin = 2.;
        style.spacing.scroll.handle_min_length = 24.;
        style.spacing.scroll.foreground_color = false;
        style.spacing.scroll.floating = false;
        style.visuals.resize_corner_size = 6.;
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::monospace(PANEL_FONT),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(PANEL_FONT),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::proportional(PANEL_FONT),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::proportional(SECONDARY_FONT),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::proportional(PANEL_FONT),
        );
    });
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

impl ThemeMode {
    pub fn palette(self) -> &'static UiPalette {
        match self {
            Self::Light => &LIGHT,
            Self::Dark => &DARK,
        }
    }

    /// Change visuals immediately without replacing fonts, input memory, widget
    /// state or authoring data. Explicit preference does not follow OS changes.
    pub fn apply(self, ctx: &egui::Context) {
        for mode in [Self::Light, Self::Dark] {
            let theme = match mode {
                Self::Light => egui::Theme::Light,
                Self::Dark => egui::Theme::Dark,
            };
            ctx.style_mut_of(theme, |style| {
                style.visuals = mode.visuals();
            });
        }
        ctx.set_theme(match self {
            Self::Light => egui::Theme::Light,
            Self::Dark => egui::Theme::Dark,
        });
        ctx.request_repaint();
    }

    fn visuals(self) -> egui::Visuals {
        let p = self.palette();
        let mut v = match self {
            Self::Light => egui::Visuals::light(),
            Self::Dark => egui::Visuals::dark(),
        };
        v.override_text_color = None;
        v.weak_text_color = Some(p.muted_text);
        v.panel_fill = p.panel_fill;
        v.window_fill = p.window_fill;
        v.window_stroke = Stroke::new(1.0_f32, p.divider);
        v.window_corner_radius = 3.into();
        v.menu_corner_radius = 3.into();
        v.extreme_bg_color = p.input_fill;
        v.text_edit_bg_color = Some(p.input_fill);
        v.code_bg_color = p.input_fill;
        v.faint_bg_color = p.surface;
        v.selection.bg_fill = p.selection_fill;
        v.selection.stroke = Stroke::new(2.0_f32, p.selection_stroke);
        v.hyperlink_color = p.accent;
        v.warn_fg_color = p.warning_text;
        v.error_fg_color = p.error_text;
        v.text_cursor.stroke = Stroke::new(2.0_f32, p.focus);
        v.disabled_alpha = 1.; // Disabled controls use the semantic role, not composited text opacity.
        for widget in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            widget.fg_stroke = Stroke::new(1.0_f32, p.text);
            widget.bg_stroke = Stroke::new(1.0_f32, p.control_border);
            widget.corner_radius = 3.into();
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.divider);
        v.widgets.noninteractive.bg_fill = p.input_fill;
        v.widgets.noninteractive.weak_bg_fill = p.panel_fill;
        v.widgets.inactive.bg_fill = p.surface;
        v.widgets.inactive.weak_bg_fill = p.surface;
        for widget in [&mut v.widgets.hovered, &mut v.widgets.open] {
            widget.bg_fill = p.hover;
            widget.weak_bg_fill = p.hover;
            widget.bg_stroke = Stroke::new(2.0_f32, p.focus);
            widget.fg_stroke = Stroke::new(1.0_f32, p.text);
        }
        // egui also uses active.fg_stroke for strong labels and headings on
        // ordinary panels, so this pair must work on both backgrounds.
        v.widgets.active.bg_fill = p.pressed;
        v.widgets.active.weak_bg_fill = p.pressed;
        v.widgets.active.bg_stroke = Stroke::new(2.0_f32, p.focus);
        v.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.text);
        v
    }
}

/// Semantic tokens shared by ordinary widgets and manual painters. Stage
/// overlays need both overlay and overlay_halo over arbitrary authored colors.
#[derive(Clone, Copy, Debug)]
pub struct UiPalette {
    pub surface: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub divider: Color32,
    pub control_border: Color32,
    pub focus: Color32,
    pub disabled_text: Color32,
    pub scrollbar_track: Color32,
    pub scrollbar_thumb: Color32,
    pub scrollbar_hover: Color32,
    pub scrollbar_drag: Color32,
    pub scrollbar_focus: Color32,
    pub text: Color32,
    pub muted_text: Color32,
    pub error_text: Color32,
    pub warning_text: Color32,
    pub panel_fill: Color32,
    pub window_fill: Color32,
    pub input_fill: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub selection_fill: Color32,
    pub selection_stroke: Color32,
    pub timeline_fill: Color32,
    pub timeline_cell: Color32,
    pub timeline_major: Color32,
    pub timeline_grid: Color32,
    pub timeline_row_separator: Color32,
    pub timeline_active_frame: Color32,
    pub timeline_active_layer: Color32,
    pub timeline_tween: Color32,
    pub timeline_tween_stroke: Color32,
    pub timeline_key: Color32,
    pub timeline_blank_key: Color32,
    pub timeline_label: Color32,
    pub timeline_script: Color32,
    pub playhead: Color32,
    pub onion_range: Color32,
    pub pasteboard: Color32,
    pub stage_title: Color32,
    pub stage_border: Color32,
    pub stage_shadow: Color32,
    pub origin_cross: Color32,
    pub onion_before: Color32,
    pub onion_after: Color32,
    pub overlay: Color32,
    pub overlay_halo: Color32,
    pub overlay_fill: Color32,
    pub handle_fill: Color32,
}
impl UiPalette {
    pub fn current(ctx: &egui::Context) -> &'static Self {
        match ctx.theme() {
            egui::Theme::Light => ThemeMode::Light.palette(),
            egui::Theme::Dark => ThemeMode::Dark.palette(),
        }
    }
}

// Embed the reviewed token file so native chrome and the reference share one palette.
static TOKENS: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../docs/design-system/tokens.json"))
        .expect("valid bundled design tokens")
});
fn token_color(theme: &str, key: &str) -> Color32 {
    let hex = TOKENS["themes"][theme][key]
        .as_str()
        .expect("semantic color token");
    let byte = |i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex color token");
    Color32::from_rgba_unmultiplied(
        byte(1),
        byte(3),
        byte(5),
        if hex.len() == 9 { byte(7) } else { 255 },
    )
}
fn palette(theme: &str) -> UiPalette {
    let c = |key| token_color(theme, key);
    let translucent = |key, alpha| {
        let color = c(key);
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
    };
    UiPalette {
        surface: c("surface"),
        hover: c("hover"),
        pressed: c("pressed"),
        divider: c("divider"),
        control_border: c("controlBorder"),
        focus: c("focus"),
        disabled_text: c("disabled"),
        scrollbar_track: c("scrollbarTrack"),
        scrollbar_thumb: c("scrollbarThumb"),
        scrollbar_hover: c("scrollbarThumbHover"),
        scrollbar_drag: c("scrollbarThumbDrag"),
        scrollbar_focus: c("scrollbarFocus"),
        text: c("text"),
        muted_text: c("muted"),
        error_text: c("error"),
        warning_text: c("warning"),
        panel_fill: c("panel"),
        window_fill: c("raised"),
        input_fill: c("inset"),
        border: c("controlBorder"),
        accent: c("accent"),
        selection_fill: c("selection"),
        selection_stroke: c("selectionText"),
        timeline_fill: c("timeline"),
        timeline_cell: c("timeline"),
        timeline_major: c("timelineMajor"),
        timeline_grid: c("grid"),
        timeline_row_separator: c("timelineDivider"),
        timeline_active_frame: c("selection"),
        timeline_active_layer: translucent("accent", 24),
        timeline_tween: c("tween"),
        timeline_tween_stroke: c("tweenLine"),
        timeline_key: c("key"),
        timeline_blank_key: c("blank"),
        timeline_label: c("text"),
        timeline_script: c("accent"),
        playhead: c("playhead"),
        onion_range: c("accent"),
        pasteboard: c("pasteboard"),
        stage_title: c("text"),
        stage_border: c("controlBorder"),
        stage_shadow: c("shadow"),
        origin_cross: c("muted"),
        onion_before: c("onionBefore"),
        onion_after: c("onionAfter"),
        overlay: c("overlay"),
        overlay_halo: c("overlayHalo"),
        overlay_fill: translucent("overlay", 24),
        handle_fill: c("handle"),
    }
}
static LIGHT: LazyLock<UiPalette> = LazyLock::new(|| palette("light"));
static DARK: LazyLock<UiPalette> = LazyLock::new(|| palette("dark"));

/// Shared disabled presentation keeps concrete colors opaque and leaves layout
/// and widget IDs intact. Native widget semantics still reject interaction.
pub(crate) trait DesignUi {
    fn disable_semantic(&mut self);
    fn add_enabled_semantic(&mut self, enabled: bool, widget: impl egui::Widget) -> egui::Response;
    fn add_enabled_ui_semantic<R>(
        &mut self,
        enabled: bool,
        content: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R>;
}
impl DesignUi for egui::Ui {
    fn disable_semantic(&mut self) {
        let p = UiPalette::current(self.ctx());
        let v = self.visuals_mut();
        v.disabled_alpha = 1.;
        v.override_text_color = Some(p.disabled_text);
        v.weak_text_color = Some(p.disabled_text);
        v.hyperlink_color = p.disabled_text;
        v.selection.stroke.color = p.disabled_text;
        for widget in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            widget.fg_stroke.color = p.disabled_text;
            widget.bg_stroke.color = p.divider;
        }
        self.disable();
    }
    fn add_enabled_semantic(&mut self, enabled: bool, widget: impl egui::Widget) -> egui::Response {
        self.add_enabled_ui_semantic(enabled, |ui| ui.add(widget))
            .inner
    }
    fn add_enabled_ui_semantic<R>(
        &mut self,
        enabled: bool,
        content: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        self.scope(|ui| {
            if !enabled {
                ui.disable_semantic();
            }
            content(ui)
        })
    }
}

/// egui scrollbars read the containing Ui's widget palette. Apply scroll roles
/// only while the ScrollArea paints its chrome; the content retains its own
/// style, stable IDs, keyboard behavior and authored colors.
pub(crate) trait ScrollTheme {
    fn themed(self) -> ThemedScrollArea;
}
impl ScrollTheme for egui::ScrollArea {
    fn themed(self) -> ThemedScrollArea {
        ThemedScrollArea(self)
    }
}
pub(crate) struct ThemedScrollArea(egui::ScrollArea);
impl ThemedScrollArea {
    pub fn show<R>(
        self,
        ui: &mut egui::Ui,
        content: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R> {
        let original = ui.style().clone();
        let p = UiPalette::current(ui.ctx());
        let style = ui.style_mut();
        style.spacing.scroll.foreground_color = false;
        style.visuals.extreme_bg_color = p.scrollbar_track;
        style.visuals.widgets.inactive.bg_fill = p.scrollbar_thumb;
        style.visuals.widgets.hovered.bg_fill = p.scrollbar_hover;
        style.visuals.widgets.active.bg_fill = p.scrollbar_drag;
        let mut result = self.0.show(ui, |ui| {
            ui.set_style(original.clone());
            content(ui)
        });
        ui.set_style(original);
        // Reuse egui's actual bar response and state: pointer geometry remains
        // owned by ScrollArea. egui 0.33 does not paint or handle bar focus.
        for axis in 0..2_usize {
            let maximum = (result.content_size[axis] - result.inner_rect.size()[axis]).max(0.);
            let Some(response) = ui.ctx().read_response(result.id.with(axis)) else {
                continue;
            };
            if maximum == 0. || !response.enabled() || !ui.is_rect_visible(response.rect) {
                continue;
            }
            let name = if axis == 0 {
                "Horizontal scroll"
            } else {
                "Vertical scroll"
            };
            response.widget_info(|| {
                egui::WidgetInfo::slider(true, result.state.offset[axis] as f64, name)
            });
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_role(egui::accesskit::Role::ScrollBar);
                node.set_min_numeric_value(0.);
                node.set_max_numeric_value(maximum as f64);
                node.set_orientation(if axis == 0 {
                    egui::accesskit::Orientation::Horizontal
                } else {
                    egui::accesskit::Orientation::Vertical
                });
            });
            if response.has_focus() {
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter {
                            horizontal_arrows: axis == 0,
                            vertical_arrows: axis == 1,
                            ..Default::default()
                        },
                    )
                });
                let before = result.state.offset[axis];
                ui.input_mut(|input| {
                    let (back, forward) = if axis == 0 {
                        (egui::Key::ArrowLeft, egui::Key::ArrowRight)
                    } else {
                        (egui::Key::ArrowUp, egui::Key::ArrowDown)
                    };
                    for (key, delta) in [
                        (back, -ROW_HEIGHT),
                        (forward, ROW_HEIGHT),
                        (egui::Key::PageUp, -result.inner_rect.size()[axis]),
                        (egui::Key::PageDown, result.inner_rect.size()[axis]),
                    ] {
                        if input.consume_key(egui::Modifiers::NONE, key) {
                            result.state.offset[axis] += delta;
                        }
                    }
                    if input.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                        result.state.offset[axis] = 0.;
                    }
                    if input.consume_key(egui::Modifiers::NONE, egui::Key::End) {
                        result.state.offset[axis] = maximum;
                    }
                });
                result.state.offset[axis] = result.state.offset[axis].clamp(0., maximum);
                if result.state.offset[axis] != before {
                    result.state.store(ui.ctx(), result.id);
                    ui.ctx().request_repaint();
                }
                ui.painter().rect_stroke(
                    response.rect.expand(1.),
                    3,
                    Stroke::new(2.0_f32, p.scrollbar_focus),
                    egui::StrokeKind::Inside,
                );
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(color: Color32) -> f64 {
        let channel = |v: u8| {
            let s = f64::from(v) / 255.;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }
    fn contrast(a: Color32, b: Color32) -> f64 {
        let a = luminance(a);
        let b = luminance(b);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    #[test]
    fn essential_text_and_timeline_markers_have_contrast_in_both_themes() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let p = mode.palette();
            for (name, a, b) in [
                ("panel text", p.text, p.panel_fill),
                ("window text", p.text, p.window_fill),
                ("input text", p.text, p.input_fill),
                ("muted text", p.muted_text, p.panel_fill),
                ("error", p.error_text, p.window_fill),
                ("warning", p.warning_text, p.window_fill),
                ("selected text", p.selection_stroke, p.selection_fill),
                ("timeline text", p.text, p.timeline_cell),
                ("key", p.timeline_key, p.timeline_active_frame),
                ("held key and end marker", p.timeline_key, p.timeline_major),
                ("blank key", p.timeline_key, p.timeline_blank_key),
                ("tween", p.timeline_tween_stroke, p.timeline_tween),
                ("label", p.timeline_label, p.timeline_cell),
                ("script", p.timeline_script, p.timeline_cell),
                ("stage title", p.stage_title, p.pasteboard),
            ] {
                assert!(contrast(a, b) >= 4.5, "{mode:?} {name}: {}", contrast(a, b));
            }
            for (name, a, b, minimum) in [
                ("scroll thumb", p.scrollbar_thumb, p.scrollbar_track, 3.),
                ("scroll hover", p.scrollbar_hover, p.scrollbar_track, 3.),
                ("scroll drag", p.scrollbar_drag, p.scrollbar_track, 3.),
                ("scroll focus", p.scrollbar_focus, p.scrollbar_track, 3.),
                ("empty frame grid", p.timeline_grid, p.timeline_cell, 2.5),
                ("held frame grid", p.timeline_grid, p.timeline_major, 2.),
                (
                    "layer separator",
                    p.timeline_row_separator,
                    p.timeline_cell,
                    3.,
                ),
            ] {
                assert!(
                    contrast(a, b) >= minimum,
                    "{mode:?} {name}: {}",
                    contrast(a, b)
                );
            }
            for background in (0..=255).map(Color32::from_gray).chain([p.pasteboard]) {
                let ratio =
                    contrast(p.overlay, background).max(contrast(p.overlay_halo, background));
                assert!(ratio >= 3., "{mode:?} overlay on {background:?}: {ratio}");
            }
        }
    }
    #[test]
    fn strong_labels_contrast_with_panels_and_clicked_widget_backgrounds() {
        let ctx = egui::Context::default();
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            mode.apply(&ctx);
            let style = ctx.style();
            let visuals = &style.visuals;
            let strong = visuals.strong_text_color();
            for (name, background) in [
                ("panel", visuals.panel_fill),
                ("window", visuals.window_fill),
                ("input", visuals.extreme_bg_color),
                ("clicked button", visuals.widgets.active.bg_fill),
                ("clicked weak fill", visuals.widgets.active.weak_bg_fill),
            ] {
                let ratio = contrast(strong, background);
                assert!(ratio >= 4.5, "{mode:?} strong text on {name}: {ratio}");
            }
        }
    }
    #[test]
    fn applying_theme_keeps_custom_typography_and_widget_memory() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("theme-preserve-draft");
        ctx.data_mut(|d| d.insert_temp(id, "unsaved text".to_owned()));
        ctx.all_styles_mut(|style| {
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(19.));
        });
        for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::Dark] {
            mode.apply(&ctx);
            assert_eq!(ctx.style().visuals.dark_mode, mode == ThemeMode::Dark);
            assert_eq!(ctx.style().visuals.panel_fill, mode.palette().panel_fill);
            assert_eq!(UiPalette::current(&ctx).text, mode.palette().text);
            assert_eq!(ctx.style().text_styles[&egui::TextStyle::Body].size, 19.);
            assert_eq!(
                ctx.data(|d| d.get_temp::<String>(id)).as_deref(),
                Some("unsaved text")
            );
            assert_eq!(
                ctx.style().visuals.selection.stroke.color,
                mode.palette().selection_stroke
            );
        }
        // Actual scrollbar focus must survive a theme change and scroll by
        // keyboard; an otherwise-unused focus color is not sufficient.
        initialize_editor_style(&ctx);
        let render = |events| {
            let mut scroll = None;
            let output = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        scroll = Some(
                            egui::ScrollArea::vertical()
                                .id_salt("keyboard-scroll")
                                .max_height(120.)
                                .themed()
                                .show(ui, |ui| {
                                    ui.set_min_size(egui::vec2(100., 800.));
                                }),
                        );
                    });
                },
            );
            (output, scroll.unwrap())
        };
        let (_, initial) = render(vec![]);
        let focus = initial.id.with(1_usize);
        ctx.memory_mut(|memory| memory.request_focus(focus));
        render(vec![]);
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            mode.apply(&ctx);
            let before = egui::scroll_area::State::load(&ctx, initial.id)
                .unwrap()
                .offset
                .y;
            let (out, scroll) = render(vec![egui::Event::Key {
                key: egui::Key::ArrowDown,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            assert_eq!(scroll.state.offset.y, before + ROW_HEIGHT);
            assert!(ctx.memory(|memory| memory.has_focus(focus)));
            assert!(
                out.shapes
                    .iter()
                    .any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect)
                if rect.stroke.width == 2. && rect.stroke.color == mode.palette().scrollbar_focus))
            );
        }
    }
}
