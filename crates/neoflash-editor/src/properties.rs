//! Contextual authoring inspector. Shared field controls live in paint_controls.
use crate::{
    app::{Editor, Tool},
    theme::UiPalette,
};
use eframe::egui;
use neoflash_core::*;

impl Editor {
    pub(crate) fn properties(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::paint_controls::{
            PropertyChanges, numeric_pair, numeric_row, paint_row, percent_row, property_row,
            text_row,
        };
        let tab_id = egui::Id::new("properties-tool-tab");
        let mut tool_tab = ctx.data(|data| data.get_temp::<bool>(tab_id).unwrap_or(false));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut tool_tab, false, "Selection");
            ui.selectable_value(&mut tool_tab, true, "Tool");
        });
        ctx.data_mut(|data| data.insert_temp(tab_id, tool_tab));
        ui.separator();
        if tool_tab {
            self.finish_property_edit();
            ui.strong("Tool settings");
            self.authoring_tool_options(ui);
            paint_row(ui, "inspector-tool-fill", "Fill", &mut self.fill);
            paint_row(ui, "inspector-tool-stroke", "Stroke", &mut self.stroke);
            numeric_row(ui, "Stroke width", &mut self.stroke_width, 0. ..=100., 0.25);
            return;
        }
        if self.selection.len() > 1 {
            self.finish_property_edit();
            ui.strong(format!("{} objects selected", self.selection.len()));
            ui.label("Mixed selection");
            ui.horizontal(|ui| {
                if ui.button("Transform…").clicked() {
                    self.show_panel(crate::workspace::PanelId::Transform);
                }
                if ui.button("Color…").clicked() {
                    self.show_panel(crate::workspace::PanelId::Color);
                }
            });
            return;
        }
        let object = self.selected_sampled();
        let scope = egui::Id::new((
            "object-properties",
            &self.clipboard.session,
            self.symbol_id(),
            self.selected_frame_range().is_some(),
            object.as_ref().map(|e| std::mem::discriminant(&e.kind)),
        ));
        self.property_edit_scope(scope);
        let mut changes = PropertyChanges::default();
        ui.push_id(scope, |ui| {
            if let Some(mut element) = object {
                let original = element.clone();
                let mut edit_symbol = None;
                let mut convert_text = false;
                ui.add_enabled_ui(self.selected_editable(), |ui| {
                    ui.strong("Object");
                    changes.note(text_row(ui, "Name", &mut element.name));
                    changes.note(percent_row(ui, "Opacity", &mut element.transform.alpha));
                    inspector_section(ui, "Transform", |ui| {
                        for r in numeric_pair(ui, ["X", "Y"], [&mut element.transform.x, &mut element.transform.y], -1_000_000. ..=1_000_000., 1.) { changes.note(r); }
                        for r in numeric_pair(ui, ["Scale X", "Scale Y"], [&mut element.transform.scale_x, &mut element.transform.scale_y], -1000. ..=1000., 0.01) { changes.note(r); }
                        changes.note(numeric_row(ui, "Rotation", &mut element.transform.rotation, -36000. ..=36000., 1.));
                    });
                    inspector_section(ui, "Appearance & behavior", |ui| {
                    match &mut element.kind {
                        ElementKind::Rectangle { width, height, fill, stroke, stroke_width }
                        | ElementKind::Ellipse { width, height, fill, stroke, stroke_width } => {
                            ui.strong("Shape");
                            for r in numeric_pair(ui, ["W", "H"], [width, height], 1. ..=10000., 1.) { changes.note(r); }
                            changes.note(paint_row(ui, "shape-fill", "Fill", fill));
                            changes.note(paint_row(ui, "shape-stroke", "Stroke", stroke));
                            changes.note(numeric_row(ui, "Stroke width", stroke_width, 0. ..=100., 0.25));
                        }
                        ElementKind::Text { text, font_family, font_size, color } => {
                            ui.strong("Text");
                            changes.note(text_row(ui, "Text", text));
                            changes.note(numeric_row(ui, "Size", font_size, 4. ..=512., 1.));
                            changes.note(paint_row(ui, "text-color", "Color", color));
                            let before = font_family.clone();
                            let mut response = property_row(ui, "Font", |ui| {
                                egui::ComboBox::from_id_salt("font_family").selected_text(font_family.as_str()).width(ui.available_width()).show_ui(ui, |ui| {
                                    ui.add(egui::TextEdit::singleline(&mut self.font_filter).hint_text("Find font").desired_width(220.));
                                    for family in &self.fonts.families {
                                        if self.font_filter.is_empty() || family.to_lowercase().contains(&self.font_filter.to_lowercase()) { ui.selectable_value(font_family, family.clone(), family); }
                                    }
                                }).response
                            });
                            if *font_family != before { response.mark_changed(); }
                            changes.note(response);
                            if let Err(error) = self.fonts.load(ctx, font_family) { ui.colored_label(UiPalette::current(ctx).error_text, error); }
                            let response = ui.button("Use text box");
                            if response.clicked() { convert_text = true; changes.control = Some(response.id); }
                        }
                        kind @ ElementKind::TextBox { .. } => {
                            if let Some(control) = self.text_box_properties(ui, ctx, kind) { changes.control = Some(control); }
                        }
                        ElementKind::Video { width, height } => {
                            ui.strong("Video");
                            changes.note(numeric_row(ui, "Width", width, 0. ..=8192., 1.));
                            changes.note(numeric_row(ui, "Height", height, 0. ..=8192., 1.));
                            ui.small("Attach a NetStream from ActionScript. Playback runs in the HTML5 preview.");
                        }
                        ElementKind::Bitmap { resource_id, width, height, smoothing } => {
                            ui.strong("Bitmap");
                            let path = self.project.resources.iter().find(|r| r.id == *resource_id).map(|r| r.path.as_str()).unwrap_or("Missing image");
                            ui.add(egui::Label::new(path).truncate()).on_hover_text(path);
                            ui.small(format!("{width} × {height} px"));
                            changes.note(ui.checkbox(smoothing, "Smoothing"));
                        }
                        ElementKind::Path { commands, fill_rule, fill, stroke, stroke_width } | ElementKind::PaintedPath { commands, fill_rule, fill, stroke, stroke_width, .. } => {
                            ui.strong("Vector path");
                            ui.small(format!("{} commands", commands.len()));
                            changes.note(paint_row(ui, "path-fill", "Fill", fill));
                            changes.note(paint_row(ui, "path-stroke", "Stroke", stroke));
                            changes.note(numeric_row(ui, "Stroke width", stroke_width, 0. ..=100., 0.25));
                            let before = *fill_rule;
                            let mut response = property_row(ui, "Fill rule", |ui| egui::ComboBox::from_id_salt("fill_rule").width(ui.available_width()).selected_text(if *fill_rule == FillRule::EvenOdd { "Even–odd" } else { "Nonzero" }).show_ui(ui, |ui| {
                                ui.selectable_value(fill_rule, FillRule::NonZero, "Nonzero");
                                ui.selectable_value(fill_rule, FillRule::EvenOdd, "Even–odd");
                            }).response);
                            if *fill_rule != before { response.mark_changed(); }
                            changes.note(response);
                            if ui.button("Edit points (A)").clicked() { self.set_tool(Tool::Subselect); self.select_single_id(original.id.clone()); }
                        }
                        ElementKind::Instance { symbol_id, playback } => {
                            ui.strong("Instance");
                            let name = self.project.symbol(symbol_id).map(|s| s.name.as_str()).unwrap_or("Missing");
                            property_row(ui, "Symbol", |ui| { ui.add(egui::Label::new(name).truncate()).on_hover_text(name); });
                            if let Some(control) = self.instance_playback_ui(ui, symbol_id, playback) { changes.control = Some(control); }
                            if ui.button("Edit symbol").clicked() { edit_symbol = Some(symbol_id.clone()); }
                        }
                    }
                    });
                });
                if convert_text && let ElementKind::Text { text, font_family, font_size, color } = &element.kind {
                    let named = crate::fonts::styled_family(font_family, false, false);
                    let family = if ctx.fonts(|fonts| fonts.families().contains(&named)) { named } else { egui::FontFamily::Proportional };
                    let galley = ui.painter().layout_no_wrap(text.clone(), egui::FontId::new(*font_size as f32, family), egui::Color32::WHITE);
                    let kind = ElementKind::TextBox {
                        text: text.clone(), width: (galley.size().x as f64 + 4.).clamp(100., 1000000.), height: (galley.size().y as f64 + 4.).clamp(100., 1000000.),
                        format: TextFormat { font: font_family.clone(), size: *font_size, color: u32::from_be_bytes([0,color[0],color[1],color[2]]), ..Default::default() }, word_wrap: true, multiline: true,
                    };
                    element.transform.alpha *= color[3] as f64 / 255.;
                    element.kind = kind;
                }
                if original != element && let Some(control) = changes.control {
                    if let Some(field) = TextFieldState::from_kind(&element.kind) && let Err(error) = field.validate() { self.message(format!("Text box edit rejected: {error}")); return; }
                    self.begin_property_edit(scope, control);
                    self.select_single_id(original.id.clone());
                    self.ensure_selected_key();
                    if let Some(current) = self.selected_element_mut() {
                        current.name = element.name;
                        current.transform = element.transform;
                        current.kind = element.kind;
                    }
                }
                if let Some(id) = edit_symbol { self.enter_symbol(id); }
            } else {
                let mut document = self.project.clone();
                let symbol_id = self.symbol_id().to_owned();
                let layer_id = self.symbol().timeline.layers[self.layer].id.clone();
                let layer = self.layer;
                let frame = self.frame;
                let frame_context = self.selected_frame_range().is_some();
                if !frame_context {
                ui.strong("Document");
                inspector_section(ui, "Stage", |ui| {
                changes.note(text_row(ui, "Name", &mut document.name));
                let (mut width, mut height) = (document.width as f64, document.height as f64);
                for r in numeric_pair(ui, ["W", "H"], [&mut width, &mut height], 1. ..=8192., 1.) { changes.note(r); }
                document.width = width.round() as u32; document.height = height.round() as u32;
                changes.note(paint_row(ui, "background", "Background", &mut document.background));
                });
                }
                let mut fps = document.fps;
                let timeline = &mut document.symbol_mut(&symbol_id).unwrap().timeline;
                if frame_context {
                    ui.strong("Frame");
                    if let Some((start, end, layers)) = self.selected_frame_range() {
                        ui.label(format!("Frames {}–{} · {} layer{}", start + 1, end + 1, layers, if layers == 1 { "" } else { "s" }));
                    }
                    changes.note(text_row(ui, "Layer", &mut timeline.layers[layer].name));
                    let mut end = timeline.layers[layer].properties.end_frame.unwrap_or(timeline.frame_count);
                    let end_response = property_row(ui, "Layer end", |ui| ui.add_sized([96., crate::theme::CONTROL_HEIGHT], egui::DragValue::new(&mut end).range(1..=10000).update_while_editing(false)));
                    if end_response.changed() { self.set_layer_duration(end); return; }
                }
                let min = timeline.layers.iter().flat_map(|l| &l.keyframes).map(|k| k.frame+1).chain(timeline.scripts.iter().map(|s|s.frame+1)).chain(timeline.layers.iter().filter_map(|l|l.properties.end_frame)).max().unwrap_or(1);
                inspector_section(ui, "Playback", |ui| {
                    if !frame_context { changes.note(property_row(ui,"FPS",|ui|ui.add_sized([96.,crate::theme::CONTROL_HEIGHT],egui::DragValue::new(&mut fps).range(1..=1000).update_while_editing(false)))); }
                    changes.note(property_row(ui,"Frames",|ui|ui.add_sized([96.,crate::theme::CONTROL_HEIGHT],egui::DragValue::new(&mut timeline.frame_count).range(min..=10000).update_while_editing(false))));
                });
                if frame_context {
                inspector_section(ui, "Keyframe", |ui| {
                ui.add_enabled_ui(self.layer_editable(), |ui| {
                    if let Some(key) = timeline.layers[layer].key_at_mut(frame) {
                        changes.note(text_row(ui, "Frame label", &mut key.label));
                        let before = key.tween;
                        let mut response = property_row(ui, "Tween", |ui| egui::ComboBox::from_id_salt("tween").width(ui.available_width()).selected_text(match key.tween { None=>"No tween",Some(Easing::Linear)=>"Linear",Some(Easing::EaseIn)=>"Ease in",Some(Easing::EaseOut)=>"Ease out",Some(Easing::EaseInOut)=>"Ease in/out",Some(Easing::CubicBezier{..})=>"Custom curve",Some(Easing::Flash{..})=>"Flash ease" }).show_ui(ui, |ui| {
                            for (label,value) in [("No tween",None),("Linear",Some(Easing::Linear)),("Ease in",Some(Easing::EaseIn)),("Ease out",Some(Easing::EaseOut)),("Ease in/out",Some(Easing::EaseInOut)),("Custom curve",Some(Easing::CubicBezier{x1:0.25,y1:0.1,x2:0.25,y2:1.}))] { ui.selectable_value(&mut key.tween,value,label); }
                        }).response);
                        if key.tween != before { response.mark_changed(); }
                        changes.note(response);
                        if let Some(Easing::CubicBezier{x1,y1,x2,y2})=&mut key.tween {
                            for (label,value) in [("Curve X1",x1),("Curve Y1",y1),("Curve X2",x2),("Curve Y2",y2)] { changes.note(numeric_row(ui,label,value,0. ..=1.,0.01)); }
                        }
                    }
                });
                });
                if ui.button("Frame script…  F9").clicked() { self.open_frame_actions(); }
                }
                document.fps = fps;
                if document != self.project && let Some(control) = changes.control {
                    self.begin_property_edit(scope, control);
                    // Copy only changed authoring fields, never the cloned document.
                    self.project.name=document.name.clone(); self.project.width=document.width; self.project.height=document.height;
                    self.project.fps=document.fps; self.project.background=document.background;
                    let draft=&document.symbol(&symbol_id).unwrap().timeline;
                    if let Some(symbol)=self.project.symbol_mut(&symbol_id) {
                        let timeline=&mut symbol.timeline;
                        let min=timeline.layers.iter().flat_map(|l|&l.keyframes).map(|k|k.frame+1).chain(timeline.scripts.iter().map(|s|s.frame+1)).chain(timeline.layers.iter().filter_map(|l|l.properties.end_frame)).max().unwrap_or(1);
                        timeline.frame_count=draft.frame_count.max(min);
                        if let Some(target)=timeline.layers.iter_mut().find(|l|l.id==layer_id) {
                            target.name=draft.layers[layer].name.clone();
                            if target.visible && !target.locked && let Some(changed)=draft.layers[layer].key_at(frame) && let Some(key)=target.keyframes.iter_mut().find(|k|k.frame==changed.frame) { key.label=changed.label.clone(); key.tween=changed.tween; }
                        }
                    }
                    self.normalize_selection();
                }
            }
        });
        self.end_property_interaction(ctx);
    }
}

fn inspector_section<R>(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui) -> R) {
    ui.add_space(4.);
    egui::CollapsingHeader::new(title)
        .id_salt(title)
        .default_open(true)
        .show(ui, content);
}
