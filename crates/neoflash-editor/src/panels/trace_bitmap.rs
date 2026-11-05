//! R-022: the approved v2.2 Trace bitmap body (`panels-dialog-trace-bitmap.txt`).
use crate::{
    app::Editor,
    bitmap_trace::{TRACE_PRESETS, TraceDialog, preset},
    vector::{PathPaint, paint_path},
};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use neoflash_core::{ElementKind, bitmap_trace::*};
use neoflash_ui::{
    components as c, palette,
    tokens::{TextStyle, space},
    typography::rich,
};

impl Editor {
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
        let p = palette(ctx);
        let modal = egui::Modal::new(egui::Id::new("trace-bitmap-dialog"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(678.);
                ui.spacing_mut().item_spacing.y = 0.;
                cancel |= c::DialogHeader {
                    title: "Trace bitmap",
                }
                .show(ui);
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 24,
                        right: 24,
                        top: 6,
                        bottom: 6,
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = space::S5;
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui_with_layout(
                                Vec2::new(240., 546.),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_width(240.);
                                    ui.spacing_mut().item_spacing.y = space::S2;
                                    preview(ui, &dialog, busy);
                                    if busy && !dialog.tracing {
                                        ui.horizontal(|ui| {
                                            c::spinner(ui);
                                            super::planned::caption(ui, "Tracing preview…");
                                        });
                                    }
                                    if let Some(error) = &dialog.error {
                                        ui.add(c::InlineMessage::error(error).width(240.));
                                    }
                                },
                            );
                            ui.allocate_ui_with_layout(
                                Vec2::new(366., 546.),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_width(366.);
                                    ui.spacing_mut().item_spacing.y = 10.;
                                    ui.add_enabled_ui(!dialog.tracing, |ui| {
                                        options(ui, &mut dialog)
                                    });
                                },
                            );
                        });
                    });
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 24,
                        right: 24,
                        top: 16,
                        bottom: 16,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S2;
                            if dialog.tracing {
                                c::progress(ui, "Tracing…", Some(progress), 300.);
                            } else {
                                ui.add(c::Checkbox::new(&mut dialog.live_preview, "Live preview"));
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    trace |= ui
                                        .add_enabled(!dialog.tracing, c::Button::primary("Trace"))
                                        .clicked();
                                    cancel |= ui.add(c::Button::new("Cancel")).clicked();
                                },
                            );
                        });
                    });
            });
        let popup = egui::Popup::is_any_open(ctx);
        if !popup && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            cancel = true;
        }
        if !popup
            && !ctx.wants_keyboard_input()
            && !dialog.tracing
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            trace = true;
        }
        if modal.should_close() {
            cancel = true;
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
fn row(ui: &mut egui::Ui, label: &str, body: impl FnOnce(&mut egui::Ui)) {
    row_sized(ui, label, 32., body);
}
// The outline gives each row's allocation. Shared toggles retain their own 20 pt hit target;
// the Method radio's 16 pt painted circle is centred in its 16 pt row, with its hit slop in the gap.
fn row_sized(ui: &mut egui::Ui, label: &str, height: f32, body: impl FnOnce(&mut egui::Ui)) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(366., height), egui::Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 10.;
    child.spacing_mut().interact_size.y = 0.;
    let (mut label_rect, _) =
        child.allocate_exact_size(Vec2::new(72., height), egui::Sense::hover());
    if height > 32. {
        // The multiline Options row aligns its label with the first checkbox in the export.
        label_rect.max.y = label_rect.min.y + neoflash_ui::metrics::TOGGLE_ROW;
    }
    let mut label_ui = child.new_child(
        egui::UiBuilder::new()
            .max_rect(label_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    label_ui.label(rich(label, TextStyle::Body).color(p.text_secondary));
    body(&mut child);
}
fn select<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    label: &str,
    current: &mut T,
    values: &[(T, &str)],
) {
    row(ui, label, |ui| {
        let shown = values
            .iter()
            .find(|(v, _)| v == current)
            .map(|(_, s)| *s)
            .unwrap_or("");
        c::Select::new(label, shown).width(284.).show(ui, |ui| {
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
fn slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    unit: &str,
    ends: (&str, &str),
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.;
        row(ui, label, |ui| {
            ui.add(
                c::Slider::new(value, range.clone(), label)
                    .width(140.)
                    .value(c::SliderValue::None),
            );
            ui.add(
                c::Field::new(value, label)
                    .width(134.)
                    .range(range)
                    .unit(unit),
            );
        });
        let (row, _) = ui.allocate_exact_size(Vec2::new(366., 14.), egui::Sense::hover());
        let mut range_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    row.min + Vec2::new(82., 0.),
                    Vec2::new(140., 14.),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        range_ui.spacing_mut().interact_size.y = 0.;
        let p = palette(ui.ctx());
        range_ui.label(rich(ends.0, TextStyle::MonoSmall).color(p.text_tertiary));
        range_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(rich(ends.1, TextStyle::MonoSmall).color(p.text_tertiary));
        });
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
    c::SectionHeader::new("Advanced options")
        .id_salt("trace-bitmap-advanced")
        .default_open(dialog.advanced)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.;
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
            row_sized(ui, "Method", 16., |ui| {
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
            row_sized(ui, "Create", 20., |ui| {
                ui.add(c::Checkbox::new(&mut options.fills, "Fills"));
                ui.add(c::Checkbox::new(&mut options.strokes, "Strokes"));
            });
            row(ui, "Stroke", |ui| {
                ui.add_enabled(
                    options.strokes,
                    c::Field::new(&mut options.stroke_width, "Stroke width")
                        .width(120.)
                        .unit("pt")
                        .range(0.01..=1000.)
                        .decimals(1),
                );
            });
            row_sized(ui, "Options", 48., |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = space::S2;
                    ui.add(c::Checkbox::new(
                        &mut options.snap_curves,
                        "Snap curves to lines",
                    ));
                    ui.add(c::Checkbox::new(&mut options.ignore_white, "Ignore white"));
                });
            });
        });
}
fn preview(ui: &mut egui::Ui, dialog: &TraceDialog, busy: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(240.), egui::Sense::hover());
    let p = palette(ui.ctx());
    ui.painter()
        .rect_filled(rect, neoflash_ui::tokens::radius::LG, p.bg_field);
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
    let (labels, _) = ui.allocate_exact_size(Vec2::new(240., 16.), egui::Sense::hover());
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
    super::planned::mono(ui, &format!("{} · {w} × {h}{result}", dialog.resource.path));
}
