//! Editor tool icon roles mapped onto NF2 SVG icons (`neoflash_ui::icons`) and the shared NF2 menu
//! rows. Buttons use `neoflash_ui::components::IconButton` directly.
use crate::theme::{DesignUi as _, ScrollTheme as _};
use eframe::egui::{self, Painter, Pos2, Rect, Response, Sense, Stroke, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Select,
    Subselect,
    Transform,
    Pen,
    Pencil,
    Line,
    Brush,
    Bucket,
    Ink,
    Eyedropper,
    Eraser,
    Lasso,
    Hand,
    Gradient,
    Rectangle,
    Ellipse,
    PolyStar,
    Text,
    ZoomIn,
    ZoomOut,
    Fit,
    ChevronRight,
}

impl Icon {
    /// The tool-rail glyph for the current density: the 20 px tool master, or the 16 px master in
    /// Classic Flash density (`icon_tool` 16 pt), so tool icons are drawn at their own grid, never
    /// scaled (R-056).
    pub(crate) fn tool_glyph(self) -> neoflash_ui::Icon {
        self.nf2(neoflash_ui::metrics::icon_tool() >= 20.0)
    }
    /// NF2 icon for this role. `tool` picks the 20 px tool glyph where one exists.
    pub(crate) fn nf2(self, tool: bool) -> neoflash_ui::Icon {
        use neoflash_ui::icons::{i16, i20};
        let pick =
            |small: neoflash_ui::Icon, large: neoflash_ui::Icon| if tool { large } else { small };
        match self {
            Self::Select => pick(i16::SELECT, i20::TOOL_SELECT),
            Self::Subselect => pick(i16::SUBSELECT, i20::TOOL_SUBSELECT),
            Self::Transform => pick(i16::TRANSFORM, i20::TOOL_TRANSFORM),
            Self::Pen => pick(i16::PEN, i20::TOOL_PEN),
            Self::Pencil => pick(i16::PENCIL, i20::TOOL_PENCIL),
            Self::Line => pick(i16::LINE, i20::TOOL_LINE),
            Self::Brush => pick(i16::BRUSH, i20::TOOL_BRUSH),
            Self::Bucket => pick(i16::BUCKET, i20::TOOL_BUCKET),
            Self::Ink => i20::TOOL_INK,
            Self::Eyedropper => pick(i16::EYEDROPPER, i20::TOOL_EYEDROPPER),
            Self::Eraser => pick(i16::ERASER, i20::TOOL_ERASER),
            Self::Lasso => pick(i16::LASSO, i20::TOOL_LASSO),
            Self::Hand => pick(i16::HAND, i20::TOOL_HAND),
            Self::Gradient => pick(i16::GRADIENT, i20::TOOL_GRADIENT),
            Self::Rectangle => pick(i16::RECT, i20::TOOL_RECT),
            Self::Ellipse => pick(i16::OVAL, i20::TOOL_OVAL),
            Self::PolyStar => i20::TOOL_POLYSTAR,
            Self::Text => pick(i16::TEXT, i20::TOOL_TEXT),
            Self::ZoomIn => pick(i16::ZOOMIN, i20::TOOL_ZOOM),
            Self::ZoomOut => pick(i16::ZOOMOUT, i20::ZOOMOUT),
            Self::Fit => pick(i16::FIT, i20::FIT),
            Self::ChevronRight => pick(i16::CHEVRIGHT, i20::CHEVRIGHT),
        }
    }
}

/// Paint an NF2 icon centred in `rect`, tinted with `stroke.color`. Rects of 20 pt or more use the
/// 20 px tool grid; smaller rects use the largest optical master that fits (16 or 12 px) and scale
/// only below 12.
pub(crate) fn paint_icon(p: &Painter, rect: Rect, icon: Icon, stroke: Stroke) {
    let size = rect.width().min(rect.height());
    if !size.is_finite() || size <= 0. {
        return;
    }
    let nf = icon.nf2(size >= neoflash_ui::metrics::icon_tool());
    let side = [nf.side(), 16.0, 12.0]
        .into_iter()
        .find(|&s| s <= size)
        .unwrap_or(size);
    neoflash_ui::icons::paint_sized(p, p.ctx(), rect, nf, side, stroke.color);
}

pub(crate) fn menu_style(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    neoflash_ui::components::menu::menu_style(style);
}

/// Size from actual labels/shortcuts, with the design-system compact bounds.
pub(crate) fn menu_width(ui: &egui::Ui, label: &str, shortcut: &str) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), ui.visuals().text_color())
            .size()
            .x
    };
    // Long measured labels use the documented wider variant, bounded by screen.
    (width(label) + width(shortcut) + 80.)
        .max(220.)
        .min((ui.ctx().content_rect().width() - 16.).max(80.))
}

pub(crate) fn menu_contents<R>(
    ui: &mut egui::Ui,
    width: f32,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    menu_style(ui.style_mut());
    ui.set_width(
        width
            .max(220.)
            .min((ui.ctx().content_rect().width() - 16.).max(80.)),
    );
    // egui gives a popup the height it remembered (400 pt on first open, or the height of a shorter
    // menu that shared the popup, e.g. another tab's menu in the same dock group). A scroll area
    // never grows past that, so long menus stayed clipped; allow up to the screen height instead.
    let max_height = (ui.ctx().content_rect().height() - 16.).max(80.);
    ui.set_max_height(max_height);
    egui::ScrollArea::vertical()
        .id_salt("menu-commands")
        .max_height(max_height)
        .auto_shrink([false, true])
        .themed()
        .show(ui, content)
        .inner
}

pub(crate) fn menu_item(ui: &mut egui::Ui, label: &str, shortcut: &str) -> Response {
    menu_item_enabled(ui, true, label, shortcut)
}

/// All menu rows share check/icon, label, shortcut and chevron columns.
/// Explicit painting avoids centering a narrow Checkbox inside a full-width row.
fn menu_row(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: &str,
    checked: Option<bool>,
    submenu: bool,
) -> Response {
    // NF2/Menu Item: 28 pt row, 24 pt check column, label, shortcut right-aligned, ▸ for submenus.
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(1.), crate::theme::row_h()),
        Sense::click(),
    );
    let p = neoflash_ui::palette(ui.ctx());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
            ui.painter()
                .rect_filled(rect, neoflash_ui::tokens::radius::MD, p.bg_hover);
        }
        let text_color = if enabled {
            p.text_primary
        } else {
            p.text_disabled
        };
        let font = egui::TextStyle::Button.resolve(ui.style());
        let small = egui::TextStyle::Small.resolve(ui.style());
        let shortcut_galley = ui.painter().layout_no_wrap(
            shortcut.to_owned(),
            small,
            if enabled {
                p.text_tertiary
            } else {
                p.text_disabled
            },
        );
        let shortcut_right = rect.right() - if submenu { 28. } else { 8. };
        let label_right = if shortcut.is_empty() {
            shortcut_right
        } else {
            shortcut_right - shortcut_galley.size().x - 24.
        };
        let label_rect = Rect::from_min_max(
            Pos2::new(rect.left() + 28., rect.top()),
            Pos2::new(label_right.max(rect.left() + 28.), rect.bottom()),
        );
        ui.painter()
            .with_clip_rect(ui.clip_rect().intersect(label_rect))
            .text(
                Pos2::new(label_rect.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                font,
                text_color,
            );
        ui.painter().galley(
            Pos2::new(
                shortcut_right - shortcut_galley.size().x,
                rect.center().y - shortcut_galley.size().y / 2.,
            ),
            shortcut_galley,
            p.text_tertiary,
        );
        if checked == Some(true) {
            let check = Rect::from_center_size(
                Pos2::new(rect.left() + 14., rect.center().y),
                Vec2::splat(16.),
            );
            neoflash_ui::icons::paint(
                ui.painter(),
                ui.ctx(),
                check,
                neoflash_ui::icons::i16::CHECK,
                if enabled {
                    p.accent_default
                } else {
                    p.text_disabled
                },
            );
        }
        if submenu {
            paint_icon(
                ui.painter(),
                Rect::from_center_size(rect.right_center() - egui::vec2(16., 0.), Vec2::splat(16.)),
                Icon::ChevronRight,
                Stroke::new(1.5_f32, p.text_secondary),
            );
        }
        if neoflash_ui::theme::keyboard_focus(ui, &response) {
            neoflash_ui::theme::focus_ring(ui, rect.shrink(2.), neoflash_ui::tokens::radius::MD);
        }
    }
    let accessible = if shortcut.is_empty() {
        label.to_owned()
    } else {
        format!("{label} ({shortcut})")
    };
    response.widget_info(|| match checked {
        Some(selected) => egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            selected,
            &accessible,
        ),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &accessible),
    });
    response.on_hover_text(accessible)
}

pub(crate) fn menu_item_enabled(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    shortcut: &str,
) -> Response {
    ui.add_enabled_ui_semantic(enabled, |ui| menu_row(ui, label, shortcut, None, false))
        .inner
}

pub(crate) fn menu_check_enabled(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    shortcut: &str,
    checked: bool,
) -> Response {
    ui.add_enabled_ui_semantic(enabled, |ui| {
        menu_row(ui, label, shortcut, Some(checked), false)
    })
    .inner
}

pub(crate) fn menu_toggle(
    ui: &mut egui::Ui,
    value: &mut bool,
    label: &str,
    shortcut: &str,
) -> Response {
    let mut response = menu_row(ui, label, shortcut, Some(*value), false);
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            *value,
            if shortcut.is_empty() {
                label.to_owned()
            } else {
                format!("{label} ({shortcut})")
            },
        )
    });
    response
}

pub(crate) fn menu_popup<R>(
    ui: &mut egui::Ui,
    label: &str,
    widest_label: &str,
    widest_shortcut: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) {
    let contents = |ui: &mut egui::Ui| {
        let width = menu_width(ui, widest_label, widest_shortcut);
        menu_contents(ui, width, content)
    };
    if egui::containers::menu::is_in_menu(ui) {
        menu_submenu(ui, label, contents);
    } else {
        ui.menu_button(label, contents);
    }
}

pub(crate) fn menu_submenu<R>(
    ui: &mut egui::Ui,
    label: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) {
    let response = menu_row(ui, label, "", None, true);
    egui::containers::menu::SubMenu::new().show(ui, &response, content);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransformModifierIcon {
    RotateAndSkew,
    Scale,
    Distort,
    Envelope,
}

/// Paints one of the four Classic Flash Free Transform sub-mode modifier icons (`Rotate and Skew`,
/// `Scale`, `Distort`, `Envelope`) centred inside `rect` on a 16×16 logical grid.
pub(crate) fn paint_transform_modifier_icon(
    p: &Painter,
    rect: Rect,
    icon: TransformModifierIcon,
    color: egui::Color32,
) {
    let size = rect.width().min(rect.height()).min(16.0);
    if !size.is_finite() || size <= 0.0 {
        return;
    }
    let box_rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    let pt = |x: f32, y: f32| -> Pos2 {
        Pos2::new(
            box_rect.left() + (x / 16.0) * box_rect.width(),
            box_rect.top() + (y / 16.0) * box_rect.height(),
        )
    };
    let stroke = Stroke::new(1.25_f32, color);
    let thin = Stroke::new(1.0_f32, color);
    match icon {
        TransformModifierIcon::RotateAndSkew => {
            // Top horizontal right-pointing arrow (`→`) + sheared parallelogram below it.
            p.line_segment([pt(3.0, 3.0), pt(12.5, 3.0)], stroke);
            p.line_segment([pt(10.0, 1.2), pt(12.8, 3.0)], stroke);
            p.line_segment([pt(10.0, 4.8), pt(12.8, 3.0)], stroke);
            let poly = [
                pt(4.5, 6.5),
                pt(13.5, 6.5),
                pt(11.5, 14.0),
                pt(2.5, 14.0),
            ];
            for i in 0..4 {
                p.line_segment([poly[i], poly[(i + 1) % 4]], stroke);
            }
        }
        TransformModifierIcon::Scale => {
            // Outer frame + inset bottom-right corner box + diagonal scale arrow (`↖`).
            let outer = Rect::from_min_max(pt(2.0, 2.0), pt(14.0, 14.0));
            p.rect_stroke(outer, 0.0, stroke, egui::StrokeKind::Inside);
            let inner = Rect::from_min_max(pt(8.5, 8.5), pt(14.0, 14.0));
            p.rect_stroke(inner, 0.0, thin, egui::StrokeKind::Inside);
            p.line_segment([pt(8.5, 8.5), pt(4.2, 4.2)], stroke);
            p.line_segment([pt(4.2, 4.2), pt(7.2, 4.2)], stroke);
            p.line_segment([pt(4.2, 4.2), pt(4.2, 7.2)], stroke);
        }
        TransformModifierIcon::Distort => {
            // Perspective-tapered trapezoid (narrower left edge, taller right edge) + corner handles.
            let quad = [
                pt(3.0, 5.5),
                pt(13.0, 2.5),
                pt(13.0, 13.5),
                pt(3.0, 11.5),
            ];
            for i in 0..4 {
                p.line_segment([quad[i], quad[(i + 1) % 4]], stroke);
                p.rect_filled(Rect::from_center_size(quad[i], Vec2::splat(2.2)), 0.0, color);
            }
        }
        TransformModifierIcon::Envelope => {
            // Outer bounding guide with 8 handle dots + pinched curved envelope contour inside.
            let outer = Rect::from_min_max(pt(2.2, 2.2), pt(13.8, 13.8));
            p.rect_stroke(outer, 0.0, thin, egui::StrokeKind::Inside);
            for (hx, hy) in [
                (2.2, 2.2),
                (8.0, 2.2),
                (13.8, 2.2),
                (13.8, 8.0),
                (13.8, 13.8),
                (8.0, 13.8),
                (2.2, 13.8),
                (2.2, 8.0),
            ] {
                p.rect_filled(
                    Rect::from_center_size(pt(hx, hy), Vec2::splat(2.0)),
                    0.0,
                    color,
                );
            }
            // Pinched bezier-like inner shape
            let curve = [
                pt(3.5, 4.2),
                pt(8.0, 6.2),
                pt(12.5, 4.2),
                pt(10.8, 8.0),
                pt(12.5, 11.8),
                pt(8.0, 9.8),
                pt(3.5, 11.8),
                pt(5.2, 8.0),
            ];
            for i in 0..curve.len() {
                p.line_segment([curve[i], curve[(i + 1) % curve.len()]], stroke);
            }
        }
    }
}

pub(crate) fn transform_modifier_button(
    ui: &mut egui::Ui,
    icon: TransformModifierIcon,
    selected: bool,
    enabled: bool,
    label: &str,
) -> Response {
    ui.add_enabled_ui_semantic(enabled, |ui| {
        let p = neoflash_ui::palette(ui.ctx());
        let side = neoflash_ui::metrics::tool_button();
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
        let s = neoflash_ui::components::State::of(ui, &response);
        let (bg, fg) = match (selected, s.enabled, s.hovered) {
            (_, false, _) => (egui::Color32::TRANSPARENT, p.text_disabled),
            (true, _, _) => (p.accent_subtle, p.accent_default),
            (false, _, true) => (p.bg_hover, p.text_primary),
            (false, _, false) => (egui::Color32::TRANSPARENT, p.text_secondary),
        };
        let r = neoflash_ui::tokens::radius::MD;
        if ui.is_rect_visible(rect) {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(r as u8), bg);
            paint_transform_modifier_icon(ui.painter(), rect, icon, fg);
            if s.focused {
                neoflash_ui::theme::focus_ring(ui, rect, r);
            }
        }
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, s.enabled, label);
            info.selected = Some(selected);
            info
        });
        neoflash_ui::components::tooltip(response, label, None)
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Color32, Shape};
    #[test]
    fn icon_roles_map_to_distinct_nf2_icons_and_paint_inside_their_rect() {
        let icons = [
            Icon::Select,
            Icon::Subselect,
            Icon::Transform,
            Icon::Pen,
            Icon::Pencil,
            Icon::Line,
            Icon::Brush,
            Icon::Bucket,
            Icon::Ink,
            Icon::Eyedropper,
            Icon::Eraser,
            Icon::Lasso,
            Icon::Hand,
            Icon::Gradient,
            Icon::Rectangle,
            Icon::Ellipse,
            Icon::PolyStar,
            Icon::Text,
            Icon::ZoomIn,
            Icon::ZoomOut,
            Icon::Fit,
            Icon::ChevronRight,
        ];
        let names: std::collections::HashSet<_> =
            icons.iter().map(|i| i.nf2(false).path()).collect();
        assert_eq!(names.len(), icons.len(), "every role has its own NF2 glyph");
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(24.));
        for icon in icons {
            let output = ctx.run(Default::default(), |ctx| {
                let p = ctx.layer_painter(egui::LayerId::background());
                paint_icon(&p, rect, icon, Stroke::new(1.5_f32, Color32::BLACK));
            });
            assert!(!output.shapes.is_empty(), "{icon:?} paints");
            for shape in &output.shapes {
                assert!(!matches!(shape.shape, Shape::Text(_)));
                assert!(
                    rect.expand(1.)
                        .contains_rect(shape.shape.visual_bounding_rect())
                );
            }
        }
        for modifier in [
            TransformModifierIcon::RotateAndSkew,
            TransformModifierIcon::Scale,
            TransformModifierIcon::Distort,
            TransformModifierIcon::Envelope,
        ] {
            let output = ctx.run(Default::default(), |ctx| {
                let p = ctx.layer_painter(egui::LayerId::background());
                paint_transform_modifier_icon(&p, rect, modifier, Color32::WHITE);
            });
            assert!(!output.shapes.is_empty(), "{modifier:?} paints");
            for shape in &output.shapes {
                assert!(
                    rect.expand(1.)
                        .contains_rect(shape.shape.visual_bounding_rect())
                );
            }
        }
    }
}
