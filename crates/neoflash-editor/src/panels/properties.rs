//! Contextual authoring inspector (NF2 `Properties`): Selection / Tool segmented
//! control, a type header for the selected object and collapsible sections built
//! from NF2 fields. Shared field controls live in paint_controls.
use crate::theme::DesignUi as _;
use crate::{
    app::{Editor, Tool},
    theme::UiPalette,
};
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::{
    TextStyle,
    components::{self as nf, Badge, BadgeType, MenuItem, SectionHeader, Segmented, TypeHeader},
    icons::i16,
    tokens::space,
    typography::rich,
};

impl Editor {
    pub(crate) fn properties(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::paint_controls::{
            PropertyChanges, color_row, numeric_pair, numeric_row, paint_row, percent_row,
            property_row, text_row,
        };
        // Figma inspector body: V gap 12 between all rows.
        ui.spacing_mut().item_spacing.y = space::S3;
        let tab_id = egui::Id::new("properties-tool-tab");
        let mut tool_tab = ctx.data(|data| data.get_temp::<bool>(tab_id).unwrap_or(false));
        Segmented::show(
            ui,
            &mut tool_tab,
            &[(false, "Selection"), (true, "Tool")],
            false,
        );
        ctx.data_mut(|data| data.insert_temp(tab_id, tool_tab));
        if tool_tab {
            self.finish_property_edit();
            inspector_section(ui, "Tool options", |ui| self.authoring_tool_options(ui));
            inspector_section(ui, "Drawing defaults", |ui| {
                paint_row(ui, "inspector-tool-fill", "Fill", &mut self.fill);
                paint_row(ui, "inspector-tool-stroke", "Stroke", &mut self.stroke);
                numeric_row(ui, "Stroke width", &mut self.stroke_width, 0. ..=100., 0.25);
            });
            return;
        }
        if self.selection.len() > 1 {
            self.finish_property_edit();
            ui.add(TypeHeader {
                icon: i16::SELECT_ALL,
                title: &format!("{} objects selected", self.selection.len()),
                subtitle: "Mixed selection",
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                if ui.add(nf::Button::new("Transform…")).clicked() {
                    self.show_panel(crate::workspace::PanelId::Transform);
                }
                if ui.add(nf::Button::new("Color…")).clicked() {
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
                let (icon, title, subtitle) = self.object_header(&element);
                ui.add(TypeHeader { icon, title: &title, subtitle: &subtitle });
                ui.add_enabled_ui_semantic(self.selected_editable(), |ui| {
                    changes.note(text_row(ui, "Name", &mut element.name));
                    inspector_section(ui, "Position and transform", |ui| {
                        for r in numeric_pair(ui, ["X", "Y"], [&mut element.transform.x, &mut element.transform.y], -1_000_000. ..=1_000_000., 1.) { changes.note(r); }
                        for r in numeric_pair(ui, ["Scale X", "Scale Y"], [&mut element.transform.scale_x, &mut element.transform.scale_y], -1000. ..=1000., 0.01) { changes.note(r); }
                        changes.note(numeric_row(ui, "Rotation", &mut element.transform.rotation, -36000. ..=36000., 1.));
                    });
                    inspector_section(ui, appearance_title(&element.kind), |ui| {
                    changes.note(percent_row(ui, "Opacity", &mut element.transform.alpha));
                    match &mut element.kind {
                        ElementKind::Rectangle { width, height, fill, stroke, stroke_width }
                        | ElementKind::Ellipse { width, height, fill, stroke, stroke_width } => {
                            for r in numeric_pair(ui, ["W", "H"], [width, height], 1. ..=10000., 1.) { changes.note(r); }
                            changes.note(paint_row(ui, "shape-fill", "Fill", fill));
                            changes.note(paint_row(ui, "shape-stroke", "Stroke", stroke));
                            changes.note(numeric_row(ui, "Stroke width", stroke_width, 0. ..=100., 0.25));
                        }
                        ElementKind::Text { text, font_family, font_size, color } => {
                            changes.note(text_row(ui, "Text", text));
                            changes.note(numeric_row(ui, "Size", font_size, 4. ..=512., 1.));
                            changes.note(paint_row(ui, "text-color", "Color", color));
                            let before = font_family.clone();
                            let mut response = property_row(ui, "Font", |ui| {
                                let width = ui.available_width();
                                let current = font_family.clone();
                                let (response, _) = nf::Select::new("font_family", &current).width(width).show(ui, |ui| {
                                    ui.add(nf::Search::new(&mut self.font_filter, "Find font").width(220.));
                                    for family in &self.fonts.families {
                                        if (self.font_filter.is_empty() || family.to_lowercase().contains(&self.font_filter.to_lowercase()))
                                            && ui.add(MenuItem::new(family).checked(family == font_family)).clicked() { *font_family = family.clone(); }
                                    }
                                });
                                response
                            });
                            if *font_family != before { response.mark_changed(); }
                            changes.note(response);
                            if let Err(error) = self.fonts.load(ctx, font_family) { ui.label(rich(error, TextStyle::Small).color(UiPalette::current(ctx).error_text)); }
                            let response = ui.add(nf::Button::new("Use text box"));
                            if response.clicked() { convert_text = true; changes.control = Some(response.id); }
                        }
                        kind @ ElementKind::TextBox { .. } => {
                            if let Some(control) = self.text_box_properties(ui, ctx, kind) { changes.control = Some(control); }
                        }
                        ElementKind::Video { width, height } => {
                            for r in numeric_pair(ui, ["Width", "Height"], [width, height], 0. ..=8192., 1.) { changes.note(r); }
                            hint(ui, "Attach a NetStream from ActionScript. Playback runs in the HTML5 preview.");
                        }
                        ElementKind::Bitmap { resource_id, width, height, smoothing } => {
                            let path = self.project.resources.iter().find(|r| r.id == *resource_id).map(|r| r.path.as_str()).unwrap_or("Missing image");
                            ui.add(egui::Label::new(rich(path, TextStyle::Body).color(neoflash_ui::palette(ctx).text_primary)).truncate()).on_hover_text(path);
                            hint(ui, &format!("{width} × {height} px"));
                            changes.note(ui.add(nf::Checkbox::new(smoothing, "Smoothing")));
                        }
                        ElementKind::Path { commands, fill_rule, fill, stroke, stroke_width } | ElementKind::PaintedPath { commands, fill_rule, fill, stroke, stroke_width, .. } => {
                            hint(ui, &format!("{} commands", commands.len()));
                            changes.note(paint_row(ui, "path-fill", "Fill", fill));
                            changes.note(paint_row(ui, "path-stroke", "Stroke", stroke));
                            changes.note(numeric_row(ui, "Stroke width", stroke_width, 0. ..=100., 0.25));
                            changes.note(select_row(ui, "Fill rule", "fill_rule", fill_rule, &[(FillRule::NonZero, "Nonzero"), (FillRule::EvenOdd, "Even–odd")]));
                            if ui.add(nf::Button::new("Edit points").shortcut("A")).clicked() { self.set_tool(Tool::Subselect); self.select_single_id(original.id.clone()); }
                        }
                        ElementKind::Instance { symbol_id, playback } => {
                            let name = self.project.symbol(symbol_id).map(|s| s.name.as_str()).unwrap_or("Missing");
                            property_row(ui, "Symbol", |ui| { ui.add(egui::Label::new(rich(name, TextStyle::Body).color(neoflash_ui::palette(ctx).text_primary)).truncate()).on_hover_text(name); });
                            let _ = playback;
                        }
                    }
                    });
                    if let Some(control) = self.primitive_options_properties_ui(ui, &mut element) {
                        changes.control = Some(control);
                    }
                    inspector_section(ui, "Behavior / linkage", |ui| {
                        if let ElementKind::Instance { symbol_id, playback } = &mut element.kind {
                            if let Some(control) = self.instance_playback_ui(ui, symbol_id, playback) { changes.control = Some(control); }
                            if let Some(symbol) = self.project.symbol(symbol_id) {
                                for (label, value) in [("Linkage", symbol.linkage_id.as_deref()), ("Class", symbol.class_name.as_deref())] {
                                    if let Some(value) = value { property_row(ui, label, |ui| { ui.add(egui::Label::new(rich(value, TextStyle::Mono).color(neoflash_ui::palette(ctx).text_primary)).truncate()).on_hover_text(value); }); }
                                }
                            }
                            if ui.add(nf::Button::new("Edit symbol")).clicked() { edit_symbol = Some(symbol_id.clone()); }
                        }
                        if ui.add(nf::Button::new("Object actions…").shortcut("F9")).clicked() {
                            self.script_target = crate::app::ScriptTarget::Object;
                            self.show_panel(crate::workspace::PanelId::Actions);
                        }
                    });
                    inspector_section(ui, "Accessibility", |ui| {
                        hint(ui, element.accessibility.as_ref().map(|a| a.name.as_str()).unwrap_or("Not exposed to assistive technology"));
                        if ui.add(nf::Button::new("Edit accessibility…")).clicked() { self.show_panel(crate::workspace::PanelId::Accessibility); }
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
                inspector_section(ui, "Stage", |ui| {
                changes.note(text_row(ui, "Name", &mut document.name));
                let (mut width, mut height) = (document.width as f64, document.height as f64);
                let (old_width, old_height) = (width, height);
                let link_id = egui::Id::new("properties-stage-size-linked");
                let mut linked = ui.data(|d| d.get_temp::<bool>(link_id).unwrap_or(false));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let pair_width = ui.available_width() - crate::theme::CONTROL_HEIGHT - space::S2;
                    ui.allocate_ui_with_layout(egui::vec2(pair_width, crate::theme::CONTROL_HEIGHT), egui::Layout::top_down(egui::Align::Min), |ui| {
                        for r in numeric_pair(ui, ["W", "H"], [&mut width, &mut height], 1. ..=8192., 1.) { changes.note(r); }
                    });
                    let link = ui.add(nf::IconButton::new(i16::LINK, "Constrain stage proportions").selected(linked));
                    if link.clicked() { linked = !linked; }
                });
                ui.data_mut(|d| d.insert_temp(link_id, linked));
                if linked && old_width > 0. && old_height > 0. {
                    if width != old_width { height = (width * old_height / old_width).round().clamp(1., 8192.); }
                    else if height != old_height { width = (height * old_width / old_height).round().clamp(1., 8192.); }
                }
                document.width = width.round() as u32; document.height = height.round() as u32;
                changes.note(color_row(ui, "background", "", "Background", &mut document.background));
                });
                }
                let mut fps = document.fps;
                let timeline = &mut document.symbol_mut(&symbol_id).unwrap().timeline;
                if frame_context {
                    let range = self.selected_frame_range();
                    let subtitle = range.map(|(start, end, layers)| format!("Frames {}–{} · {} layer{}", start + 1, end + 1, layers, if layers == 1 { "" } else { "s" })).unwrap_or_default();
                    // Inside a motion tween the frame inspector is Properties — Motion tween (PR-10).
                    let motion = self.motion_tween_span();
                    match &motion {
                        Some(span) => { ui.add(TypeHeader { icon: i16::TWEEN_MOTION, title: "Motion tween", subtitle: &format!("Layer \"{}\" · {}", span.layer_name, span.frame_label()) }); }
                        None => { ui.add(TypeHeader { icon: i16::FRAME, title: "Frame", subtitle: &subtitle }); }
                    }
                    let mut layer_end = None;
                    inspector_section(ui, "Range / duration", |ui| {
                        changes.note(text_row(ui, "Layer", &mut timeline.layers[layer].name));
                        let mut end = timeline.layers[layer].properties.end_frame.unwrap_or(timeline.frame_count);
                        let response = ui.add(nf::DragField::new(&mut end, "Layer end").prefix("Layer end").range(1..=10000).speed(0.25));
                        if response.changed() { layer_end = Some(end); }
                    });
                    if let Some(end) = layer_end { self.set_layer_duration(end); return; }
                    if let Some(span) = &motion { self.motion_tween_properties(ui, span); } else {
                    inspector_section(ui, "Tween", |ui| {
                        ui.add_enabled_ui_semantic(self.layer_editable(), |ui| {
                            if let Some(key) = timeline.layers[layer].key_at_mut(frame) {
                                let mut options = vec![(None, "No tween"), (Some(Easing::Linear), "Linear"), (Some(Easing::EaseIn), "Ease in"), (Some(Easing::EaseOut), "Ease out"), (Some(Easing::EaseInOut), "Ease in/out")];
                                options.push((if let Some(curve @ Easing::CubicBezier { .. }) = key.tween { Some(curve) } else { Some(Easing::CubicBezier { x1: 0.25, y1: 0.1, x2: 0.25, y2: 1. }) }, "Custom curve"));
                                options.push((if let Some(flash @ Easing::Flash { .. }) = key.tween { Some(flash) } else { Some(Easing::Flash { acceleration: 0. }) }, "Flash ease"));
                                changes.note(select_row(ui, "Easing", "tween", &mut key.tween, &options));
                                // Y may overshoot (Back) or undershoot; the document allows |y| ≤ 100, x stays in 0–1.
                                if let Some(Easing::CubicBezier{x1,y1,x2,y2})=&mut key.tween {
                                    for (labels, values) in [(["X1","Y1"],[&mut *x1,&mut *y1]),(["X2","Y2"],[&mut *x2,&mut *y2])] {
                                        for r in numeric_pair(ui, labels, values,-100. ..=100.,0.01) { changes.note(r); }
                                    }
                                    *x1 = x1.clamp(0., 1.); *x2 = x2.clamp(0., 1.);
                                }
                                if let Some(Easing::Flash{acceleration})=&mut key.tween {
                                    changes.note(numeric_row(ui, "Acceleration", acceleration, -100. ..=100., 1.));
                                }
                            } else { hint(ui, "Select a frame with a keyframe to edit its tween."); }
                        });
                    });
                    }
                    inspector_section(ui, "Label / script", |ui| {
                        ui.add_enabled_ui_semantic(self.layer_editable(), |ui| {
                            if let Some(key) = timeline.layers[layer].key_at_mut(frame) { changes.note(text_row(ui, "Frame label", &mut key.label)); }
                        });
                        if ui.add(nf::Button::new("Frame script…").shortcut("F9")).clicked() { self.open_frame_actions(); }
                    });
                    self.frame_sound_section(ui);
                } else {
                    let min = timeline.layers.iter().flat_map(|l| &l.keyframes).map(|k| k.frame+1).chain(timeline.scripts.iter().map(|s|s.frame+1)).chain(timeline.layers.iter().filter_map(|l|l.properties.end_frame)).max().unwrap_or(1);
                    inspector_section(ui, "Playback", |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S2;
                            let width = (ui.available_width() - space::S2) / 2.;
                            changes.note(ui.add(nf::DragField::new(&mut fps, "FPS").prefix("FPS").range(1..=1000).speed(0.25).width(width)));
                            changes.note(ui.add(nf::DragField::new(&mut timeline.frame_count, "Frames").prefix("Frames").range(min..=10000).speed(0.25).width(width)));
                        });
                    });
                    inspector_section(ui, "Publish", |ui| {
                        ui.add(Badge::new("HTML5 · Rust / WebAssembly", BadgeType::Accent));
                        // §4: "Publish settings…"; publishing itself is File ▸ Publish and the top-bar Publish button.
                        if ui.add(nf::Button::new("Publish settings…")).on_hover_text("Publish settings (preview). Publish HTML5 with the Publish button or File ▸ Publish.").clicked() { Self::open_planned(ui.ctx(), "Properties ▸ Publish settings…"); }
                    });
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
                            if matches!(draft.layers[layer].key_at(frame).and_then(|k| k.tween), Some(Easing::Flash { .. })) { self.project.format_version = self.project.format_version.max(5); }
                        }
                    }
                    self.normalize_selection();
                }
            }
        });
        self.end_property_interaction(ctx);
    }
}

impl Editor {
    /// Icon, title and subtitle of the Properties type header for one object.
    fn object_header(&self, element: &Element) -> (neoflash_ui::Icon, String, String) {
        let layer = self
            .selection
            .targets
            .first()
            .and_then(|target| {
                self.symbol()
                    .timeline
                    .layers
                    .iter()
                    .find(|l| l.id == target.layer)
            })
            .map(|l| format!("layer “{}”", l.name))
            .unwrap_or_default();
        let (icon, title) = match &element.kind {
            ElementKind::Rectangle { .. } => (i16::RECT, "Rectangle"),
            ElementKind::Ellipse { .. } => (i16::OVAL, "Oval"),
            ElementKind::Text { .. } => (i16::TEXT, "Text"),
            ElementKind::TextBox { .. } => (i16::TEXT, "Text box"),
            ElementKind::Video { .. } => (i16::VIDEO, "Video"),
            ElementKind::Bitmap { .. } => (i16::BITMAP, "Bitmap"),
            ElementKind::Path { .. } => (i16::PEN, "Vector path"),
            ElementKind::PaintedPath { .. } => (i16::BRUSH, "Painted path"),
            ElementKind::Instance { symbol_id, .. } => {
                let symbol = self.project.symbol(symbol_id);
                let (icon, title) = match symbol.map(|s| s.kind) {
                    Some(SymbolKind::Graphic) => (i16::GRAPHIC, "Graphic"),
                    Some(SymbolKind::Button) => (i16::BUTTON_SYMBOL, "Button"),
                    _ => (i16::MOVIECLIP, "Movie clip"),
                };
                let name = symbol.map(|s| s.name.as_str()).unwrap_or("Missing");
                return (icon, title.into(), format!("Instance of “{name}”"));
            }
        };
        let subtitle = if layer.is_empty() {
            "Drawing object".to_owned()
        } else {
            format!("Drawing object · {layer}")
        };
        (icon, title.into(), subtitle)
    }
}

fn appearance_title(kind: &ElementKind) -> &'static str {
    match kind {
        ElementKind::Rectangle { .. }
        | ElementKind::Ellipse { .. }
        | ElementKind::Path { .. }
        | ElementKind::PaintedPath { .. } => "Fill and stroke",
        ElementKind::Text { .. } | ElementKind::TextBox { .. } => "Character",
        ElementKind::Instance { .. } => "Colour effect",
        ElementKind::Video { .. } | ElementKind::Bitmap { .. } => "Display",
    }
}

/// Secondary helper text (`NF2/Small`, `text/secondary`).
pub(crate) fn hint(ui: &mut egui::Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    let color = if ui.is_enabled() {
        p.text_secondary
    } else {
        p.text_disabled
    };
    ui.add(egui::Label::new(rich(text, TextStyle::Small).color(color)).wrap());
}

/// Labelled `NF2/Select` over fixed options. The response is marked changed
/// only when the value changed, and keeps the Select's stable id.
pub(crate) fn select_row<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    label: &str,
    id_salt: &str,
    value: &mut T,
    options: &[(T, &str)],
) -> egui::Response {
    let before = *value;
    let current = options
        .iter()
        .find(|(v, _)| *v == *value)
        .map_or("—", |(_, l)| *l);
    let mut response = crate::paint_controls::property_row(ui, label, |ui| {
        let width = ui.available_width();
        nf::Select::new(id_salt, current)
            .width(width)
            .show(ui, |ui| {
                for (option, name) in options {
                    if ui
                        .add(MenuItem::new(name).checked(*option == *value))
                        .clicked()
                    {
                        *value = *option;
                    }
                }
            })
            .0
    });
    if *value != before {
        response.mark_changed();
    }
    response
}

/// `NF2/Section Header` (collapsible overline) with 8 px row gaps.
pub(crate) fn inspector_section<R>(
    ui: &mut egui::Ui,
    title: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = crate::theme::CONTROL_HEIGHT;
        SectionHeader::new(title)
            .id_salt(("inspector-section", title))
            .default_open(!matches!(title, "Behavior / linkage" | "Accessibility"))
            .show(ui, content);
    });
}
