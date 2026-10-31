//! Accessibility panel (NF2 reference `panels/*/png/panels-more-2.png`): expose the selected object
//! to assistive technology in HTML5 with a name, description, role and reading order.
use crate::app::{Editor, ScriptTarget};
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::workspace::PanelId;
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components as nf;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, space};

const NAME_LIMIT: usize = 256;
const DESCRIPTION_LIMIT: usize = 4096;

impl Editor {
    pub(crate) fn set_accessibility(&mut self, accessibility: Option<Accessibility>) {
        if !self.selected_editable() {
            return;
        }
        let Some(element) = self.selected_sampled() else {
            return;
        };
        self.finish_edits();
        self.select_single_id(element.id);
        self.set_playing(false);
        self.transact(|editor| {
            editor.ensure_selected_key();
            if let Some(element) = editor.selected_element_mut() {
                element.accessibility = accessibility;
            }
        });
    }

    pub(crate) fn accessibility_panel(&mut self, ui: &mut egui::Ui) {
        let p = neoflash_ui::palette(ui.ctx());
        let note = |ui: &mut egui::Ui, text: &str| {
            ui.label(neoflash_ui::typography::rich(text, TextStyle::Small).color(p.text_tertiary));
        };
        if self.selection.len() > 1 {
            let title = format!("{} objects selected", self.selection.len());
            nf::EmptyState {
                icon: i16::ACCESS,
                title: &title,
                body: "Select one object to edit its accessibility information.",
                action: None,
            }
            .show(ui);
            return;
        }
        let Some(element) = self.selected_sampled() else {
            nf::EmptyState {
                icon: i16::ACCESS,
                title: "No object selected",
                body: "Select an object or symbol instance to describe it for the web.",
                action: None,
            }
            .show(ui);
            ui.vertical_centered(|ui| {
                note(ui, "Buttons support keyboard focus and activation. Images expose a name and description.")
            });
            return;
        };
        ui.add(
            egui::Label::new(
                neoflash_ui::typography::rich(
                    if element.name.is_empty() {
                        "Selected object"
                    } else {
                        &element.name
                    },
                    TextStyle::BodyStrong,
                )
                .color(p.text_primary),
            )
            .truncate(),
        )
        .on_hover_text(&element.name);
        ui.add_space(space::S1);
        let mut metadata = element.accessibility.clone();
        egui::ScrollArea::vertical().id_salt("accessibility_form").auto_shrink([false, false]).themed().show(ui, |ui| {
            // Wide docks keep the form at two inspector widths (NF2 floating panel proportions).
                ui.set_max_width(ui.available_width().min(2. * neoflash_ui::metrics::INSPECTOR));
            ui.spacing_mut().item_spacing.y = space::S2;
            ui.add_enabled_ui_semantic(self.selected_editable(), |ui| {
                let mut enabled = metadata.is_some();
                let expose = ui.add(nf::Switch::new(&mut enabled, "Expose object in HTML5"));
                if nf::tooltip(expose, "Expose this object to assistive technology", None).changed() {
                    metadata = enabled.then(|| Accessibility {
                        name: if element.name.trim().is_empty() { "Button".into() } else { element.name.chars().take(NAME_LIMIT).collect() },
                        description: String::new(), role: AccessibleRole::Button, tab_order: 0, disabled: false,
                    });
                }
                let Some(data) = &mut metadata else {
                    return;
                };
                nf::SectionHeader::new("Name and description").id_salt("accessibility-name").show(ui, |ui| {
                    ui.add(nf::TextField::new(&mut data.name, "Accessible name").prefix("Name").hint("Accessible name"));
                    if data.name.chars().count() > NAME_LIMIT {
                        data.name = data.name.chars().take(NAME_LIMIT).collect();
                    }
                    if data.name.trim().is_empty() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S2;
                            neoflash_ui::icons::show(ui, i16::ERROR, p.danger_default);
                            ui.label(neoflash_ui::typography::rich("Enter a name before saving or publishing.", TextStyle::Small).color(p.danger_default));
                        });
                    }
                    ui.add(nf::TextArea::new(&mut data.description, "Description").hint("Optional description").rows(3).char_limit(DESCRIPTION_LIMIT));
                });
                nf::SectionHeader::new("Keyboard and role").id_salt("accessibility-keyboard").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        let half = ((ui.available_width() - space::S2) / 2.).max(80.);
                        let role = match data.role { AccessibleRole::Button => "Role: Button", AccessibleRole::Image => "Role: Image" };
                        nf::Select::new("accessible_role", role).width(half).show(ui, |ui| {
                            for (value, label) in [(AccessibleRole::Button, "Button"), (AccessibleRole::Image, "Image")] {
                                if ui.add(nf::MenuItem::new(label).checked(data.role == value)).clicked() {
                                    data.role = value;
                                }
                            }
                        });
                        let mut order = data.tab_order as f64;
                        let field = ui.add(nf::Field::new(&mut order, "Read order").prefix("Tab").stepper().range(0.0..=10000.0).width(half));
                        nf::tooltip(field, "Reading and keyboard focus order", None);
                        data.tab_order = order.round().clamp(0., 10000.) as u32;
                    });
                    if data.role == AccessibleRole::Button {
                        ui.add(nf::Checkbox::new(&mut data.disabled, "Disabled"));
                        if ui.add(nf::Button::new("Edit Click action").icon(i16::CODE)).clicked() {
                            self.set_playing(false);
                            self.select_single_id(element.id.clone());
                            self.show_panel(PanelId::Actions);
                            self.script_target = ScriptTarget::Object;
                            self.script_event = ObjectEvent::Click;
                        }
                        note(ui, "Tab focuses this button. Enter or Space runs its Click action.");
                    } else {
                        note(ui, "Images expose their name and description without entering the keyboard focus order.");
                    }
                });
            }).response.on_disabled_hover_text("Unlock the selected layer to edit this object's accessibility.");
        });
        if metadata != element.accessibility {
            self.set_accessibility(metadata);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor_with_object() -> Editor {
        let mut editor = Editor::from_project(&egui::Context::default(), Project::blank());
        editor.add_element(
            ElementKind::Rectangle {
                width: 40.,
                height: 20.,
                fill: [30, 80, 170, 255],
                stroke: [0; 4],
                stroke_width: 0.,
            },
            12.,
            24.,
        );
        editor.history.clear();
        editor
    }

    fn metadata() -> Accessibility {
        Accessibility {
            name: "Details".into(),
            description: String::new(),
            role: AccessibleRole::Button,
            tab_order: 0,
            disabled: false,
        }
    }

    #[test]
    fn accessibility_resolves_selected_layer_after_reorder_and_ignores_active_layer() {
        let mut editor = editor_with_object();
        let selected_layer = editor.selection.single().unwrap().layer.clone();
        editor.symbol_mut().timeline.layers.push(Layer {
            properties: Default::default(),
            id: new_id("layer"),
            name: "Locked insertion layer".into(),
            visible: true,
            locked: true,
            keyframes: vec![],
        });
        editor.symbol_mut().timeline.layers.swap(0, 1);
        editor.layer = 0;
        editor.frame = 5;
        let before = editor.project.clone();
        editor.set_accessibility(Some(metadata()));
        assert_eq!(editor.symbol().timeline.layers[1].id, selected_layer);
        assert_eq!(
            editor.selected_element().unwrap().accessibility,
            Some(metadata())
        );
        assert_eq!(editor.symbol().timeline.layers[1].keyframes.len(), 2);
        assert_eq!(
            editor.symbol().timeline.layers[0],
            before.symbols[0].timeline.layers[0]
        );
        assert_eq!(
            editor.symbol().timeline.layers[1].keyframes[0],
            before.symbols[0].timeline.layers[1].keyframes[0]
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }

    #[test]
    fn accessibility_rejects_multiple_targets_without_editing_one_of_them() {
        let mut editor = editor_with_object();
        let first = editor.selection.single().unwrap().clone();
        editor.duplicate();
        let second = editor.selection.single().unwrap().clone();
        editor.set_selection(vec![first, second]);
        editor.history.clear();
        let before = editor.project.clone();
        editor.set_accessibility(Some(metadata()));
        assert_eq!(editor.selection.len(), 2);
        assert_eq!(editor.project, before);
        assert!(!editor.history.can_undo());
    }
}
