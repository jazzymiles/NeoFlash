//! Workspace colors only. Authored fill/stroke/background colors stay in Project.
use eframe::egui::{self, Color32, Stroke};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

pub(crate) const PANEL_FONT: f32 = 12.;
pub(crate) const CONTROL_HEIGHT: f32 = 24.;
pub(crate) const LABEL_WIDTH: f32 = 76.;

/// Give small timeline controls clear rails, handles and check marks without
/// changing the rest of the panel or adding any layout padding.
pub(crate) fn timeline_controls<R>(
    ui: &mut egui::Ui,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.scope(|ui| {
        let p = UiPalette::current(ui.ctx());
        let rail = if ui.visuals().dark_mode {
            Color32::from_rgb(84, 101, 123)
        } else {
            Color32::from_rgb(145, 156, 171)
        };
        ui.spacing_mut().slider_rail_height = 6.;
        let visuals = ui.visuals_mut();
        visuals.slider_trailing_fill = true;
        visuals.selection.bg_fill = p.accent;
        visuals.widgets.inactive.bg_fill = rail;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.5_f32, p.text);
        // Slider handles use fg_stroke for their outline; checkboxes use it
        // for the check. Keep both readable even before hover or focus.
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.5_f32, p.text);
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
        style.spacing.item_spacing = egui::vec2(6., 5.);
        style.spacing.button_padding = egui::vec2(6., 3.);
        style.spacing.interact_size = egui::vec2(36., CONTROL_HEIGHT);
        style.spacing.combo_width = 120.;
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(PANEL_FONT),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::proportional(PANEL_FONT),
        );
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(11.));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(14.));
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
        v.window_stroke = Stroke::new(1.0_f32, p.border);
        v.extreme_bg_color = p.input_fill;
        v.text_edit_bg_color = Some(p.input_fill);
        v.code_bg_color = p.input_fill;
        v.faint_bg_color = p.window_fill;
        v.selection.bg_fill = p.selection_fill;
        v.selection.stroke = Stroke::new(1.0_f32, p.selection_stroke);
        v.hyperlink_color = p.accent;
        v.warn_fg_color = p.warning_text;
        v.error_fg_color = p.error_text;
        v.text_cursor.stroke = Stroke::new(1.5_f32, p.accent);
        v.disabled_alpha = 0.62;
        for widget in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            widget.fg_stroke = Stroke::new(1.0_f32, p.text);
            widget.bg_stroke = Stroke::new(1.0_f32, p.border);
            widget.corner_radius = 2.into();
        }
        v.widgets.noninteractive.bg_fill = p.input_fill;
        v.widgets.noninteractive.weak_bg_fill = p.panel_fill;
        v.widgets.inactive.bg_fill = p.window_fill;
        v.widgets.inactive.weak_bg_fill = p.window_fill;
        for widget in [&mut v.widgets.hovered, &mut v.widgets.open] {
            widget.bg_fill = p.selection_fill;
            widget.weak_bg_fill = p.selection_fill;
            widget.bg_stroke = Stroke::new(1.0_f32, p.accent);
            widget.fg_stroke = Stroke::new(1.0_f32, p.selection_stroke);
        }
        // egui also uses active.fg_stroke for strong labels and headings on
        // ordinary panels, so this pair must work on both backgrounds.
        v.widgets.active.bg_fill = p.selection_fill;
        v.widgets.active.weak_bg_fill = p.selection_fill;
        v.widgets.active.bg_stroke = Stroke::new(1.5_f32, p.accent);
        v.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.selection_stroke);
        v
    }
}

/// Semantic tokens shared by ordinary widgets and manual painters. Stage
/// overlays need both overlay and overlay_halo over arbitrary authored colors.
#[derive(Clone, Copy, Debug)]
pub struct UiPalette {
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

static LIGHT: LazyLock<UiPalette> = LazyLock::new(|| {
    let c = Color32::from_rgb;
    UiPalette {
        text: c(32, 38, 46),
        muted_text: c(78, 88, 101),
        error_text: c(154, 36, 40),
        warning_text: c(126, 76, 0),
        panel_fill: c(229, 231, 234),
        window_fill: c(240, 241, 243),
        input_fill: c(252, 252, 252),
        border: c(118, 129, 144),
        accent: c(25, 86, 154),
        selection_fill: c(188, 216, 245),
        selection_stroke: c(17, 64, 111),
        timeline_fill: c(249, 249, 249),
        timeline_cell: c(248, 248, 248),
        timeline_major: c(210, 217, 225),
        timeline_grid: c(137, 149, 164),
        timeline_row_separator: c(113, 128, 146),
        timeline_active_frame: c(143, 189, 231),
        timeline_active_layer: Color32::from_rgba_unmultiplied(54, 118, 190, 28),
        timeline_tween: c(190, 190, 232),
        timeline_tween_stroke: c(51, 45, 91),
        timeline_key: c(40, 47, 57),
        timeline_blank_key: Color32::WHITE,
        timeline_label: c(133, 32, 38),
        timeline_script: c(30, 69, 133),
        playhead: c(174, 31, 42),
        onion_range: c(30, 95, 144),
        pasteboard: c(163, 167, 173),
        stage_title: c(29, 34, 42),
        stage_border: c(86, 93, 104),
        stage_shadow: Color32::from_black_alpha(40),
        origin_cross: c(64, 72, 85),
        onion_before: c(31, 100, 174),
        onion_after: c(184, 68, 38),
        overlay: c(15, 68, 133),
        overlay_halo: Color32::WHITE,
        overlay_fill: Color32::from_rgba_unmultiplied(60, 125, 210, 25),
        handle_fill: Color32::WHITE,
    }
});
static DARK: LazyLock<UiPalette> = LazyLock::new(|| {
    let c = Color32::from_rgb;
    UiPalette {
        text: c(231, 235, 241),
        muted_text: c(172, 181, 194),
        error_text: c(255, 147, 151),
        warning_text: c(248, 199, 105),
        panel_fill: c(40, 44, 51),
        window_fill: c(46, 50, 58),
        input_fill: c(27, 30, 36),
        border: c(107, 120, 138),
        accent: c(106, 176, 255),
        selection_fill: c(45, 79, 119),
        selection_stroke: c(224, 237, 255),
        timeline_fill: c(31, 35, 42),
        timeline_cell: c(34, 39, 47),
        timeline_major: c(67, 76, 89),
        timeline_grid: c(116, 132, 151),
        timeline_row_separator: c(137, 153, 173),
        timeline_active_frame: c(53, 94, 139),
        timeline_active_layer: Color32::from_rgba_unmultiplied(113, 180, 255, 32),
        timeline_tween: c(80, 76, 124),
        timeline_tween_stroke: c(220, 214, 255),
        timeline_key: c(231, 237, 247),
        timeline_blank_key: c(28, 32, 39),
        timeline_label: c(255, 173, 168),
        timeline_script: c(166, 200, 255),
        playhead: c(255, 117, 129),
        onion_range: c(126, 197, 255),
        pasteboard: c(24, 27, 33),
        stage_title: c(226, 232, 240),
        stage_border: c(136, 148, 166),
        stage_shadow: Color32::from_black_alpha(110),
        origin_cross: c(162, 174, 192),
        onion_before: c(93, 177, 255),
        onion_after: c(255, 145, 106),
        overlay: c(113, 188, 255),
        overlay_halo: Color32::BLACK,
        overlay_fill: Color32::from_rgba_unmultiplied(113, 188, 255, 35),
        handle_fill: c(31, 36, 43),
    }
});

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
    }
}
