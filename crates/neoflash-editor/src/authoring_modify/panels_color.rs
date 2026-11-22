use super::fill_type::*;
use super::panels_align_transform::caption_label;
use super::*;

impl Editor {
    pub(crate) fn color_panel_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_title(ui, PanelId::Color, "Color");
        // Compact state (R-055): the mixers read these and request a toggle / a recent colour.
        let recent: Vec<Color> = self.workspace.recent_colors.clone();
        ui.data_mut(|d| {
            d.insert_temp(
                color_picker_expanded(),
                self.workspace.color_picker_expanded,
            );
            d.insert_temp(color_recent(), recent);
            d.remove::<bool>(color_picker_toggle());
        });
        panel_scroll_body(ui, PanelId::Color, "color-content", |ui| {
            self.color_panel_contents(ui)
        });
        if ui.data_mut(|d| d.remove_temp::<bool>(color_picker_toggle())) == Some(true) {
            self.workspace.color_picker_expanded = !self.workspace.color_picker_expanded;
        }
    }

    pub(crate) fn bitmap_fill_section_ui(&mut self, ui: &mut egui::Ui) {
        let nf = neoflash_ui::palette(ui.ctx());
        let mut selected_tile = self.authoring_modify.drawing_bitmap_fill.unwrap_or(0);
        let lock_id = egui::Id::new("nf2-color-bitmap-lock-fill");
        let mut lock_fill = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(lock_id))
            .unwrap_or(false);
        let tile = |h: f32, s: f32| -> (egui::Color32, egui::Color32) {
            (hsv(h, s, 0.85, 1.0).into(), hsv(h, s, 0.35, 1.0).into())
        };
        let bitmaps = [
            tile(0.07, 0.7),
            tile(0.64, 0.55),
            tile(0.38, 0.55),
            tile(0.0, 0.0),
            tile(0.96, 0.65),
            tile(0.62, 0.15),
        ];
        for (row_idx, row) in bitmaps.chunks(4).enumerate() {
            ui.horizontal(|ui| {
                for (col_idx, (from, to)) in row.iter().enumerate() {
                    let idx = row_idx * 4 + col_idx;
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(58.0, 58.0), egui::Sense::click());
                    neoflash_ui::components::swatches::paint_gradient(
                        ui.painter(),
                        rect,
                        neoflash_ui::components::GradientSwatch {
                            from: *from,
                            to: *to,
                            radial: false,
                        },
                    );
                    let selected = selected_tile == idx;
                    let (w, color) = if selected {
                        (2.0_f32, nf.accent_default)
                    } else {
                        (1.0, nf.border_subtle)
                    };
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::same(neoflash_ui::tokens::radius::md() as u8),
                        egui::Stroke::new(w, color),
                        egui::StrokeKind::Inside,
                    );
                    let tile_label = format!("tile_{}.png", idx + 1);
                    if neoflash_ui::components::tooltip(resp, &tile_label, None).clicked() {
                        selected_tile = idx;
                        self.authoring_modify.drawing_bitmap_fill = Some(idx);
                        let _ = self.apply_bitmap_swatch_tile(idx);
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.add(Button::new("Import…").icon(i16::PLUS)).clicked() {
                self.authoring_modify.drawing_bitmap_fill = Some(selected_tile);
                let _ = self.apply_bitmap_swatch_tile(selected_tile);
            }
            if ui
                .add(IconButton::new(i16::LOCK_FILL, "Lock fill").selected(lock_fill))
                .clicked()
            {
                lock_fill = !lock_fill;
                ui.ctx().data_mut(|d| d.insert_temp(lock_id, lock_fill));
            }
            caption_label(ui, "Lock fill");
        });
        let img_count = self
            .project
            .resources
            .iter()
            .filter(|r| r.is_image())
            .count();
        caption_label(
            ui,
            &format!(
                "tile_{}.png · {} bitmap resource(s) · tiles across selection",
                selected_tile + 1,
                img_count
            ),
        );
    }

    pub(crate) fn color_fill_popover_contents(&mut self, ui: &mut egui::Ui) -> bool {
        let width = crate::paint_controls::paint_popover_width();
        ui.set_min_width(width);
        ui.set_max_width(width);
        ui.data_mut(|d| {
            d.remove::<bool>(color_picker_expanded());
            d.remove::<bool>(color_picker_toggle());
        });
        self.color_panel_contents(ui);
        let mut should_close = false;
        ui.horizontal(|ui| {
            if ui
                .add(Button::new("Add to swatches").icon(i16::PLUS))
                .clicked()
            {
                self.add_active_swatch(ui.ctx());
            }
            if ui
                .add(IconButton::new(i16::EYEDROPPER, "Eyedropper"))
                .clicked()
            {
                self.set_tool(crate::app::Tool::Eyedropper);
                should_close = true;
            }
        });
        should_close
    }

    pub(crate) fn show_active_color_popover(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(active) =
            ctx.data(|d| d.get_temp::<ActiveColorPopover>(active_color_popover_id()))
        else {
            return;
        };
        let pass_guard_id = egui::Id::new("nf2-active-color-popover-pass");
        let pass = ctx.cumulative_pass_nr();
        if ctx.data(|d| d.get_temp::<u64>(pass_guard_id)) == Some(pass) {
            return;
        }
        ctx.data_mut(|d| d.insert_temp(pass_guard_id, pass));
        let mut close_now = false;
        let area_resp =
            egui::Area::new(egui::Id::new(("nf2-color-fill-popover", &active.source_id)))
                .order(egui::Order::Foreground)
                .constrain(true)
                .fixed_pos(egui::pos2(
                    active.anchor_rect.left(),
                    active.anchor_rect.bottom() + 4.0,
                ))
                .show(&ctx, |ui| {
                    neoflash_ui::components::popover_frame(&ctx).show(ui, |ui| {
                        close_now = self.color_fill_popover_contents(ui);
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
                    && !active.anchor_rect.contains(pos)
                    && ctx
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer.order < egui::Order::Foreground)
            });
        if close_now || clicked_outside || escape {
            close_color_popover(&ctx);
        }
    }

    fn color_panel_contents(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = space::s2();
        let targets = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect::<Vec<_>>();
        let values: Vec<_> = targets
            .iter()
            .filter_map(|(t, e)| {
                paints_with_gradients_for_element(e, self.target_has_bitmap_fill(t))
            })
            .collect();
        // Which paint the wells edit: remembered per panel, Fill by default.
        let active_id = egui::Id::new("color-panel-active-stroke");
        let mut stroke_active = ui.data(|d| d.get_temp::<bool>(active_id).unwrap_or(false));
        // Unified contextual colour: with nothing paintable selected the Color panel edits the
        // tool (drawing) colours; when paintable objects are selected, edits apply to both the
        // selection and the active tool paint without switching modes.
        if !self.authoring_modify.color_selection || targets.is_empty() || values.is_empty() {
            let fill_grad = self.authoring_modify.drawing_fill_gradient.clone();
            let stroke_grad = self.authoring_modify.drawing_stroke_gradient.clone();
            let action = color_wells_and_actions(
                ui,
                &mut stroke_active,
                self.fill,
                fill_grad.as_ref(),
                self.stroke,
                stroke_grad.as_ref(),
                true,
            );
            ui.data_mut(|d| d.insert_temp(active_id, stroke_active));
            match action {
                Some(WellAction::Swap) => {
                    self.swap_active_paints();
                    return;
                }
                Some(WellAction::NoColor) => {
                    self.set_drawing_fill_type(stroke_active, PaintFillType::None);
                    return;
                }
                Some(WellAction::DefaultBlackWhite) => {
                    self.authoring_modify.drawing_bitmap_fill = None;
                    self.reset_default_paints();
                    return;
                }
                None => {}
            }
            let active_color = if stroke_active {
                self.stroke
            } else {
                self.fill
            };
            let active_grad = if stroke_active {
                self.authoring_modify.drawing_stroke_gradient.clone()
            } else {
                self.authoring_modify.drawing_fill_gradient.clone()
            };
            let has_bitmap = !stroke_active && self.authoring_modify.drawing_bitmap_fill.is_some();
            let current_mode = classify_fill_type(active_color, active_grad.as_ref(), has_bitmap);
            if let Some(new_mode) = fill_type_select(ui, current_mode, stroke_active) {
                self.set_drawing_fill_type(stroke_active, new_mode);
                return;
            }
            if current_mode == PaintFillType::BitmapFill {
                self.bitmap_fill_section_ui(ui);
            } else if let Some(mut grad) = active_grad {
                if let Some((spread, linear_rgb)) = flow_row(ui, grad.spread, grad.linear_rgb) {
                    grad.spread = spread;
                    grad.linear_rgb = linear_rgb;
                    self.set_selection_gradient_options(stroke_active, spread, linear_rgb);
                }
                let stop_count = grad.stops.len().max(1);
                if self.authoring_modify.selected_stop >= stop_count {
                    self.authoring_modify.selected_stop = stop_count - 1;
                }
                let mut stop_color = grad.stops[self.authoring_modify.selected_stop].color;
                let mixer_key = egui::Id::new((
                    "drawing-grad-hsb",
                    stroke_active,
                    self.authoring_modify.selected_stop,
                ));
                if color_mixer_ui(
                    ui,
                    mixer_key,
                    if stroke_active {
                        "Stroke colour"
                    } else {
                        "Fill colour"
                    },
                    &mut stop_color,
                    &mut self.authoring_modify.color_model_hsb,
                )
                .is_some()
                {
                    grad.stops[self.authoring_modify.selected_stop].color = stop_color;
                    self.set_selection_gradient_stops(stroke_active, grad.stops.clone(), None);
                }
                if gradient_stops_bar_ui(ui, &mut grad, &mut self.authoring_modify.selected_stop)
                    .0
                    .is_some()
                {
                    self.set_selection_gradient_stops(stroke_active, grad.stops, None);
                }
            } else {
                let target_color = if stroke_active {
                    &mut self.stroke
                } else {
                    &mut self.fill
                };
                let mixer_key = egui::Id::new(("drawing-solid-hsb", stroke_active));
                let _ = color_mixer_ui(
                    ui,
                    mixer_key,
                    if stroke_active {
                        "Stroke colour"
                    } else {
                        "Fill colour"
                    },
                    target_color,
                    &mut self.authoring_modify.color_model_hsb,
                );
            }
            let _ = stroke_section_ui(ui, &mut self.stroke_width, false);
            return;
        }

        let (fill, fill_grad, stroke, stroke_grad, width) = self.contextual_paints();
        let supports_stroke = values.iter().any(|value| value.supports_stroke);
        let mixed_fill = values
            .iter()
            .filter(|v| v.has_fill)
            .any(|v| v.fill != fill || v.fill_gradient != fill_grad);
        let mixed_stroke = values
            .iter()
            .filter(|v| v.has_stroke)
            .any(|v| v.stroke != stroke || v.stroke_gradient != stroke_grad);
        let mixed_width = values
            .iter()
            .filter(|v| v.has_stroke)
            .any(|v| v.stroke_width != width);
        if !supports_stroke {
            stroke_active = false;
        }
        let scope = self.modify_scope();
        self.property_edit_scope(scope);
        ui.push_id(scope.with("color"), |ui| {
            let action = color_wells_and_actions(
                ui,
                &mut stroke_active,
                fill,
                fill_grad.as_ref(),
                stroke,
                stroke_grad.as_ref(),
                supports_stroke,
            );
            ui.data_mut(|d| d.insert_temp(active_id, stroke_active));
            match action {
                Some(WellAction::Swap) => {
                    self.swap_active_paints();
                    return;
                }
                Some(WellAction::NoColor) => {
                    self.set_selection_fill_type(stroke_active, PaintFillType::None);
                    return;
                }
                Some(WellAction::DefaultBlackWhite) => {
                    self.authoring_modify.drawing_bitmap_fill = None;
                    self.reset_default_paints();
                    return;
                }
                None => {}
            }

            let active_color = if stroke_active { stroke } else { fill };
            let active_grad = if stroke_active {
                stroke_grad.clone()
            } else {
                fill_grad.clone()
            };
            let has_bitmap = !stroke_active
                && (self.authoring_modify.drawing_bitmap_fill.is_some()
                    || targets.iter().any(|(t, _)| self.target_has_bitmap_fill(t)));
            let current_mode = classify_fill_type(active_color, active_grad.as_ref(), has_bitmap);
            if let Some(new_mode) = fill_type_select(ui, current_mode, stroke_active) {
                self.set_drawing_fill_type(stroke_active, new_mode);
                self.set_selection_fill_type(stroke_active, new_mode);
                return;
            }

            if current_mode == PaintFillType::BitmapFill {
                self.bitmap_fill_section_ui(ui);
            } else if let Some(mut grad) = active_grad {
                if let Some((spread, linear_rgb)) = flow_row(ui, grad.spread, grad.linear_rgb) {
                    grad.spread = spread;
                    grad.linear_rgb = linear_rgb;
                    self.set_selection_gradient_options(stroke_active, spread, linear_rgb);
                }
                let stop_count = grad.stops.len().max(1);
                if self.authoring_modify.selected_stop >= stop_count {
                    self.authoring_modify.selected_stop = stop_count - 1;
                }
                let mut stop_color = grad.stops[self.authoring_modify.selected_stop].color;
                let mixer_key = scope.with((
                    "sel-grad-hsb",
                    stroke_active,
                    self.authoring_modify.selected_stop,
                ));
                if let Some(control) = color_mixer_ui(
                    ui,
                    mixer_key,
                    if stroke_active {
                        "Stroke colour"
                    } else {
                        "Fill colour"
                    },
                    &mut stop_color,
                    &mut self.authoring_modify.color_model_hsb,
                ) {
                    grad.stops[self.authoring_modify.selected_stop].color = stop_color;
                    self.set_selection_gradient_stops(
                        stroke_active,
                        grad.stops.clone(),
                        Some(control),
                    );
                }
                let (changed, focused) =
                    gradient_stops_bar_ui(ui, &mut grad, &mut self.authoring_modify.selected_stop);
                if let Some(control) = changed {
                    let drag_control = ui
                        .input(|input| input.pointer.primary_down())
                        .then_some(control);
                    self.set_selection_gradient_stops(stroke_active, grad.stops, drag_control);
                }
                if focused && ui.is_enabled() && !self.has_modal_dialog_in(ui.ctx()) {
                    // The stop edits the selected object. Undo here belongs to the document.
                    use crate::commands::CommandId;
                    for cmd in [CommandId::EditUndo, CommandId::EditRedo] {
                        if self.take_control_shortcut(ui.ctx(), cmd, true) {
                            self.execute_menu_command(ui.ctx(), cmd);
                            break;
                        }
                    }
                }
            } else {
                let (mut new_fill, mut new_stroke) = (fill, stroke);
                let control = if !stroke_active {
                    color_mixer_ui(
                        ui,
                        scope.with("sel-fill-hsb"),
                        "Fill colour",
                        &mut new_fill,
                        &mut self.authoring_modify.color_model_hsb,
                    )
                } else {
                    color_mixer_ui(
                        ui,
                        scope.with("sel-stroke-hsb"),
                        "Stroke colour",
                        &mut new_stroke,
                        &mut self.authoring_modify.color_model_hsb,
                    )
                };
                if let Some(control) = control {
                    if stroke_active {
                        self.stroke = new_stroke;
                        update_gradient_for_color_edit(
                            &mut self.authoring_modify.drawing_stroke_gradient,
                            stroke,
                            new_stroke,
                        );
                    } else {
                        self.fill = new_fill;
                        update_gradient_for_color_edit(
                            &mut self.authoring_modify.drawing_fill_gradient,
                            fill,
                            new_fill,
                        );
                    }
                    let multi = targets.len() > 1;
                    let changes = targets
                        .iter()
                        .cloned()
                        .filter_map(|(target, mut element)| {
                            let has_bm = self.target_has_bitmap_fill(&target);
                            let eligible = if stroke_active {
                                element_has_stroke(&element) || (!multi && !is_merge_fill(&element))
                            } else {
                                element_has_fill(&element, has_bm)
                                    || (!multi
                                        && !element_has_stroke(&element)
                                        && !is_merge_stroke(&element))
                            };
                            if !eligible {
                                return None;
                            }
                            let previous = paints(&element.kind)?;
                            let old_fill = if !stroke_active && (mixed_fill || previous.0 != fill) {
                                [
                                    new_fill[0].wrapping_add(1),
                                    new_fill[1],
                                    new_fill[2],
                                    new_fill[3].wrapping_add(1),
                                ]
                            } else {
                                fill
                            };
                            let old_stroke =
                                if stroke_active && (mixed_stroke || previous.1 != stroke) {
                                    [
                                        new_stroke[0].wrapping_add(1),
                                        new_stroke[1],
                                        new_stroke[2],
                                        new_stroke[3].wrapping_add(1),
                                    ]
                                } else {
                                    stroke
                                };
                            let changed = edit_paints(
                                &mut element.kind,
                                (
                                    if !stroke_active { old_fill } else { previous.0 },
                                    if stroke_active {
                                        old_stroke
                                    } else {
                                        previous.1
                                    },
                                    previous.2,
                                ),
                                (
                                    if !stroke_active { new_fill } else { previous.0 },
                                    if stroke_active {
                                        new_stroke
                                    } else {
                                        previous.1
                                    },
                                    previous.2,
                                ),
                            );
                            changed.then_some((target, element))
                        })
                        .collect();
                    self.apply_modified(changes, Some(control));
                }
            }

            if supports_stroke {
                let mut new_width = width;
                if let Some(control) = stroke_section_ui(ui, &mut new_width, true) {
                    self.stroke_width = new_width;
                    let multi = targets.len() > 1;
                    let changes = targets
                        .into_iter()
                        .filter_map(|(target, mut element)| {
                            if (multi || is_merge_fill(&element)) && !element_has_stroke(&element) {
                                return None;
                            }
                            let previous = paints(&element.kind)?;
                            let changed = edit_paints(
                                &mut element.kind,
                                (
                                    previous.0,
                                    previous.1,
                                    if mixed_width || previous.2 != width {
                                        new_width - 1.0
                                    } else {
                                        width
                                    },
                                ),
                                (previous.0, previous.1, new_width),
                            );
                            changed.then_some((target, element))
                        })
                        .collect();
                    self.apply_modified(changes, Some(control));
                }
            }
        });
        self.end_property_interaction(ui.ctx());
    }
}

#[derive(Clone, Copy)]
pub(super) enum WellAction {
    Swap,
    NoColor,
    DefaultBlackWhite,
}

/// Fill and Stroke `NF2/Color Well`s (Fill first on the left, Stroke second on the right)
/// plus Swap / No colour / Black and white quick actions.
pub(super) fn color_wells_and_actions(
    ui: &mut egui::Ui,
    stroke_active: &mut bool,
    fill: Color,
    fill_grad: Option<&GradientPaint>,
    stroke: Color,
    stroke_grad: Option<&GradientPaint>,
    stroke_enabled: bool,
) -> Option<WellAction> {
    let shown = |c: Color, g: Option<&GradientPaint>| {
        if let Some(paint) = g
            && let Some(stop) = paint.stops.first()
        {
            return Some(egui::Color32::from_rgba_unmultiplied(
                stop.color[0],
                stop.color[1],
                stop.color[2],
                stop.color[3],
            ));
        }
        (c != [0, 0, 0, 0]).then(|| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
    };
    let overpaint_well_gradient =
        |ui: &egui::Ui, well_rect: egui::Rect, grad: Option<&GradientPaint>| {
            if let Some(g) = grad {
                // The well's chip: field padding, 16 px icon, control gap, then the swatch.
                let side = neoflash_ui::metrics::swatch();
                let sw = egui::Rect::from_min_size(
                    egui::pos2(
                        well_rect.left() + space::s3() + 16.0 + space::s2(),
                        well_rect.center().y - side / 2.0,
                    ),
                    egui::vec2(side, side),
                );
                crate::paint_controls::paint_gradient_swatch(
                    ui.painter(),
                    sw,
                    g,
                    neoflash_ui::tokens::radius::sm(),
                );
            }
        };
    let mut action = None;
    // Tool-sized quick actions (28 / 24 / 22), so the row fits the inspector in every density.
    let action_side = neoflash_ui::metrics::tool_button();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::s2();
        let fill_well = ui.add(ColorWell {
            target: ColorWellTarget::Fill,
            color: shown(fill, fill_grad),
            active: !*stroke_active,
        });
        overpaint_well_gradient(ui, fill_well.rect, fill_grad);
        if fill_well.clicked() {
            *stroke_active = false;
        }
        let stroke_well = ui.add_enabled(
            stroke_enabled,
            ColorWell {
                target: ColorWellTarget::Stroke,
                color: shown(stroke, stroke_grad),
                active: *stroke_active,
            },
        );
        overpaint_well_gradient(ui, stroke_well.rect, stroke_grad);
        if stroke_well.clicked() {
            *stroke_active = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    stroke_enabled,
                    IconButton::new(i20::COLOR_SWAP, "Swap colours").side(action_side),
                )
                .clicked()
            {
                action = Some(WellAction::Swap);
            }
            if ui
                .add(IconButton::new(i20::COLOR_NONE, "No colour").side(action_side))
                .clicked()
            {
                action = Some(WellAction::NoColor);
            }
            if ui
                .add(IconButton::new(i20::COLOR_DEFAULT, "Black and white").side(action_side))
                .clicked()
            {
                action = Some(WellAction::DefaultBlackWhite);
            }
        });
    });
    action
}

/// Fill type `NF2/Select` (§6 "Color — type menu"): edits None, Solid colour, Linear gradient,
/// Radial gradient and Bitmap fill directly.
pub(super) fn fill_type_select(
    ui: &mut egui::Ui,
    current: PaintFillType,
    stroke_active: bool,
) -> Option<PaintFillType> {
    let width = ui.available_width();
    let mut chosen = None;
    Select::new("color-fill-type", current.label())
        .width(width)
        .show(ui, |ui| {
            for mode in [
                PaintFillType::None,
                PaintFillType::Solid,
                PaintFillType::LinearGradient,
                PaintFillType::RadialGradient,
                PaintFillType::BitmapFill,
            ] {
                if mode == PaintFillType::BitmapFill && stroke_active {
                    continue;
                }
                if ui
                    .add(MenuItem::new(mode.label()).checked(current == mode))
                    .clicked()
                {
                    chosen = Some(mode);
                    ui.close();
                }
            }
        });
    chosen
}

pub(super) fn spread_label(spread: neoflash_core::GradientSpread) -> &'static str {
    match spread {
        neoflash_core::GradientSpread::Extend => "Extend",
        neoflash_core::GradientSpread::Reflect => "Reflect",
        neoflash_core::GradientSpread::Repeat => "Repeat",
    }
}

pub(super) fn gradient_overflow_mode(
    spread: neoflash_core::GradientSpread,
) -> neoflash_ui::components::GradientOverflowMode {
    match spread {
        neoflash_core::GradientSpread::Extend => {
            neoflash_ui::components::GradientOverflowMode::Extend
        }
        neoflash_core::GradientSpread::Reflect => {
            neoflash_ui::components::GradientOverflowMode::Reflect
        }
        neoflash_core::GradientSpread::Repeat => {
            neoflash_ui::components::GradientOverflowMode::Repeat
        }
    }
}

pub(super) fn flow_row(
    ui: &mut egui::Ui,
    mut spread: neoflash_core::GradientSpread,
    mut linear_rgb: bool,
) -> Option<(neoflash_core::GradientSpread, bool)> {
    let nf = neoflash_ui::palette(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(rich("Overflow", TextStyle::Body).color(nf.text_secondary));
        let mut chosen = None;
        let (select_resp, _) = Select::new("color-gradient-overflow", spread_label(spread))
            .width(114.0)
            .icon(i16::FLOW_EXTEND)
            .show(ui, |ui| {
                ui.set_min_width(136.0);
                for (mode, padded_label) in [
                    (neoflash_core::GradientSpread::Extend, "       Extend"),
                    (neoflash_core::GradientSpread::Reflect, "       Reflect"),
                    (neoflash_core::GradientSpread::Repeat, "       Repeat"),
                ] {
                    let item_resp = ui.add(MenuItem::new(padded_label).checked(spread == mode));
                    let preview_rect = egui::Rect::from_center_size(
                        egui::pos2(item_resp.rect.left() + 38.0, item_resp.rect.center().y),
                        egui::vec2(22.0, 14.0),
                    );
                    neoflash_ui::components::paint_gradient_overflow_preview(
                        ui.painter(),
                        ui.ctx(),
                        preview_rect,
                        gradient_overflow_mode(mode),
                    );
                    if item_resp.clicked() {
                        chosen = Some(mode);
                        ui.close();
                    }
                }
            });
        // Overpaint the 16x16 placeholder icon slot inside the closed Select with the visual overflow preview.
        let closed_icon_bg = if select_resp.hovered() {
            nf.bg_hover
        } else {
            nf.bg_raised
        };
        let closed_preview = egui::Rect::from_center_size(
            egui::pos2(
                select_resp.rect.left() + space::s3() + 8.0,
                select_resp.rect.center().y,
            ),
            egui::vec2(20.0, 13.0),
        );
        ui.painter().rect_filled(
            closed_preview.expand(1.0),
            egui::CornerRadius::ZERO,
            closed_icon_bg,
        );
        neoflash_ui::components::paint_gradient_overflow_preview(
            ui.painter(),
            ui.ctx(),
            closed_preview,
            gradient_overflow_mode(spread),
        );
        if let Some(next_spread) = chosen
            && next_spread != spread
        {
            spread = next_spread;
            changed = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(Checkbox::new(&mut linear_rgb, "Linear RGB"))
                .changed()
            {
                changed = true;
            }
        });
    });
    changed.then_some((spread, linear_rgb))
}

pub(super) fn color_to_hsb(c: Color) -> Hsb {
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
}

/// Color panel compact state (R-055), shared with the mixers through egui temp data.
pub(super) fn color_picker_expanded() -> egui::Id {
    egui::Id::new("color-picker-expanded")
}
pub(super) fn color_picker_toggle() -> egui::Id {
    egui::Id::new("color-picker-toggle")
}
pub(super) fn color_recent() -> egui::Id {
    egui::Id::new("color-recent")
}

pub(super) fn color_mixer_ui(
    ui: &mut egui::Ui,
    hsb_cache_id: egui::Id,
    field_label: &str,
    color: &mut Color,
    model_hsb: &mut bool,
) -> Option<egui::Id> {
    let was_none = *color == [0, 0, 0, 0];
    let mut hsb = ui
        .data(|d| d.get_temp::<Hsb>(hsb_cache_id))
        .filter(|cached| egui::Color32::from(*cached).to_srgba_unmultiplied() == *color)
        .unwrap_or_else(|| color_to_hsb(*color));
    let mut changed_id = None;
    // Outside the Color panel (no flag set) the mixer keeps its full picker.
    let expanded = ui
        .data(|d| d.get_temp::<bool>(color_picker_expanded()))
        .unwrap_or(true);
    if !expanded {
        // Compact: hex · alpha with the picker toggle, then recent colours.
        let mut field_c32 =
            egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
        // The picker toggle leads the row, clear of the panel's overlay scroll bar.
        ui.horizontal(|ui| {
            if ui
                .add(neoflash_ui::components::IconButton::new(
                    neoflash_ui::icons::i16::CHEVRIGHT,
                    "Show color picker",
                ))
                .clicked()
            {
                ui.data_mut(|d| d.insert_temp(color_picker_toggle(), true));
            }
            let w = ui.available_width();
            let cf = ColorField::new(&mut field_c32, field_label)
                .width(w)
                .show(ui);
            if cf.swatch_clicked {
                ui.data_mut(|d| d.insert_temp(color_picker_toggle(), true));
            }
            if cf.response.changed() {
                let prev = *color;
                *color = field_c32.to_srgba_unmultiplied();
                if was_none && color[..3] != prev[..3] && color[3] == 0 {
                    color[3] = 255;
                }
                *color = ensure_solid_non_none(*color);
                changed_id = Some(cf.response.id);
            }
        });
        let recent: Vec<Color> = ui.data(|d| d.get_temp(color_recent())).unwrap_or_default();
        if !recent.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::s1(), space::s1());
                for c in recent {
                    let c32 = egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
                    let hex = format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
                    let r = neoflash_ui::components::tooltip(
                        ui.add(neoflash_ui::components::SwatchCell {
                            color: c32,
                            selected: c == *color,
                        }),
                        &hex,
                        None,
                    );
                    if r.clicked() {
                        *color = ensure_solid_non_none(c);
                        changed_id = Some(r.id);
                    }
                }
            });
        }
        ui.data_mut(|d| d.insert_temp(hsb_cache_id, color_to_hsb(*color)));
        return changed_id;
    }

    // Expanded (or outside the Color panel): the hex · alpha row stays on top, where the compact
    // state shows it, and the picker opens below it (R-055).
    let mut field_c32 =
        egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
    let in_color_panel = ui
        .data(|d| d.get_temp::<bool>(color_picker_expanded()))
        .is_some();
    ui.horizontal(|ui| {
        if in_color_panel
            && ui
                .add(neoflash_ui::components::IconButton::new(
                    neoflash_ui::icons::i16::CHEVDOWN,
                    "Hide color picker",
                ))
                .clicked()
        {
            ui.data_mut(|d| d.insert_temp(color_picker_toggle(), true));
        }
        let w = ui.available_width();
        let cf = ColorField::new(&mut field_c32, field_label)
            .width(w)
            .show(ui);
        if cf.response.changed() {
            let prev = *color;
            *color = field_c32.to_srgba_unmultiplied();
            if was_none && color[..3] != prev[..3] && color[3] == 0 {
                color[3] = 255;
            }
            *color = ensure_solid_non_none(*color);
            hsb = color_to_hsb(*color);
            changed_id = Some(cf.response.id);
        }
    });

    let before_hsb = hsb;
    let picker_r = ui.add(ColorPicker { color: &mut hsb });
    if picker_r.changed() {
        if was_none
            && before_hsb.a == 0.0
            && (hsb.h != before_hsb.h || hsb.s != before_hsb.s || hsb.v != before_hsb.v)
        {
            hsb.a = 1.0;
        }
        *color = ensure_solid_non_none(egui::Color32::from(hsb).to_srgba_unmultiplied());
        changed_id = Some(picker_r.id);
    }

    Segmented::show(ui, model_hsb, &[(false, "RGB"), (true, "HSB")], false);

    // Three-value row: three equal cells and two `gap/control` across the visible row.
    let third = ((crate::paint_controls::visible_row_width(ui) - 2.0 * space::s2()) / 3.0)
        .floor()
        .max(0.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::s2();
        if !*model_hsb {
            let mut r = f64::from(color[0]);
            let mut g = f64::from(color[1]);
            let mut b = f64::from(color[2]);
            let rr = ui.add(
                Field::new(&mut r, "Red")
                    .prefix("R")
                    .range(0.0..=255.0)
                    .decimals(0)
                    .width(third),
            );
            let rg = ui.add(
                Field::new(&mut g, "Green")
                    .prefix("G")
                    .range(0.0..=255.0)
                    .decimals(0)
                    .width(third),
            );
            let rb = ui.add(
                Field::new(&mut b, "Blue")
                    .prefix("B")
                    .range(0.0..=255.0)
                    .decimals(0)
                    .width(third),
            );
            for resp in [&rr, &rg, &rb] {
                if resp.changed() {
                    color[0] = r.round().clamp(0.0, 255.0) as u8;
                    color[1] = g.round().clamp(0.0, 255.0) as u8;
                    color[2] = b.round().clamp(0.0, 255.0) as u8;
                    if was_none && color[3] == 0 {
                        color[3] = 255;
                    }
                    *color = ensure_solid_non_none(*color);
                    hsb = color_to_hsb(*color);
                    changed_id = Some(resp.id);
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
                    .width(third),
            );
            let rs = ui.add(
                Field::new(&mut s_pct, "Saturation")
                    .prefix("S")
                    .unit("%")
                    .range(0.0..=100.0)
                    .decimals(0)
                    .width(third),
            );
            let rv = ui.add(
                Field::new(&mut v_pct, "Brightness")
                    .prefix("B")
                    .unit("%")
                    .range(0.0..=100.0)
                    .decimals(0)
                    .width(third),
            );
            for resp in [&rh, &rs, &rv] {
                if resp.changed() {
                    hsb.h = (h_deg.clamp(0.0, 360.0) / 360.0) as f32;
                    hsb.s = (s_pct.clamp(0.0, 100.0) / 100.0) as f32;
                    hsb.v = (v_pct.clamp(0.0, 100.0) / 100.0) as f32;
                    if was_none && hsb.a == 0.0 {
                        hsb.a = 1.0;
                    }
                    *color =
                        ensure_solid_non_none(egui::Color32::from(hsb).to_srgba_unmultiplied());
                    changed_id = Some(resp.id);
                }
            }
        }
    });

    ui.data_mut(|d| d.insert_temp(hsb_cache_id, hsb));
    changed_id
}

pub(super) fn gradient_stops_bar_ui(
    ui: &mut egui::Ui,
    grad: &mut GradientPaint,
    selected_stop: &mut usize,
) -> (Option<egui::Id>, bool) {
    let p = neoflash_ui::palette(ui.ctx());
    if grad.stops.is_empty() {
        return (None, false);
    }
    grad.stops.sort_by_key(|s| s.ratio);
    if *selected_stop >= grad.stops.len() {
        *selected_stop = grad.stops.len() - 1;
    }
    let width = ui.available_width().max(120.0);
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::hover());
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(width, 20.0));
    crate::paint_controls::paint_checkerboard(ui.painter(), bar.shrink(1.0), 4.0);

    // 64-slice mesh sampled through `GradientPaint::sample_at_unit` so the bar matches
    // sRGB / Linear RGB and multi-stop ratios accurately.
    let inner = bar.shrink(1.0);
    let mut mesh = egui::Mesh::default();
    const SLICES: usize = 64;
    for i in 0..=SLICES {
        let u = i as f64 / SLICES as f64;
        let x = egui::lerp(inner.left()..=inner.right(), i as f32 / SLICES as f32);
        let [r, g, b, a] = grad.sample_at_unit(u);
        let c = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
        mesh.colored_vertex(egui::pos2(x, inner.top()), c);
        mesh.colored_vertex(egui::pos2(x, inner.bottom()), c);
    }
    for i in 0..(SLICES as u32) {
        let b = i * 2;
        mesh.add_triangle(b, b + 1, b + 2);
        mesh.add_triangle(b + 1, b + 3, b + 2);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    ui.painter().rect_stroke(
        bar,
        egui::CornerRadius::same(neoflash_ui::tokens::radius::sm() as u8),
        egui::Stroke::new(1.0_f32, p.border_default),
        egui::StrokeKind::Inside,
    );

    let mut changed = false;
    // Register the rail first so a pointer over a stop hits that stop.
    let rail = ui.interact(rect, response.id.with("bar-or-rail"), egui::Sense::click());
    rail.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add gradient stop")
    });
    neoflash_ui::theme::focus_ring_inside(ui, &rail, neoflash_ui::tokens::radius::sm());
    let mut add_from_keyboard =
        rail.has_focus() && rail.clicked() && !rail.clicked_by(egui::PointerButton::Primary);
    let mut focused = rail.has_focus();
    let mut restore_stop_focus = add_from_keyboard;
    let mut remove_idx: Option<usize> = None;
    let mut move_target: Option<(usize, u8)> = None;

    for i in 0..grad.stops.len() {
        let stop = grad.stops[i].clone();
        let t = f32::from(stop.ratio) / 255.0;
        let x = bar.left() + t * bar.width();
        let sr = egui::Rect::from_center_size(
            egui::pos2(x, bar.bottom() + 10.0),
            egui::vec2(12.0, 16.0),
        );
        let id = response.id.with(("stop", i));
        let r = ui.interact(sr, id, egui::Sense::click_and_drag());
        r.widget_info(|| {
            egui::WidgetInfo::slider(
                true,
                f64::from(stop.ratio),
                format!("Gradient stop {}", i + 1),
            )
        });
        if r.has_focus() || r.clicked() || r.drag_started() || r.dragged() {
            *selected_stop = i;
        }
        if r.has_focus() {
            focused = true;
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        ..Default::default()
                    },
                )
            });
            ui.input_mut(|input| {
                use egui::{Key, Modifiers};
                let modifiers = input.modifiers;
                if modifiers != Modifiers::NONE && modifiers != Modifiers::SHIFT {
                    return;
                }
                let step = if modifiers.shift { 10 } else { 1 };
                let ratio = if input.consume_key(modifiers, Key::ArrowLeft) {
                    stop.ratio.saturating_sub(step)
                } else if input.consume_key(modifiers, Key::ArrowRight) {
                    stop.ratio.saturating_add(step)
                } else if input.consume_key(Modifiers::NONE, Key::Home) {
                    0
                } else if input.consume_key(Modifiers::NONE, Key::End) {
                    255
                } else {
                    stop.ratio
                };
                if ratio != stop.ratio {
                    move_target = Some((i, ratio));
                    restore_stop_focus = true;
                }
                if input.consume_key(Modifiers::NONE, Key::Enter)
                    || input.consume_key(Modifiers::NONE, Key::Insert)
                {
                    add_from_keyboard = true;
                    restore_stop_focus = true;
                }
                if (input.consume_key(Modifiers::NONE, Key::Backspace)
                    || input.consume_key(Modifiers::NONE, Key::Delete))
                    && grad.stops.len() > 2
                {
                    remove_idx = Some(i);
                    restore_stop_focus = true;
                }
            });
        }
        if r.dragged()
            && let Some(pos) = r.interact_pointer_pos()
        {
            if (pos.y - bar.bottom()).abs() > 36.0 && grad.stops.len() > 2 {
                remove_idx = Some(i);
            } else {
                let next_t = ((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
                let next_ratio = (next_t * 255.0).round() as u8;
                if next_ratio != stop.ratio {
                    move_target = Some((i, next_ratio));
                }
            }
        }
        let sel = i == *selected_stop;
        let border = if sel {
            p.accent_default
        } else {
            p.border_strong
        };
        let top = egui::pos2(x, bar.bottom() + 2.0);
        let bl = egui::pos2(sr.left(), bar.bottom() + 7.0);
        let br = egui::pos2(sr.right(), bar.bottom() + 7.0);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![top, br, bl],
            p.bg_raised,
            egui::Stroke::new(if sel { 1.5_f32 } else { 1.0_f32 }, border),
        ));
        let body = egui::Rect::from_min_max(bl, sr.max);
        ui.painter()
            .rect_filled(body, egui::CornerRadius::same(2), p.bg_raised);
        let c32 = egui::Color32::from_rgba_unmultiplied(
            stop.color[0],
            stop.color[1],
            stop.color[2],
            stop.color[3],
        );
        ui.painter().rect_filled(
            egui::Rect::from_center_size(body.center(), egui::vec2(6.0, 6.0)),
            egui::CornerRadius::same(1),
            c32,
        );
        ui.painter().rect_stroke(
            body,
            egui::CornerRadius::same(2),
            egui::Stroke::new(if sel { 1.5_f32 } else { 1.0_f32 }, border),
            egui::StrokeKind::Inside,
        );
        neoflash_ui::theme::focus_ring_inside(ui, &r, neoflash_ui::tokens::radius::sm());
    }

    if let Some((idx, next_ratio)) = move_target {
        grad.stops[idx].ratio = next_ratio;
        let mut tagged: Vec<(bool, GradientStop)> = grad
            .stops
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, s)| (i == idx, s))
            .collect();
        tagged.sort_by_key(|(_, s)| s.ratio);
        if let Some(new_sel) = tagged.iter().position(|(moved, _)| *moved) {
            *selected_stop = new_sel;
        }
        grad.stops = tagged.into_iter().map(|(_, s)| s).collect();
        changed = true;
    } else if let Some(idx) = remove_idx {
        grad.stops.remove(idx);
        *selected_stop = (*selected_stop).min(grad.stops.len().saturating_sub(1));
        changed = true;
    } else if grad.stops.len() < 15 {
        let t = if add_from_keyboard {
            // Insert on the following span, or the preceding span at the last stop.
            let index = *selected_stop;
            let neighbour = grad
                .stops
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|i| grad.stops.get(i)));
            let ratio = f64::from(grad.stops[index].ratio);
            Some((ratio + neighbour.map_or(255.0, |s| f64::from(s.ratio))) / 510.0)
        } else if rail.clicked_by(egui::PointerButton::Primary) {
            rail.interact_pointer_pos()
                .map(|p| f64::from(((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0)))
        } else {
            None
        };
        if let Some(t) = t {
            let ratio = (t * 255.0).round() as u8;
            let color = grad.sample_at_unit(t);
            let index = grad.stops.partition_point(|s| s.ratio <= ratio);
            grad.stops.insert(index, GradientStop { ratio, color });
            *selected_stop = index;
            changed = true;
        }
    }

    if changed {
        if restore_stop_focus {
            ui.memory_mut(|m| m.request_focus(response.id.with(("stop", *selected_stop))));
        }
        response.mark_changed();
        return (Some(response.id), focused);
    }
    (None, focused)
}

pub(super) fn stroke_section_ui(
    ui: &mut egui::Ui,
    width: &mut f64,
    with_divider: bool,
) -> Option<egui::Id> {
    let nf = neoflash_ui::palette(ui.ctx());
    if with_divider {
        ui.add_space(space::s1());
        let r = ui.available_rect_before_wrap();
        ui.painter().hline(
            r.x_range(),
            r.top(),
            egui::Stroke::new(1.0_f32, nf.border_subtle),
        );
        ui.add_space(space::s2());
    }
    ui.label(rich("Stroke", TextStyle::Overline).color(nf.text_tertiary));
    let mut changed = None;
    let (width_response, _) = two_column_row(
        ui,
        |ui, cell_width| {
            ui.add(
                Field::new(width, "Stroke width")
                    .prefix("Width")
                    .unit("px")
                    .range(0.0..=100.0)
                    .decimals(1)
                    .width(cell_width),
            )
        },
        |ui, cell_width| {
            ui.add_enabled_ui(false, |ui| {
                Select::new("color-stroke-style", "Solid")
                    .width(cell_width)
                    .show(ui, |_| {});
            })
        },
    );
    if width_response.changed() {
        changed = Some(width_response.id);
    }
    changed
}
