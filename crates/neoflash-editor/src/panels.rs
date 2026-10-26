use crate::app::*;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use neoflash_core::*;

impl Editor {
    pub fn menu(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu")
            .exact_height(32.)
            .show(ctx, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.strong("NeoFlash");
                    ui.separator();
                    ui.menu_button("File", |ui| {
                        if ui.button("New                         ⌘N").clicked() {
                            self.request(Pending::New, ctx);
                            ui.close();
                        }
                        if ui.button("Open…                      ⌘O").clicked() {
                            self.request(Pending::Open, ctx);
                            ui.close();
                        }
                        if ui.button("Import legacy Flash…").clicked() {
                            self.request(Pending::Import, ctx);
                            ui.close();
                        }
                        if ui.button("Save                         ⌘S").clicked() {
                            self.save(false);
                            ui.close();
                        }
                        if ui.button("Save As…").clicked() {
                            self.save(true);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Publish HTML5…").clicked() {
                            self.export(false);
                            ui.close();
                        }
                        if ui.button("Open example").clicked() {
                            self.request(Pending::Demo, ctx);
                            ui.close();
                        }
                    });
                    ui.menu_button("Edit", |ui| {
                        if ui
                            .add_enabled(
                                self.history.can_undo() || self.vector.draft.is_some(),
                                egui::Button::new("Undo     ⌘Z"),
                            )
                            .clicked()
                        {
                            self.undo();
                            ui.close();
                        }
                        if ui
                            .add_enabled(self.history.can_redo(), egui::Button::new("Redo     ⇧⌘Z"))
                            .clicked()
                        {
                            self.redo();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Select All     ⌘A").clicked() {
                            self.select_all();
                            ui.close();
                        }
                        if ui
                            .add_enabled(!self.selection.is_empty(), egui::Button::new("Deselect"))
                            .clicked()
                        {
                            self.finish_edits();
                            self.clear_selection();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                !self.selection.is_empty(),
                                egui::Button::new("Duplicate     ⌘D"),
                            )
                            .clicked()
                        {
                            self.duplicate();
                            ui.close();
                        }
                        if ui
                            .add_enabled(!self.selection.is_empty(), egui::Button::new("Delete"))
                            .clicked()
                        {
                            self.delete_selected();
                            ui.close();
                        }
                    });
                    ui.menu_button("Insert", |ui| {
                        if ui.button("Frame                  F5").clicked() {
                            self.finish_edits();
                            self.insert_frame();
                            ui.close();
                        }
                        if ui.button("Keyframe             F6").clicked() {
                            self.finish_edits();
                            self.make_key(false);
                            ui.close();
                        }
                        if ui.button("Blank Keyframe   F7").clicked() {
                            self.finish_edits();
                            self.make_key(true);
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.selection.len() == 1,
                                egui::Button::new("Convert to Symbol…  F8"),
                            )
                            .clicked()
                        {
                            self.convert_to_symbol();
                            ui.close();
                        }
                    });
                    ui.menu_button("Control", |ui| {
                        if ui
                            .button(if self.playing {
                                "Stop    Enter"
                            } else {
                                "Play    Enter"
                            })
                            .clicked()
                        {
                            self.set_playing(!self.playing);
                            ui.close();
                        }
                        if ui.button("Test Movie    ⌘Enter").clicked() {
                            self.export(true);
                            ui.close();
                        }
                        if ui.button("Rewind").clicked() {
                            self.finish_edits();
                            self.timeline_key_drag = None;
                            self.frame = 0;
                            ui.close();
                        }
                    });
                    ui.menu_button("Window", |ui| {
                        for (name, tab) in [
                            ("Properties", BottomTab::Properties),
                            ("Actions    F9", BottomTab::Actions),
                            ("Output", BottomTab::Output),
                        ] {
                            if ui.button(name).clicked() {
                                self.bottom = tab;
                                ui.close();
                            }
                        }
                        if ui.button("About NeoFlash").clicked() {
                            self.show_about = true;
                            ui.close();
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Test Movie  ↗").clicked() {
                            self.export(true);
                        }
                        ui.label(
                            egui::RichText::new("CLASSIC WORKSPACE")
                                .small()
                                .color(Color32::from_gray(105)),
                        );
                    });
                });
            });
    }
    pub fn timeline(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("timeline")
            .default_height(207.)
            .min_height(130.)
            .max_height(380.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Timeline");
                    ui.separator();
                    let crumbs = self.editing.clone();
                    for (index, id) in crumbs.iter().enumerate() {
                        let name = self
                            .project
                            .symbol(id)
                            .map(|s| s.name.clone())
                            .unwrap_or_default();
                        if index > 0 {
                            ui.label("›");
                        }
                        if ui
                            .selectable_label(index + 1 == crumbs.len(), name)
                            .clicked()
                        {
                            self.finish_edits();
                            self.timeline_key_drag = None;
                            self.editing.truncate(index + 1);
                            self.frame = 0;
                            self.layer = 0;
                            self.clear_selection();
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("{} fps", self.project.fps));
                        ui.label(format!(
                            "{:.2} s",
                            self.frame as f64 / self.project.fps as f64
                        ));
                        ui.label(format!(
                            "Frame {} / {}",
                            self.frame + 1,
                            self.symbol().timeline.frame_count
                        ));
                    });
                });
                ui.separator();
                let height = (ui.available_height() - 35.).max(65.);
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(210., height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Layers");
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.small("lock / show");
                                    },
                                );
                            });
                            let layers = self.symbol().timeline.layers.clone();
                            for (idx, layer) in layers.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(idx == self.layer, &layer.name)
                                        .clicked()
                                    {
                                        self.finish_edits();
                                        self.timeline_key_drag = None;
                                        self.layer = idx;
                                        self.clear_selection();
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .small_button(if layer.locked { "L" } else { "·" })
                                                .on_hover_text("Lock layer")
                                                .clicked()
                                            {
                                                self.finish_edits();
                                                self.transact(|s| {
                                                    s.symbol_mut().timeline.layers[idx].locked =
                                                        !layer.locked
                                                });
                                            }
                                            if ui
                                                .small_button(if layer.visible {
                                                    "●"
                                                } else {
                                                    "○"
                                                })
                                                .on_hover_text("Show layer")
                                                .clicked()
                                            {
                                                self.finish_edits();
                                                self.transact(|s| {
                                                    s.symbol_mut().timeline.layers[idx].visible =
                                                        !layer.visible
                                                });
                                            }
                                        },
                                    );
                                });
                            }
                        },
                    );
                    egui::ScrollArea::horizontal()
                        .id_salt("frames")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let count = self.symbol().timeline.frame_count;
                            let width = (count.max(72) as f32 * self.timeline_cell)
                                .max(ui.available_width());
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(width, height),
                                Sense::click_and_drag(),
                            );
                            let painter = ui.painter_at(rect);
                            let row = 25.;
                            let ruler = 25.;
                            painter.rect_filled(rect, 0, Color32::from_gray(249));
                            let first = (((ui.clip_rect().left() - rect.left())
                                / self.timeline_cell)
                                .max(0.) as u32)
                                .min(count);
                            let last = (((ui.clip_rect().right() - rect.left())
                                / self.timeline_cell)
                                .ceil()
                                .max(0.) as u32)
                                .min(count.max(72));
                            for f in first..last {
                                let x = rect.left() + f as f32 * self.timeline_cell;
                                let color = if f == self.frame {
                                    Color32::from_rgb(174, 206, 235)
                                } else if f % 5 == 4 {
                                    Color32::from_gray(234)
                                } else {
                                    Color32::from_gray(248)
                                };
                                painter.rect_filled(
                                    Rect::from_min_size(
                                        Pos2::new(x, rect.top()),
                                        Vec2::new(self.timeline_cell, height),
                                    ),
                                    0,
                                    color,
                                );
                                painter.line_segment(
                                    [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                                    Stroke::new(0.5_f32, Color32::from_gray(207)),
                                );
                                if f == 0 || (f + 1) % 5 == 0 {
                                    painter.text(
                                        Pos2::new(x + self.timeline_cell / 2., rect.top() + 12.),
                                        Align2::CENTER_CENTER,
                                        (f + 1).to_string(),
                                        FontId::proportional(10.),
                                        Color32::from_gray(70),
                                    );
                                }
                            }
                            let layers = self.symbol().timeline.layers.clone();
                            for (li, layer) in layers.iter().enumerate() {
                                let y = rect.top() + ruler + li as f32 * row;
                                if y > rect.bottom() {
                                    break;
                                }
                                if li == self.layer {
                                    painter.rect_filled(
                                        Rect::from_min_size(
                                            Pos2::new(rect.left(), y),
                                            Vec2::new(width, row),
                                        ),
                                        0,
                                        Color32::from_rgba_unmultiplied(90, 151, 215, 28),
                                    );
                                }
                                for (ki, key) in layer.keyframes.iter().enumerate() {
                                    let next = layer
                                        .keyframes
                                        .get(ki + 1)
                                        .map(|k| k.frame)
                                        .unwrap_or(count);
                                    if next < first || key.frame > last {
                                        continue;
                                    }
                                    let x = rect.left() + key.frame as f32 * self.timeline_cell;
                                    if key.tween.is_some() && ki + 1 < layer.keyframes.len() {
                                        let end = rect.left() + next as f32 * self.timeline_cell;
                                        painter.rect_filled(
                                            Rect::from_min_max(
                                                Pos2::new(x, y + 1.),
                                                Pos2::new(end, y + row - 1.),
                                            ),
                                            0,
                                            Color32::from_rgb(203, 204, 238),
                                        );
                                        if end - x > 25. {
                                            painter.arrow(
                                                Pos2::new(x + 12., y + row / 2.),
                                                Vec2::new(end - x - 20., 0.),
                                                Stroke::new(
                                                    1.0_f32,
                                                    Color32::from_rgb(82, 77, 113),
                                                ),
                                            );
                                        }
                                    }
                                    let center =
                                        Pos2::new(x + self.timeline_cell / 2., y + row / 2.);
                                    painter.circle(
                                        center,
                                        3.,
                                        if key.elements.is_empty() {
                                            Color32::WHITE
                                        } else {
                                            Color32::from_gray(50)
                                        },
                                        Stroke::new(1.0_f32, Color32::from_gray(50)),
                                    );
                                    if !key.label.is_empty() {
                                        painter.text(
                                            Pos2::new(x + 8., y + 2.),
                                            Align2::LEFT_TOP,
                                            &key.label,
                                            FontId::proportional(9.),
                                            Color32::from_rgb(145, 40, 37),
                                        );
                                    }
                                }
                                painter.line_segment(
                                    [
                                        Pos2::new(rect.left(), y + row),
                                        Pos2::new(rect.right(), y + row),
                                    ],
                                    Stroke::new(1.0_f32, Color32::from_gray(190)),
                                );
                            }
                            for script in &self.symbol().timeline.scripts {
                                let x = rect.left() + script.frame as f32 * self.timeline_cell;
                                painter.text(
                                    Pos2::new(x + self.timeline_cell / 2., rect.top() + ruler + 3.),
                                    Align2::CENTER_TOP,
                                    "a",
                                    FontId::monospace(9.),
                                    Color32::from_rgb(40, 70, 150),
                                );
                            }
                            let playx = rect.left()
                                + self.frame as f32 * self.timeline_cell
                                + self.timeline_cell / 2.;
                            painter.line_segment(
                                [
                                    Pos2::new(playx, rect.top()),
                                    Pos2::new(playx, rect.bottom()),
                                ],
                                Stroke::new(1.5_f32, Color32::from_rgb(191, 44, 49)),
                            );
                            painter.rect_filled(
                                Rect::from_min_size(
                                    Pos2::new(playx - 4., rect.top()),
                                    Vec2::new(8., 7.),
                                ),
                                0,
                                Color32::from_rgb(191, 44, 49),
                            );
                            if (response.clicked() || response.dragged())
                                && let Some(pos) = response.interact_pointer_pos()
                            {
                                self.finish_edits();
                                if self.playing {
                                    self.set_playing(false);
                                }
                                self.frame = (((pos.x - rect.left()) / self.timeline_cell)
                                    .floor()
                                    .max(0.) as u32)
                                    .min(count - 1);
                                if pos.y > rect.top() + ruler {
                                    self.layer =
                                        (((pos.y - rect.top() - ruler) / row).floor().max(0.)
                                            as usize)
                                            .min(layers.len() - 1);
                                }
                                self.clear_selection();
                            }
                            if response.drag_started()
                                && let Some(pos) = ui.input(|i| i.pointer.press_origin())
                            {
                                let f = (((pos.x - rect.left()) / self.timeline_cell)
                                    .floor()
                                    .max(0.) as u32)
                                    .min(count - 1);
                                if pos.y >= rect.top() + ruler {
                                    let li = (((pos.y - rect.top() - ruler) / row).floor().max(0.)
                                        as usize)
                                        .min(layers.len() - 1);
                                    if layers[li].keyframes.iter().any(|k| k.frame == f) {
                                        self.timeline_key_drag = Some((
                                            self.symbol_id().to_owned(),
                                            layers[li].id.clone(),
                                            f,
                                        ));
                                    }
                                }
                            }
                            if response.drag_stopped()
                                && let Some((symbol, layer_id, from)) =
                                    self.timeline_key_drag.take()
                                && symbol == self.symbol_id()
                                && let Some(li) = self
                                    .symbol()
                                    .timeline
                                    .layers
                                    .iter()
                                    .position(|layer| layer.id == layer_id)
                            {
                                let release = ui
                                    .input(|input| {
                                        input.events.iter().rev().find_map(|event| match event {
                                            egui::Event::PointerButton {
                                                pos,
                                                button: egui::PointerButton::Primary,
                                                pressed: false,
                                                ..
                                            } => Some(*pos),
                                            _ => None,
                                        })
                                    })
                                    .or_else(|| response.interact_pointer_pos())
                                    .or_else(|| ui.input(|input| input.pointer.latest_pos()));
                                let destination = release.map_or(self.frame, |pos| {
                                    (((pos.x - rect.left()) / self.timeline_cell).floor().max(0.)
                                        as u32)
                                        .min(count - 1)
                                });
                                self.move_key(li, from, destination, ui.input(|i| i.modifiers.alt));
                            }
                            response.context_menu(|ui| {
                                if ui.button("Insert Keyframe   F6").clicked() {
                                    self.finish_edits();
                                    self.make_key(false);
                                    ui.close();
                                }
                                if ui.button("Insert Blank Keyframe   F7").clicked() {
                                    self.finish_edits();
                                    self.make_key(true);
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        self.layer_editable(),
                                        egui::Button::new("Create Classic Tween"),
                                    )
                                    .clicked()
                                {
                                    self.finish_edits();
                                    self.transact(|s| {
                                        let f = s.frame;
                                        let l = s.layer;
                                        if let Some(k) =
                                            s.symbol_mut().timeline.layers[l].key_at_mut(f)
                                        {
                                            k.tween = Some(Easing::EaseInOut);
                                        }
                                    });
                                    ui.close();
                                }
                                if ui.button("Actions   F9").clicked() {
                                    self.bottom = BottomTab::Actions;
                                    self.script_target = ScriptTarget::Frame;
                                    ui.close();
                                }
                            });
                            if self.onion {
                                let (start, end) = self.onion_range();
                                let left = rect.left() + start as f32 * self.timeline_cell;
                                let right = rect.left() + (end + 1) as f32 * self.timeline_cell;
                                painter.line_segment(
                                    [
                                        Pos2::new(left, rect.top() + 22.),
                                        Pos2::new(right, rect.top() + 22.),
                                    ],
                                    Stroke::new(3.0_f32, Color32::from_rgb(54, 123, 170)),
                                );
                                for (is_start, x) in [(true, left), (false, right)] {
                                    let r = Rect::from_center_size(
                                        Pos2::new(x, rect.top() + 18.),
                                        Vec2::new(9., 14.),
                                    );
                                    painter.rect_filled(r, 1, Color32::from_rgb(54, 123, 170));
                                    let handle = ui.interact(
                                        r,
                                        ui.id().with(("onion", is_start)),
                                        Sense::drag(),
                                    );
                                    if handle.dragged()
                                        && let Some(p) = handle.interact_pointer_pos()
                                    {
                                        let f = (((p.x - rect.left()) / self.timeline_cell)
                                            .round()
                                            .max(0.)
                                            as u32)
                                            .min(count - 1);
                                        if self.onion_anchor {
                                            if is_start {
                                                self.anchored_range.0 = f.min(end);
                                            } else {
                                                self.anchored_range.1 = f.max(start);
                                            }
                                        } else if is_start {
                                            self.onion_before = self.frame.saturating_sub(f);
                                        } else {
                                            self.onion_after = f.saturating_sub(self.frame);
                                        }
                                    }
                                }
                            }
                        });
                });
                ui.horizontal(|ui| {
                    if ui.small_button("+ Layer").clicked() {
                        self.finish_edits();
                        self.transact(|s| {
                            let n = s.symbol().timeline.layers.len() + 1;
                            s.symbol_mut().timeline.layers.push(Layer {
                                id: new_id("layer"),
                                name: format!("Layer {n}"),
                                visible: true,
                                locked: false,
                                keyframes: vec![Keyframe {
                                    frame: 0,
                                    elements: vec![],
                                    tween: None,
                                    label: String::new(),
                                }],
                            });
                            s.layer = s.symbol().timeline.layers.len() - 1;
                        });
                    }
                    if ui
                        .add_enabled(
                            self.symbol().timeline.layers.len() > 1,
                            egui::Button::new("−").small(),
                        )
                        .on_hover_text("Delete selected layer")
                        .clicked()
                    {
                        self.finish_edits();
                        self.transact(|s| {
                            let l = s.layer;
                            s.symbol_mut().timeline.layers.remove(l);
                            s.normalize_selection();
                        });
                    }
                    ui.separator();
                    if ui.small_button("|◀").clicked() {
                        self.finish_edits();
                        self.timeline_key_drag = None;
                        self.frame = 0;
                    }
                    if ui
                        .small_button(if self.playing { "Ⅱ" } else { "▶" })
                        .clicked()
                    {
                        self.set_playing(!self.playing);
                    }
                    ui.separator();
                    ui.checkbox(&mut self.onion, "Onion Skin");
                    ui.checkbox(&mut self.onion_outlines, "Outlines");
                    ui.label("Before");
                    ui.add(egui::DragValue::new(&mut self.onion_before).range(0..=120));
                    ui.label("After");
                    ui.add(egui::DragValue::new(&mut self.onion_after).range(0..=120));
                    if ui.checkbox(&mut self.onion_anchor, "Anchor").changed() {
                        self.anchored_range = (
                            self.frame.saturating_sub(self.onion_before),
                            (self.frame + self.onion_after)
                                .min(self.symbol().timeline.frame_count - 1),
                        );
                    }
                    ui.add(
                        egui::Slider::new(&mut self.onion_opacity, 0.05..=0.7)
                            .text("Opacity")
                            .show_value(false),
                    );
                });
            });
    }
    pub fn onion_range(&self) -> (u32, u32) {
        if self.onion_anchor {
            self.anchored_range
        } else {
            (
                self.frame.saturating_sub(self.onion_before),
                (self.frame + self.onion_after).min(self.symbol().timeline.frame_count - 1),
            )
        }
    }
    pub fn tools(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("tools")
            .exact_width(58.)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(8.);
                ui.small("Tools");
                ui.separator();
                for (tool, label, hint) in [
                    (Tool::Select, "↖", "Selection (V)"),
                    (
                        Tool::Transform,
                        "Q",
                        "Free Transform — resize and rotate (Q)",
                    ),
                    (Tool::Subselect, "◇", "Subselection — edit path points (A)"),
                    (Tool::Pen, "P", "Pen — click corners, drag curves (P)"),
                    (Tool::Pencil, "✎", "Pencil (Y)"),
                    (Tool::Rectangle, "▭", "Rectangle (R)"),
                    (Tool::Ellipse, "○", "Oval (O)"),
                    (Tool::Text, "T", "Text (T)"),
                ] {
                    if ui
                        .add_sized(
                            [36., 34.],
                            egui::Button::new(egui::RichText::new(label).size(22.))
                                .selected(self.tool == tool),
                        )
                        .on_hover_text(hint)
                        .clicked()
                    {
                        self.set_tool(tool);
                    }
                }
                ui.separator();
                ui.small("Fill");
                ui.color_edit_button_srgba_unmultiplied(&mut self.fill);
                ui.add_space(14.);
                ui.small("View");
                if ui.button("+").on_hover_text("Zoom in").clicked() {
                    self.finish_edits();
                    self.zoom = (self.zoom * 1.25).min(4.);
                }
                if ui.button("−").on_hover_text("Zoom out").clicked() {
                    self.finish_edits();
                    self.zoom = (self.zoom / 1.25).max(0.1);
                }
                if ui.button("Fit").clicked() {
                    self.finish_edits();
                    self.zoom = 0.;
                    self.pan = Vec2::ZERO;
                }
            });
    }
    pub fn library(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("library")
            .default_width(228.)
            .min_width(190.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add_space(4.);
                ui.strong("Library");
                ui.separator();
                ui.small(format!(
                    "{} symbols",
                    self.project.symbols.len().saturating_sub(1)
                ));
                let symbols = self.project.symbols.clone();
                let root = self.project.root.clone();
                for symbol in symbols.iter().filter(|s| s.id != root) {
                    let response = ui.selectable_label(
                        self.library_selected.as_ref() == Some(&symbol.id),
                        format!("◇  {}", symbol.name),
                    );
                    if response.clicked() {
                        self.library_selected = Some(symbol.id.clone());
                    }
                    if response.double_clicked() {
                        self.enter_symbol(symbol.id.clone());
                    }
                }
                ui.add_space(12.);
                if let Some(id) = self.library_selected.clone() {
                    if let Some(symbol) = self.project.symbol(&id) {
                        ui.label(format!(
                            "{:?} · {} frames",
                            symbol.kind, symbol.timeline.frame_count
                        ));
                    }
                    if ui.button("Place instance on stage").clicked() {
                        if id == self.symbol_id() {
                            self.message("A symbol cannot contain itself.");
                        } else {
                            self.finish_edits();
                            self.add_element(
                                ElementKind::Instance {
                                    symbol_id: id,
                                    playback: InstancePlayback::Independent,
                                },
                                120.,
                                120.,
                            );
                        }
                    }
                    if ui.button("Edit symbol").clicked() {
                        self.enter_symbol(self.library_selected.clone().unwrap());
                    }
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.selection.len() == 1,
                        egui::Button::new("Convert to Symbol   F8"),
                    )
                    .clicked()
                {
                    self.convert_to_symbol();
                }
                ui.add_space(18.);
                ui.strong("Document");
                ui.separator();
                ui.label(&self.project.name);
                ui.small(format!(
                    "{} × {} px · {} fps",
                    self.project.width, self.project.height, self.project.fps
                ));
                ui.small("Target: HTML5");
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.small("Double-click a symbol to edit its timeline.");
                });
            });
    }
    pub fn bottom_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("properties")
            .default_height(190.)
            .min_height(145.)
            .max_height(420.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.bottom, BottomTab::Properties, "Properties");
                    ui.selectable_value(&mut self.bottom, BottomTab::Actions, "Actions");
                    ui.selectable_value(
                        &mut self.bottom,
                        BottomTab::Accessibility,
                        "Accessibility",
                    );
                    ui.selectable_value(&mut self.bottom, BottomTab::Output, "Output");
                });
                ui.separator();
                match self.bottom {
                    BottomTab::Properties => self.properties(ui, ctx),
                    BottomTab::Actions => self.actions(ui),
                    BottomTab::Accessibility => self.accessibility_panel(ui),
                    BottomTab::Output => {
                        egui::ScrollArea::vertical()
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                for line in &self.output {
                                    ui.monospace(line);
                                }
                            });
                    }
                }
            });
    }
    fn properties(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.selection.len() > 1 {
            ui.strong(format!("{} objects selected", self.selection.len()));
            ui.label("Select one object to edit its properties.");
            ui.small("V: move the selection · Q: scale and rotate · Shift-click: add or remove an object");
            return;
        }
        if let Some(mut element) = self.selected_sampled() {
            let original = element.clone();
            let mut edit_symbol = None;
            ui.add_enabled_ui(self.selected_editable(), |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Instance / object");
                    ui.label("Name");
                    ui.text_edit_singleline(&mut element.name);
                    ui.separator();
                    ui.label("Opacity");
                    ui.add(
                        egui::DragValue::new(&mut element.transform.alpha)
                            .range(0.0..=1.0)
                            .speed(0.01),
                    );
                });
                ui.horizontal(|ui| {
                    for (name, value) in [
                        ("X", &mut element.transform.x),
                        ("Y", &mut element.transform.y),
                        ("Scale X", &mut element.transform.scale_x),
                        ("Scale Y", &mut element.transform.scale_y),
                        ("Rotation", &mut element.transform.rotation),
                    ] {
                        ui.label(name);
                        ui.add(
                            egui::DragValue::new(value)
                                .speed(if name.starts_with("Scale") { 0.01 } else { 1. })
                                .max_decimals(2),
                        );
                    }
                });
                match &mut element.kind {
                    ElementKind::Rectangle {
                        width,
                        height,
                        fill,
                        stroke,
                        stroke_width,
                    }
                    | ElementKind::Ellipse {
                        width,
                        height,
                        fill,
                        stroke,
                        stroke_width,
                    } => {
                        ui.horizontal(|ui| {
                            ui.label("Width");
                            ui.add(egui::DragValue::new(width).range(1.0..=10000.));
                            ui.label("Height");
                            ui.add(egui::DragValue::new(height).range(1.0..=10000.));
                            ui.label("Fill");
                            ui.color_edit_button_srgba_unmultiplied(fill);
                            ui.label("Stroke");
                            ui.color_edit_button_srgba_unmultiplied(stroke);
                            ui.add(egui::DragValue::new(stroke_width).range(0.0..=100.));
                        });
                    }
                    ElementKind::Text {
                        text,
                        font_family,
                        font_size,
                        color,
                    } => {
                        ui.horizontal(|ui| {
                            ui.label("Text");
                            ui.text_edit_singleline(text);
                            ui.label("Size");
                            ui.add(egui::DragValue::new(font_size).range(4.0..=512.));
                            ui.color_edit_button_srgba_unmultiplied(color);
                            egui::ComboBox::from_id_salt("font_family")
                                .selected_text(font_family.as_str())
                                .width(180.)
                                .show_ui(ui, |ui| {
                                    ui.text_edit_singleline(&mut self.font_filter);
                                    for family in &self.fonts.families {
                                        if self.font_filter.is_empty()
                                            || family
                                                .to_lowercase()
                                                .contains(&self.font_filter.to_lowercase())
                                        {
                                            ui.selectable_value(
                                                font_family,
                                                family.clone(),
                                                family,
                                            );
                                        }
                                    }
                                });
                        });
                        if let Err(e) = self.fonts.load(ctx, font_family) {
                            ui.colored_label(Color32::DARK_RED, e);
                        }
                    }
                    ElementKind::Path {
                        commands,
                        fill_rule,
                        fill,
                        stroke,
                        stroke_width,
                    } => {
                        ui.horizontal(|ui| {
                            ui.label(format!("Vector path · {} commands", commands.len()));
                            ui.label("Fill");
                            ui.color_edit_button_srgba_unmultiplied(fill);
                            ui.label("Stroke");
                            ui.color_edit_button_srgba_unmultiplied(stroke);
                            ui.add(egui::DragValue::new(stroke_width).range(0.0..=100.));
                            egui::ComboBox::from_id_salt("fill_rule")
                                .selected_text(if *fill_rule == FillRule::EvenOdd {
                                    "Even–odd"
                                } else {
                                    "Nonzero"
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(fill_rule, FillRule::NonZero, "Nonzero");
                                    ui.selectable_value(fill_rule, FillRule::EvenOdd, "Even–odd");
                                });
                            if ui.button("Edit points (A)").clicked() {
                                self.set_tool(Tool::Subselect);
                                self.select_single_id(original.id.clone());
                            }
                        });
                    }
                    ElementKind::Instance {
                        symbol_id,
                        playback,
                    } => {
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "Symbol: {}",
                                self.project
                                    .symbol(symbol_id)
                                    .map(|s| s.name.as_str())
                                    .unwrap_or("Missing")
                            ));
                            egui::ComboBox::from_id_salt("playback")
                                .selected_text(format!("{playback:?}"))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        playback,
                                        InstancePlayback::Independent,
                                        "MovieClip · independent",
                                    );
                                    ui.selectable_value(
                                        playback,
                                        InstancePlayback::Loop,
                                        "Graphic · loop",
                                    );
                                    ui.selectable_value(
                                        playback,
                                        InstancePlayback::PlayOnce,
                                        "Graphic · play once",
                                    );
                                    ui.selectable_value(
                                        playback,
                                        InstancePlayback::SingleFrame(0),
                                        "Graphic · single frame",
                                    );
                                });
                            if ui.button("Edit").clicked() {
                                edit_symbol = Some(symbol_id.clone());
                            }
                        });
                    }
                }
            });
            if original != element {
                self.finish_edits();
                self.select_single_id(original.id.clone());
                self.transact(|s| {
                    s.ensure_selected_key();
                    if let Some(e) = s.selected_element_mut() {
                        *e = element
                    }
                });
            }
            if let Some(id) = edit_symbol {
                self.enter_symbol(id);
            }
        } else {
            // Edit a draft so finishing an active gesture precedes any document mutation.
            // Apply only these fields: finishing a pen path can add new elements and keys.
            let mut document = self.project.clone();
            let symbol_id = self.symbol_id().to_owned();
            let layer_id = self.symbol().timeline.layers[self.layer].id.clone();
            let layer = self.layer;
            let frame = self.frame;
            ui.horizontal(|ui| {
                ui.strong("Document");
                ui.text_edit_singleline(&mut document.name);
                ui.label("Size");
                ui.add(egui::DragValue::new(&mut document.width).range(1..=8192));
                ui.label("×");
                ui.add(egui::DragValue::new(&mut document.height).range(1..=8192));
                ui.label("FPS");
                ui.add(egui::DragValue::new(&mut document.fps).range(1..=120));
                ui.label("Background");
                ui.color_edit_button_srgba_unmultiplied(&mut document.background);
            });
            let timeline = &mut document.symbol_mut(&symbol_id).unwrap().timeline;
            ui.horizontal(|ui| {
                ui.label("Layer");
                ui.text_edit_singleline(&mut timeline.layers[layer].name);
                let min = timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .map(|k| k.frame + 1)
                    .chain(timeline.scripts.iter().map(|s| s.frame + 1))
                    .max()
                    .unwrap_or(1);
                ui.label("Frames");
                ui.add(egui::DragValue::new(&mut timeline.frame_count).range(min..=10000));
                ui.add_enabled_ui(self.layer_editable(), |ui| {
                    if let Some(key) = timeline.layers[layer].key_at_mut(frame) {
                        ui.label("Label");
                        ui.text_edit_singleline(&mut key.label);
                        egui::ComboBox::from_id_salt("tween")
                            .selected_text(match key.tween {
                                None => "No tween",
                                Some(Easing::Linear) => "Linear",
                                Some(Easing::EaseIn) => "Ease in",
                                Some(Easing::EaseOut) => "Ease out",
                                Some(Easing::EaseInOut) => "Ease in/out",
                                Some(Easing::CubicBezier { .. }) => "Custom curve",
                            })
                            .show_ui(ui, |ui| {
                                for (label, value) in [
                                    ("No tween", None),
                                    ("Linear", Some(Easing::Linear)),
                                    ("Ease in", Some(Easing::EaseIn)),
                                    ("Ease out", Some(Easing::EaseOut)),
                                    ("Ease in/out", Some(Easing::EaseInOut)),
                                    (
                                        "Custom curve",
                                        Some(Easing::CubicBezier {
                                            x1: 0.25,
                                            y1: 0.1,
                                            x2: 0.25,
                                            y2: 1.,
                                        }),
                                    ),
                                ] {
                                    ui.selectable_value(&mut key.tween, value, label);
                                }
                            });
                    }
                });
            });
            ui.add_enabled_ui(self.layer_editable(), |ui| {
                if let Some(key) = timeline.layers[layer].key_at_mut(frame)
                    && let Some(Easing::CubicBezier { x1, y1, x2, y2 }) = &mut key.tween
                {
                    ui.horizontal(|ui| {
                        ui.label("Bezier control points");
                        for v in [x1, y1, x2, y2] {
                            ui.add(egui::DragValue::new(v).range(0.0..=1.0).speed(0.01));
                        }
                    });
                }
            });
            if document != self.project {
                self.finish_edits();
                self.clear_selection();
                self.transact(|editor| {
                    editor.project.name = document.name.clone();
                    editor.project.width = document.width;
                    editor.project.height = document.height;
                    editor.project.fps = document.fps;
                    editor.project.background = document.background;
                    let draft = &document.symbol(&symbol_id).unwrap().timeline;
                    if let Some(symbol) = editor.project.symbol_mut(&symbol_id) {
                        let timeline = &mut symbol.timeline;
                        let min = timeline
                            .layers
                            .iter()
                            .flat_map(|l| &l.keyframes)
                            .map(|k| k.frame + 1)
                            .chain(timeline.scripts.iter().map(|s| s.frame + 1))
                            .max()
                            .unwrap_or(1);
                        timeline.frame_count = draft.frame_count.max(min);
                        if let Some(target) = timeline.layers.iter_mut().find(|l| l.id == layer_id)
                        {
                            target.name = draft.layers[layer].name.clone();
                            if target.visible
                                && !target.locked
                                && let Some(changed) = draft.layers[layer].key_at(frame)
                                && let Some(key) = target
                                    .keyframes
                                    .iter_mut()
                                    .find(|k| k.frame == changed.frame)
                            {
                                key.label = changed.label.clone();
                                key.tween = changed.tween;
                            }
                        }
                    }
                    editor.normalize_selection();
                });
            }
        }
    }
    fn actions(&mut self, ui: &mut egui::Ui) {
        if self.playing {
            self.set_playing(false);
        }
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.script_target,
                ScriptTarget::Frame,
                format!("Frame {}", self.frame + 1),
            );
            ui.add_enabled_ui(self.selected_editable(), |ui| {
                ui.selectable_value(
                    &mut self.script_target,
                    ScriptTarget::Object,
                    "Selected object",
                );
            });
            if self.script_target == ScriptTarget::Object {
                egui::ComboBox::from_id_salt("event")
                    .selected_text(format!("{:?}", self.script_event))
                    .show_ui(ui, |ui| {
                        for e in [
                            ObjectEvent::Click,
                            ObjectEvent::Load,
                            ObjectEvent::EnterFrame,
                        ] {
                            ui.selectable_value(&mut self.script_event, e, format!("{e:?}"));
                        }
                    });
            }
        });
        if self.script_target == ScriptTarget::Object && !self.selected_editable() {
            if self.selection.len() > 1 {
                ui.strong(format!("{} objects selected", self.selection.len()));
            }
            ui.label("Select one editable object to edit its actions, or select Frame above.");
            return;
        }
        let object_id = self.selected_element().map(|element| element.id.clone());
        let original = match self.script_target {
            ScriptTarget::Frame => self
                .symbol()
                .timeline
                .scripts
                .iter()
                .find(|s| s.frame == self.frame)
                .map(|s| s.script.clone()),
            ScriptTarget::Object => self
                .selected_element()
                .and_then(|e| e.scripts.iter().find(|s| s.event == self.script_event))
                .map(|s| s.script.clone()),
        }
        .unwrap_or_default();
        let mut script = original.clone();
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("language")
                .selected_text(format!("{:?}", script.language))
                .show_ui(ui, |ui| {
                    for (language, label) in [
                        (ScriptLanguage::JavaScript, "JavaScript"),
                        (ScriptLanguage::TypeScript, "TypeScript"),
                        (ScriptLanguage::ActionScript2, "AS2 · initial subset"),
                        (ScriptLanguage::ActionScript3, "AS3 · initial subset"),
                    ] {
                        ui.selectable_value(&mut script.language, language, label);
                    }
                });
            if ui.button("Check script").clicked() {
                match neoflash_script::compile(&script) {
                    Ok(_) => self.message("Script syntax accepted for current language support."),
                    Err(errors) => {
                        for e in errors {
                            self.message(format!(
                                "Line {}, column {}: {}",
                                e.line, e.column, e.message
                            ));
                        }
                        self.bottom = BottomTab::Output;
                    }
                }
            }
            ui.small("Test Movie runs code. Timeline preview does not.");
        });
        let editable = self.script_target == ScriptTarget::Frame || self.selected_editable();
        ui.add_enabled_ui(editable, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_sized(
                    [ui.available_width(), ui.available_height().max(60.)],
                    egui::TextEdit::multiline(&mut script.source)
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .hint_text(
                            "this.stop();\n// Use this.gotoAndPlay(1) to control a timeline.",
                        ),
                );
            });
        });
        if script != original && editable {
            self.finish_edits();
            if self.script_target == ScriptTarget::Object
                && let Some(id) = object_id
            {
                self.select_single_id(id);
            }
            self.transact(|s| match s.script_target {
                ScriptTarget::Frame => {
                    let frame = s.frame;
                    let scripts = &mut s.symbol_mut().timeline.scripts;
                    scripts.retain(|fs| fs.frame != frame);
                    if !script.source.trim().is_empty()
                        || script.language != ScriptLanguage::JavaScript
                    {
                        scripts.push(FrameScript { frame, script });
                    }
                }
                ScriptTarget::Object => {
                    let event = s.script_event;
                    s.ensure_selected_key();
                    if let Some(e) = s.selected_element_mut() {
                        e.scripts.retain(|os| os.event != event);
                        if !script.source.trim().is_empty()
                            || script.language != ScriptLanguage::JavaScript
                        {
                            e.scripts.push(ObjectScript { event, script });
                        }
                    }
                }
            });
        }
    }
}
