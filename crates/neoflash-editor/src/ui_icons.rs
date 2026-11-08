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
    EraserSize(f32),
    TouchSelects,
    SliceMergeFills,
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
        ToolModifierGlyph::EraserSize(px) => {
            let norm = ((px - 2.0) / 46.0).clamp(0.0, 1.0);
            let r = (1.6 + norm * 3.6) * scale;
            p.circle_stroke(pt(7.6, 7.8), r, stroke);
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
            ToolModifierGlyph::Faucet,
            ToolModifierGlyph::EraserSize(16.0),
            ToolModifierGlyph::TouchSelects,
            ToolModifierGlyph::SliceMergeFills,
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
    }
}
