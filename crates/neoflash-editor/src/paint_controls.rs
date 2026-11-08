//! Bounded property rows and non-destructive RGBA editing shared by panels,
//! composed from NF2 fields (`neoflash_ui::components`).
use crate::{app::Editor, theme::LABEL_WIDTH};
use eframe::egui::{self, Response};
use neoflash_core::Color;
use neoflash_ui::{
    TextStyle,
    components::{DragField, TextField},
    palette as nf_palette,
    tokens::space,
    typography::rich,
};
use std::ops::RangeInclusive;

pub(crate) struct PropertyEdit {
    scope: egui::Id,
    control: egui::Id,
    context: egui::Id,
}

#[derive(Default)]
pub(crate) struct PropertyChanges {
    pub control: Option<egui::Id>,
}
impl PropertyChanges {
    pub fn note(&mut self, response: Response) {
        if response.changed() {
            self.control = Some(response.id);
        }
    }
}

/// Label column + content: used where a control has no inline prefix (selects, icon groups, toggles).
pub(crate) fn property_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let label_width = LABEL_WIDTH;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        ui.set_min_height(crate::theme::property_row_h() - ui.spacing().item_spacing.y);
        // ComboBox::height controls its menu; its button uses interact_size.
        ui.spacing_mut().interact_size.y = crate::theme::control_h();
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, crate::theme::control_h()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                // Reserve the whole label column, while keeping the label
                // itself at its left edge rather than centering its widget.
                ui.set_min_size(egui::vec2(label_width, crate::theme::control_h()));
                ui.set_max_width(label_width);
                let color = if ui.is_enabled() {
                    nf_palette(ui.ctx()).text_secondary
                } else {
                    nf_palette(ui.ctx()).text_disabled
                };
                ui.add(
                    egui::Label::new(rich(label, TextStyle::Body).color(color))
                        .halign(egui::Align::Min)
                        .truncate(),
                )
                .on_hover_text(label);
            },
        );
        content(ui)
    })
    .inner
}

/// `NF2/Field` (text) with the label as inline prefix. The returned response
/// is the text editor itself, so callers can request focus.
pub(crate) fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> Response {
    let mut response = ui.add(TextField::new(value, label).prefix(label));
    response.id = TextField::edit_id(response.id);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::text_edit(ui.is_enabled(), &*value, &*value, label);
        info.label = Some(label.into());
        info
    });
    response
}

pub(crate) fn numeric_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    speed: f64,
) -> Response {
    let id = ui.id().with(("numeric-property", label));
    ui.scope_builder(egui::UiBuilder::new().id(id), |ui| {
        number_field(ui, label, value, range, speed, "", None)
    })
    .inner
}

fn numeric_name(label: &str) -> &str {
    match label {
        "X" => "Position X (px)",
        "Y" => "Position Y (px)",
        "W" => "Width (px)",
        "H" => "Height (px)",
        "Scale X" => "Horizontal scale factor",
        "Scale Y" => "Vertical scale factor",
        _ => label,
    }
}

/// One inspector number: `DragField` with the label as prefix and an
/// accessible name derived from it.
pub(crate) fn number_field(
    ui: &mut egui::Ui,
    prefix: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    speed: f64,
    suffix: &str,
    width: Option<f32>,
) -> Response {
    let name = numeric_name(prefix).to_owned();
    let before = *value;
    let mut field = DragField::new(value, &name)
        .prefix(prefix)
        .suffix(suffix)
        .range(range)
        .speed(speed)
        .max_decimals(2);
    if let Some(width) = width {
        field = field.width(width);
    }
    let mut response = ui.add(field);
    // TextEdit also marks changes while a pending number is being typed.
    // Consumers should begin a document transaction only for applied values.
    response
        .flags
        .set(egui::response::Flags::CHANGED, *value != before);
    response.on_hover_text(
        "Click to type · drag to scrub · Shift for precision · Enter to apply · Esc to cancel",
    )
}

/// Narrowest paired field that still shows a prefix and four digits.
const PAIR_FIELD_MIN: f32 = 80.;

/// Paired values use the same scrub/commit interaction as single rows.
pub(crate) fn numeric_pair(
    ui: &mut egui::Ui,
    labels: [&str; 2],
    values: [&mut f64; 2],
    range: RangeInclusive<f64>,
    speed: f64,
) -> [Response; 2] {
    let [a, b] = values;
    // Resolve property identity before choosing the responsive layout. The
    // global child scopes keep DragValue/TextEdit IDs through row wrapping.
    let ids = labels.map(|label| ui.id().with(("numeric-property", label)));
    if ui.available_width() < 2. * PAIR_FIELD_MIN + space::S2 {
        return [
            numeric_row(ui, labels[0], a, range.clone(), speed),
            numeric_row(ui, labels[1], b, range, speed),
        ];
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        let width = (ui.available_width() - space::S2) / 2.;
        let first = ui
            .scope_builder(egui::UiBuilder::new().id(ids[0]), |ui| {
                number_field(ui, labels[0], a, range.clone(), speed, "", Some(width))
            })
            .inner;
        let second = ui
            .scope_builder(egui::UiBuilder::new().id(ids[1]), |ui| {
                number_field(ui, labels[1], b, range, speed, "", Some(width))
            })
            .inner;
        [first, second]
    })
    .inner
}

pub(crate) fn percent_row(ui: &mut egui::Ui, label: &str, value: &mut f64) -> Response {
    let mut percent = *value * 100.;
    let response = number_field(ui, label, &mut percent, 0. ..=100., 0.5, " %", None);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::drag_value(ui.is_enabled(), percent);
        info.label = Some(format!("{label} percent"));
        info
    });
    if response.changed() {
        *value = percent / 100.;
    }
    response
}

/// Only write picker output on a real edit. Keeping RGB and alpha separate
/// avoids the lossy premultiply/unpremultiply round trip for transparent colors.
pub(crate) fn paint_row(ui: &mut egui::Ui, id: &str, label: &str, value: &mut Color) -> Response {
    color_row(ui, id, label, label, value)
}

pub(crate) fn set_paint_row_gradient_swatch(
    ctx: &egui::Context,
    id: &str,
    swatch: Option<neoflash_ui::components::GradientSwatch>,
) {
    let key = egui::Id::new(("paint-row-grad-swatch", id));
    ctx.data_mut(|d| {
        if let Some(s) = swatch {
            d.insert_temp(key, s);
        } else {
            d.remove::<neoflash_ui::components::GradientSwatch>(key);
        }
    });
}

/// `NF2/Color Field` layout for an unmultiplied RGBA value: swatch (opens the
/// NF2 Color & Fill Type popover), hex with optional inline prefix, and an `α` percentage field.
pub(crate) fn color_row(
    ui: &mut egui::Ui,
    id: &str,
    prefix: &str,
    label: &str,
    value: &mut Color,
) -> Response {
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let swatch = color_swatch(ui, id, label, value);
            // Figma Background row: swatch 22 · hex 116 · α 134 (of 288 with 8 pt gaps).
            let alpha_width = ((ui.available_width() - space::S2) * 134. / 250.).max(64.);
            let hex_width =
                (ui.available_width() - alpha_width - space::S2).max(crate::theme::control_h());
            let hex = hex_field(ui, prefix, label, value, hex_width);
            let mut alpha = value[3] as f64 / 255. * 100.;
            let before = alpha;
            let alpha_name = if label == "Background" {
                "Alpha".to_owned()
            } else {
                format!("{label} alpha")
            };
            let mut alpha_response = ui.add(
                DragField::new(&mut alpha, &alpha_name)
                    .prefix("α")
                    .suffix(" %")
                    .range(0. ..=100.)
                    .speed(0.5)
                    .max_decimals(2)
                    .width(ui.available_width()),
            );
            alpha_response
                .flags
                .set(egui::response::Flags::CHANGED, alpha != before);
            let alpha_response = alpha_response.on_hover_text(format!("{label} alpha"));
            if alpha_response.changed() {
                value[3] = (alpha / 100. * 255.).round().clamp(0., 255.) as u8;
            }
            if alpha_response.changed() {
                alpha_response
            } else if hex.changed() {
                hex
            } else {
                swatch
            }
        })
        .inner
    })
    .inner
}

/// 32 px hit area with a 22 px `NF2/Swatch`; click opens the NF2 Color & Fill Type popover.
fn color_swatch(ui: &mut egui::Ui, id: &str, label: &str, value: &mut Color) -> Response {
    // Figma `NF2/Swatch` 22 × 22 at the row's left edge; the hit area keeps the 32 pt row height.
    let (rect, mut response) = ui.allocate_exact_size(
        egui::vec2(22., crate::theme::control_h()),
        egui::Sense::click(),
    );
    let chip = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(22.));
    let grad_key = egui::Id::new(("paint-row-grad-swatch", id));
    if let Some(grad_swatch) =
        ui.data(|d| d.get_temp::<neoflash_ui::components::GradientSwatch>(grad_key))
    {
        neoflash_ui::components::swatches::paint_gradient(ui.painter(), chip, grad_swatch);
    } else {
        let shown = egui::Color32::from_rgba_unmultiplied(value[0], value[1], value[2], value[3]);
        neoflash_ui::components::display::paint_swatch(ui, chip, shown, space::S1);
    }
    let p = nf_palette(ui.ctx());
    if response.hovered() && ui.is_enabled() {
        ui.painter().rect_stroke(
            chip.expand(1.),
            egui::CornerRadius::same(space::S1 as u8),
            egui::Stroke::new(1.0_f32, p.border_strong),
            egui::StrokeKind::Outside,
        );
    }
    if neoflash_ui::theme::keyboard_focus(ui, &response) {
        neoflash_ui::theme::focus_ring(ui, chip, space::S1);
    }
    let popover_target = match id {
        "shape-fill" => Some(("shape-fill", false, true)),
        "shape-stroke" => Some(("shape-stroke", true, true)),
        "path-fill" => Some(("path-fill", false, true)),
        "path-stroke" => Some(("path-stroke", true, true)),
        "text-color" => Some(("text-color", false, true)),
        "text-box-color" => Some(("text-box-color", false, true)),
        "inspector-tool-fill" => Some(("inspector-tool-fill", false, false)),
        "inspector-tool-stroke" => Some(("inspector-tool-stroke", true, false)),
        _ => None,
    };
    if let Some((static_id, stroke_well, color_selection)) = popover_target {
        crate::authoring_modify::update_color_popover_anchor(ui.ctx(), static_id, chip);
        if response.clicked() {
            crate::authoring_modify::request_color_popover(
                ui.ctx(),
                static_id,
                chip,
                stroke_well,
                color_selection,
            );
        }
    } else if show_rgba_color_popup(ui, &response, chip, label, value) {
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::ColorButton,
            ui.is_enabled(),
            format!("{label} color"),
        )
    });
    neoflash_ui::components::tooltip(response, &format!("{label} color"), None)
}

pub(crate) fn show_color32_field_popup(
    ui: &mut egui::Ui,
    response: &Response,
    swatch_clicked: bool,
    label: &str,
    color: &mut egui::Color32,
) -> bool {
    let ctx = ui.ctx().clone();
    let open_id = response.id.with("nf2-rgba-popup-open");
    let mut is_open = ctx.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    if swatch_clicked {
        is_open = !is_open;
        ctx.data_mut(|d| d.insert_temp(open_id, is_open));
    }
    if !is_open {
        return false;
    }
    let anchor = response.rect;
    let mut rgba = color.to_srgba_unmultiplied();
    let mut changed = false;
    let mut close_now = false;
    let area_resp = egui::Area::new(response.id.with("nf2-rgba-popup-area"))
        .order(egui::Order::Foreground)
        .constrain(true)
        .fixed_pos(egui::pos2(anchor.left(), anchor.bottom() + 4.0))
        .show(&ctx, |ui| {
            neoflash_ui::components::popover_frame(&ctx).show(ui, |ui| {
                let (c, should_close) =
                    rgba_color_popover(ui, response.id.with("nf2-rgba-state"), label, &mut rgba);
                changed = c;
                close_now = should_close;
            });
        });
    if changed {
        *color = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
    }
    let popup_rect = area_resp.response.rect;
    let (pressed, pointer_pos, escape) = ctx.input(|i| {
        (
            i.pointer.any_pressed(),
            i.pointer.interact_pos(),
            i.key_pressed(egui::Key::Escape),
        )
    });
    let clicked_outside = pressed
        && pointer_pos.is_some_and(|pos| {
            !popup_rect.contains(pos)
                && !anchor.contains(pos)
                && ctx
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer.order < egui::Order::Foreground)
        });
    if close_now || clicked_outside || escape {
        ctx.data_mut(|d| d.insert_temp(open_id, false));
    }
    changed
}

pub(crate) fn show_rgba_color_popup(
    ui: &mut egui::Ui,
    trigger: &Response,
    anchor: egui::Rect,
    label: &str,
    value: &mut Color,
) -> bool {
    let ctx = ui.ctx().clone();
    let open_id = trigger.id.with("nf2-rgba-popup-open");
    let mut is_open = ctx.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    if trigger.clicked() {
        is_open = !is_open;
        ctx.data_mut(|d| d.insert_temp(open_id, is_open));
    }
    if !is_open {
        return false;
    }
    let mut changed = false;
    let mut close_now = false;
    let area_resp = egui::Area::new(trigger.id.with("nf2-rgba-popup-area"))
        .order(egui::Order::Foreground)
        .constrain(true)
        .fixed_pos(egui::pos2(anchor.left(), anchor.bottom() + 4.0))
        .show(&ctx, |ui| {
            neoflash_ui::components::popover_frame(&ctx).show(ui, |ui| {
                let (c, should_close) =
                    rgba_color_popover(ui, trigger.id.with("nf2-rgba-state"), label, value);
                changed = c;
                close_now = should_close;
            });
        });
    let popup_rect = area_resp.response.rect;
    let (pressed, pointer_pos, escape) = ctx.input(|i| {
        (
            i.pointer.any_pressed(),
            i.pointer.interact_pos(),
            i.key_pressed(egui::Key::Escape),
        )
    });
    let clicked_outside = pressed
        && pointer_pos.is_some_and(|pos| {
            !popup_rect.contains(pos)
                && !anchor.contains(pos)
                && ctx
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer.order < egui::Order::Foreground)
        });
    if close_now || clicked_outside || escape {
        ctx.data_mut(|d| d.insert_temp(open_id, false));
    }
    changed
}

pub(crate) fn rgba_color_popover(
    ui: &mut egui::Ui,
    state_id: egui::Id,
    label: &str,
    value: &mut Color,
) -> (bool, bool) {
    use crate::authoring_modify::PaintFillType;
    use neoflash_ui::components::{
        self as c, ColorField, ColorPicker, Field, GradientBar, MenuItem, Segmented, Select,
        SwatchesPopover,
        color_picker::{Hsb, hsv},
    };

    ui.set_min_width(276.0);
    ui.set_max_width(276.0);
    ui.spacing_mut().item_spacing.y = space::S2;

    let mode_id = state_id.with("mode");
    let model_id = state_id.with("model-hsb");
    let stops_id = state_id.with("stops");
    let stop_sel_id = state_id.with("stop-sel");
    let hsb_id = state_id.with("hsb");

    let default_mode = if value[3] == 0 {
        PaintFillType::None
    } else {
        PaintFillType::Solid
    };
    let mut mode = ui
        .data(|d| d.get_temp::<PaintFillType>(mode_id))
        .unwrap_or(default_mode);
    let mut model_hsb = ui.data(|d| d.get_temp::<bool>(model_id)).unwrap_or(false);
    let mut changed = false;
    let mut should_close = false;

    let width = ui.available_width();
    Select::new(state_id.with("fill-type-select"), mode.label())
        .width(width)
        .show(ui, |ui| {
            for candidate in [
                PaintFillType::None,
                PaintFillType::Solid,
                PaintFillType::LinearGradient,
                PaintFillType::RadialGradient,
                PaintFillType::BitmapFill,
            ] {
                if ui
                    .add(MenuItem::new(candidate.label()).checked(mode == candidate))
                    .clicked()
                {
                    mode = candidate;
                    match mode {
                        PaintFillType::None => {
                            if value[3] != 0 {
                                value[3] = 0;
                                changed = true;
                            }
                        }
                        PaintFillType::Solid
                        | PaintFillType::LinearGradient
                        | PaintFillType::RadialGradient
                        | PaintFillType::BitmapFill => {
                            if value[3] == 0 {
                                value[3] = 255;
                                changed = true;
                            }
                        }
                    }
                    ui.close();
                }
            }
        });
    ui.data_mut(|d| d.insert_temp(mode_id, mode));

    let mut c32 = egui::Color32::from_rgba_unmultiplied(value[0], value[1], value[2], value[3]);
    let cf = ColorField::new(&mut c32, label).width(width).show(ui);
    if cf.response.changed() {
        *value = c32.to_srgba_unmultiplied();
        changed = true;
    }

    let to_hsb = |c: Color| -> Hsb {
        let (r, g, b, a) = (
            c[0] as f32 / 255.0,
            c[1] as f32 / 255.0,
            c[2] as f32 / 255.0,
            c[3] as f32 / 255.0,
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d <= 1e-6 {
            0.0
        } else if (max - r).abs() <= 1e-6 {
            ((g - b) / d).rem_euclid(6.0) / 6.0
        } else if (max - g).abs() <= 1e-6 {
            ((b - r) / d + 2.0) / 6.0
        } else {
            ((r - g) / d + 4.0) / 6.0
        };
        let s = if max <= 1e-6 { 0.0 } else { d / max };
        hsv(h, s, max, a)
    };
    let mut hsb = ui
        .data(|d| d.get_temp::<Hsb>(hsb_id))
        .filter(|cached| egui::Color32::from(*cached).to_srgba_unmultiplied() == *value)
        .unwrap_or_else(|| to_hsb(*value));

    if ui.add(ColorPicker { color: &mut hsb }).changed() {
        *value = egui::Color32::from(hsb).to_srgba_unmultiplied();
        if mode == PaintFillType::None && value[3] > 0 {
            mode = PaintFillType::Solid;
            ui.data_mut(|d| d.insert_temp(mode_id, mode));
        }
        changed = true;
    }

    Segmented::show(ui, &mut model_hsb, &[(false, "RGB"), (true, "HSB")], false);
    ui.data_mut(|d| d.insert_temp(model_id, model_hsb));

    ui.horizontal(|ui| {
        if !model_hsb {
            let mut r = f64::from(value[0]);
            let mut g = f64::from(value[1]);
            let mut b = f64::from(value[2]);
            let rr = ui.add(
                Field::new(&mut r, "Red")
                    .prefix("R")
                    .range(0.0..=255.0)
                    .decimals(0)
                    .width(80.0),
            );
            let rg = ui.add(
                Field::new(&mut g, "Green")
                    .prefix("G")
                    .range(0.0..=255.0)
                    .decimals(0)
                    .width(80.0),
            );
            let rb = ui.add(
                Field::new(&mut b, "Blue")
                    .prefix("B")
                    .range(0.0..=255.0)
                    .decimals(0)
                    .width(80.0),
            );
            for resp in [&rr, &rg, &rb] {
                if resp.changed() {
                    value[0] = r.round().clamp(0.0, 255.0) as u8;
                    value[1] = g.round().clamp(0.0, 255.0) as u8;
                    value[2] = b.round().clamp(0.0, 255.0) as u8;
                    if value[3] == 0 {
                        value[3] = 255;
                    }
                    hsb = to_hsb(*value);
                    changed = true;
                }
            }
        } else {
            let mut h_deg = f64::from(hsb.h * 360.0);
            let mut s_pct = f64::from(hsb.s * 100.0);
            let mut v_pct = f64::from(hsb.v * 100.0);
            let rh = ui.add(
                Field::new(&mut h_deg, "Hue")
                    .prefix("H")
                    .unit("°")
                    .range(0.0..=360.0)
                    .decimals(0)
                    .width(80.0),
            );
            let rs = ui.add(
                Field::new(&mut s_pct, "Saturation")
                    .prefix("S")
                    .unit("%")
                    .range(0.0..=100.0)
                    .decimals(0)
                    .width(80.0),
            );
            let rv = ui.add(
                Field::new(&mut v_pct, "Brightness")
                    .prefix("B")
                    .unit("%")
                    .range(0.0..=100.0)
                    .decimals(0)
                    .width(80.0),
            );
            for resp in [&rh, &rs, &rv] {
                if resp.changed() {
                    hsb.h = (h_deg.clamp(0.0, 360.0) / 360.0) as f32;
                    hsb.s = (s_pct.clamp(0.0, 100.0) / 100.0) as f32;
                    hsb.v = (v_pct.clamp(0.0, 100.0) / 100.0) as f32;
                    if hsb.a == 0.0 {
                        hsb.a = 1.0;
                    }
                    *value = egui::Color32::from(hsb).to_srgba_unmultiplied();
                    changed = true;
                }
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(hsb_id, hsb));

    if matches!(
        mode,
        PaintFillType::LinearGradient | PaintFillType::RadialGradient
    ) {
        let mut stops = ui
            .data(|d| d.get_temp::<Vec<(f32, egui::Color32)>>(stops_id))
            .unwrap_or_else(|| {
                vec![
                    (
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(
                            value[0], value[1], value[2], value[3],
                        ),
                    ),
                    (1.0, egui::Color32::BLACK),
                ]
            });
        let mut selected = ui.data(|d| d.get_temp::<usize>(stop_sel_id)).unwrap_or(0);
        if selected >= stops.len() {
            selected = stops.len().saturating_sub(1);
        }
        let bar_w = ui.available_width().max(120.0);
        if ui
            .add(GradientBar {
                stops: &mut stops,
                selected: &mut selected,
                width: bar_w,
            })
            .changed()
            && let Some((_, stop_col)) = stops.get(selected)
        {
            *value = stop_col.to_srgba_unmultiplied();
            changed = true;
        }
        ui.data_mut(|d| {
            d.insert_temp(stops_id, stops);
            d.insert_temp(stop_sel_id, selected);
        });
    }

    c::SectionHeader::new("Swatches")
        .id_salt(state_id.with("swatches-sec"))
        .default_open(false)
        .show(ui, |ui| {
            let palette = c::default_palette();
            let mut swatch_c32 =
                egui::Color32::from_rgba_unmultiplied(value[0], value[1], value[2], value[3]);
            let before = swatch_c32;
            let out = SwatchesPopover {
                color: &mut swatch_c32,
                recent: &[],
                document: &[before],
                palette: &palette,
                gradients: &[],
            }
            .show(ui);
            if out.no_colour {
                value[3] = 0;
                changed = true;
                should_close = true;
            } else if out.picked || swatch_c32 != before {
                *value = swatch_c32.to_srgba_unmultiplied();
                if value[3] == 0 {
                    value[3] = 255;
                }
                changed = true;
                if out.picked {
                    should_close = true;
                }
            }
        });

    (changed, should_close)
}

/// Hex value (`#RRGGBB`, Roboto Mono). Typed text applies on Enter/blur;
/// Escape or an invalid value restores the stored color.
fn hex_field(
    ui: &mut egui::Ui,
    prefix: &str,
    label: &str,
    value: &mut Color,
    width: f32,
) -> Response {
    let draft_id = ui.id().with("hex-draft");
    let current = format!("#{:02X}{:02X}{:02X}", value[0], value[1], value[2]);
    let mut text = ui
        .data(|d| d.get_temp::<String>(draft_id))
        .unwrap_or_else(|| current.clone());
    let name = format!("{label} hex");
    let mut response = ui.add(
        TextField::new(&mut text, &name)
            .prefix(prefix)
            .mono()
            .width(width),
    );
    response.id = TextField::edit_id(response.id);
    let mut applied = false;
    if response.lost_focus() {
        if !ui.input(|i| i.key_pressed(egui::Key::Escape))
            && let Some(rgb) = neoflash_ui::components::field::parse_hex(&text)
            && rgb[..] != value[..3]
        {
            value[..3].copy_from_slice(&rgb);
            applied = true;
        }
        ui.data_mut(|d| d.remove::<String>(draft_id));
    } else if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(draft_id, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(draft_id));
    }
    response.flags.set(egui::response::Flags::CHANGED, applied);
    response.on_hover_text(format!(
        "Stored RGB: {current}; alpha changes preserve this color"
    ))
}

impl Editor {
    fn property_context(&self) -> egui::Id {
        egui::Id::new((
            &self.clipboard.session,
            self.symbol_id(),
            self.frame,
            self.layer,
            self.selection
                .targets
                .iter()
                .map(|target| (&target.layer, &target.element))
                .collect::<Vec<_>>(),
        ))
    }
    pub(crate) fn finish_property_edit(&mut self) {
        if self.property_edit.take().is_some() {
            self.commit_history();
        }
    }
    pub(crate) fn begin_property_edit(&mut self, scope: egui::Id, control: egui::Id) {
        let context = self.property_context();
        if self.property_edit.as_ref().is_some_and(|edit| {
            edit.scope == scope && edit.control == control && edit.context == context
        }) {
            return;
        }
        self.finish_edits();
        self.playing = false;
        self.accumulator = 0.;
        self.history.begin(&self.project);
        self.property_edit = Some(PropertyEdit {
            scope,
            control,
            context,
        });
    }
    pub(crate) fn property_edit_scope(&mut self, _scope: egui::Id) {
        let context = self.property_context();
        if self
            .property_edit
            .as_ref()
            .is_some_and(|edit| edit.context != context)
        {
            self.finish_property_edit();
        }
    }
    pub(crate) fn end_property_interaction(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.pointer.any_down())
            && !ctx.wants_keyboard_input()
            && !egui::Popup::is_any_open(ctx)
        {
            self.finish_property_edit();
        }
    }
}
