//! Symbol conversion is prepared without touching the document, then committed once.
use crate::theme::DesignUi as _;
use crate::{app::Editor, selection::Selection, stage};
use eframe::egui::{self, Pos2, Rect};
use neoflash_core::*;
use neoflash_ui::{
    TextStyle,
    components::{
        Button, Checkbox, DialogHeader, MenuItem, RegistrationPicker, RegistrationPoint,
        SectionHeader, Select, Surface, TextField, surface::dialog_footer,
    },
    tokens::space,
    typography::rich,
};

#[derive(Clone)]
pub(crate) struct SymbolConversion {
    symbol: String,
    frame: u32,
    selection: Selection,
    sampled: Vec<(crate::selection::SelectionTarget, Element)>,
    new_symbol: bool,
    pub name: String,
    pub kind: SymbolKind,
    pub folder: String,
    /// Row-major position in the classic nine-point registration selector.
    pub registration: usize,
    pub export_enabled: bool,
    pub linkage_id: String,
    pub class_name: String,
    focus_name: bool,
    error: Option<String>,
}

impl Editor {
    pub(crate) fn begin_new_symbol(&mut self) {
        self.set_playing(false);
        let mut name = "Symbol".to_owned();
        let mut suffix = 2;
        while self
            .project
            .symbols
            .iter()
            .any(|symbol| symbol.name == name)
        {
            name = format!("Symbol {suffix}");
            suffix += 1;
        }
        self.symbol_conversion = Some(SymbolConversion {
            symbol: self.symbol_id().to_owned(),
            frame: self.frame,
            selection: self.selection.clone(),
            sampled: vec![],
            new_symbol: true,
            name: name.clone(),
            kind: SymbolKind::MovieClip,
            folder: self.library_panel.selected_folder.clone(),
            registration: 4,
            export_enabled: false,
            linkage_id: name,
            class_name: String::new(),
            focus_name: true,
            error: None,
        });
    }

    pub(crate) fn can_convert_selection(&self) -> bool {
        self.selection.symbol == self.symbol_id()
            && !self.selection.is_empty()
            && self
                .selection
                .targets
                .iter()
                .all(|target| self.target_editable(target))
    }

    pub(crate) fn begin_symbol_conversion(&mut self) {
        self.finish_edits();
        if !self.can_convert_selection() {
            return;
        }
        let sampled = self.sampled_targets();
        if sampled.is_empty() {
            return;
        }
        if sampled
            .iter()
            .any(|(target, _)| target.layer != sampled[0].0.layer)
        {
            self.message("Convert to Symbol currently requires artwork from one layer. Select objects on the same layer to preserve their stacking order.");
            return;
        }
        let layer = self
            .symbol()
            .timeline
            .layers
            .iter()
            .find(|layer| layer.id == sampled[0].0.layer)
            .unwrap();
        let elements = sample_layer(layer, self.frame);
        let indices: Vec<_> = elements
            .iter()
            .enumerate()
            .filter(|(_, element)| {
                sampled
                    .iter()
                    .any(|(_, selected)| selected.id == element.id)
            })
            .map(|(index, _)| index)
            .collect();
        if indices.last().unwrap() - indices[0] + 1 != indices.len() {
            self.message("Select a contiguous stack of objects on one layer. Grouping around unselected artwork would change its stacking order.");
            return;
        }
        self.set_playing(false);
        let name = if sampled.len() == 1 {
            format!("{} symbol", sampled[0].1.name)
        } else {
            "Symbol".to_owned()
        };
        let mut unique_name = name.clone();
        let mut suffix = 2;
        while self
            .project
            .symbols
            .iter()
            .any(|symbol| symbol.name == unique_name)
        {
            unique_name = format!("{name} {suffix}");
            suffix += 1;
        }
        self.symbol_conversion = Some(SymbolConversion {
            symbol: self.symbol_id().to_owned(),
            frame: self.frame,
            selection: self.selection.clone(),
            sampled,
            new_symbol: false,
            name: unique_name.clone(),
            kind: SymbolKind::MovieClip,
            folder: self.library_panel.selected_folder.clone(),
            registration: 0,
            export_enabled: false,
            linkage_id: unique_name,
            class_name: String::new(),
            focus_name: true,
            error: None,
        });
    }

    pub(crate) fn symbol_creation_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.symbol_conversion.take() else {
            return;
        };
        let mut accept = false;
        let mut cancel = false;
        let p = neoflash_ui::palette(ctx);
        let response = egui::Modal::new(egui::Id::new("convert-to-symbol"))
            .frame(Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
            ui.set_width(560.);
            let title = if draft.new_symbol { "Create New Symbol" } else { "Convert to Symbol" };
            cancel |= DialogHeader { title }.show(ui);
            egui::Frame::new()
                .inner_margin(egui::Margin { left: space::S5 as i8, right: space::S5 as i8, top: 0, bottom: space::S3 as i8 })
                .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = space::S3;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::S1;
                field_caption(ui, "Name");
                let name = ui.add(TextField::new(&mut draft.name, "Name"));
                let edit_id = TextField::edit_id(name.id);
                if std::mem::take(&mut draft.focus_name) {
                    ctx.memory_mut(|memory| memory.request_focus(edit_id));
                    if let Some(mut state) = egui::text_edit::TextEditState::load(ctx, edit_id) {
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0), egui::text::CCursor::new(draft.name.chars().count()),
                        )));
                        state.store(ctx, edit_id);
                    }
                }
            });
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = space::S3;
                let column = (ui.available_width() - 2. * space::S3 - 110.) / 2.;
                ui.allocate_ui_with_layout(egui::vec2(column, 52.), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = space::S1;
                    field_caption(ui, "Type");
                    Select::new("symbol-kind", kind_label(draft.kind)).width(column).show(ui, |ui| {
                        for kind in [SymbolKind::MovieClip, SymbolKind::Button, SymbolKind::Graphic] {
                            if ui.add(MenuItem::new(kind_label(kind)).shortcut(kind_hint(kind)).checked(draft.kind == kind)).clicked() {
                                draft.kind = kind;
                            }
                        }
                    });
                });
                ui.allocate_ui_with_layout(egui::vec2(column, 52.), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = space::S1;
                    field_caption(ui, "Folder");
                    crate::library::folder_picker(ui, "symbol-folder", &mut draft.folder, &self.project.library_folder_paths(), None);
                });
                ui.allocate_ui_with_layout(egui::vec2(110., 52.), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = space::S1;
                    field_caption(ui, "Registration");
                    ui.add_enabled_ui_semantic(!draft.new_symbol, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S2;
                            let mut point = RegistrationPoint::ALL[draft.registration.min(8)];
                            if ui.add(RegistrationPicker { point: &mut point }).changed() {
                                draft.registration = point.index();
                            }
                            ui.label(rich(point.label(), TextStyle::Body).color(if ui.is_enabled() { p.text_secondary } else { p.text_disabled }));
                        });
                    })
                    .response
                    .on_disabled_hover_text("An empty symbol has no artwork bounds; its origin starts at 0, 0.");
                });
            });
            crate::panels::properties::hint(ui, match draft.kind {
                SymbolKind::MovieClip => "Movie clips have an independent timeline and can be scripted.",
                SymbolKind::Graphic => "Graphics follow the parent timeline in loop mode.",
                SymbolKind::Button => "Buttons have Up, Over, Down and Hit frames. Add keyframes to customize each state.",
            });
            SectionHeader::new("Advanced").id_salt("symbol-advanced").default_open(false).show(ui, |ui| {
                ui.add_enabled_ui_semantic(draft.kind != SymbolKind::Graphic, |ui| {
                    ui.spacing_mut().item_spacing.y = space::S2;
                    ui.label(rich("ActionScript linkage", TextStyle::Overline).color(if ui.is_enabled() { p.text_secondary } else { p.text_disabled }));
                    ui.add(Checkbox::new(&mut draft.export_enabled, "Export for ActionScript"));
                    ui.add_enabled_ui_semantic(draft.export_enabled, |ui| {
                        crate::paint_controls::text_row(ui, "Identifier", &mut draft.linkage_id);
                        crate::paint_controls::text_row(ui, "Class", &mut draft.class_name).on_hover_text("Optional existing class, such as app.Card");
                    });
                    crate::panels::properties::hint(ui, "Define the optional class and its base class in Project files.");
                }).response.on_disabled_hover_text("Graphic symbols follow their parent timeline and have no runtime export linkage.");
            });
            if let Some(error) = &draft.error {
                ui.add(egui::Label::new(rich(error, TextStyle::Body).color(p.danger_default)).wrap());
            }
                });
            dialog_footer(ui, |_| {}, |ui| {
                let label = if draft.new_symbol { "Create" } else { "Convert" };
                accept |= ui.add_enabled_semantic(!draft.name.trim().is_empty(), Button::primary(label).shortcut("↵")).clicked();
                cancel |= ui.add(Button::new("Cancel").shortcut("Esc")).clicked();
            });
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                accept = !draft.name.trim().is_empty();
            }
            if draft.new_symbol { Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::ZERO)) } else { conversion_bounds(self, &draft, ui.painter()) }
        });
        if cancel || response.should_close() {
            return;
        }
        if accept {
            match response
                .inner
                .ok_or_else(|| {
                    "The selection has no visible bounds. Choose visible artwork before converting."
                        .to_owned()
                })
                .and_then(|bounds| self.commit_symbol_conversion(&draft, bounds))
            {
                Ok(()) => return,
                Err(error) => draft.error = Some(error),
            }
        }
        self.symbol_conversion = Some(draft);
    }

    fn commit_empty_symbol(&mut self, draft: &SymbolConversion) -> Result<(), String> {
        let id = new_id("symbol");
        let symbol = symbol_from_draft(draft, id.clone(), vec![], vec![])?;
        let export = symbol.linkage_id.is_some();
        let mut project = self.project.clone();
        project.add_library_folder(&draft.folder)?;
        project.symbols.push(symbol);
        if export {
            project.enable_program();
        }
        project.validate().map_err(|error| error.to_string())?;
        self.transact(|editor| {
            editor.project = project;
            editor.library_selected = Some(id.clone());
        });
        self.enter_symbol(id);
        self.message(format!(
            "Created {} ‘{}’. Edit its artwork around the registration point.",
            kind_name(draft.kind),
            draft.name.trim()
        ));
        Ok(())
    }

    pub(crate) fn commit_symbol_conversion(
        &mut self,
        draft: &SymbolConversion,
        bounds: Rect,
    ) -> Result<(), String> {
        if draft.new_symbol {
            return self.commit_empty_symbol(draft);
        }
        if self.symbol_id() != draft.symbol
            || self.frame != draft.frame
            || self.selection != draft.selection
            || self.sampled_targets() != draft.sampled
        {
            return Err("The selection changed. Cancel and open Convert to Symbol again.".into());
        }
        let name = draft.name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err("Enter a symbol name without control characters.".into());
        }
        if !bounds.is_finite() || draft.registration >= 9 {
            return Err("The selected artwork has invalid bounds.".into());
        }
        let registration = draft.registration;
        let anchor = Pos2::new(
            bounds.left() + bounds.width() * (registration % 3) as f32 / 2.,
            bounds.top() + bounds.height() * (registration / 3) as f32 / 2.,
        );
        let playback = match draft.kind {
            SymbolKind::MovieClip => InstancePlayback::Independent,
            SymbolKind::Graphic => InstancePlayback::Loop,
            SymbolKind::Button => InstancePlayback::SingleFrame(0),
        };
        let id = new_id("symbol");
        let mut elements: Vec<_> = draft
            .sampled
            .iter()
            .map(|(_, element)| element.clone())
            .collect();
        let mut instance = if elements.len() == 1 {
            let mut instance = elements[0].clone();
            let shifted = crate::transform_geometry::transformed_point(
                instance.transform,
                [anchor.x as f64, anchor.y as f64],
            );
            instance.transform.x = shifted[0];
            instance.transform.y = shifted[1];
            let content = &mut elements[0];
            content.id = new_id("object");
            content.transform = Transform {
                x: -anchor.x as f64,
                y: -anchor.y as f64,
                ..Transform::default()
            };
            content.scripts.clear();
            content.accessibility = None;
            instance
        } else {
            for element in &mut elements {
                element.id = new_id("object");
                element.transform.x -= anchor.x as f64;
                element.transform.y -= anchor.y as f64;
            }
            Element {
                id: new_id("object"),
                name: name.to_owned(),
                transform: Transform {
                    x: anchor.x as f64,
                    y: anchor.y as f64,
                    ..Transform::default()
                },
                kind: ElementKind::Instance {
                    symbol_id: id.clone(),
                    playback,
                },
                scripts: vec![],
                accessibility: None,
            }
        };
        instance.kind = ElementKind::Instance {
            symbol_id: id.clone(),
            playback,
        };
        let remapped: std::collections::HashMap<_, _> = draft
            .sampled
            .iter()
            .zip(&elements)
            .map(|((_, source), content)| (source.id.clone(), content.id.clone()))
            .collect();
        let groups = self
            .symbol()
            .timeline
            .layers
            .iter()
            .find(|layer| layer.id == draft.sampled[0].0.layer)
            .into_iter()
            .flat_map(|layer| &layer.properties.groups)
            .filter(|group| group.members.iter().all(|id| remapped.contains_key(id)))
            .map(|group| AuthoringGroup {
                id: new_id("group"),
                name: group.name.clone(),
                members: group
                    .members
                    .iter()
                    .map(|id| remapped[id].clone())
                    .collect(),
            })
            .collect();
        let symbol = symbol_from_draft(draft, id.clone(), elements, groups)?;
        let export = symbol.linkage_id.is_some();
        // Validate the complete proposed document before creating an undo entry.
        let mut project = self.project.clone();
        let layer = project
            .symbol_mut(&draft.symbol)
            .unwrap()
            .timeline
            .layers
            .iter_mut()
            .find(|layer| layer.id == draft.sampled[0].0.layer)
            .unwrap();
        if !layer.keyframes.iter().any(|key| key.frame == draft.frame) {
            let sampled = sample_layer(layer, draft.frame);
            let mut key = layer.key_at(draft.frame).cloned().unwrap();
            key.frame = draft.frame;
            key.label.clear();
            key.elements = sampled;
            layer.keyframes.push(key);
            layer.keyframes.sort_by_key(|key| key.frame);
        }
        let key = layer.key_at_mut(draft.frame).unwrap();
        let selected = |element: &Element| {
            draft
                .sampled
                .iter()
                .any(|(_, source)| source.id == element.id)
        };
        let insertion = key.elements.iter().rposition(selected).unwrap();
        let first = key.elements.iter().position(selected).unwrap();
        if insertion - first + 1 != draft.sampled.len() {
            return Err("The selection is no longer a contiguous stack. Cancel and select adjacent objects on one layer.".into());
        }
        let insertion = key.elements[..insertion]
            .iter()
            .filter(|element| !selected(element))
            .count();
        key.elements.retain(|element| !selected(element));
        key.elements.insert(insertion, instance.clone());
        // Groups may also cover other keys: retain their old member IDs where
        // artwork still exists, and let the replacement participate here.
        for group in &mut layer.properties.groups {
            if group
                .members
                .iter()
                .any(|id| draft.sampled.iter().any(|(_, source)| &source.id == id))
                && !group.members.contains(&instance.id)
            {
                group.members.push(instance.id.clone());
            }
        }
        project.add_library_folder(&draft.folder)?;
        project.symbols.push(symbol);
        if export {
            project.enable_program();
        }
        project.prune_authoring_groups();
        project.validate().map_err(|error| error.to_string())?;
        self.transact(|editor| {
            editor.project = project;
            editor.select_single_id(instance.id);
            editor.library_selected = Some(id);
        });
        self.message(format!("Created {} ‘{}’.", kind_name(draft.kind), name));
        Ok(())
    }
}

fn symbol_from_draft(
    draft: &SymbolConversion,
    id: Id,
    elements: Vec<Element>,
    groups: Vec<AuthoringGroup>,
) -> Result<Symbol, String> {
    let name = draft.name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err("Enter a symbol name without control characters.".into());
    }
    let export = draft.export_enabled && draft.kind != SymbolKind::Graphic;
    if export && draft.linkage_id.trim().is_empty() {
        return Err("Enter an export identifier for runtime creation.".into());
    }
    Ok(Symbol {
        id,
        name: name.to_owned(),
        folder: draft.folder.clone(),
        kind: draft.kind,
        class_name: (export && !draft.class_name.trim().is_empty())
            .then(|| draft.class_name.trim().to_owned()),
        linkage_id: export.then(|| draft.linkage_id.trim().to_owned()),
        timeline: Timeline {
            guides: vec![],
            frame_count: if draft.kind == SymbolKind::Button {
                4
            } else {
                1
            },
            layers: vec![Layer {
                properties: LayerProperties {
                    groups,
                    ..Default::default()
                },
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements,
                    tween: None,
                    label: String::new(),
                    ..Default::default()
                }],
            }],
            scripts: vec![],
        },
    })
}

/// Caption above a dialog field (`NF2/Small`, secondary).
fn field_caption(ui: &mut egui::Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.label(rich(text, TextStyle::Small).color(p.text_secondary));
}

fn kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::MovieClip => "Movie clip",
        SymbolKind::Graphic => "Graphic",
        SymbolKind::Button => "Button",
    }
}

fn kind_hint(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::MovieClip => "own timeline",
        SymbolKind::Graphic => "synced to parent",
        SymbolKind::Button => "up · over · down · hit",
    }
}

fn kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::MovieClip => "Movie Clip",
        SymbolKind::Graphic => "Graphic",
        SymbolKind::Button => "Button",
    }
}

fn conversion_bounds(
    editor: &Editor,
    draft: &SymbolConversion,
    painter: &egui::Painter,
) -> Option<Rect> {
    let single = (draft.sampled.len() == 1).then(|| transform_matrix(draft.sampled[0].1.transform));
    let scene = evaluate(&editor.project, &draft.symbol, draft.frame);
    let mut result = Rect::NOTHING;
    for item in scene {
        if !item
            .instance_path
            .first()
            .is_some_and(|id| draft.sampled.iter().any(|(_, element)| &element.id == id))
        {
            continue;
        }
        let local = stage::local_bounds(&item, painter);
        if !local.is_finite() {
            continue;
        }
        for point in [
            local.left_top(),
            local.right_top(),
            local.left_bottom(),
            local.right_bottom(),
        ] {
            let world = stage::affine(item.transform, point);
            let point = if let Some(matrix) = single {
                stage::inverse(matrix, world)?
            } else {
                world
            };
            result.extend_with(point);
        }
    }
    result.is_finite().then_some(result)
}
