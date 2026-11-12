//! Accessibility panel (NF2 reference `panels/*/png/panels-more-2.png`): expose the selected object
//! to assistive technology in HTML5 with a name, description, role and reading order.
use crate::app::{Editor, ScriptTarget};
use crate::paint_controls::two_column_row;
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::workspace::PanelId;
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components as nf;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, space};

const NAME_LIMIT: usize = 256;
const DESCRIPTION_LIMIT: usize = 4096;

/// Validates explicit `tab_order` (`tab_index`) read order across exposed objects on `frame`,
/// returning actionable diagnostics for duplicate `tab_order` conflicts and sequence gaps.
pub(crate) fn validate_accessibility_read_order(timeline: &Timeline, frame: u32) -> Vec<String> {
    let mut by_order: std::collections::BTreeMap<u32, Vec<String>> =
        std::collections::BTreeMap::new();
    for layer in &timeline.layers {
        if !layer.visible
            || matches!(
                layer.properties.role,
                LayerRole::Guide | LayerRole::Mask | LayerRole::Folder
            )
        {
            continue;
        }
        let Some(kf) = layer.key_at(frame) else {
            continue;
        };
        let ext_map: std::collections::HashMap<&str, &ExtendedAccessibility> = kf
            .extended_accessibility
            .iter()
            .map(|e| (e.element_id.as_str(), e))
            .collect();
        for element in &kf.elements {
            if !element.visible {
                continue;
            }
            let ext = ext_map.get(element.id.as_str()).copied();
            if ext.is_some_and(|x| x.silent) {
                continue;
            }
            let explicit_order = element
                .accessibility
                .as_ref()
                .filter(|a| !a.disabled && a.tab_order > 0)
                .map(|a| a.tab_order)
                .or_else(|| ext.and_then(|x| x.reading_order).filter(|&ro| ro > 0));
            if let Some(order) = explicit_order {
                let label = if let Some(acc) = &element.accessibility
                    && !acc.name.trim().is_empty()
                {
                    acc.name.trim().to_owned()
                } else if !element.name.trim().is_empty() {
                    element.name.trim().to_owned()
                } else {
                    element.id.clone()
                };
                by_order.entry(order).or_default().push(label);
            }
        }
    }

    let mut diagnostics = Vec::new();
    for (order, names) in &by_order {
        if names.len() > 1 {
            diagnostics.push(format!(
                "Read-order conflict: Tab order {order} is shared by {}",
                names.join(", ")
            ));
        }
    }
    let orders: Vec<u32> = by_order.keys().copied().collect();
    if let Some(&first) = orders.first()
        && first > 1
    {
        diagnostics.push(format!(
            "Read-order gap: Tab order starts at {first} instead of 1."
        ));
    }
    for pair in orders.windows(2) {
        if pair[1] > pair[0] + 1 {
            diagnostics.push(format!(
                "Read-order gap: missing Tab order between {} and {}.",
                pair[0], pair[1]
            ));
        }
    }
    diagnostics
}

impl Editor {
    pub(crate) fn accessibility_read_order_diagnostics(&self) -> Vec<String> {
        validate_accessibility_read_order(&self.symbol().timeline, self.frame)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn selected_extended_accessibility(&self) -> Option<ExtendedAccessibility> {
        let target = self.selection.single()?;
        let layer = self
            .symbol()
            .timeline
            .layers
            .iter()
            .find(|l| l.id == target.layer)?;
        let kf = layer.key_at(self.frame)?;
        kf.extended_accessibility
            .iter()
            .find(|e| e.element_id == target.element)
            .cloned()
    }

    #[allow(dead_code)]
    pub(crate) fn set_extended_accessibility(&mut self, mut ext: ExtendedAccessibility) {
        if !self.selected_editable() {
            return;
        }
        let Some(target) = self.selection.single().cloned() else {
            return;
        };
        ext.element_id = target.element.clone();
        self.finish_edits();
        self.select_single_id(target.element.clone());
        self.set_playing(false);
        let frame = self.frame;
        self.transact(|editor| {
            editor.ensure_selected_key();
            if let Some(layer) = editor
                .symbol_mut()
                .timeline
                .layers
                .iter_mut()
                .find(|l| l.id == target.layer)
                && let Some(key_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame)
            {
                let list = &mut layer.keyframes[key_idx].extended_accessibility;
                list.retain(|item| item.element_id != target.element);
                if !ext.is_default() {
                    list.push(ext.clone());
                }
            }
            editor.project.upgrade_format_for_features();
        });
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_accessibility_aria_bindings(
        &mut self,
        role: ContentAccessibleRole,
        live: AccessibilityLivePoliteness,
        description: Option<String>,
        silent: bool,
        shortcut: Option<String>,
    ) {
        if !self.selected_editable() {
            return;
        }
        let Some(target) = self.selection.single().cloned() else {
            return;
        };
        let Some(sampled) = self.selected_sampled() else {
            return;
        };
        let mut next_acc = sampled.accessibility.clone().unwrap_or(Accessibility {
            name: if sampled.name.trim().is_empty() {
                "Object".into()
            } else {
                sampled.name.chars().take(NAME_LIMIT).collect()
            },
            description: String::new(),
            role: if role == ContentAccessibleRole::Image {
                AccessibleRole::Image
            } else {
                AccessibleRole::Button
            },
            tab_order: 0,
            disabled: false,
        });
        if let Some(desc) = description {
            next_acc.description = desc.chars().take(DESCRIPTION_LIMIT).collect();
        }
        next_acc.role = if role == ContentAccessibleRole::Image {
            AccessibleRole::Image
        } else {
            AccessibleRole::Button
        };

        let mut ext =
            self.selected_extended_accessibility()
                .unwrap_or_else(|| ExtendedAccessibility {
                    element_id: target.element.clone(),
                    ..ExtendedAccessibility::default()
                });
        ext.element_id = target.element.clone();
        ext.role = role;
        ext.live = live;
        ext.silent = silent;
        if let Some(sc) = shortcut {
            ext.shortcut = sc;
        }

        self.finish_edits();
        self.select_single_id(target.element.clone());
        self.set_playing(false);
        let frame = self.frame;
        self.transact(|editor| {
            editor.ensure_selected_key();
            if let Some(el) = editor.selected_element_mut() {
                el.accessibility = Some(next_acc.clone());
            }
            if let Some(layer) = editor
                .symbol_mut()
                .timeline
                .layers
                .iter_mut()
                .find(|l| l.id == target.layer)
                && let Some(key_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame)
            {
                let list = &mut layer.keyframes[key_idx].extended_accessibility;
                list.retain(|item| item.element_id != target.element);
                if !ext.is_default() {
                    list.push(ext.clone());
                }
            }
            editor.project.upgrade_format_for_features();
        });
    }

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
                ui.set_max_width(ui.available_width().min(2. * neoflash_ui::metrics::inspector()));
            ui.spacing_mut().item_spacing.y = space::S2;
            ui.add_enabled_ui_semantic(self.selected_editable(), |ui| {
                let mut enabled = metadata.is_some();
                let expose = ui.add(nf::Switch::new(&mut enabled, "Expose object in HTML5"));
                expose.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Checkbox,
                        ui.is_enabled(),
                        enabled,
                        "Expose object in HTML5",
                    )
                });
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
                    let name_resp = ui.add(nf::TextField::new(&mut data.name, "Accessible name").prefix("Name").hint("Accessible name"));
                    name_resp.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::TextEdit,
                            ui.is_enabled(),
                            "Accessible name",
                        )
                    });
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
                    let desc_resp = ui.add(nf::TextArea::new(&mut data.description, "Description").hint("Optional description").rows(3).char_limit(DESCRIPTION_LIMIT));
                    desc_resp.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::TextEdit,
                            ui.is_enabled(),
                            "Accessible description",
                        )
                    });
                });
                nf::SectionHeader::new("Keyboard and role").id_salt("accessibility-keyboard").show(ui, |ui| {
                    let mut role = data.role;
                    let mut order = data.tab_order as f64;
                    two_column_row(ui, |ui, width| {
                        let label = match role { AccessibleRole::Button => "Role: Button", AccessibleRole::Image => "Role: Image" };
                        let role_resp = nf::Select::new("accessible_role", label).width(width).show(ui, |ui| {
                            for (value, label) in [(AccessibleRole::Button, "Button"), (AccessibleRole::Image, "Image")] {
                                if ui.add(nf::MenuItem::new(label).checked(role == value)).clicked() {
                                    role = value;
                                }
                            }
                        }).0;
                        role_resp.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::ComboBox,
                                ui.is_enabled(),
                                "Accessible role",
                            )
                        });
                        role_resp
                    }, |ui, width| {
                        let field = ui.add(nf::Field::new(&mut order, "Read order").prefix("Tab").stepper().range(0.0..=10000.0).width(width));
                        field.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::DragValue,
                                ui.is_enabled(),
                                "Read order",
                            )
                        });
                        nf::tooltip(field, "Reading and keyboard focus order", None);
                    });
                    data.role = role;
                    data.tab_order = order.round().clamp(0., 10000.) as u32;
                    for diag in self.accessibility_read_order_diagnostics() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S2;
                            neoflash_ui::icons::show(ui, i16::ERROR, p.danger_default);
                            ui.label(
                                neoflash_ui::typography::rich(&diag, TextStyle::Small)
                                    .color(p.danger_default),
                            );
                        });
                    }
                    if data.role == AccessibleRole::Button {
                        let disabled_resp = ui.add(nf::Checkbox::new(&mut data.disabled, "Disabled"));
                        disabled_resp.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Checkbox,
                                ui.is_enabled(),
                                data.disabled,
                                "Disabled",
                            )
                        });
                        let edit_click_resp = ui.add(nf::Button::new("Edit Click action").icon(i16::CODE));
                        edit_click_resp.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                "Edit Click action",
                            )
                        });
                        if edit_click_resp.clicked() {
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
        assert_eq!(editor.symbol().timeline.layers[1].keyframes.len(), 1);
        assert_eq!(
            editor.symbol().timeline.layers[0],
            before.symbols[0].timeline.layers[0]
        );
        assert_eq!(
            editor.symbol().timeline.layers[1].keyframes[0].elements[0].accessibility,
            Some(metadata())
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

    #[test]
    fn accessibility_read_order_validation_and_extended_aria_bindings_work() {
        let mut editor = editor_with_object();
        let first_id = editor.selected_element().unwrap().id.clone();
        editor.set_accessibility(Some(Accessibility {
            name: "First Button".into(),
            description: "Primary CTA".into(),
            role: AccessibleRole::Button,
            tab_order: 2,
            disabled: false,
        }));

        editor.add_element(
            ElementKind::Rectangle {
                width: 40.,
                height: 20.,
                fill: [80, 120, 40, 255],
                stroke: [0; 4],
                stroke_width: 0.,
            },
            80.,
            24.,
        );
        editor.set_accessibility(Some(Accessibility {
            name: "Second Button".into(),
            description: "Secondary CTA".into(),
            role: AccessibleRole::Button,
            tab_order: 2,
            disabled: false,
        }));

        let diags = editor.accessibility_read_order_diagnostics();
        assert_eq!(diags.len(), 2);
        assert!(diags[0].contains("Read-order conflict") && diags[0].contains("Tab order 2"));
        assert!(diags[1].contains("Read-order gap") && diags[1].contains("starts at 2"));

        // Fix read order to 1 and 2, and apply extended ARIA bindings on the second element.
        editor.select_single_id(first_id);
        editor.set_accessibility(Some(Accessibility {
            name: "First Button".into(),
            description: "Primary CTA".into(),
            role: AccessibleRole::Button,
            tab_order: 1,
            disabled: false,
        }));
        assert!(editor.accessibility_read_order_diagnostics().is_empty());

        let before_ext = editor.project.clone();
        editor.set_accessibility_aria_bindings(
            ContentAccessibleRole::Heading,
            AccessibilityLivePoliteness::Polite,
            Some("Updated live heading".into()),
            false,
            Some("Alt+H".into()),
        );
        let ext = editor.selected_extended_accessibility().unwrap();
        assert_eq!(ext.role, ContentAccessibleRole::Heading);
        assert_eq!(ext.live, AccessibilityLivePoliteness::Polite);
        assert_eq!(ext.shortcut, "Alt+H");
        assert!(!ext.silent);
        assert_eq!(
            editor
                .selected_element()
                .unwrap()
                .accessibility
                .as_ref()
                .unwrap()
                .description,
            "Updated live heading"
        );
        editor.project.validate().unwrap();

        editor.undo();
        assert_eq!(editor.project, before_ext);
    }
}
