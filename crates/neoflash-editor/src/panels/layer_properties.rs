//! Layer properties modal dialog (NF2 `Dialog / Layer properties`, DL-16; reference
//! `symbols-panels.png`), opened from `Modify ▸ Timeline ▸ Layer properties…`,
//! `Layer menu ▸ Properties…`, or double-clicking a layer row's type icon.
//!
//! Edits live on a draft in `egui::Context` temp memory until confirmed: `Cancel` / `Esc` discards
//! without touching the project, while `OK` / `Enter` commits document layer properties in a single
//! Undo step (`"Layer properties"`) and updates timeline row-height preferences.
use super::dialog_helpers::{
    FormDialog, first_field, fixed_dialog_size, form_dialog_height, form_hint, form_row,
    group_caption,
};
use crate::app::Editor;
use crate::workspace::FramePreview;
use eframe::egui::{self, Color32};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::metrics;
use neoflash_ui::tokens::space;

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
    Compact,
    Short,
    Normal100,
    Preview200,
}

impl LayerHeightChoice {
    pub(crate) const ALL: [Self; 4] = [
        Self::Compact,
        Self::Short,
        Self::Normal100,
        Self::Preview200,
    ];

    pub(crate) const fn menu_label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
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
    pub(crate) mask_mode: MaskMode,
    pub(crate) mask_invert: bool,
    pub(crate) outline_color: Color32,
    pub(crate) outline: bool,
    pub(crate) height: LayerHeightChoice,
    pub(crate) blend_mode: BlendMode,
    pub(crate) sync_timeline: bool,
    pub(crate) attach_to_camera: bool,
    pub(crate) z_depth: f64,
    pub(crate) existing_mask: Option<Id>,
    pub(crate) existing_motion_guide: Option<Id>,
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-layer-properties-draft")
}

/// One size for every layer type: ten rows (with the Mask mode row), two captions and the
/// two-line Sync hint. Folder and non-mask types leave empty space instead of resizing.
pub(crate) fn layer_properties_size(ctx: &egui::Context) -> egui::Vec2 {
    fixed_dialog_size(
        ctx,
        metrics::dialog_m(),
        form_dialog_height(10, 2, metrics::row_gap() + 2.0 * metrics::toggle_row()),
    )
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

fn layer_can_be_mask_with_mode(
    project: &Project,
    symbol_id: &str,
    layer_index: usize,
    mask_mode: MaskMode,
) -> bool {
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
    layer.properties.mask_mode = mask_mode;
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
    candidate.upgrade_format_for_features();
    candidate.validate().is_ok()
}

fn layer_can_be_mask(project: &Project, symbol_id: &str, layer_index: usize) -> bool {
    layer_can_be_mask_with_mode(project, symbol_id, layer_index, MaskMode::Geometry)
        || layer_can_be_mask_with_mode(project, symbol_id, layer_index, MaskMode::Alpha)
        || layer_can_be_mask_with_mode(project, symbol_id, layer_index, MaskMode::Luminance)
}

impl Editor {
    fn current_layer_height_choice(&self) -> LayerHeightChoice {
        let view = &self.workspace.timeline;
        if view.preview != FramePreview::Off {
            LayerHeightChoice::Preview200
        } else if view.compact_rows {
            LayerHeightChoice::Compact
        } else if view.short_rows {
            LayerHeightChoice::Short
        } else {
            LayerHeightChoice::Normal100
        }
    }

    fn apply_layer_height_choice(&mut self, choice: LayerHeightChoice) {
        let view = &mut self.workspace.timeline;
        match choice {
            LayerHeightChoice::Compact => {
                view.compact_rows = true;
                view.short_rows = false;
                view.preview = FramePreview::Off;
            }
            LayerHeightChoice::Short => {
                view.compact_rows = false;
                view.short_rows = true;
                view.preview = FramePreview::Off;
            }
            LayerHeightChoice::Normal100 => {
                view.compact_rows = false;
                view.short_rows = false;
                view.preview = FramePreview::Off;
            }
            LayerHeightChoice::Preview200 => {
                view.compact_rows = false;
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
            mask_mode: layer.properties.mask_mode,
            mask_invert: layer.properties.mask_invert,
            outline_color: Color32::from_rgb(color[0], color[1], color[2]),
            outline: layer.properties.outline,
            height: self.current_layer_height_choice(),
            blend_mode: layer.properties.blend_mode,
            sync_timeline: layer.properties.sync_timeline,
            attach_to_camera: layer.properties.attach_to_camera,
            z_depth: layer.properties.z_depth,
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
            if draft.kind == LayerTypeChoice::Mask {
                layer.properties.mask_mode = draft.mask_mode;
                layer.properties.mask_invert = draft.mask_invert;
            } else {
                layer.properties.mask_mode = MaskMode::Geometry;
                layer.properties.mask_invert = false;
            }
            if draft.kind == LayerTypeChoice::Guide {
                layer.properties.blend_mode = BlendMode::Normal;
                layer.properties.sync_timeline = false;
            } else {
                layer.properties.blend_mode = draft.blend_mode;
                layer.properties.sync_timeline = draft.sync_timeline;
            }
            if draft.kind == LayerTypeChoice::Folder {
                layer.properties.attach_to_camera = false;
                layer.properties.z_depth = 0.0;
            } else {
                layer.properties.attach_to_camera = draft.attach_to_camera;
                layer.properties.z_depth = if draft.z_depth.is_finite() {
                    draft.z_depth.clamp(-10_000.0, 10_000.0)
                } else {
                    0.0
                };
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
        candidate.upgrade_format_for_features();

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
        let Some((
            layer_index,
            has_other_guide,
            has_other_mask,
            can_be_folder,
            can_be_mask,
            inherited_blend,
        )) = ({
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
                let inherited = crate::layer_controls::inherited_folder_blend_mode(
                    &sym.timeline,
                    &sym.timeline.layers[idx],
                );
                Some((idx, has_guide, has_mask, can_folder, can_mask, inherited))
            })
        })
        else {
            self.cancel_layer_properties(ctx);
            return;
        };
        let _ = layer_index;

        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut close_ok = false;

        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        let size = layer_properties_size(ctx);
        let mut in_name = false;
        let modal = egui::Modal::new(egui::Id::new("layer-properties"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                let dialog = FormDialog::show(ui, "Layer properties", size.x, Some(size.y));
                close_cancel |= dialog.close;
                dialog.page(ui, "layer-properties-page", |ui, _| {
                    form_row(ui, "Name", |ui| {
                        let width = ui.available_width();
                        let name = ui.add(c::TextField::new(&mut draft.name, "Name").width(width));
                        in_name = first_field(ui, "layer-properties", &name);
                    });
                    form_row(ui, "", |ui| {
                        ui.spacing_mut().item_spacing.x = space::s4();
                        ui.add(c::Checkbox::new(&mut draft.visible, "Visible"));
                        ui.add_enabled(
                            draft.kind != LayerTypeChoice::Folder,
                            c::Checkbox::new(&mut draft.locked, "Locked"),
                        );
                    });

                    let mut type_radio = |ui: &mut egui::Ui, choice: LayerTypeChoice| {
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
                            if choice == LayerTypeChoice::Mask
                                && !layer_can_be_mask_with_mode(
                                    &self.project,
                                    &draft.symbol_id,
                                    layer_index,
                                    draft.mask_mode,
                                )
                                && layer_can_be_mask_with_mode(
                                    &self.project,
                                    &draft.symbol_id,
                                    layer_index,
                                    MaskMode::Alpha,
                                )
                            {
                                draft.mask_mode = MaskMode::Alpha;
                            }
                        }
                    };
                    // Two radio rows under one label: four and three choices.
                    form_row(ui, "Type", |ui| {
                        ui.spacing_mut().item_spacing.x = space::s4();
                        for choice in &LayerTypeChoice::ALL[..4] {
                            type_radio(ui, *choice);
                        }
                    });
                    form_row(ui, "", |ui| {
                        ui.spacing_mut().item_spacing.x = space::s4();
                        for choice in &LayerTypeChoice::ALL[4..] {
                            type_radio(ui, *choice);
                        }
                        let armature = ui.add_enabled(false, c::Radio::new(false, "Armature"));
                        let _ = c::not_available(armature, ARMATURE_REASON);
                    });
                    if draft.kind == LayerTypeChoice::Mask {
                        form_row(ui, "Mask mode", |ui| {
                            ui.spacing_mut().item_spacing.x = space::s3();
                            c::Select::new("layer-properties-mask-mode", draft.mask_mode.label())
                                .width(metrics::field_m())
                                .show(ui, |ui| {
                                    crate::layer_controls::populate_mask_mode_menu(
                                        ui,
                                        draft.mask_mode,
                                        |mode| draft.mask_mode = mode,
                                    );
                                });
                            ui.add(c::Checkbox::new(&mut draft.mask_invert, "Invert mask"));
                        });
                    }

                    let non_guide = draft.kind != LayerTypeChoice::Guide;
                    form_row(ui, "Layer height", |ui| {
                        c::Select::new("layer-properties-height", draft.height.menu_label())
                            .width(metrics::field_m())
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
                    });
                    form_row(ui, "Blending", |ui| {
                        let blend_label = crate::layer_controls::layer_blending_select_label(
                            draft.blend_mode,
                            inherited_blend,
                        );
                        let blend_label = blend_label
                            .strip_prefix("Blending: ")
                            .unwrap_or(&blend_label);
                        ui.add_enabled_ui(non_guide, |ui| {
                            c::Select::new("layer-properties-blending", blend_label)
                                .width(metrics::field_m())
                                .show(ui, |ui| {
                                    crate::layer_controls::populate_blend_mode_menu(
                                        ui,
                                        draft.blend_mode,
                                        |mode| draft.blend_mode = mode,
                                    );
                                });
                        });
                    });
                    form_row(ui, "", |ui| {
                        ui.add_enabled(
                            non_guide,
                            c::Checkbox::new(&mut draft.sync_timeline, "Sync layer to timeline"),
                        );
                    });
                    form_hint(
                        ui,
                        "Instances on this layer start at the parent timeline frame instead of restarting at frame 1.",
                    );

                    group_caption(ui, "Outline");
                    form_row(ui, "Colour", |ui| {
                        ui.spacing_mut().item_spacing.x = space::s3();
                        let swatch = ui.add_enabled(
                            draft.kind != LayerTypeChoice::Folder,
                            c::SwatchCell {
                                color: draft.outline_color,
                                selected: true,
                            },
                        );
                        c::popup_below(&swatch, metrics::field_l() + 2.0 * space::s3(), |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(space::s1(), space::s2());
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
                            let cf = c::ColorField::new(&mut draft.outline_color, "Outline colour")
                                .width(metrics::field_l())
                                .show(ui);
                            super::dialog_helpers::track_rgba_popup(ui.ctx(), cf.response.id);
                            crate::paint_controls::show_color32_field_popup(
                                ui,
                                &cf.response,
                                cf.swatch_clicked,
                                "Outline colour",
                                &mut draft.outline_color,
                            );
                        });
                        ui.add_enabled(
                            draft.kind != LayerTypeChoice::Folder,
                            c::Checkbox::new(&mut draft.outline, "View layer as outlines"),
                        );
                    });

                    if draft.kind != LayerTypeChoice::Folder {
                        group_caption(ui, "Camera");
                        form_row(ui, "Z-depth", |ui| {
                            ui.spacing_mut().item_spacing.x = space::s3();
                            ui.add(
                                c::Field::new(&mut draft.z_depth, "Z-depth")
                                    .unit("px")
                                    .range(-10_000.0..=10_000.0)
                                    .width(metrics::field_s()),
                            );
                            ui.add(c::Checkbox::new(
                                &mut draft.attach_to_camera,
                                "Attach layer to camera",
                            ));
                        });
                    }
                });
                dialog.footer(
                    ui,
                    |_| {},
                    |ui| {
                        close_ok |= ui.add(c::Button::primary("OK")).clicked();
                        close_cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
            });

        if !close_cancel && !close_ok {
            match kb.resolve_first_field(ctx, &modal, true, in_name) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close_cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => close_ok = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
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
