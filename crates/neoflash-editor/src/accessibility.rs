use crate::app::{BottomTab, Editor, ScriptTarget};
use eframe::egui;
use neoflash_core::*;

impl Editor {
    pub(crate) fn set_accessibility(&mut self, accessibility: Option<Accessibility>) {
        if !self.layer_editable() || self.selected_sampled().is_none() {
            return;
        }
        self.playing = false;
        self.transact(|editor| {
            editor.ensure_key(false);
            if let Some(element) = editor.selected_element_mut() {
                element.accessibility = accessibility;
            }
        });
    }

    pub(crate) fn accessibility_panel(&mut self, ui: &mut egui::Ui) {
        let Some(element) = self.selected_sampled() else {
            ui.label("Select an object or symbol instance to describe it for the web.");
            ui.small("Buttons support keyboard focus and activation. Images expose a name and description.");
            return;
        };
        let mut metadata = element.accessibility.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_enabled_ui(self.layer_editable(), |ui| {
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
                                self.playing = false;
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
