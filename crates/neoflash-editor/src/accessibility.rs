use crate::app::{Editor, ScriptTarget};
use crate::{
    paint_controls::property_row,
    theme::{CONTROL_HEIGHT, UiPalette},
    workspace::PanelId,
};
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
        egui::ScrollArea::vertical().id_salt("accessibility_form").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            ui.add_enabled_ui(self.selected_editable(), |ui| {
                let mut enabled = metadata.is_some();
                if ui.checkbox(&mut enabled, "Expose object in HTML5").on_hover_text("Expose this object to assistive technology").changed() {
                    metadata = enabled.then(|| Accessibility {
                        name: if element.name.trim().is_empty() { "Button".into() } else { element.name.chars().take(256).collect() },
                        description: String::new(), role: AccessibleRole::Button, tab_order: 0, disabled: false,
                    });
                }
                if let Some(data) = &mut metadata {
                    ui.add_space(4.);
                    property_row(ui, "Name", |ui| {
                        ui.add_sized([ui.available_width(), CONTROL_HEIGHT], egui::TextEdit::singleline(&mut data.name).char_limit(256).hint_text("Accessible name"));
                    });
                    property_row(ui, "Role", |ui| {
                        egui::ComboBox::from_id_salt("accessible_role").width(ui.available_width()).truncate().selected_text(match data.role {
                            AccessibleRole::Button => "Button", AccessibleRole::Image => "Image",
                        }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut data.role, AccessibleRole::Button, "Button");
                            ui.selectable_value(&mut data.role, AccessibleRole::Image, "Image");
                        });
                    });
                    property_row(ui, "Read order", |ui| {
                        ui.add_sized([ui.available_width(), CONTROL_HEIGHT], egui::DragValue::new(&mut data.tab_order).range(0..=10000)).on_hover_text("Reading and keyboard focus order");
                    });
                    if data.role == AccessibleRole::Button {
                        property_row(ui, "State", |ui| {ui.checkbox(&mut data.disabled, "Disabled");});
                    }
                    property_row(ui, "Description", |ui| {
                        ui.add_sized([ui.available_width(), CONTROL_HEIGHT * 2.], egui::TextEdit::multiline(&mut data.description).char_limit(4096).desired_rows(2).hint_text("Optional description"));
                    });
                    if data.name.trim().is_empty() { ui.colored_label(UiPalette::current(ui.ctx()).error_text, "Enter a name before saving or publishing."); }
                    ui.add_space(4.);
                    ui.separator();
                    if data.role == AccessibleRole::Button {
                        if ui.add_sized([ui.available_width(), CONTROL_HEIGHT], egui::Button::new("Edit Click action")).clicked() {
                            self.set_playing(false);
                            self.select_single_id(element.id.clone());
                            self.show_panel(PanelId::Actions);
                            self.script_target = ScriptTarget::Object;
                            self.script_event = ObjectEvent::Click;
                        }
                        ui.small("Tab focuses this button. Enter or Space runs its Click action.");
                    } else {
                        ui.small("Images expose their name and description without entering the keyboard focus order.");
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
