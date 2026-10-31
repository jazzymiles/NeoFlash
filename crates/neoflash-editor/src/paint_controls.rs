//! Bounded property rows and non-destructive RGBA editing shared by panels,
//! composed from NF2 fields (`neoflash_ui::components`).
use crate::{
    app::Editor,
    theme::{CONTROL_HEIGHT, LABEL_WIDTH, PROPERTY_ROW_HEIGHT},
};
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
        ui.set_min_height(PROPERTY_ROW_HEIGHT - ui.spacing().item_spacing.y);
        // ComboBox::height controls its menu; its button uses interact_size.
        ui.spacing_mut().interact_size.y = CONTROL_HEIGHT;
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, CONTROL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                // Reserve the whole label column, while keeping the label
                // itself at its left edge rather than centering its widget.
                ui.set_min_size(egui::vec2(label_width, CONTROL_HEIGHT));
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

/// `NF2/Color Field` layout for an unmultiplied RGBA value: swatch (opens the
/// picker), hex with optional inline prefix, and an `α` percentage field.
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
            let swatch = color_swatch(ui, label, value);
            // Figma Background row: swatch 22 · hex 116 · α 134 (of 288 with 8 pt gaps).
            let alpha_width = ((ui.available_width() - space::S2) * 134. / 250.).max(64.);
            let hex_width = (ui.available_width() - alpha_width - space::S2).max(CONTROL_HEIGHT);
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

/// 32 px hit area with a 22 px `NF2/Swatch`; click opens egui's HSV picker
/// on RGB only, so alpha and hidden RGB of transparent paints are preserved.
fn color_swatch(ui: &mut egui::Ui, label: &str, value: &mut Color) -> Response {
    // Figma `NF2/Swatch` 22 × 22 at the row's left edge; the hit area keeps the 32 pt row height.
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(22., CONTROL_HEIGHT), egui::Sense::click());
    let chip = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(22.));
    let shown = egui::Color32::from_rgba_unmultiplied(value[0], value[1], value[2], value[3]);
    neoflash_ui::components::display::paint_swatch(ui, chip, shown, space::S1);
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
    let mut rgb = egui::Color32::from_rgb(value[0], value[1], value[2]);
    let mut edited = false;
    egui::Popup::menu(&response)
        .id(response.id.with("picker"))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.spacing_mut().slider_width = 240.;
            edited = egui::widgets::color_picker::color_picker_color32(
                ui,
                &mut rgb,
                egui::widgets::color_picker::Alpha::Opaque,
            );
        });
    if edited && rgb.to_array()[..3] != value[..3] {
        value[..3].copy_from_slice(&rgb.to_array()[..3]);
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
            self.history.commit(&self.project);
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
