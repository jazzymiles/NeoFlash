//! Layer properties modal dialog (NF2 `Dialog / Layer properties`, DL-16; reference
//! `symbols-panels.png`), opened from `Modify ▸ Timeline ▸ Layer properties…`,
//! `Layer menu ▸ Properties…`, or double-clicking a layer row's type icon.
//!
//! Edits live on a draft in `egui::Context` temp memory until confirmed: `Cancel` / `Esc` discards
//! without touching the project, while `OK` / `Enter` commits document layer properties in a single
//! Undo step (`"Layer properties"`) and updates timeline row-height preferences.
use super::planned_dialogs::group;
use crate::app::Editor;
use crate::workspace::FramePreview;
use eframe::egui::{self, Color32};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;

const ARMATURE_REASON: &str = "Armature layers";

const OUTLINE_PRESETS: [[u8; 3]; 8] = [
    [62, 153, 235],
    [212, 95, 193],
    [45, 182, 150],
    [229, 137, 54],
    [135, 120, 225],
    [225, 100, 107],
    [68, 171, 185],
    [165, 162, 64],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayerTypeChoice {
    Normal,
    Guide,
    Guided,
    Mask,
    Masked,
    Folder,
}

impl LayerTypeChoice {
    pub(crate) const ALL: [Self; 6] = [
        Self::Normal,
        Self::Guide,
        Self::Guided,
        Self::Mask,
        Self::Masked,
        Self::Folder,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Guide => "Guide",
            Self::Guided => "Guided",
            Self::Mask => "Mask",
            Self::Masked => "Masked",
            Self::Folder => "Folder",
        }
    }

    fn from_layer(layer: &Layer) -> Self {
        match layer.properties.role {
            LayerRole::Folder => Self::Folder,
            LayerRole::Guide => Self::Guide,
            LayerRole::Mask => Self::Mask,
            LayerRole::Normal if layer.properties.mask.is_some() => Self::Masked,
            LayerRole::Normal if layer.properties.motion_guide.is_some() => Self::Guided,
            LayerRole::Normal => Self::Normal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayerHeightChoice {
    Short,
    Normal100,
    Preview200,
}

impl LayerHeightChoice {
    pub(crate) const ALL: [Self; 3] = [Self::Short, Self::Normal100, Self::Preview200];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Short => "Layer height: Short",
            Self::Normal100 => "Layer height: 100 %",
            Self::Preview200 => "Layer height: 200 %",
        }
    }

    pub(crate) const fn menu_label(self) -> &'static str {
        match self {
            Self::Short => "Short",
            Self::Normal100 => "100 %",
            Self::Preview200 => "200 %",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LayerPropertiesDraft {
    pub(crate) symbol_id: Id,
    pub(crate) layer_id: Id,
    pub(crate) name: String,
    pub(crate) visible: bool,
    pub(crate) locked: bool,
    pub(crate) kind: LayerTypeChoice,
    pub(crate) outline_color: Color32,
    pub(crate) outline: bool,
    pub(crate) height: LayerHeightChoice,
    pub(crate) existing_mask: Option<Id>,
    pub(crate) existing_motion_guide: Option<Id>,
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-layer-properties-draft")
}

pub(crate) fn has_layer_properties_dialog(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<LayerPropertiesDraft>(draft_id()))
        .is_some()
}

#[cfg(test)]
pub(crate) fn layer_properties_draft(ctx: &egui::Context) -> Option<LayerPropertiesDraft> {
    ctx.data(|d| d.get_temp::<LayerPropertiesDraft>(draft_id()))
}

#[cfg(test)]
pub(crate) fn edit_layer_properties_draft(
    ctx: &egui::Context,
    f: impl FnOnce(&mut LayerPropertiesDraft),
) {
    let id = draft_id();
    if let Some(mut draft) = ctx.data(|d| d.get_temp::<LayerPropertiesDraft>(id)) {
        f(&mut draft);
        ctx.data_mut(|d| d.insert_temp(id, draft));
    }
}

fn pick_target_layer(
    layers: &[Layer],
    index: usize,
    preferred: Option<&str>,
    role: LayerRole,
) -> Option<Id> {
    let self_id = &layers[index].id;
    if let Some(pref) = preferred
        && layers
            .iter()
            .any(|l| l.id == pref && &l.id != self_id && l.properties.role == role)
    {
        return Some(pref.to_owned());
    }
    // Higher index in `timeline.layers` is visually above `index`.
    (index + 1..layers.len())
        .find(|&i| layers[i].properties.role == role)
        .or_else(|| (0..index).rfind(|&i| layers[i].properties.role == role))
        .map(|i| layers[i].id.clone())
}

fn layer_can_be_folder(layer: &Layer) -> bool {
    !layer
        .keyframes
        .iter()
        .any(|k| !k.elements.is_empty() || k.sound.is_some())
}

fn layer_can_be_mask(project: &Project, symbol_id: &str, layer_index: usize) -> bool {
    let mut candidate = project.clone();
    let Some(symbol) = candidate.symbol_mut(symbol_id) else {
        return false;
    };
    let Some(layer) = symbol.timeline.layers.get_mut(layer_index) else {
        return false;
    };
    let layer_id = layer.id.clone();
    let was_folder = layer.properties.role == LayerRole::Folder;
    layer.properties.role = LayerRole::Mask;
    layer.properties.mask = None;
    layer.properties.motion_guide = None;
    if was_folder && layer.keyframes.is_empty() {
        layer.keyframes.push(Keyframe::default());
    }
    for other in &mut symbol.timeline.layers {
        if other.properties.parent.as_deref() == Some(layer_id.as_str()) {
            other.properties.parent = None;
        }
        if other.properties.motion_guide.as_deref() == Some(layer_id.as_str()) {
            other.properties.motion_guide = None;
        }
    }
    candidate.format_version = candidate.format_version.max(4);
    candidate.validate().is_ok()
}

impl Editor {
    fn current_layer_height_choice(&self) -> LayerHeightChoice {
        let view = &self.workspace.timeline;
        if view.preview != FramePreview::Off {
            LayerHeightChoice::Preview200
        } else if view.short_rows {
            LayerHeightChoice::Short
        } else {
            LayerHeightChoice::Normal100
        }
    }

    fn apply_layer_height_choice(&mut self, choice: LayerHeightChoice) {
        let view = &mut self.workspace.timeline;
        match choice {
            LayerHeightChoice::Short => {
                view.short_rows = true;
                view.preview = FramePreview::Off;
            }
            LayerHeightChoice::Normal100 => {
                view.short_rows = false;
                view.preview = FramePreview::Off;
            }
            LayerHeightChoice::Preview200 => {
                view.short_rows = false;
                if view.preview == FramePreview::Off {
                    view.preview = FramePreview::Layer;
                }
            }
        }
    }

    /// Open the `Layer properties` modal dialog for `layer_index` on the active symbol timeline.
    pub(crate) fn open_layer_properties(&mut self, ctx: &egui::Context, layer_index: usize) {
        self.finish_edits();
        let symbol_id = self.symbol_id().to_owned();
        let Some(layer) = self.symbol().timeline.layers.get(layer_index).cloned() else {
            return;
        };
        let color = layer_outline_color(&layer);
        let draft = LayerPropertiesDraft {
            symbol_id,
            layer_id: layer.id.clone(),
            name: layer.name.clone(),
            visible: layer.visible,
            locked: layer.locked,
            kind: LayerTypeChoice::from_layer(&layer),
            outline_color: Color32::from_rgb(color[0], color[1], color[2]),
            outline: layer.properties.outline,
            height: self.current_layer_height_choice(),
            existing_mask: layer.properties.mask.clone(),
            existing_motion_guide: layer.properties.motion_guide.clone(),
        };
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    pub(crate) fn cancel_layer_properties(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.remove::<LayerPropertiesDraft>(draft_id()));
    }

    pub(crate) fn confirm_layer_properties(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(draft) = ctx.data(|d| d.get_temp::<LayerPropertiesDraft>(draft_id())) else {
            return Ok(());
        };
        let mut candidate = self.project.clone();
        let Some(symbol) = candidate.symbol_mut(&draft.symbol_id) else {
            ctx.data_mut(|d| d.remove::<LayerPropertiesDraft>(draft_id()));
            return Err("Symbol no longer exists.".into());
        };
        let Some(index) = symbol
            .timeline
            .layers
            .iter()
            .position(|l| l.id == draft.layer_id)
        else {
            ctx.data_mut(|d| d.remove::<LayerPropertiesDraft>(draft_id()));
            return Err("Layer no longer exists.".into());
        };

        let target_guide = if draft.kind == LayerTypeChoice::Guided {
            let Some(id) = pick_target_layer(
                &symbol.timeline.layers,
                index,
                draft.existing_motion_guide.as_deref(),
                LayerRole::Guide,
            ) else {
                return Err("Guided layers require a Guide layer in the timeline.".into());
            };
            Some(id)
        } else {
            None
        };
        let target_mask = if draft.kind == LayerTypeChoice::Masked {
            let Some(id) = pick_target_layer(
                &symbol.timeline.layers,
                index,
                draft.existing_mask.as_deref(),
                LayerRole::Mask,
            ) else {
                return Err("Masked layers require a Mask layer in the timeline.".into());
            };
            Some(id)
        } else {
            None
        };

        {
            let layer = &mut symbol.timeline.layers[index];
            if draft.kind == LayerTypeChoice::Folder && !layer_can_be_folder(layer) {
                return Err("Only an empty layer can become a folder.".into());
            }
            let was_folder = layer.properties.role == LayerRole::Folder;
            let trimmed = draft.name.trim();
            if !trimmed.is_empty() {
                layer.name = trimmed.to_owned();
            }
            layer.visible = draft.visible;
            let rgb = [
                draft.outline_color.r(),
                draft.outline_color.g(),
                draft.outline_color.b(),
            ];
            if layer_outline_color(layer)[..3] != rgb {
                layer.properties.outline_color = Some(rgb);
            }
            match draft.kind {
                LayerTypeChoice::Normal => {
                    layer.properties.role = LayerRole::Normal;
                    layer.properties.mask = None;
                    layer.properties.motion_guide = None;
                    layer.locked = draft.locked;
                    layer.properties.outline = draft.outline;
                }
                LayerTypeChoice::Guide => {
                    layer.properties.role = LayerRole::Guide;
                    layer.properties.mask = None;
                    layer.properties.motion_guide = None;
                    layer.locked = draft.locked;
                    layer.properties.outline = draft.outline;
                }
                LayerTypeChoice::Guided => {
                    layer.properties.role = LayerRole::Normal;
                    layer.properties.mask = None;
                    layer.properties.motion_guide = target_guide;
                    layer.locked = draft.locked;
                    layer.properties.outline = draft.outline;
                }
                LayerTypeChoice::Mask => {
                    layer.properties.role = LayerRole::Mask;
                    layer.properties.mask = None;
                    layer.properties.motion_guide = None;
                    layer.locked = draft.locked;
                    layer.properties.outline = draft.outline;
                }
                LayerTypeChoice::Masked => {
                    layer.properties.role = LayerRole::Normal;
                    layer.properties.motion_guide = None;
                    layer.properties.mask = target_mask;
                    layer.locked = draft.locked;
                    layer.properties.outline = draft.outline;
                }
                LayerTypeChoice::Folder => {
                    layer.properties.role = LayerRole::Folder;
                    layer.properties.mask = None;
                    layer.properties.motion_guide = None;
                    layer.locked = false;
                    layer.properties.outline = false;
                    layer.keyframes.clear();
                }
            }
            if was_folder && draft.kind != LayerTypeChoice::Folder && layer.keyframes.is_empty() {
                layer.keyframes.push(Keyframe::default());
            }
        }

        let new_role = symbol.timeline.layers[index].properties.role;
        for (i, other) in symbol.timeline.layers.iter_mut().enumerate() {
            if i == index {
                continue;
            }
            if new_role != LayerRole::Mask
                && other.properties.mask.as_deref() == Some(draft.layer_id.as_str())
            {
                other.properties.mask = None;
            }
            if new_role != LayerRole::Guide
                && other.properties.motion_guide.as_deref() == Some(draft.layer_id.as_str())
            {
                other.properties.motion_guide = None;
            }
            if new_role != LayerRole::Folder
                && other.properties.parent.as_deref() == Some(draft.layer_id.as_str())
            {
                other.properties.parent = None;
            }
        }

        let has_non_default_props = symbol
            .timeline
            .layers
            .iter()
            .any(|l| !l.properties.is_default());
        let has_motion_guide = symbol
            .timeline
            .layers
            .iter()
            .any(|l| l.properties.motion_guide.is_some());
        if has_non_default_props {
            candidate.format_version = candidate.format_version.max(4);
        }
        if has_motion_guide {
            candidate.format_version = candidate.format_version.max(6);
        }

        candidate.validate().map_err(|e| e.to_string())?;
        self.apply_layer_height_choice(draft.height);
        if candidate != self.project {
            self.transact_named("Layer properties", |s| {
                s.project = candidate;
            });
            self.normalize_selection();
        }
        ctx.data_mut(|d| d.remove::<LayerPropertiesDraft>(draft_id()));
        Ok(())
    }

    pub(crate) fn layer_properties_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<LayerPropertiesDraft>(draft_id())) else {
            return;
        };
        let Some((layer_index, has_other_guide, has_other_mask, can_be_folder, can_be_mask)) = ({
            self.project.symbol(&draft.symbol_id).and_then(|sym| {
                let idx = sym
                    .timeline
                    .layers
                    .iter()
                    .position(|l| l.id == draft.layer_id)?;
                let has_guide = sym
                    .timeline
                    .layers
                    .iter()
                    .enumerate()
                    .any(|(i, l)| i != idx && l.properties.role == LayerRole::Guide);
                let has_mask = sym
                    .timeline
                    .layers
                    .iter()
                    .enumerate()
                    .any(|(i, l)| i != idx && l.properties.role == LayerRole::Mask);
                let can_folder = layer_can_be_folder(&sym.timeline.layers[idx]);
                let can_mask = layer_can_be_mask(&self.project, &draft.symbol_id, idx);
                Some((idx, has_guide, has_mask, can_folder, can_mask))
            })
        }) else {
            self.cancel_layer_properties(ctx);
            return;
        };
        let _ = layer_index;

        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut close_ok = false;

        let modal = egui::Modal::new(egui::Id::new("layer-properties"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(440.0);
                let header_top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Layer properties",
                }
                .show(ui);
                ui.painter().hline(
                    header_top.x..=header_top.x + 440.0,
                    header_top.y + metrics::DIALOG_HEADER - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );

                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        let w = 440.0 - 2.0 * space::S5;
                        ui.set_width(w);
                        ui.spacing_mut().item_spacing.y = space::S3;

                        ui.add(
                            c::TextField::new(&mut draft.name, "Name")
                                .prefix("Name")
                                .width(w),
                        );
                        ui.horizontal(|ui| {
                            ui.add(c::Switch::new(&mut draft.visible, "Visible"));
                            ui.add_enabled(
                                draft.kind != LayerTypeChoice::Folder,
                                c::Switch::new(&mut draft.locked, "Locked"),
                            );
                        });

                        group(ui, "Type");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(space::S4, space::S2);
                            for choice in LayerTypeChoice::ALL {
                                let enabled = match choice {
                                    LayerTypeChoice::Normal | LayerTypeChoice::Guide => true,
                                    LayerTypeChoice::Guided => {
                                        has_other_guide || draft.kind == LayerTypeChoice::Guided
                                    }
                                    LayerTypeChoice::Mask => {
                                        can_be_mask || draft.kind == LayerTypeChoice::Mask
                                    }
                                    LayerTypeChoice::Masked => {
                                        has_other_mask || draft.kind == LayerTypeChoice::Masked
                                    }
                                    LayerTypeChoice::Folder => {
                                        can_be_folder || draft.kind == LayerTypeChoice::Folder
                                    }
                                };
                                if ui
                                    .add_enabled(
                                        enabled,
                                        c::Radio::new(draft.kind == choice, choice.label()),
                                    )
                                    .clicked()
                                {
                                    draft.kind = choice;
                                }
                            }
                            let armature = ui.add_enabled(false, c::Radio::new(false, "Armature"));
                            let _ = c::not_available(armature, ARMATURE_REASON);
                        });

                        group(ui, "Outline");
                        ui.horizontal(|ui| {
                            ui.label(rich("Colour", TextStyle::Body).color(p.text_secondary));
                            let swatch = ui.add_enabled(
                                draft.kind != LayerTypeChoice::Folder,
                                c::SwatchCell {
                                    color: draft.outline_color,
                                    selected: true,
                                },
                            );
                            c::popup_below(&swatch, 232.0, |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(space::S1, space::S2);
                                ui.horizontal_wrapped(|ui| {
                                    for rgb in OUTLINE_PRESETS {
                                        let preset = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                                        if ui
                                            .add(c::SwatchCell {
                                                color: preset,
                                                selected: draft.outline_color == preset,
                                            })
                                            .clicked()
                                        {
                                            draft.outline_color = preset;
                                        }
                                    }
                                });
                                c::ColorField::new(&mut draft.outline_color, "Outline colour")
                                    .width(216.0)
                                    .show(ui);
                            });
                            ui.add_enabled(
                                draft.kind != LayerTypeChoice::Folder,
                                c::Checkbox::new(&mut draft.outline, "View layer as outlines"),
                            );
                        });

                        c::Select::new("layer-properties-height", draft.height.label())
                            .width(200.0)
                            .show(ui, |ui| {
                                for choice in LayerHeightChoice::ALL {
                                    if ui
                                        .add(
                                            c::MenuItem::new(choice.menu_label())
                                                .checked(draft.height == choice),
                                        )
                                        .clicked()
                                    {
                                        draft.height = choice;
                                        ui.close();
                                    }
                                }
                            });

                        ui.add_space(space::S2);
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui.add(c::Button::primary("OK")).clicked() {
                                    close_ok = true;
                                }
                                if ui.add(c::Button::new("Cancel")).clicked() {
                                    close_cancel = true;
                                }
                            },
                        );
                    });
            });

        if !close_cancel && !close_ok {
            let typing = ctx.wants_keyboard_input();
            let popup_open = egui::Popup::is_any_open(ctx);
            if !popup_open
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                close_cancel = true;
            } else if !typing
                && !popup_open
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            {
                close_ok = true;
            } else if modal.should_close() {
                close_cancel = true;
            }
        }

        if close_cancel {
            self.cancel_layer_properties(ctx);
        } else if close_ok {
            ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
            if let Err(err) = self.confirm_layer_properties(ctx) {
                self.message(format!("Layer properties rejected: {err}"));
            }
        } else {
            ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
        }
    }
}
