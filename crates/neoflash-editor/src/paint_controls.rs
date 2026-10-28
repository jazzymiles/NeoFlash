//! Bounded property rows and non-destructive RGBA editing shared by panels.
use crate::{
    app::Editor,
    theme::{CONTROL_HEIGHT, LABEL_WIDTH},
};
use eframe::egui::{self, Response};
use neoflash_core::Color;
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

pub(crate) fn property_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let label_width = (ui.available_width() * 0.38).clamp(50., LABEL_WIDTH);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 7.;
        // ComboBox::height controls its menu; its button uses interact_size.
        ui.spacing_mut().interact_size.y = CONTROL_HEIGHT;
        ui.spacing_mut().button_padding.y = 3.;
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, CONTROL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                // Reserve the whole label column, while keeping the label
                // itself at its left edge rather than centering its widget.
                ui.set_min_size(egui::vec2(label_width, CONTROL_HEIGHT));
                ui.set_max_width(label_width);
                ui.add(
                    egui::Label::new(egui::RichText::new(label).size(crate::theme::PANEL_FONT))
                        .halign(egui::Align::Min)
                        .truncate(),
                );
            },
        );
        content(ui)
    })
    .inner
}

pub(crate) fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> Response {
    property_row(ui, label, |ui| {
        ui.add_sized(
            [ui.available_width(), CONTROL_HEIGHT],
            egui::TextEdit::singleline(value),
        )
    })
}

pub(crate) fn numeric_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    speed: f64,
) -> Response {
    property_row(ui, label, |ui| compact_number(ui, value, range, speed, ""))
}

/// Paired values use the same scrub/commit interaction as single rows.
pub(crate) fn numeric_pair(
    ui: &mut egui::Ui,
    labels: [&str; 2],
    values: [&mut f64; 2],
    range: RangeInclusive<f64>,
    speed: f64,
) -> [Response; 2] {
    let [a, b] = values;
    if ui.available_width() < 248. {
        return [
            numeric_row(ui, labels[0], a, range.clone(), speed),
            numeric_row(ui, labels[1], b, range, speed),
        ];
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.;
        let width = (ui.available_width() - 8.) / 2.;
        let first = ui
            .allocate_ui_with_layout(
                egui::vec2(width, CONTROL_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_max_width(width);
                    ui.label(labels[0]);
                    compact_number(ui, a, range.clone(), speed, "").on_hover_text(labels[0])
                },
            )
            .inner;
        let second = ui
            .allocate_ui_with_layout(
                egui::vec2(width, CONTROL_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_max_width(width);
                    ui.label(labels[1]);
                    compact_number(ui, b, range, speed, "").on_hover_text(labels[1])
                },
            )
            .inner;
        [first, second]
    })
    .inner
}

/// A single click edits text; dragging scrubs. Typed text is committed by
/// Enter/blur, while Escape discards it without touching authoring history.
pub(crate) fn compact_number(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: RangeInclusive<f64>,
    speed: f64,
    unit: &str,
) -> Response {
    let before = *value;
    let mut response = ui.add_sized(
        [ui.available_width().min(96.), CONTROL_HEIGHT],
        egui::DragValue::new(value)
            .range(range)
            .clamp_existing_to_range(false)
            .speed(speed)
            .max_decimals(2)
            .update_while_editing(false)
            .suffix(unit),
    );
    // TextEdit also marks changes while a pending number is being typed.
    // Consumers should begin a document transaction only for applied values.
    response
        .flags
        .set(egui::response::Flags::CHANGED, *value != before);
    response.on_hover_text(
        "Click to type · drag to scrub · Shift for precision · Enter to apply · Esc to cancel",
    )
}

pub(crate) fn percent_row(ui: &mut egui::Ui, label: &str, value: &mut f64) -> Response {
    let mut percent = *value * 100.;
    let response = property_row(ui, label, |ui| {
        compact_number(ui, &mut percent, 0. ..=100., 0.5, " %")
    });
    if response.changed() {
        *value = percent / 100.;
    }
    response
}

/// Only write picker output on a real edit. Keeping RGB and alpha separate
/// avoids the lossy premultiply/unpremultiply round trip for transparent colors.
pub(crate) fn paint_row(ui: &mut egui::Ui, id: &str, label: &str, value: &mut Color) -> Response {
    ui.push_id(id, |ui| {
        let color_response = property_row(ui, label, |ui| {
            let mut rgb = [value[0], value[1], value[2]];
            let response = ui
                .color_edit_button_srgb(&mut rgb)
                .on_hover_text(format!("{label} color"));
            if response.changed() {
                value[..3].copy_from_slice(&rgb);
            }
            ui.label(
                egui::RichText::new(format!("#{:02X}{:02X}{:02X}", value[0], value[1], value[2]))
                    .monospace()
                    .size(11.),
            );
            response
        });
        let mut alpha = value[3] as f64 / 255.;
        let alpha_label = if label == "Background" {
            "Alpha".to_owned()
        } else {
            format!("{label} alpha")
        };
        let alpha_response =
            percent_row(ui, &alpha_label, &mut alpha).on_hover_text(format!("{label} alpha"));
        if alpha_response.changed() {
            value[3] = (alpha * 255.).round().clamp(0., 255.) as u8;
        }
        if alpha_response.changed() {
            alpha_response
        } else {
            color_response
        }
    })
    .inner
}

pub(crate) fn compact_paint(ui: &mut egui::Ui, id: &str, label: &str, value: &mut Color) {
    ui.push_id(id, |ui| {
        ui.small(label);
        let mut rgb = [value[0], value[1], value[2]];
        if ui
            .color_edit_button_srgb(&mut rgb)
            .on_hover_text(format!("{label} color"))
            .changed()
        {
            value[..3].copy_from_slice(&rgb);
        }
        let mut alpha = value[3] as f64 * 100. / 255.;
        if ui
            .add_sized(
                [ui.available_width(), CONTROL_HEIGHT],
                egui::DragValue::new(&mut alpha)
                    .range(0. ..=100.)
                    .speed(1.)
                    .max_decimals(0)
                    .suffix("%"),
            )
            .on_hover_text(format!("{label} alpha"))
            .changed()
        {
            value[3] = (alpha * 255. / 100.).round().clamp(0., 255.) as u8;
        }
    });
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
