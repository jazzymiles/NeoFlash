//! Editor tool icon roles mapped onto NF2 SVG icons (`neoflash_ui::icons`) and the shared NF2 menu
//! rows. Buttons use `neoflash_ui::components::IconButton` directly.
use crate::theme::{DesignUi as _, ScrollTheme as _};
use eframe::egui::{self, Painter, Pos2, Rect, Response, Sense, Stroke, Vec2};

#[allow(dead_code)]
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
    Bone,
    Bind,
    Puppet,
    Deco,
    SprayBrush,
    Translate3D,
    Rotate3D,
    ZoomIn,
    ZoomOut,
    Fit,
    ChevronRight,
}

const TOOL_BONE_16: neoflash_ui::Icon = neoflash_ui::Icon {
    size: 16,
    name: "tool-bone-16",
    svg: r##"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
<circle cx="4" cy="12" r="2" stroke="#9AA3B6" stroke-width="1"/>
<circle cx="12" cy="4" r="2" stroke="#9AA3B6" stroke-width="1"/>
<line x1="5.5" y1="10.5" x2="10.5" y2="5.5" stroke="#9AA3B6" stroke-width="1.5" stroke-linecap="round"/>
</svg>"##,
};

const TOOL_BIND_16: neoflash_ui::Icon = neoflash_ui::Icon {
    size: 16,
    name: "tool-bind-16",
    svg: r##"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
<line x1="3" y1="13" x2="13" y2="3" stroke="#9AA3B6" stroke-width="1" stroke-linecap="round"/>
<circle cx="8" cy="8" r="2.5" stroke="#9AA3B6" stroke-width="1"/>
<rect x="2" y="2" width="2.5" height="2.5" stroke="#9AA3B6" stroke-width="1"/>
<rect x="11.5" y="11.5" width="2.5" height="2.5" stroke="#9AA3B6" stroke-width="1"/>
</svg>"##,
};

const TOOL_PUPPET_16: neoflash_ui::Icon = neoflash_ui::Icon {
    size: 16,
    name: "tool-puppet-16",
    svg: r##"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
<circle cx="8" cy="4.5" r="2.5" stroke="#9AA3B6" stroke-width="1"/>
<circle cx="8" cy="4.5" r="1" fill="#9AA3B6"/>
<line x1="8" y1="7" x2="8" y2="12.5" stroke="#9AA3B6" stroke-width="1" stroke-linecap="round"/>
<path d="M3 13.5L8 10.5L13 13.5" stroke="#9AA3B6" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"##,
};

const TOOL_PUPPET_20: neoflash_ui::Icon = neoflash_ui::Icon {
    size: 20,
    name: "tool-puppet",
    svg: r##"<svg width="20" height="20" viewBox="0 0 20 20" fill="none" xmlns="http://www.w3.org/2000/svg">
<circle cx="10" cy="5.5" r="3" stroke="#9AA3B6" stroke-width="1.5"/>
<circle cx="10" cy="5.5" r="1" fill="#9AA3B6"/>
<line x1="10" y1="8.5" x2="10" y2="15.5" stroke="#9AA3B6" stroke-width="1.5" stroke-linecap="round"/>
<path d="M3.5 16.5L10 12.5L16.5 16.5" stroke="#9AA3B6" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"##,
};

const TOOL_DECO_16: neoflash_ui::Icon = neoflash_ui::Icon {
    size: 16,
    name: "tool-deco-16",
    svg: r##"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
<path d="M8 2.5C9.5 5 11 6.5 13.5 8C11 9.5 9.5 11 8 13.5C6.5 11 5 9.5 2.5 8C5 6.5 6.5 5 8 2.5Z" stroke="#9AA3B6" stroke-width="1" stroke-linejoin="round"/>
<circle cx="8" cy="8" r="1.2" fill="#9AA3B6"/>
</svg>"##,
};

const TOOL_SPRAY_16: neoflash_ui::Icon = neoflash_ui::Icon {
    size: 16,
    name: "tool-spray-16",
    svg: r##"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
<rect x="2.5" y="6.5" width="6" height="7" rx="1" stroke="#9AA3B6" stroke-width="1"/>
<path d="M4.5 6.5V4.5H7.5V6.5" stroke="#9AA3B6" stroke-width="1"/>
<circle cx="11" cy="4.5" r="1" fill="#9AA3B6"/>
<circle cx="13.5" cy="3.5" r="1" fill="#9AA3B6"/>
<circle cx="13.5" cy="6.5" r="1" fill="#9AA3B6"/>
<circle cx="11.5" cy="7.5" r="1" fill="#9AA3B6"/>
</svg>"##,
};

const TOOL_3D_TRANSLATE_16: neoflash_ui::Icon = neoflash_ui::Icon {
    size: 16,
    name: "tool-3d-translate-16",
    svg: r##"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
<path d="M7 9V2.5M7 2.5L5 4.5M7 2.5L9 4.5" stroke="#9AA3B6" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"/>
<path d="M7 9H13.5M13.5 9L11.5 7M13.5 9L11.5 11" stroke="#9AA3B6" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"/>
<path d="M7 9L2.5 13.5M2.5 13.5H5M2.5 13.5V11" stroke="#9AA3B6" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"##,
};

const TOOL_3D_ROTATE_16: neoflash_ui::Icon = neoflash_ui::Icon {
    size: 16,
    name: "tool-3d-rotate-16",
    svg: r##"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
<circle cx="8" cy="8" r="5.5" stroke="#9AA3B6" stroke-width="1"/>
<ellipse cx="8" cy="8" rx="5.5" ry="2.2" stroke="#9AA3B6" stroke-width="1"/>
<ellipse cx="8" cy="8" rx="2.2" ry="5.5" stroke="#9AA3B6" stroke-width="1"/>
</svg>"##,
};

const GRADIENT_TRANSFORM_16: neoflash_ui::Icon = neoflash_ui::Icon {
    size: 16,
    name: "gradient-transform",
    svg: r##"<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
<defs>
<linearGradient id="gt16" x1="2" y1="8" x2="14" y2="8" gradientUnits="userSpaceOnUse">
<stop offset="0" stop-color="#9AA3B6" stop-opacity="1"/>
<stop offset="0.5" stop-color="#9AA3B6" stop-opacity="0.48"/>
<stop offset="1" stop-color="#9AA3B6" stop-opacity="0.02"/>
</linearGradient>
</defs>
<rect x="2" y="4" width="12" height="8" fill="url(#gt16)"/>
<rect x="1.5" y="3.5" width="13" height="9" stroke="#9AA3B6" stroke-width="1"/>
</svg>"##,
};

const TOOL_GRADIENT_TRANSFORM_20: neoflash_ui::Icon = neoflash_ui::Icon {
    size: 20,
    name: "tool-gradient-transform",
    svg: r##"<svg width="20" height="20" viewBox="0 0 20 20" fill="none" xmlns="http://www.w3.org/2000/svg">
<defs>
<linearGradient id="gt20" x1="3" y1="10" x2="17" y2="10" gradientUnits="userSpaceOnUse">
<stop offset="0" stop-color="#9AA3B6" stop-opacity="1"/>
<stop offset="0.5" stop-color="#9AA3B6" stop-opacity="0.48"/>
<stop offset="1" stop-color="#9AA3B6" stop-opacity="0.02"/>
</linearGradient>
</defs>
<rect x="3" y="5" width="14" height="10" fill="url(#gt20)"/>
<rect x="2.5" y="4.5" width="15" height="11" stroke="#9AA3B6" stroke-width="1"/>
</svg>"##,
};

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
            Self::Gradient => pick(GRADIENT_TRANSFORM_16, TOOL_GRADIENT_TRANSFORM_20),
            Self::Rectangle => pick(i16::RECT, i20::TOOL_RECT),
            Self::Ellipse => pick(i16::OVAL, i20::TOOL_OVAL),
            Self::PolyStar => i20::TOOL_POLYSTAR,
            Self::Text => pick(i16::TEXT, i20::TOOL_TEXT),
            Self::Bone => pick(TOOL_BONE_16, i20::TOOL_BONE),
            Self::Bind => pick(TOOL_BIND_16, i20::TOOL_BIND),
            Self::Puppet => pick(TOOL_PUPPET_16, TOOL_PUPPET_20),
            Self::Deco => pick(TOOL_DECO_16, i20::TOOL_DECO),
            Self::SprayBrush => pick(TOOL_SPRAY_16, i20::TOOL_SPRAY),
            Self::Translate3D => pick(TOOL_3D_TRANSLATE_16, i20::TOOL_3D_TRANSLATE),
            Self::Rotate3D => pick(TOOL_3D_ROTATE_16, i20::TOOL_3D_ROTATE),
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
            let poly = [pt(4.5, 6.5), pt(13.5, 6.5), pt(11.5, 14.0), pt(2.5, 14.0)];
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
            let quad = [pt(3.0, 5.5), pt(13.0, 2.5), pt(13.0, 13.5), pt(3.0, 11.5)];
            for i in 0..4 {
                p.line_segment([quad[i], quad[(i + 1) % 4]], stroke);
                p.rect_filled(
                    Rect::from_center_size(quad[i], Vec2::splat(2.2)),
                    0.0,
                    color,
                );
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ToolModifierGlyph {
    ObjectDrawing,
    GapSize(crate::authoring_tools::GapCloseMode),
    BrushMode(neoflash_core::BrushPaintMode),
    BrushSize(f32),
    BrushShape(neoflash_ui::components::BrushShapeKind),
    EraserMode(crate::authoring_tools::EraserMode),
    Faucet,
    EraserSize(f32, crate::authoring_tools::EraserShape),
    TouchSelects,
    SliceMergeFills,
    LassoPolygon,
    MagicWand,
    MagicWandSettings,
}

fn paint_dropdown_corner(p: &Painter, pt: impl Fn(f32, f32) -> Pos2, color: egui::Color32) {
    p.add(egui::Shape::convex_polygon(
        vec![pt(11.8, 14.2), pt(14.5, 14.2), pt(14.5, 11.5)],
        color,
        Stroke::NONE,
    ));
}

pub(crate) fn paint_tool_modifier_glyph(
    p: &Painter,
    rect: Rect,
    glyph: ToolModifierGlyph,
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
    let scale = box_rect.width() / 16.0;
    let stroke = Stroke::new(1.25_f32, color);
    let thin = Stroke::new(1.0_f32, color);
    match glyph {
        ToolModifierGlyph::ObjectDrawing => {
            // Classic Flash Object Drawing (J): circle inside a bounding box with 4 corner handles.
            let outer = Rect::from_min_max(pt(2.5, 2.5), pt(13.5, 13.5));
            p.rect_stroke(outer, 0.0, thin, egui::StrokeKind::Inside);
            for (hx, hy) in [(2.5, 2.5), (13.5, 2.5), (13.5, 13.5), (2.5, 13.5)] {
                p.rect_filled(
                    Rect::from_center_size(pt(hx, hy), Vec2::splat(2.2 * scale)),
                    0.0,
                    color,
                );
            }
            p.circle_stroke(pt(8.0, 8.0), 3.4 * scale, stroke);
        }
        ToolModifierGlyph::GapSize(mode) => {
            // Circle contour with a gap in the top-right quadrant proportional to GapCloseMode.
            let gap_half_rad = match mode {
                crate::authoring_tools::GapCloseMode::None => 0.0_f32,
                crate::authoring_tools::GapCloseMode::Small => 0.24_f32,
                crate::authoring_tools::GapCloseMode::Medium => 0.50_f32,
                crate::authoring_tools::GapCloseMode::Large => 0.82_f32,
            };
            let center = pt(7.6, 7.8);
            let r = 4.6 * scale;
            if gap_half_rad <= 0.01 {
                p.circle_stroke(center, r, stroke);
            } else {
                let gap_center = -std::f32::consts::FRAC_PI_4;
                let a0 = gap_center + gap_half_rad;
                let a1 = gap_center + std::f32::consts::TAU - gap_half_rad;
                let steps = 20;
                let pts: Vec<Pos2> = (0..=steps)
                    .map(|i| {
                        let t = i as f32 / steps as f32;
                        let a = a0 + (a1 - a0) * t;
                        Pos2::new(center.x + r * a.cos(), center.y + r * a.sin())
                    })
                    .collect();
                p.add(egui::Shape::line(pts, stroke));
            }
            paint_dropdown_corner(p, pt, color);
        }
        ToolModifierGlyph::BrushMode(mode) => {
            let back_rect = Rect::from_min_max(pt(2.4, 2.6), pt(9.8, 10.0));
            let front_center = pt(9.8, 9.6);
            let front_r = 3.4 * scale;
            match mode {
                neoflash_core::BrushPaintMode::PaintNormal => {
                    p.rect_stroke(back_rect, 0.0, thin, egui::StrokeKind::Inside);
                    p.circle_filled(front_center, front_r, color);
                }
                neoflash_core::BrushPaintMode::PaintFills => {
                    p.rect_filled(back_rect, 0.0, color.gamma_multiply(0.35));
                    p.rect_stroke(back_rect, 0.0, thin, egui::StrokeKind::Inside);
                    p.circle_stroke(front_center, front_r, stroke);
                }
                neoflash_core::BrushPaintMode::PaintBehind => {
                    p.circle_filled(pt(6.2, 6.2), front_r, color.gamma_multiply(0.45));
                    let fg_rect = Rect::from_min_max(pt(6.0, 6.0), pt(13.0, 13.0));
                    p.rect_stroke(fg_rect, 0.0, stroke, egui::StrokeKind::Inside);
                }
                neoflash_core::BrushPaintMode::PaintSelection => {
                    p.rect_stroke(back_rect, 0.0, stroke, egui::StrokeKind::Inside);
                    p.line_segment([pt(3.4, 6.3), pt(8.8, 6.3)], thin);
                    p.circle_filled(front_center, front_r * 0.85, color);
                }
                neoflash_core::BrushPaintMode::PaintInside => {
                    let outer = Rect::from_min_max(pt(2.6, 2.6), pt(12.6, 12.6));
                    p.rect_stroke(outer, 0.0, stroke, egui::StrokeKind::Inside);
                    p.circle_filled(pt(7.6, 7.6), 2.8 * scale, color);
                }
            }
            paint_dropdown_corner(p, pt, color);
        }
        ToolModifierGlyph::BrushSize(px) => {
            let norm = ((px - 1.0) / 40.0).clamp(0.0, 1.0);
            let r = (1.4 + norm * 3.8) * scale;
            p.circle_filled(pt(7.6, 7.8), r, color);
            paint_dropdown_corner(p, pt, color);
        }
        ToolModifierGlyph::BrushShape(shape) => {
            use neoflash_ui::components::BrushShapeKind as S;
            let c = pt(7.6, 7.8);
            match shape {
                S::Round => {
                    p.circle_filled(c, 3.8 * scale, color);
                }
                S::OvalH => {
                    p.rect_filled(
                        Rect::from_center_size(c, Vec2::new(8.4 * scale, 4.2 * scale)),
                        2.1 * scale,
                        color,
                    );
                }
                S::OvalV => {
                    p.rect_filled(
                        Rect::from_center_size(c, Vec2::new(4.2 * scale, 8.4 * scale)),
                        2.1 * scale,
                        color,
                    );
                }
                S::Square => {
                    p.rect_filled(
                        Rect::from_center_size(c, Vec2::splat(7.2 * scale)),
                        0.0,
                        color,
                    );
                }
                S::RectH => {
                    p.rect_filled(
                        Rect::from_center_size(c, Vec2::new(8.4 * scale, 3.6 * scale)),
                        0.0,
                        color,
                    );
                }
                S::RectV => {
                    p.rect_filled(
                        Rect::from_center_size(c, Vec2::new(3.6 * scale, 8.4 * scale)),
                        0.0,
                        color,
                    );
                }
                S::Slash => {
                    p.line_segment([pt(4.2, 11.4), pt(11.0, 4.2)], Stroke::new(2.2_f32, color));
                }
                S::Backslash => {
                    p.line_segment([pt(4.2, 4.2), pt(11.0, 11.4)], Stroke::new(2.2_f32, color));
                }
            }
            paint_dropdown_corner(p, pt, color);
        }
        ToolModifierGlyph::EraserMode(mode) => {
            use crate::authoring_tools::EraserMode as E;
            let box_r = Rect::from_min_max(pt(2.6, 3.0), pt(12.2, 12.2));
            match mode {
                E::StrokeSegments => {
                    p.rect_filled(box_r.shrink(1.5 * scale), 0.0, color.gamma_multiply(0.35));
                    p.rect_stroke(box_r, 0.0, stroke, egui::StrokeKind::Inside);
                }
                E::Fills => {
                    p.rect_filled(box_r.shrink(1.5 * scale), 0.0, color);
                }
                E::Strokes => {
                    p.rect_stroke(box_r, 0.0, stroke, egui::StrokeKind::Inside);
                }
                E::Objects => {
                    p.rect_stroke(box_r, 0.0, thin, egui::StrokeKind::Inside);
                    p.line_segment([pt(3.6, 4.0), pt(11.2, 11.2)], thin);
                }
                E::SelectedFills => {
                    p.rect_filled(box_r.shrink(1.5 * scale), 0.0, color.gamma_multiply(0.55));
                    p.line_segment([pt(4.0, 7.6), pt(10.8, 7.6)], thin);
                }
                E::Inside => {
                    p.rect_stroke(box_r, 0.0, thin, egui::StrokeKind::Inside);
                    p.rect_filled(box_r.shrink(2.8 * scale), 0.0, color);
                }
            }
            paint_dropdown_corner(p, pt, color);
        }
        ToolModifierGlyph::Faucet => {
            // Classic Flash Faucet (Кран): T-valve on top, horizontal pipe, downward spout, and water drop.
            p.line_segment([pt(5.0, 2.8), pt(10.6, 2.8)], stroke);
            p.line_segment([pt(7.8, 2.8), pt(7.8, 5.6)], stroke);
            p.line_segment([pt(2.6, 5.6), pt(11.4, 5.6)], Stroke::new(1.8_f32, color));
            p.line_segment([pt(11.4, 5.6), pt(11.4, 9.2)], Stroke::new(1.8_f32, color));
            p.circle_filled(pt(11.4, 12.4), 1.5 * scale, color);
        }
        ToolModifierGlyph::EraserSize(px, shape) => {
            let norm = ((px - 2.0) / 46.0).clamp(0.0, 1.0);
            let r = (1.6 + norm * 3.6) * scale;
            let c = pt(7.6, 7.8);
            match shape {
                crate::authoring_tools::EraserShape::Round => {
                    p.circle_stroke(c, r, stroke);
                }
                crate::authoring_tools::EraserShape::Square => {
                    p.rect_stroke(
                        Rect::from_center_size(c, Vec2::splat(r * 2.0)),
                        0.0,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
            }
            paint_dropdown_corner(p, pt, color);
        }
        ToolModifierGlyph::TouchSelects => {
            let shape_r = Rect::from_min_max(pt(7.2, 3.0), pt(13.4, 9.2));
            p.rect_filled(shape_r, 0.0, color.gamma_multiply(0.35));
            p.rect_stroke(shape_r, 0.0, thin, egui::StrokeKind::Inside);
            let sel_r = Rect::from_min_max(pt(2.4, 6.8), pt(9.4, 13.4));
            p.rect_stroke(sel_r, 0.0, stroke, egui::StrokeKind::Inside);
        }
        ToolModifierGlyph::SliceMergeFills => {
            // Polygon lasso contour slicing across a filled shape.
            let poly = [pt(2.4, 11.8), pt(5.8, 3.2), pt(13.4, 5.6), pt(10.6, 13.2)];
            for i in 0..4 {
                p.line_segment([poly[i], poly[(i + 1) % 4]], stroke);
                p.rect_filled(
                    Rect::from_center_size(poly[i], Vec2::splat(2.0 * scale)),
                    0.0,
                    color,
                );
            }
        }
        ToolModifierGlyph::LassoPolygon => {
            // Classic Flash Polygon Lasso modifier: angular straight-segment lasso loop.
            let pts = [
                pt(2.5, 11.5),
                pt(4.5, 3.0),
                pt(12.8, 4.2),
                pt(13.5, 10.5),
                pt(8.2, 13.4),
            ];
            for i in 0..pts.len() {
                p.line_segment([pts[i], pts[(i + 1) % pts.len()]], stroke);
            }
        }
        ToolModifierGlyph::MagicWand => {
            // Classic Flash Magic Wand modifier: diagonal wand with sparkles at tip.
            p.line_segment([pt(3.0, 13.0), pt(10.5, 5.5)], Stroke::new(1.6_f32, color));
            let tip = pt(11.4, 4.6);
            for (dx, dy) in [
                (0.0, -2.6),
                (0.0, 2.6),
                (-2.6, 0.0),
                (2.6, 0.0),
                (-1.8, -1.8),
                (1.8, -1.8),
                (-1.8, 1.8),
                (1.8, 1.8),
            ] {
                p.line_segment(
                    [
                        tip + Vec2::new(dx * 0.45, dy * 0.45) * scale,
                        tip + Vec2::new(dx, dy) * scale,
                    ],
                    thin,
                );
            }
        }
        ToolModifierGlyph::MagicWandSettings => {
            // Magic Wand + small settings gear/sliders indicator in bottom-right.
            p.line_segment([pt(2.6, 12.6), pt(9.2, 6.0)], Stroke::new(1.5_f32, color));
            let tip = pt(10.0, 5.2);
            for (dx, dy) in [(0.0, -2.2), (0.0, 2.2), (-2.2, 0.0), (2.2, 0.0)] {
                p.line_segment(
                    [
                        tip + Vec2::new(dx * 0.4, dy * 0.4) * scale,
                        tip + Vec2::new(dx, dy) * scale,
                    ],
                    thin,
                );
            }
            p.line_segment([pt(8.5, 11.2), pt(14.0, 11.2)], thin);
            p.line_segment([pt(8.5, 13.4), pt(14.0, 13.4)], thin);
            p.circle_filled(pt(10.5, 11.2), 1.1 * scale, color);
            p.circle_filled(pt(12.4, 13.4), 1.1 * scale, color);
        }
    }
}

pub(crate) fn tool_modifier_button(
    ui: &mut egui::Ui,
    glyph: ToolModifierGlyph,
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
            paint_tool_modifier_glyph(ui.painter(), rect, glyph, fg);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextPropertyGlyph {
    OrientationHorizontal,
    OrientationVerticalLtr,
    OrientationVerticalRtl,
    Underline,
    Strikethrough,
    Superscript,
    Subscript,
    AlignLeft,
    AlignCenter,
    AlignRight,
    JustifyLastLeft,
    JustifyAll,
    BulletList,
    DirectionLtr,
    DirectionRtl,
    VerticalAlignTop,
    VerticalAlignCenter,
    VerticalAlignBottom,
    VerticalAlignJustify,
    ColumnsCount,
    ColumnGutter,
    BorderStroke,
    BackgroundFill,
}

pub(crate) fn paint_text_property_glyph(
    p: &Painter,
    rect: Rect,
    glyph: TextPropertyGlyph,
    color: egui::Color32,
) {
    let side = rect.width().min(rect.height()).min(16.0);
    if !side.is_finite() || side <= 0.0 {
        return;
    }
    let r = Rect::from_center_size(rect.center(), Vec2::splat(side));
    // Reuse approved NF2 optical masters whenever the requested text role already exists. Keep
    // the local painter only for Flash text roles that do not yet have a 12/16 pt master; R-066
    // tracks those missing masters for the design/Figma source.
    let approved = match glyph {
        TextPropertyGlyph::Superscript => Some(neoflash_ui::icons::i16::SUPERSCRIPT),
        TextPropertyGlyph::Subscript => Some(neoflash_ui::icons::i16::SUBSCRIPT),
        TextPropertyGlyph::AlignLeft => Some(neoflash_ui::icons::i16::TALIGN_LEFT),
        TextPropertyGlyph::AlignCenter => Some(neoflash_ui::icons::i16::TALIGN_CENTER),
        TextPropertyGlyph::AlignRight => Some(neoflash_ui::icons::i16::TALIGN_RIGHT),
        TextPropertyGlyph::JustifyLastLeft => Some(neoflash_ui::icons::i16::TALIGN_JUSTIFY),
        TextPropertyGlyph::BorderStroke => Some(neoflash_ui::icons::i16::TEXT_BORDER),
        _ => None,
    };
    if let Some(icon) = approved {
        neoflash_ui::icons::paint_sized(p, p.ctx(), r, icon, side, color);
        return;
    }
    let scale = side / 16.0;
    let pt = |x: f32, y: f32| Pos2::new(r.left() + x * scale, r.top() + y * scale);
    let stroke = Stroke::new((1.35 * scale).max(1.0), color);
    let thin = Stroke::new((1.1 * scale).max(0.9), color);
    let bold = Stroke::new((1.7 * scale).max(1.2), color);

    let draw_t = |p: &Painter, cx: f32, top_y: f32, bot_y: f32, half_w: f32| {
        p.line_segment([pt(cx - half_w, top_y), pt(cx + half_w, top_y)], bold);
        p.line_segment([pt(cx, top_y), pt(cx, bot_y)], bold);
        // Small serifs on the T crossbar and stem foot like Flash CS5
        p.line_segment([pt(cx - half_w, top_y), pt(cx - half_w, top_y + 1.5)], thin);
        p.line_segment([pt(cx + half_w, top_y), pt(cx + half_w, top_y + 1.5)], thin);
        p.line_segment([pt(cx - 1.6, bot_y), pt(cx + 1.6, bot_y)], thin);
    };

    match glyph {
        TextPropertyGlyph::OrientationHorizontal => {
            draw_t(p, 5.5, 3.2, 10.2, 3.2);
            p.line_segment([pt(3.0, 12.8), pt(13.0, 12.8)], thin);
            p.line_segment([pt(10.6, 10.8), pt(13.0, 12.8)], thin);
            p.line_segment([pt(10.6, 14.4), pt(13.0, 12.8)], thin);
        }
        TextPropertyGlyph::OrientationVerticalLtr => {
            draw_t(p, 9.8, 3.2, 10.2, 3.0);
            p.line_segment([pt(4.0, 2.8), pt(4.0, 13.2)], thin);
            p.line_segment([pt(2.3, 10.8), pt(4.0, 13.2)], thin);
            p.line_segment([pt(5.7, 10.8), pt(4.0, 13.2)], thin);
        }
        TextPropertyGlyph::OrientationVerticalRtl => {
            draw_t(p, 6.2, 3.2, 10.2, 3.0);
            p.line_segment([pt(12.2, 2.8), pt(12.2, 13.2)], thin);
            p.line_segment([pt(10.5, 10.8), pt(12.2, 13.2)], thin);
            p.line_segment([pt(13.9, 10.8), pt(12.2, 13.2)], thin);
        }
        TextPropertyGlyph::Underline => {
            draw_t(p, 8.0, 3.0, 11.2, 4.0);
            p.line_segment([pt(3.2, 13.4), pt(12.8, 13.4)], stroke);
        }
        TextPropertyGlyph::Strikethrough => {
            draw_t(p, 8.0, 3.0, 12.2, 4.0);
            p.line_segment([pt(2.6, 7.8), pt(13.4, 7.8)], stroke);
        }
        TextPropertyGlyph::Superscript => {
            draw_t(p, 6.2, 4.4, 12.6, 3.4);
            // Superscript "1" at top-right
            p.line_segment([pt(11.4, 3.6), pt(12.6, 2.6)], thin);
            p.line_segment([pt(12.6, 2.6), pt(12.6, 7.4)], stroke);
            p.line_segment([pt(11.4, 7.4), pt(13.8, 7.4)], thin);
        }
        TextPropertyGlyph::Subscript => {
            draw_t(p, 6.2, 3.2, 11.4, 3.4);
            // Subscript "1" at bottom-right
            p.line_segment([pt(11.4, 9.6), pt(12.6, 8.6)], thin);
            p.line_segment([pt(12.6, 8.6), pt(12.6, 13.4)], stroke);
            p.line_segment([pt(11.4, 13.4), pt(13.8, 13.4)], thin);
        }
        TextPropertyGlyph::AlignLeft => {
            for (y, w) in [(3.5, 10.5), (6.5, 7.5), (9.5, 9.5), (12.5, 6.0)] {
                p.line_segment([pt(2.5, y), pt(2.5 + w, y)], stroke);
            }
        }
        TextPropertyGlyph::AlignCenter => {
            for (y, w) in [(3.5, 10.6), (6.5, 7.0), (9.5, 9.6), (12.5, 5.6)] {
                let x0 = 8.0 - w * 0.5;
                p.line_segment([pt(x0, y), pt(x0 + w, y)], stroke);
            }
        }
        TextPropertyGlyph::AlignRight => {
            for (y, w) in [(3.5, 10.5), (6.5, 7.5), (9.5, 9.5), (12.5, 6.0)] {
                p.line_segment([pt(13.5 - w, y), pt(13.5, y)], stroke);
            }
        }
        TextPropertyGlyph::JustifyLastLeft => {
            for y in [3.5, 6.5, 9.5] {
                p.line_segment([pt(2.5, y), pt(13.5, y)], stroke);
            }
            p.line_segment([pt(2.5, 12.5), pt(8.5, 12.5)], stroke);
        }
        TextPropertyGlyph::JustifyAll => {
            for y in [3.5, 6.5, 9.5, 12.5] {
                p.line_segment([pt(2.5, y), pt(13.5, y)], stroke);
            }
        }
        TextPropertyGlyph::BulletList => {
            for y in [4.0, 8.0, 12.0] {
                p.circle_filled(pt(3.5, y), (1.1 * scale).max(1.0), color);
                p.line_segment([pt(6.2, y), pt(13.5, y)], stroke);
            }
        }
        TextPropertyGlyph::DirectionLtr => {
            p.line_segment([pt(3.0, 4.5), pt(10.0, 4.5)], stroke);
            p.line_segment([pt(3.0, 7.5), pt(8.5, 7.5)], stroke);
            p.line_segment([pt(3.0, 11.8), pt(13.0, 11.8)], stroke);
            p.line_segment([pt(10.4, 9.6), pt(13.0, 11.8)], thin);
            p.line_segment([pt(10.4, 14.0), pt(13.0, 11.8)], thin);
        }
        TextPropertyGlyph::DirectionRtl => {
            p.line_segment([pt(6.0, 4.5), pt(13.0, 4.5)], stroke);
            p.line_segment([pt(7.5, 7.5), pt(13.0, 7.5)], stroke);
            p.line_segment([pt(3.0, 11.8), pt(13.0, 11.8)], stroke);
            p.line_segment([pt(5.6, 9.6), pt(3.0, 11.8)], thin);
            p.line_segment([pt(5.6, 14.0), pt(3.0, 11.8)], thin);
        }
        TextPropertyGlyph::VerticalAlignTop => {
            let box_r = Rect::from_min_max(pt(2.2, 2.2), pt(13.8, 13.8));
            p.rect_stroke(box_r, 1.0, thin, egui::StrokeKind::Inside);
            p.line_segment([pt(4.2, 4.8), pt(11.8, 4.8)], stroke);
            p.line_segment([pt(4.2, 7.4), pt(9.8, 7.4)], stroke);
        }
        TextPropertyGlyph::VerticalAlignCenter => {
            let box_r = Rect::from_min_max(pt(2.2, 2.2), pt(13.8, 13.8));
            p.rect_stroke(box_r, 1.0, thin, egui::StrokeKind::Inside);
            p.line_segment([pt(4.2, 6.7), pt(11.8, 6.7)], stroke);
            p.line_segment([pt(4.2, 9.3), pt(9.8, 9.3)], stroke);
        }
        TextPropertyGlyph::VerticalAlignBottom => {
            let box_r = Rect::from_min_max(pt(2.2, 2.2), pt(13.8, 13.8));
            p.rect_stroke(box_r, 1.0, thin, egui::StrokeKind::Inside);
            p.line_segment([pt(4.2, 8.6), pt(11.8, 8.6)], stroke);
            p.line_segment([pt(4.2, 11.2), pt(9.8, 11.2)], stroke);
        }
        TextPropertyGlyph::VerticalAlignJustify => {
            let box_r = Rect::from_min_max(pt(2.2, 2.2), pt(13.8, 13.8));
            p.rect_stroke(box_r, 1.0, thin, egui::StrokeKind::Inside);
            p.line_segment([pt(4.2, 4.8), pt(11.8, 4.8)], stroke);
            p.line_segment([pt(4.2, 8.0), pt(11.8, 8.0)], stroke);
            p.line_segment([pt(4.2, 11.2), pt(11.8, 11.2)], stroke);
        }
        TextPropertyGlyph::ColumnsCount => {
            let c1 = Rect::from_min_max(pt(2.2, 2.6), pt(7.2, 13.4));
            let c2 = Rect::from_min_max(pt(8.8, 2.6), pt(13.8, 13.4));
            p.rect_stroke(c1, 0.5, thin, egui::StrokeKind::Inside);
            p.rect_stroke(c2, 0.5, thin, egui::StrokeKind::Inside);
        }
        TextPropertyGlyph::ColumnGutter => {
            let c1 = Rect::from_min_max(pt(1.8, 2.6), pt(5.4, 13.4));
            let c2 = Rect::from_min_max(pt(10.6, 2.6), pt(14.2, 13.4));
            p.rect_stroke(c1, 0.5, thin, egui::StrokeKind::Inside);
            p.rect_stroke(c2, 0.5, thin, egui::StrokeKind::Inside);
            p.line_segment([pt(6.2, 8.0), pt(9.8, 8.0)], thin);
            p.line_segment([pt(7.4, 6.5), pt(6.2, 8.0)], thin);
            p.line_segment([pt(7.4, 9.5), pt(6.2, 8.0)], thin);
            p.line_segment([pt(8.6, 6.5), pt(9.8, 8.0)], thin);
            p.line_segment([pt(8.6, 9.5), pt(9.8, 8.0)], thin);
        }
        TextPropertyGlyph::BorderStroke => {
            p.line_segment([pt(4.0, 11.5), pt(11.5, 4.0)], stroke);
            p.line_segment([pt(2.5, 13.5), pt(13.5, 13.5)], bold);
        }
        TextPropertyGlyph::BackgroundFill => {
            let bucket = [pt(4.5, 7.5), pt(8.5, 3.5), pt(12.2, 7.2), pt(8.2, 11.2)];
            for i in 0..4 {
                p.line_segment([bucket[i], bucket[(i + 1) % 4]], thin);
            }
            p.circle_filled(pt(12.8, 11.5), (1.3 * scale).max(1.0), color);
        }
    }
}

pub(crate) fn text_property_icon_button(
    ui: &mut egui::Ui,
    glyph: TextPropertyGlyph,
    selected: bool,
    enabled: bool,
    label: &str,
) -> Response {
    ui.add_enabled_ui_semantic(enabled, |ui| {
        let p = neoflash_ui::palette(ui.ctx());
        let side = crate::theme::control_h().min(22.0);
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
        let s = neoflash_ui::components::State::of(ui, &response);
        let (bg, fg, border) = match (selected, s.enabled, s.hovered) {
            (_, false, _) => (
                egui::Color32::TRANSPARENT,
                p.text_disabled,
                egui::Color32::TRANSPARENT,
            ),
            (true, _, _) => (p.accent_subtle, p.accent_default, p.accent_default),
            (false, _, true) => (p.bg_hover, p.text_primary, p.border_default),
            (false, _, false) => (p.bg_panel, p.text_secondary, p.border_subtle),
        };
        let r = neoflash_ui::tokens::radius::SM;
        if ui.is_rect_visible(rect) {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(r as u8), bg);
            if border != egui::Color32::TRANSPARENT {
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(r as u8),
                    Stroke::new(1.0_f32, border),
                    egui::StrokeKind::Inside,
                );
            }
            paint_text_property_glyph(ui.painter(), rect.shrink(2.0), glyph, fg);
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

pub(crate) fn text_property_badge(ui: &mut egui::Ui, glyph: TextPropertyGlyph, tooltip_text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_text_property_glyph(ui.painter(), rect, glyph, p.text_secondary);
    }
    let _ = neoflash_ui::components::tooltip(response, tooltip_text, None);
}

/// Flash CS5 optional color swatch: shows the classic diagonal red slash when `enabled_flag` is
/// `false` (clicking enables it), or shows the NF2 color swatch + a small clear-to-none slash
/// toggle when `enabled_flag` is `true`.
pub(crate) fn text_optional_color_swatch(
    ui: &mut egui::Ui,
    id_salt: &str,
    enabled_flag: &mut bool,
    rgb: &mut u32,
    label: &str,
) -> bool {
    let mut changed = false;
    let p = neoflash_ui::palette(ui.ctx());
    let danger = crate::theme::UiPalette::current(ui.ctx()).error_text;
    if !*enabled_flag {
        let size = Vec2::new(26.0, (crate::theme::control_h() - 4.0).max(16.0));
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        if ui.is_rect_visible(rect) {
            let r = neoflash_ui::tokens::radius::SM;
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(r as u8), p.bg_panel);
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(r as u8),
                Stroke::new(
                    1.0_f32,
                    if response.hovered() {
                        p.text_secondary
                    } else {
                        p.border_default
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let inner = rect.shrink(2.0);
            ui.painter().line_segment(
                [
                    Pos2::new(inner.left(), inner.bottom()),
                    Pos2::new(inner.right(), inner.top()),
                ],
                Stroke::new(1.5_f32, danger),
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        });
        let response = neoflash_ui::components::tooltip(response, label, None);
        if response.clicked() {
            *enabled_flag = true;
            changed = true;
        }
    } else {
        let mut rgba = [
            ((*rgb >> 16) & 0xff) as u8,
            ((*rgb >> 8) & 0xff) as u8,
            (*rgb & 0xff) as u8,
            255u8,
        ];
        if crate::paint_controls::color_swatch(ui, id_salt, label, &mut rgba).changed() {
            *rgb = ((rgba[0] as u32) << 16) | ((rgba[1] as u32) << 8) | (rgba[2] as u32);
            changed = true;
        }
        let clear_size = Vec2::splat(16.0);
        let (rect, resp) = ui.allocate_exact_size(clear_size, Sense::click());
        if ui.is_rect_visible(rect) {
            let r = neoflash_ui::tokens::radius::SM;
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(r as u8),
                if resp.hovered() {
                    p.bg_hover
                } else {
                    p.bg_panel
                },
            );
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(r as u8),
                Stroke::new(1.0_f32, p.border_subtle),
                egui::StrokeKind::Inside,
            );
            let inner = rect.shrink(2.5);
            ui.painter().line_segment(
                [
                    Pos2::new(inner.left(), inner.bottom()),
                    Pos2::new(inner.right(), inner.top()),
                ],
                Stroke::new(1.4_f32, danger),
            );
        }
        let clear_label = format!("Disable {label}");
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &clear_label)
        });
        if neoflash_ui::components::tooltip(resp, &clear_label, None).clicked() {
            *enabled_flag = false;
            changed = true;
        }
    }
    changed
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuthoringToolIcon {
    Bone,
    Bind,
    Puppet,
    Deco,
    SprayBrush,
    Translate3D,
    Rotate3D,
}

/// Optical-grid-aligned (`1pt` on 16px, `1.5pt` on 20px) vector painter for authoring
/// tools/modifiers (`Bone`, `Bind`, `Puppet / Asset Warp`, `Deco`, `Spray Brush`,
/// `3D Translate`, `3D Rotate`). Uses approved `neoflash_ui::icons::i20` masters where
/// available and crisp vector geometry at 16px or for `Puppet` (tracked in R-067).
#[allow(dead_code)]
pub(crate) fn paint_authoring_tool_icon(
    p: &Painter,
    rect: Rect,
    icon: AuthoringToolIcon,
    color: egui::Color32,
) {
    let size = rect.width().min(rect.height()).min(20.0);
    if !size.is_finite() || size <= 0.0 {
        return;
    }
    let grid = if size >= 20.0 { 20.0 } else { 16.0 };
    let r = Rect::from_center_size(rect.center(), Vec2::splat(grid));
    let scale = grid / 16.0;
    let pt = |x: f32, y: f32| Pos2::new(r.left() + x * scale, r.top() + y * scale);
    let stroke_w = if grid >= 20.0 { 1.5_f32 } else { 1.0_f32 };
    let stroke = Stroke::new(stroke_w, color);
    let thin = Stroke::new(1.0_f32, color);

    match icon {
        AuthoringToolIcon::Bone => {
            p.circle_stroke(pt(4.0, 12.0), 2.0 * scale, stroke);
            p.circle_stroke(pt(12.0, 4.0), 2.0 * scale, stroke);
            p.line_segment([pt(5.5, 10.5), pt(10.5, 5.5)], stroke);
        }
        AuthoringToolIcon::Bind => {
            p.line_segment([pt(3.0, 13.0), pt(13.0, 3.0)], thin);
            p.circle_stroke(pt(8.0, 8.0), 2.4 * scale, stroke);
            p.rect_stroke(
                Rect::from_center_size(pt(3.5, 3.5), Vec2::splat(2.5 * scale)),
                0.0,
                thin,
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                Rect::from_center_size(pt(12.5, 12.5), Vec2::splat(2.5 * scale)),
                0.0,
                thin,
                egui::StrokeKind::Inside,
            );
        }
        AuthoringToolIcon::Puppet => {
            p.circle_stroke(pt(8.0, 4.5), 2.4 * scale, stroke);
            p.circle_filled(pt(8.0, 4.5), 0.9 * scale, color);
            p.line_segment([pt(8.0, 7.0), pt(8.0, 12.5)], stroke);
            p.line_segment([pt(3.0, 13.5), pt(8.0, 10.5)], thin);
            p.line_segment([pt(8.0, 10.5), pt(13.0, 13.5)], thin);
            p.line_segment([pt(3.0, 13.5), pt(13.0, 13.5)], thin);
        }
        AuthoringToolIcon::Deco => {
            let pts = [
                pt(8.0, 2.5),
                pt(10.0, 6.0),
                pt(13.5, 8.0),
                pt(10.0, 10.0),
                pt(8.0, 13.5),
                pt(6.0, 10.0),
                pt(2.5, 8.0),
                pt(6.0, 6.0),
            ];
            for i in 0..pts.len() {
                p.line_segment([pts[i], pts[(i + 1) % pts.len()]], stroke);
            }
            p.circle_filled(pt(8.0, 8.0), 1.1 * scale, color);
        }
        AuthoringToolIcon::SprayBrush => {
            let can = Rect::from_min_max(pt(2.5, 6.5), pt(8.5, 13.5));
            p.rect_stroke(can, 1.0, stroke, egui::StrokeKind::Inside);
            p.line_segment([pt(4.5, 6.5), pt(4.5, 4.5)], thin);
            p.line_segment([pt(4.5, 4.5), pt(7.5, 4.5)], thin);
            p.line_segment([pt(7.5, 4.5), pt(7.5, 6.5)], thin);
            for (cx, cy) in [(11.0, 4.5), (13.2, 3.5), (13.2, 6.5), (11.2, 7.5)] {
                p.circle_filled(pt(cx, cy), 0.9 * scale, color);
            }
        }
        AuthoringToolIcon::Translate3D => {
            p.line_segment([pt(7.0, 9.0), pt(7.0, 2.5)], stroke);
            p.line_segment([pt(7.0, 2.5), pt(5.2, 4.3)], thin);
            p.line_segment([pt(7.0, 2.5), pt(8.8, 4.3)], thin);
            p.line_segment([pt(7.0, 9.0), pt(13.5, 9.0)], stroke);
            p.line_segment([pt(13.5, 9.0), pt(11.7, 7.2)], thin);
            p.line_segment([pt(13.5, 9.0), pt(11.7, 10.8)], thin);
            p.line_segment([pt(7.0, 9.0), pt(2.8, 13.2)], stroke);
            p.line_segment([pt(2.8, 13.2), pt(5.2, 13.2)], thin);
            p.line_segment([pt(2.8, 13.2), pt(2.8, 10.8)], thin);
        }
        AuthoringToolIcon::Rotate3D => {
            p.circle_stroke(pt(8.0, 8.0), 5.2 * scale, stroke);
            p.line_segment([pt(2.8, 8.0), pt(13.2, 8.0)], thin);
            p.line_segment([pt(8.0, 2.8), pt(8.0, 13.2)], thin);
            p.circle_filled(pt(8.0, 8.0), 1.1 * scale, color);
        }
    }
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
            Icon::Bone,
            Icon::Bind,
            Icon::Puppet,
            Icon::Deco,
            Icon::SprayBrush,
            Icon::Translate3D,
            Icon::Rotate3D,
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
        for glyph in [
            ToolModifierGlyph::ObjectDrawing,
            ToolModifierGlyph::GapSize(crate::authoring_tools::GapCloseMode::None),
            ToolModifierGlyph::GapSize(crate::authoring_tools::GapCloseMode::Medium),
            ToolModifierGlyph::BrushMode(neoflash_core::BrushPaintMode::PaintNormal),
            ToolModifierGlyph::BrushMode(neoflash_core::BrushPaintMode::PaintInside),
            ToolModifierGlyph::BrushSize(12.0),
            ToolModifierGlyph::BrushShape(neoflash_ui::components::BrushShapeKind::Round),
            ToolModifierGlyph::BrushShape(neoflash_ui::components::BrushShapeKind::Slash),
            ToolModifierGlyph::EraserMode(crate::authoring_tools::EraserMode::StrokeSegments),
            ToolModifierGlyph::EraserMode(crate::authoring_tools::EraserMode::SelectedFills),
            ToolModifierGlyph::EraserMode(crate::authoring_tools::EraserMode::Inside),
            ToolModifierGlyph::Faucet,
            ToolModifierGlyph::EraserSize(16.0, crate::authoring_tools::EraserShape::Round),
            ToolModifierGlyph::EraserSize(16.0, crate::authoring_tools::EraserShape::Square),
            ToolModifierGlyph::TouchSelects,
            ToolModifierGlyph::SliceMergeFills,
            ToolModifierGlyph::LassoPolygon,
            ToolModifierGlyph::MagicWand,
            ToolModifierGlyph::MagicWandSettings,
        ] {
            let output = ctx.run(Default::default(), |ctx| {
                let p = ctx.layer_painter(egui::LayerId::background());
                paint_tool_modifier_glyph(&p, rect, glyph, Color32::WHITE);
            });
            assert!(!output.shapes.is_empty(), "{glyph:?} paints");
            for shape in &output.shapes {
                assert!(
                    rect.expand(1.)
                        .contains_rect(shape.shape.visual_bounding_rect())
                );
            }
        }
        for text_glyph in [
            TextPropertyGlyph::OrientationHorizontal,
            TextPropertyGlyph::OrientationVerticalLtr,
            TextPropertyGlyph::OrientationVerticalRtl,
            TextPropertyGlyph::Underline,
            TextPropertyGlyph::Strikethrough,
            TextPropertyGlyph::Superscript,
            TextPropertyGlyph::Subscript,
            TextPropertyGlyph::AlignLeft,
            TextPropertyGlyph::AlignCenter,
            TextPropertyGlyph::AlignRight,
            TextPropertyGlyph::JustifyLastLeft,
            TextPropertyGlyph::JustifyAll,
            TextPropertyGlyph::BulletList,
            TextPropertyGlyph::DirectionLtr,
            TextPropertyGlyph::DirectionRtl,
            TextPropertyGlyph::VerticalAlignTop,
            TextPropertyGlyph::VerticalAlignCenter,
            TextPropertyGlyph::VerticalAlignBottom,
            TextPropertyGlyph::VerticalAlignJustify,
            TextPropertyGlyph::ColumnsCount,
            TextPropertyGlyph::ColumnGutter,
            TextPropertyGlyph::BorderStroke,
            TextPropertyGlyph::BackgroundFill,
        ] {
            let output = ctx.run(Default::default(), |ctx| {
                let p = ctx.layer_painter(egui::LayerId::background());
                paint_text_property_glyph(&p, rect, text_glyph, Color32::WHITE);
            });
            assert!(!output.shapes.is_empty(), "{text_glyph:?} paints");
            for shape in &output.shapes {
                assert!(
                    rect.expand(1.)
                        .contains_rect(shape.shape.visual_bounding_rect())
                );
            }
        }
        for authoring_icon in [
            AuthoringToolIcon::Bone,
            AuthoringToolIcon::Bind,
            AuthoringToolIcon::Puppet,
            AuthoringToolIcon::Deco,
            AuthoringToolIcon::SprayBrush,
            AuthoringToolIcon::Translate3D,
            AuthoringToolIcon::Rotate3D,
        ] {
            let output = ctx.run(Default::default(), |ctx| {
                let p = ctx.layer_painter(egui::LayerId::background());
                paint_authoring_tool_icon(&p, rect, authoring_icon, Color32::WHITE);
            });
            assert!(!output.shapes.is_empty(), "{authoring_icon:?} paints");
            for shape in &output.shapes {
                assert!(
                    rect.expand(1.)
                        .contains_rect(shape.shape.visual_bounding_rect())
                );
            }
        }
    }
}
