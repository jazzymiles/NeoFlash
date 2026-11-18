//! Trace bitmap dialog (v3 `Dialog / Trace bitmap`): the split preview with its result line and
//! the options on the label grid; each slider row is slider · value · range.
use super::dialog_helpers::{DialogShell, fixed_dialog_size, form_dialog_height, form_row};
use crate::{
    app::Editor,
    bitmap_trace::{TRACE_PRESETS, TraceDialog, preset},
    vector::{PathPaint, paint_path},
};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use neoflash_core::{ElementKind, bitmap_trace::*};
use neoflash_ui::{
    components as c, metrics, palette,
    tokens::{TextStyle, space},
    typography::rich,
};

/// Side of the split preview and width of its column.
pub(crate) fn trace_preview_side() -> f32 {
    metrics::field_l()
}

/// One size whether or not `Advanced options` is open: eleven option rows and the section
/// header, with room for the preview column's status lines.
pub(crate) fn trace_bitmap_size(ctx: &egui::Context) -> Vec2 {
    fixed_dialog_size(
        ctx,
        metrics::dialog_l(),
        form_dialog_height(11, 1, 2.0 * metrics::toggle_row()),
    )
}

impl Editor {
    /// [`trace_bitmap_size`] for tests outside `panels`.
    #[cfg(test)]
    pub(crate) fn trace_bitmap_dialog_size(ctx: &egui::Context) -> Vec2 {
        trace_bitmap_size(ctx)
    }

    pub(crate) fn trace_bitmap_dialog(&mut self, ctx: &egui::Context) {
        self.poll_bitmap_trace(ctx);
        let busy = self.bitmap_trace.busy();
        let progress = self.bitmap_trace.progress();
        let Some(mut dialog) = self.bitmap_trace.dialog.take() else {
            return;
        };
        let before = dialog.options.clone();
        let live_before = dialog.live_preview;
        let mut cancel = false;
        let mut trace = false;
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        let p = palette(ctx);
        let size = trace_bitmap_size(ctx);
        let side = trace_preview_side();
        let modal = egui::Modal::new(egui::Id::new("trace-bitmap-dialog"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                let shell = DialogShell::single(ui, "Trace bitmap", size);
                cancel |= shell.close;
                shell.aside(ui, side, |ui| {
                    preview(ui, &dialog, busy);
                    if busy && !dialog.tracing {
                        ui.horizontal(|ui| {
                            c::spinner(ui);
                            super::planned::caption(ui, "Tracing preview…");
                        });
                    }
                    if let Some(error) = &dialog.error {
                        ui.add(c::InlineMessage::error(error).width(side));
                    }
                });
                shell.form(ui, "trace-bitmap-options", side, |ui, _| {
                    ui.add_enabled_ui(!dialog.tracing, |ui| options(ui, &mut dialog));
                });
                shell.footer(
                    ui,
                    |ui| {
                        if dialog.tracing {
                            c::progress(ui, "Tracing…", Some(progress), metrics::field_l());
                        } else {
                            ui.add(c::Checkbox::new(&mut dialog.live_preview, "Live preview"));
                        }
                    },
                    |ui| {
                        trace |= ui
                            .add_enabled(!dialog.tracing, c::Button::primary("Trace"))
                            .clicked();
                        cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
            });
        if !cancel && !trace {
            match kb.resolve(ctx, &modal, !dialog.tracing) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => trace = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }
        let changed = dialog.options != before;
        let preview = dialog.live_preview && (changed || !live_before);
        self.bitmap_trace.dialog = Some(dialog);
        if cancel {
            self.bitmap_trace.cancel();
        } else if trace {
            self.bitmap_trace.request(true);
        } else if preview {
            self.bitmap_trace.request(false);
        }
    }
}

fn select<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    label: &str,
    current: &mut T,
    values: &[(T, &str)],
) {
    form_row(ui, label, |ui| {
        let shown = values
            .iter()
            .find(|(v, _)| v == current)
            .map(|(_, s)| *s)
            .unwrap_or("");
        c::Select::new(label, shown)
            .width(metrics::field_l())
            .show(ui, |ui| {
                for (value, text) in values {
                    if ui
                        .add(c::MenuItem::new(text).checked(current == value))
                        .clicked()
                    {
                        *current = *value;
                        ui.close();
                    }
                }
            });
    });
}

/// Slider row: slider · value · range caption on one line.
fn slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    unit: &str,
    ends: (&str, &str),
) {
    form_row(ui, label, |ui| {
        let p = palette(ui.ctx());
        ui.add(
            c::Slider::new(value, range.clone(), label)
                .width(metrics::field_m())
                .value(c::SliderValue::None),
        );
        ui.add(
            c::Field::new(value, label)
                .width(metrics::field_num())
                .range(range)
                .unit(unit),
        );
        ui.label(rich(format!("{} – {}", ends.0, ends.1), TextStyle::Small).color(p.text_tertiary));
    });
}

fn options(ui: &mut egui::Ui, dialog: &mut TraceDialog) {
    let presets: Vec<_> = TRACE_PRESETS
        .iter()
        .enumerate()
        .map(|(i, s)| (i, *s))
        .collect();
    let before = dialog.preset;
    select(ui, "Preset", &mut dialog.preset, &presets);
    if before != dialog.preset {
        dialog.options = preset(dialog.preset);
    }
    let options = &mut dialog.options;
    select(
        ui,
        "Mode",
        &mut options.mode,
        &[
            (TraceMode::Colour, "Colour"),
            (TraceMode::Greyscale, "Greyscale"),
            (TraceMode::BlackAndWhite, "Black and white"),
        ],
    );
    select(
        ui,
        "Palette",
        &mut options.palette,
        &[
            (TracePalette::Limited, "Limited"),
            (TracePalette::Full, "Full"),
            (TracePalette::Automatic, "Automatic"),
        ],
    );
    let mut colours = f64::from(options.colours);
    ui.add_enabled_ui(
        options.palette == TracePalette::Limited && options.mode != TraceMode::BlackAndWhite,
        |ui| slider(ui, "Colours", &mut colours, 2.0..=30., "", ("2", "30")),
    );
    options.colours = colours.round() as u8;
    let row_spacing = ui.spacing().item_spacing;
    c::SectionHeader::new("Advanced options")
        .id_salt("trace-bitmap-advanced")
        .default_open(dialog.advanced)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = row_spacing;
            slider(
                ui,
                "Paths",
                &mut options.paths,
                0.0..=100.,
                "%",
                ("Low", "High"),
            );
            slider(
                ui,
                "Corners",
                &mut options.corners,
                0.0..=100.,
                "%",
                ("Less", "More"),
            );
            let mut noise = f64::from(options.noise);
            slider(ui, "Noise", &mut noise, 1.0..=100., "px", ("1", "100"));
            options.noise = noise.round() as u32;
            form_row(ui, "Method", |ui| {
                ui.spacing_mut().item_spacing.x = space::s4();
                if ui
                    .add(c::Radio::new(
                        options.method == TraceMethod::Abutting,
                        "Abutting",
                    ))
                    .clicked()
                {
                    options.method = TraceMethod::Abutting;
                }
                if ui
                    .add(c::Radio::new(
                        options.method == TraceMethod::Overlapping,
                        "Overlapping",
                    ))
                    .clicked()
                {
                    options.method = TraceMethod::Overlapping;
                }
            });
            form_row(ui, "Create", |ui| {
                ui.spacing_mut().item_spacing.x = space::s4();
                ui.add(c::Checkbox::new(&mut options.fills, "Fills"));
                ui.add(c::Checkbox::new(&mut options.strokes, "Strokes"));
            });
            form_row(ui, "Stroke", |ui| {
                ui.add_enabled(
                    options.strokes,
                    c::Field::new(&mut options.stroke_width, "Stroke width")
                        .width(metrics::field_s())
                        .unit("pt")
                        .range(0.01..=1000.)
                        .decimals(1),
                );
            });
            form_row(ui, "Options", |ui| {
                ui.spacing_mut().item_spacing.x = space::s4();
                ui.add(c::Checkbox::new(
                    &mut options.snap_curves,
                    "Snap curves to lines",
                ));
                ui.add(c::Checkbox::new(&mut options.ignore_white, "Ignore white"));
            });
        });
}
fn preview(ui: &mut egui::Ui, dialog: &TraceDialog, busy: bool) {
    let side = trace_preview_side();
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::hover());
    let p = palette(ui.ctx());
    ui.painter()
        .rect_filled(rect, neoflash_ui::tokens::radius::md(), p.bg_field);
    if let Some(preview) = &dialog.preview {
        let trace = &preview.trace;
        let zoom = (rect.width() / trace.width as f32).min(rect.height() / trace.height as f32);
        let art = Rect::from_center_size(
            rect.center(),
            Vec2::new(trace.width as f32, trace.height as f32) * zoom,
        );
        let left = Rect::from_min_max(rect.min, Pos2::new(rect.center().x, rect.bottom()));
        let right = Rect::from_min_max(Pos2::new(rect.center().x, rect.top()), rect.max);
        ui.painter()
            .with_clip_rect(left.intersect(ui.clip_rect()))
            .image(
                preview.source.id(),
                art,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                Color32::WHITE,
            );
        let painter = ui.painter().with_clip_rect(right.intersect(ui.clip_rect()));
        let opacity = if busy { 0.45 } else { 1. };
        let source_width = match dialog.target.element.kind {
            ElementKind::Bitmap { width, .. } => width,
            _ => trace.width,
        };
        let scale = trace.width as f64 / source_width as f64;
        for shape in &trace.shapes {
            let colour = Color32::from_rgba_unmultiplied(
                shape.colour[0],
                shape.colour[1],
                shape.colour[2],
                shape.colour[3],
            )
            .gamma_multiply(opacity);
            paint_path(
                &painter,
                &shape.commands,
                PathPaint {
                    rule: shape.fill_rule,
                    fill: if preview.options.fills {
                        colour
                    } else {
                        Color32::TRANSPARENT
                    },
                    stroke: if preview.options.strokes {
                        colour
                    } else {
                        Color32::TRANSPARENT
                    },
                    width: preview.options.stroke_width * scale,
                    tolerance: 0.25 / zoom,
                    fingerprint: None,
                },
                |p| art.min + p.to_vec2() * zoom,
            );
        }
    }
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        egui::Stroke::new(1.0_f32, p.border_strong),
    );
    let (labels, _) =
        ui.allocate_exact_size(Vec2::new(side, metrics::toggle_row()), egui::Sense::hover());
    let mut label_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(labels)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    label_ui.spacing_mut().interact_size.y = 0.;
    super::planned::caption(&mut label_ui, "Bitmap");
    label_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        super::planned::caption(ui, "Traced")
    });
    let (w, h) = match dialog.target.element.kind {
        ElementKind::Bitmap { width, height, .. } => (width, height),
        _ => (0, 0),
    };
    let result = dialog
        .preview
        .as_ref()
        .map(|p| {
            format!(
                " → {}{} paths, {} colours",
                if p.trace.width < w || p.trace.height < h {
                    "≈ "
                } else {
                    ""
                },
                p.trace.path_count,
                p.trace.shapes.len()
            )
        })
        .unwrap_or_default();
    ui.add(
        egui::Label::new(
            rich(
                format!("{} · {w} × {h}{result}", dialog.resource.path),
                TextStyle::MonoSmall,
            )
            .color(p.text_tertiary),
        )
        .wrap(),
    );
}
