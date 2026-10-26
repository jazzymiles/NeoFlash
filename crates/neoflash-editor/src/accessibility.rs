use crate::app::{BottomTab, Editor, ScriptTarget};
use eframe::egui;
use neoflash_core::*;

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
        if self.selection.len() > 1 {
            ui.strong(format!("{} objects selected", self.selection.len()));
            ui.label("Select one object to edit its accessibility information.");
            return;
        }
        let Some(element) = self.selected_sampled() else {
            ui.label("Select an object or symbol instance to describe it for the web.");
            ui.small("Buttons support keyboard focus and activation. Images expose a name and description.");
            return;
        };
        let mut metadata = element.accessibility.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_enabled_ui(self.selected_editable(), |ui| {
                let mut enabled = metadata.is_some();
                if ui.checkbox(&mut enabled, "Expose this object to assistive technology").changed() {
                    metadata = enabled.then(|| Accessibility {
                        name: if element.name.trim().is_empty() { "Button".into() } else { element.name.chars().take(256).collect() },
                        description: String::new(), role: AccessibleRole::Button, tab_order: 0, disabled: false,
                    });
                }
                if let Some(data) = &mut metadata {
                    ui.horizontal_wrapped(|ui| {
                        let label = ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut data.name).char_limit(256).desired_width(240.)).labelled_by(label.id);
                        ui.label("Role");
                        egui::ComboBox::from_id_salt("accessible_role").selected_text(match data.role {
                            AccessibleRole::Button => "Button", AccessibleRole::Image => "Image",
                        }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut data.role, AccessibleRole::Button, "Button");
                            ui.selectable_value(&mut data.role, AccessibleRole::Image, "Image");
                        });
                        let label = ui.label("Reading / focus order");
                        ui.add(egui::DragValue::new(&mut data.tab_order).range(0..=10000)).labelled_by(label.id);
                        if data.role == AccessibleRole::Button { ui.checkbox(&mut data.disabled, "Disabled"); }
                    });
                    ui.horizontal(|ui| {
                        let label = ui.label("Description");
                        ui.add(egui::TextEdit::singleline(&mut data.description).char_limit(4096).desired_width(600.)).labelled_by(label.id);
                    });
                    if data.name.trim().is_empty() { ui.colored_label(egui::Color32::DARK_RED, "Enter a name before saving or publishing."); }
                    if data.role == AccessibleRole::Button {
                        ui.horizontal_wrapped(|ui| {
                            ui.small("Tab focuses the button; Enter or Space runs its Click action in HTML5.");
                            if ui.button("Edit Click action").clicked() {
                                self.set_playing(false);
                                self.select_single_id(element.id.clone());
                                self.bottom = BottomTab::Actions;
                                self.script_target = ScriptTarget::Object;
                                self.script_event = ObjectEvent::Click;
                            }
                        });
                    } else {
                        ui.small("Images are described to assistive technology and do not enter the keyboard focus order.");
                    }
                }
            });
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
