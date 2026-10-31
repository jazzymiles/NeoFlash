//! Workspace colors only. Authored fill/stroke/background colors stay in Project.
use eframe::egui::{self, Color32, Stroke};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

// Editor layout constants are NF2 metrics (crates/neoflash-ui/src/metrics.rs); keep names stable
// for existing call sites. Never introduce a size here that is not in the design system.
use neoflash_ui::metrics as nf;
/// `NF2/Body` size (13).
pub(crate) const PANEL_FONT: f32 = 13.;
/// Fields, buttons, icon buttons (`NF2/Field`, `NF2/Button`).
pub(crate) const CONTROL_HEIGHT: f32 = nf::CONTROL;
pub(crate) const LABEL_WIDTH: f32 = 76.;
/// `NF2/Small` size (12).
pub(crate) const SECONDARY_FONT: f32 = 12.;
/// Tree rows, menu items, layer rows (`NF2/Tree Item`, `NF2/TL Layer`).
pub(crate) const ROW_HEIGHT: f32 = nf::ROW;
/// One inspector field row.
pub(crate) const PROPERTY_ROW_HEIGHT: f32 = nf::CONTROL;
/// Panel tabs / floating panel header (`NF2/Panel Tab`).
pub(crate) const HEADER_HEIGHT: f32 = nf::PANEL_TAB;
pub(crate) const FOOTER_HEIGHT: f32 = nf::CONTROL;
/// Docked tools rail (`NF2/Tools Panel` Layout = 1 Column).
pub(crate) const TOOLS_WIDTH: f32 = nf::TOOLS_RAIL;
/// Bottom dock: controls 44 + timeline body 336 (dock tabs 40 come from the group header).
pub(crate) const TIMELINE_HEIGHT: f32 = nf::BOTTOM_DOCK - nf::DOCK_TAB;
pub(crate) const TIMELINE_MIN_HEIGHT: f32 = 200.;
pub(crate) const TRANSFORM_HANDLE: f32 = 6.;

/// Install NF2 spacing and text styles once (fonts are installed by `fonts.rs`).
pub(crate) fn initialize_editor_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        neoflash_ui::theme::spacing(style);
        neoflash_ui::typography::install_text_styles(style);
        style.visuals.resize_corner_size = 6.;
    });
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    /// NF2 default theme.
    #[default]
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
        neoflash_ui::theme::apply_visuals(ctx, self.nf2());
    }

    pub fn nf2(self) -> neoflash_ui::Theme {
        match self {
            Self::Light => neoflash_ui::Theme::Light,
            Self::Dark => neoflash_ui::Theme::Dark,
        }
    }
}

/// Semantic tokens shared by ordinary widgets and manual painters. Stage
/// overlays need both overlay and overlay_halo over arbitrary authored colors.
#[derive(Clone, Copy, Debug)]
pub struct UiPalette {
    pub divider: Color32,
    pub focus: Color32,
    pub disabled_text: Color32,
    pub scrollbar_track: Color32,
    pub scrollbar_thumb: Color32,
    pub scrollbar_hover: Color32,
    pub scrollbar_drag: Color32,
    pub scrollbar_focus: Color32,
    pub muted_text: Color32,
    pub error_text: Color32,
    #[allow(dead_code)] // Read by contrast tests; reserved for warning rows.
    pub warning_text: Color32,
    pub panel_fill: Color32,
    pub window_fill: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub timeline_script: Color32,
    pub pasteboard: Color32,
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

/// Editor roles mapped onto NF2 tokens (`neoflash_ui::Palette`). This is the only place where the
/// editor names colours; panels and painters read roles, never hex values.
/// See docs/DESIGN-SYSTEM.md › "Editor palette roles" for the table.
fn palette(theme: ThemeMode) -> UiPalette {
    let t = match theme {
        ThemeMode::Light => &neoflash_ui::LIGHT,
        ThemeMode::Dark => &neoflash_ui::DARK,
    };
    let x = StageOverlay::of(theme);
    UiPalette {
        divider: t.border_subtle,
        focus: t.border_focus,
        disabled_text: t.text_disabled,
        scrollbar_track: t.bg_hover,
        scrollbar_thumb: t.text_disabled,
        scrollbar_hover: t.text_tertiary,
        scrollbar_drag: t.text_secondary,
        scrollbar_focus: t.border_focus,
        muted_text: t.text_secondary,
        error_text: t.danger_default,
        warning_text: t.warning_default,
        panel_fill: t.bg_panel,
        window_fill: t.bg_raised,
        border: t.border_default,
        accent: t.accent_default,
        timeline_script: t.accent_default,
        pasteboard: t.bg_canvas,
        stage_border: t.border_default,
        stage_shadow: x.stage_shadow,
        origin_cross: t.text_tertiary,
        onion_before: x.onion_before,
        onion_after: x.onion_after,
        overlay: x.overlay,
        overlay_halo: x.overlay_halo,
        overlay_fill: x.overlay.gamma_multiply(24. / 255.),
        handle_fill: x.handle,
    }
}

/// Editor-only colours that NF2 does not define (yet). They draw over arbitrary authored artwork
/// (selection outline + halo pair with ≥9:1 contrast, onion-skin tints). Promote them to `NF2 · Tokens` in Figma when the designer
/// covers them; until then they are documented in docs/DESIGN-SYSTEM.md.
struct StageOverlay {
    overlay: Color32,
    overlay_halo: Color32,
    handle: Color32,
    onion_before: Color32,
    onion_after: Color32,
    stage_shadow: Color32,
}
impl StageOverlay {
    fn of(theme: ThemeMode) -> Self {
        let rgb = |v: u32| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        match theme {
            ThemeMode::Light => Self {
                overlay: rgb(0x0f4485),
                overlay_halo: rgb(0xffffff),
                handle: rgb(0xffffff),
                onion_before: rgb(0x256eaa),
                onion_after: rgb(0xa05c16),
                stage_shadow: Color32::from_black_alpha(38),
            },
            ThemeMode::Dark => Self {
                overlay: rgb(0x98c5ff),
                overlay_halo: rgb(0x18202c),
                handle: rgb(0x20242b),
                onion_before: rgb(0x8ac4f0),
                onion_after: rgb(0xf3bd80),
                stage_shadow: Color32::from_black_alpha(102),
            },
        }
    }
}
static LIGHT: LazyLock<UiPalette> = LazyLock::new(|| palette(ThemeMode::Light));
static DARK: LazyLock<UiPalette> = LazyLock::new(|| palette(ThemeMode::Dark));

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
            let t = mode.nf2().palette();
            for (name, a, b) in [
                ("panel text", t.text_primary, p.panel_fill),
                ("window text", t.text_primary, p.window_fill),
                ("input text", t.text_primary, t.bg_field),
                ("muted text", p.muted_text, p.panel_fill),
                ("error", p.error_text, p.window_fill),
                ("timeline text", t.text_primary, t.timeline_cell),
                ("key", t.timeline_marker_inverse, t.timeline_selected),
                (
                    "held key and end marker",
                    t.timeline_marker,
                    t.timeline_empty_5th,
                ),
                ("blank key", t.timeline_marker, t.timeline_empty),
                ("tween", t.timeline_marker, t.timeline_classic_tween),
                ("label", t.timeline_label, t.timeline_cell),
                ("script", p.timeline_script, t.timeline_cell),
            ] {
                assert!(contrast(a, b) >= 4.5, "{mode:?} {name}: {}", contrast(a, b));
            }
            for (name, a, b, minimum) in [
                // NF2 Light `warning/default` on `bg/raised` is 4.24:1. Recorded as a design finding in
                // docs/DESIGN-SYSTEM.md; warnings always pair the colour with an icon or label.
                ("warning", p.warning_text, p.window_fill, 4.2),
                // NF2 selected rows keep `text/primary` labels; the accent marks icons and outlines
                // (non-text contrast, WCAG 1.4.11).
                ("selected text", t.text_primary, t.bg_selected, 4.5),
                ("selected marker", t.accent_default, t.bg_selected, 3.),
                // NF2 idle thumb is a quiet 4 pt floating bar (≈2:1); hover and drag reach 3:1.
                ("scroll thumb", p.scrollbar_thumb, p.scrollbar_track, 1.9),
                ("scroll hover", p.scrollbar_hover, p.scrollbar_track, 3.),
                ("scroll drag", p.scrollbar_drag, p.scrollbar_track, 3.),
                ("scroll focus", p.scrollbar_focus, p.scrollbar_track, 3.),
                // NF2 keeps the frame grid quiet on purpose (Figma `timeline/*` tokens); these only
                // guard that grid, 5th-frame shading and row lines stay distinguishable.
                ("empty frame grid", t.timeline_grid, t.timeline_empty, 1.15),
                (
                    "fifth frame shading",
                    t.timeline_empty_5th,
                    t.timeline_empty,
                    1.05,
                ),
                ("layer separator", t.timeline_line, p.panel_fill, 1.05),
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
            assert_eq!(
                neoflash_ui::palette(&ctx).text_primary,
                mode.nf2().palette().text_primary
            );
            assert_eq!(ctx.style().text_styles[&egui::TextStyle::Body].size, 19.);
            assert_eq!(
                ctx.data(|d| d.get_temp::<String>(id)).as_deref(),
                Some("unsaved text")
            );
            assert_eq!(
                ctx.style().visuals.selection.stroke.color,
                mode.nf2().palette().accent_default
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
