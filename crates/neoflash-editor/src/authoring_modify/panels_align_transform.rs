use super::*;

impl Editor {
    pub(crate) fn open_scale_and_rotate_dialog(&mut self) {
        self.finish_edits();
        if self.selection.is_empty() {
            return;
        }
        let current = self
            .authoring_modify
            .scale_and_rotate_dialog
            .unwrap_or((100.0, 0.0));
        self.authoring_modify.scale_and_rotate_dialog = Some(current);
    }

    pub(crate) fn apply_scale_and_rotate(
        &mut self,
        ctx: &egui::Context,
        scale_percent: f64,
        rotate_degrees: f64,
    ) -> bool {
        if !scale_percent.is_finite() || !rotate_degrees.is_finite() {
            return false;
        }
        let s = (scale_percent / 100.0).clamp(0.01, 100.0);
        let (sin, cos) = rotate_degrees.to_radians().sin_cos();
        self.affine_selection(ctx, [s * cos, s * sin, -s * sin, s * cos, 0.0, 0.0]);
        true
    }

    pub(crate) fn scale_and_rotate_modal(&mut self, ctx: &egui::Context) {
        let Some((mut scale_pct, mut rotate_deg)) = self.authoring_modify.scale_and_rotate_dialog
        else {
            return;
        };
        use neoflash_ui::{components as c, tokens::TextStyle, typography::rich};
        let mut accept = false;
        let mut cancel = false;
        let kb = crate::panels::dialog_helpers::DialogKeyboardState::capture(ctx);
        use crate::panels::dialog_helpers::{FormDialog, dialog_width, form_row};
        let width = dialog_width(ctx, neoflash_ui::metrics::dialog_s());
        let modal = egui::Modal::new(egui::Id::new("nf2-scale-and-rotate"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(neoflash_ui::palette(ctx).overlay)
            .show(ctx, |ui| {
                let dialog = FormDialog::show(ui, "Scale and Rotate", width, None);
                cancel |= dialog.close;
                dialog.page(ui, "scale-and-rotate-page", |ui, _| {
                    form_row(ui, "Scale", |ui| {
                        ui.add(
                            c::Field::new(&mut scale_pct, "Scale")
                                .unit("%")
                                .range(1.0..=1000.0)
                                .decimals(1)
                                .stepper()
                                .width(neoflash_ui::metrics::field_s()),
                        );
                    });
                    form_row(ui, "Rotate", |ui| {
                        ui.add(
                            c::Field::new(&mut rotate_deg, "Rotate")
                                .unit("°")
                                .range(-360.0..=360.0)
                                .decimals(1)
                                .stepper()
                                .width(neoflash_ui::metrics::field_s()),
                        );
                    });
                    let p = neoflash_ui::palette(ui.ctx());
                    ui.add(
                        egui::Label::new(
                            rich(
                                "Scales and rotates the selection around its transformation point.",
                                TextStyle::Small,
                            )
                            .color(p.text_tertiary),
                        )
                        .wrap(),
                    );
                });
                dialog.footer(
                    ui,
                    |_| {},
                    |ui| {
                        accept |= ui.add(c::Button::primary("OK").shortcut("↵")).clicked();
                        cancel |= ui.add(c::Button::new("Cancel").shortcut("Esc")).clicked();
                    },
                );
            });
        if !accept && !cancel {
            match kb.resolve(ctx, &modal, true) {
                crate::panels::dialog_helpers::DialogKeyboardOutcome::Cancel => cancel = true,
                crate::panels::dialog_helpers::DialogKeyboardOutcome::Confirm => accept = true,
                crate::panels::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }
        scale_pct = if scale_pct.is_finite() {
            scale_pct.clamp(1.0, 1000.0)
        } else {
            100.0
        };
        rotate_deg = if rotate_deg.is_finite() {
            rotate_deg.clamp(-360.0, 360.0)
        } else {
            0.0
        };
        if accept {
            self.authoring_modify.scale_and_rotate_dialog = None;
            self.apply_scale_and_rotate(ctx, scale_pct, rotate_deg);
        } else if cancel {
            self.authoring_modify.scale_and_rotate_dialog = None;
        } else {
            self.authoring_modify.scale_and_rotate_dialog = Some((scale_pct, rotate_deg));
        }
    }

    /// Transform ▸ (Modify and the stage object menu).
    pub(crate) fn transform_menu(&mut self, ui: &mut egui::Ui) {
        use crate::authoring_tools::FreeTransformMode;
        use crate::commands::CommandId as C;
        use crate::ui_icons::{menu_check_enabled, menu_item_enabled};
        let selected = !self.selection.is_empty();
        let can_deform = self.can_distort_or_envelope_selection();
        let is_transform_tool = self.tool == crate::app::Tool::Transform;
        let active_mode = self.active_free_transform_mode();
        let scale_rot_sc = self.sc(C::ModifyScaleAndRotate);
        let cw_sc = self.sc(C::ModifyRotate90Cw);
        let ccw_sc = self.sc(C::ModifyRotate90Ccw);
        let remove_custom_sc = self.sc(C::ModifyRemoveTransform);
        let remove_sc = if remove_custom_sc.is_empty() {
            "⇧⌘Z".to_owned()
        } else {
            remove_custom_sc
        };
        crate::ui_icons::menu_popup(ui, "Transform", "Scale and Rotate…", &scale_rot_sc, |ui| {
            if menu_check_enabled(
                ui,
                selected,
                "Free Transform",
                &self.sc(C::ModifyFreeTransform),
                is_transform_tool && active_mode == FreeTransformMode::FreeTransform,
            )
            .clicked()
            {
                self.set_free_transform_mode(FreeTransformMode::FreeTransform);
                ui.close();
            }
            if menu_check_enabled(
                ui,
                can_deform,
                "Distort",
                &self.sc(C::ModifyDistort),
                is_transform_tool && active_mode == FreeTransformMode::Distort,
            )
            .clicked()
            {
                self.set_free_transform_mode(FreeTransformMode::Distort);
                ui.close();
            }
            if menu_check_enabled(
                ui,
                can_deform,
                "Envelope",
                &self.sc(C::ModifyEnvelope),
                is_transform_tool && active_mode == FreeTransformMode::Envelope,
            )
            .clicked()
            {
                self.set_free_transform_mode(FreeTransformMode::Envelope);
                ui.close();
            }
            if menu_check_enabled(
                ui,
                selected,
                "Scale",
                &self.sc(C::ModifyScale),
                is_transform_tool && active_mode == FreeTransformMode::Scale,
            )
            .clicked()
            {
                self.set_free_transform_mode(FreeTransformMode::Scale);
                ui.close();
            }
            if menu_check_enabled(
                ui,
                selected,
                "Rotate and Skew",
                &self.sc(C::ModifyRotateAndSkew),
                is_transform_tool && active_mode == FreeTransformMode::RotateAndSkew,
            )
            .clicked()
            {
                self.set_free_transform_mode(FreeTransformMode::RotateAndSkew);
                ui.close();
            }
            if menu_item_enabled(ui, selected, "Scale and Rotate…", &scale_rot_sc).clicked() {
                self.open_scale_and_rotate_dialog();
                ui.close();
            }
            neoflash_ui::components::menu_separator(ui);
            if menu_item_enabled(ui, selected, "Rotate 90° CW", &cw_sc).clicked() {
                self.rotate_90_cw_selection(ui.ctx());
                ui.close();
            }
            if menu_item_enabled(ui, selected, "Rotate 90° CCW", &ccw_sc).clicked() {
                self.rotate_90_ccw_selection(ui.ctx());
                ui.close();
            }
            neoflash_ui::components::menu_separator(ui);
            if menu_item_enabled(
                ui,
                selected,
                "Flip Vertical",
                &self.sc(C::ModifyFlipVertical),
            )
            .clicked()
            {
                self.flip_vertical_selection(ui.ctx());
                ui.close();
            }
            if menu_item_enabled(
                ui,
                selected,
                "Flip Horizontal",
                &self.sc(C::ModifyFlipHorizontal),
            )
            .clicked()
            {
                self.flip_horizontal_selection(ui.ctx());
                ui.close();
            }
            neoflash_ui::components::menu_separator(ui);
            if menu_item_enabled(ui, selected, "Remove Transform", &remove_sc).clicked() {
                self.reset_selection_transform(ui.ctx());
                ui.close();
            }
            neoflash_ui::components::menu_separator(ui);
            if crate::ui_icons::menu_item(ui, "Transform panel", &self.sc(C::WindowTransform))
                .clicked()
            {
                self.show_panel(PanelId::Transform);
                ui.close();
            }
        });
    }

    /// Arrange ▸ (Modify and the stage object menu).
    pub(crate) fn arrange_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::CommandId as C;
        crate::ui_icons::menu_popup(ui, "Arrange", "Bring Forward", "⇧⌘↑", |ui| {
            for (label, cmd_id, direction) in [
                ("Bring to front", C::ModifyArrangeFront, Arrange::Front),
                ("Bring forward", C::ModifyArrangeForward, Arrange::Forward),
                ("Send backward", C::ModifyArrangeBackward, Arrange::Backward),
                ("Send to back", C::ModifyArrangeBack, Arrange::Back),
            ] {
                let enabled = self.can_arrange_selection(Some(direction));
                if crate::ui_icons::menu_item_enabled(ui, enabled, label, &self.sc(cmd_id))
                    .clicked()
                {
                    self.arrange_selection(direction);
                    ui.close();
                }
            }
        });
    }

    /// Align ▸ (Modify and the stage object menu): the Align panel's edge commands.
    pub(crate) fn align_menu(&mut self, ui: &mut egui::Ui) {
        use crate::commands::CommandId as C;
        crate::ui_icons::menu_popup(ui, "Align", "Vertical middle", "", |ui| {
            let ctx = ui.ctx().clone();
            let enabled = self.can_align_selection();
            for (axis, labels) in [
                ["Left", "Horizontal centre", "Right"],
                ["Top", "Vertical middle", "Bottom"],
            ]
            .into_iter()
            .enumerate()
            {
                for (edge, label) in labels.into_iter().enumerate() {
                    if crate::ui_icons::menu_item_enabled(ui, enabled, label, "").clicked() {
                        self.align_selection(&ctx, axis, edge);
                        ui.close();
                    }
                }
                neoflash_ui::components::menu_separator(ui);
            }
            crate::ui_icons::menu_toggle(ui, &mut self.authoring_modify.to_stage, "To stage", "");
            if crate::ui_icons::menu_item(ui, "Align panel", &self.sc(C::WindowAlign)).clicked() {
                self.show_panel(PanelId::Align);
                ui.close();
            }
        });
    }

    /// Color panel ⋯ menu (§6 "Color — panel menu"): Add swatch · Help · Close · Close group.
    pub(crate) fn color_panel_menu(&mut self, ui: &mut egui::Ui) {
        if crate::ui_icons::menu_item(ui, "Add swatch", "").clicked() {
            let ctx = ui.ctx().clone();
            self.add_active_swatch(&ctx);
            ui.close();
        }
        if crate::ui_icons::menu_item(ui, "Help", "F1").clicked() {
            let ctx = ui.ctx().clone();
            self.open_function_search(&ctx);
            ui.close();
        }
        if crate::ui_icons::menu_item(ui, "Close", "").clicked() {
            self.set_panel_visible(PanelId::Color, false);
            ui.close();
        }
        if crate::ui_icons::menu_item(ui, "Close group", "").clicked() {
            let group = self
                .workspace
                .dock
                .location(PanelId::Color)
                .and_then(|at| {
                    self.workspace
                        .dock
                        .area(at.edge)
                        .groups
                        .iter()
                        .find(|g| g.id == at.group)
                        .map(|g| g.tabs.clone())
                })
                .or_else(|| {
                    self.workspace
                        .floating_window(PanelId::Color)
                        .map(|w| w.panels())
                });
            for panel in group.unwrap_or_else(|| vec![PanelId::Color]) {
                self.set_panel_visible(panel, false);
            }
            ui.close();
        }
    }

    pub(crate) fn align_panel_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_title(ui, PanelId::Align, "Align");
        panel_scroll_body(ui, PanelId::Align, "align-content", |ui| {
            self.align_panel_contents(ui)
        });
    }

    fn align_panel_contents(&mut self, ui: &mut egui::Ui) {
        use crate::panels::properties::inspector_section;
        ui.spacing_mut().item_spacing.y = neoflash_ui::metrics::row_gap();
        // `Align to` stays on top: the caption takes the free width, the choice ends the row.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::s2();
            caption_label(ui, "Align to");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                Segmented::show(
                    ui,
                    &mut self.authoring_modify.to_stage,
                    &[(false, "Selection"), (true, "Stage")],
                    false,
                );
            });
        });
        let count = self.modify_layout_units(ui.ctx()).len();
        let to_stage = self.authoring_modify.to_stage;
        inspector_section(ui, "Align", |ui| {
            let align_resp = ui
                .add_enabled_ui_semantic(count >= 2 || (count > 0 && to_stage), |ui| {
                    alignment_groups(ui, "Align", ALIGN_ICONS, |ui, axis, edge| {
                        self.align_selection(ui.ctx(), axis, edge)
                    });
                })
                .response;
            crate::ui_icons::hover_tooltip(
                align_resp,
                "",
                None,
                Some("Select two objects, or choose Stage to align one object"),
            );
        });
        inspector_section(ui, "Distribute", |ui| {
            let dist_resp = ui
                .add_enabled_ui_semantic(count >= 3 || (count >= 2 && to_stage), |ui| {
                    alignment_groups(ui, "Distribute", DISTRIBUTE_ICONS, |ui, axis, edge| {
                        self.distribute_selection(ui.ctx(), axis, edge)
                    })
                })
                .response;
            crate::ui_icons::hover_tooltip(
                dist_resp,
                "",
                None,
                Some("Select at least three objects to distribute their positions"),
            );
        });
        inspector_section(ui, "Spacing", |ui| {
            let space_resp = ui
                .add_enabled_ui_semantic(count >= 2, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::s2();
                        let even = IconGroup {
                            items: &[
                                (
                                    i16::SPACE_H,
                                    "Space evenly horizontally (keeps outer bounds)",
                                ),
                                (i16::SPACE_V, "Space evenly vertically (keeps outer bounds)"),
                            ],
                            selected: None,
                        }
                        .show(ui);
                        if let Some(axis) = even {
                            self.space_selection(ui.ctx(), axis, None);
                        }
                        let gap = &mut self.authoring_modify.horizontal_gap;
                        let before = *gap;
                        // The gap field takes what the icon group leaves of the row.
                        let width = crate::paint_controls::visible_row_width(ui)
                            .max(neoflash_ui::metrics::field_num());
                        ui.add(
                            DragField::new(gap, "Spacing gap (px)")
                                .prefix("Gap")
                                .suffix(" px")
                                .range(-100_000. ..=100_000.)
                                .max_decimals(2)
                                .width(width),
                        );
                        if *gap != before {
                            self.authoring_modify.vertical_gap = *gap;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::s2();
                        let apply = IconGroup {
                            items: &[
                                (i16::SPACE_H, "Apply gap horizontally"),
                                (i16::SPACE_V, "Apply gap vertically"),
                            ],
                            selected: None,
                        }
                        .show(ui);
                        caption_label(ui, "Apply the gap");
                        if let Some(axis) = apply {
                            let gap = if axis == 0 {
                                self.authoring_modify.horizontal_gap
                            } else {
                                self.authoring_modify.vertical_gap
                            };
                            self.space_selection(ui.ctx(), axis, Some(gap));
                        }
                    });
                })
                .response;
            crate::ui_icons::hover_tooltip(
                space_resp,
                "",
                None,
                Some("Select two or more objects to space them"),
            );
        });
        inspector_section(ui, "Match size", |ui| {
            let match_resp = ui
                .add_enabled_ui_semantic(count >= 2 || (count > 0 && to_stage), |ui| {
                    let clicked = IconGroup {
                        items: &[
                            (i16::MATCH_W, "Match width"),
                            (i16::MATCH_H, "Match height"),
                        ],
                        selected: None,
                    }
                    .show(ui);
                    if let Some(axis) = clicked {
                        self.match_selection_size(ui.ctx(), axis);
                    }
                })
                .response;
            crate::ui_icons::hover_tooltip(
                match_resp,
                "",
                None,
                Some("Select two or more objects to match their size"),
            );
        });
    }

    pub(crate) fn transform_panel_ui(&mut self, ui: &mut egui::Ui) {
        self.panel_title(ui, PanelId::Transform, "Transform");
        panel_scroll_body(ui, PanelId::Transform, "transform-content", |ui| {
            self.transform_panel_contents(ui)
        });
    }

    fn transform_panel_contents(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = neoflash_ui::metrics::row_gap();
        let placements = self.modify_placements(ui.ctx());
        if placements.is_empty() {
            crate::panels::properties::hint(ui, "Select artwork on the Stage.");
            return;
        }
        let scope = self.modify_scope();
        self.property_edit_scope(scope);
        ui.push_id(scope.with("transform"), |ui| {
            let bounds = combined_bounds(&placements);
            let mut fields = [
                bounds.min.x as f64,
                bounds.min.y as f64,
                bounds.width() as f64,
                bounds.height() as f64,
            ];
            let original = fields;
            let size_min = if original[2] <= 1e-6 || original[3] <= 1e-6 {
                0.0
            } else {
                0.01
            };
            // v3: two equal columns in every row; the constrain toggle sits in the section header.
            let mut position = Vec::new();
            {
                let [ref mut x, ref mut y, ref mut width, ref mut height] = fields;
                crate::panels::properties::inspector_section_with_trailing(
                    ui,
                    "Position and size",
                    true,
                    crate::panels::properties::section_action_side(),
                    |ui| {
                        position.extend(field_pair(
                            ui,
                            ["X", "Y"],
                            [x, y],
                            -1_000_000. ..=1_000_000.,
                            "",
                        ));
                        position.extend(field_pair(
                            ui,
                            ["W", "H"],
                            [width, height],
                            size_min..=1_000_000.,
                            "",
                        ));
                    },
                    |ui| section_link_toggle(ui, &mut self.authoring_modify.linked_scale),
                );
            }
            for (index, response) in position.into_iter().enumerate() {
                if response.changed() {
                    let mut matrix = [1., 0., 0., 1., 0., 0.];
                    if index < 2 {
                        let delta = fields[index] - original[index];
                        if delta.abs() <= 1e-9 {
                            continue;
                        }
                        matrix[4 + index] = delta;
                    } else if original[index] > 1e-9 && fields[index] > 1e-9 {
                        let scale = fields[index] / original[index];
                        if !scale.is_finite() || (scale - 1.0).abs() <= 1e-9 {
                            continue;
                        }
                        let axis = index - 2;
                        matrix[if axis == 0 { 0 } else { 3 }] = scale;
                        if self.authoring_modify.linked_scale {
                            matrix[0] = scale;
                            matrix[3] = scale;
                        }
                        matrix = around(bounds.min, matrix);
                    } else {
                        continue;
                    }
                    if let Some(changes) = transformed(&placements, matrix) {
                        self.apply_modified(changes, Some(response.id));
                    } else {
                        self.message("This size exceeds transform limits.");
                    }
                }
            }
            if placements.len() == 1 {
                let item = &placements[0];
                let old = item.element.transform;
                let mut values = [
                    old.scale_x * 100.,
                    old.scale_y * 100.,
                    old.rotation,
                    old.skew,
                ];
                let responses =
                    scale_rotate_sections(ui, &mut values, &mut self.authoring_modify.linked_scale);
                for (index, response) in responses {
                    if !response.changed() {
                        continue;
                    }
                    let value = values[index];
                    let mut new = old;
                    match index {
                        0 => {
                            let sx_factor = (value / 100.).clamp(-1e4, 1e4);
                            if sx_factor.abs() < 1e-4 {
                                continue;
                            }
                            new.scale_x = sx_factor;
                            if self.authoring_modify.linked_scale && old.scale_x.abs() > 1e-9 {
                                new.scale_y =
                                    (new.scale_y * (new.scale_x / old.scale_x)).clamp(-1e4, 1e4);
                            }
                        }
                        1 => {
                            let sy_factor = (value / 100.).clamp(-1e4, 1e4);
                            if sy_factor.abs() < 1e-4 {
                                continue;
                            }
                            new.scale_y = sy_factor;
                            if self.authoring_modify.linked_scale && old.scale_y.abs() > 1e-9 {
                                new.scale_x =
                                    (new.scale_x * (new.scale_y / old.scale_y)).clamp(-1e4, 1e4);
                            }
                        }
                        2 => new.rotation = value.clamp(-360_000., 360_000.),
                        _ => new.skew = value.clamp(-89., 89.),
                    }
                    if new.scale_x.abs().max(new.scale_y.abs()) <= 1e4 {
                        let mut element = item.element.clone();
                        element.transform = retain_center(old, new, item.bounds.center());
                        self.apply_modified(
                            vec![(item.target.clone(), element)],
                            Some(response.id),
                        );
                    }
                }
            } else if placements.len() > 1 {
                let mut values = [100.0_f64, 100.0_f64, 0.0_f64, 0.0_f64];
                let responses =
                    scale_rotate_sections(ui, &mut values, &mut self.authoring_modify.linked_scale);
                let pivot = self
                    .authoring_modify
                    .group_pivot
                    .as_ref()
                    .filter(|(targets, _)| *targets == self.selection.targets)
                    .map(|(_, p)| Pos2::new(p[0] as f32, p[1] as f32))
                    .unwrap_or_else(|| bounds.center());
                for (index, response) in responses {
                    if !response.changed() {
                        continue;
                    }
                    let value = values[index];
                    let matrix = match index {
                        0 => {
                            let s = value / 100.0;
                            let sy_f = if self.authoring_modify.linked_scale {
                                s
                            } else {
                                1.0
                            };
                            around(pivot, [s, 0.0, 0.0, sy_f, 0.0, 0.0])
                        }
                        1 => {
                            let s = value / 100.0;
                            let sx_f = if self.authoring_modify.linked_scale {
                                s
                            } else {
                                1.0
                            };
                            around(pivot, [sx_f, 0.0, 0.0, s, 0.0, 0.0])
                        }
                        2 => {
                            let (sin, cos) = value.to_radians().sin_cos();
                            around(pivot, [cos, sin, -sin, cos, 0.0, 0.0])
                        }
                        _ => {
                            let shx = value.to_radians().tan();
                            around(pivot, [1.0, 0.0, shx, 1.0, 0.0, 0.0])
                        }
                    };
                    if let Some(changes) = transformed(&placements, matrix) {
                        self.apply_modified(changes, Some(response.id));
                    } else {
                        self.message("This transformation exceeds transform limits.");
                    }
                }
            }
            neoflash_ui::components::divider(ui, false);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::s2();
                let clicked = IconGroup {
                    items: &[
                        (i16::ROTATE_CCW, "Rotate 90° CCW"),
                        (i16::ROTATE_CW, "Rotate 90° CW"),
                        (i16::FLIP_H, "Flip Horizontal"),
                        (i16::FLIP_V, "Flip Vertical"),
                    ],
                    selected: None,
                }
                .show(ui);
                if let Some(index) = clicked {
                    self.affine_selection(ui.ctx(), TRANSFORM_COMMANDS[index].1);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(IconButton::new(i16::RESET_TRANSFORM, "Remove Transform"))
                        .clicked()
                    {
                        self.reset_selection_transform(ui.ctx());
                    }
                });
            });
        });
        self.end_property_interaction(ui.ctx());
    }

    #[allow(dead_code)]
    fn transform_commands(&mut self, ui: &mut egui::Ui) {
        for (label, matrix) in TRANSFORM_COMMANDS {
            let response = if egui::containers::menu::is_in_menu(ui) {
                crate::ui_icons::menu_item(ui, label, "")
            } else {
                ui.add(neoflash_ui::components::Button::new(label).width(ui.available_width()))
            };
            if response.clicked() {
                self.affine_selection(ui.ctx(), matrix);
            }
        }
        let reset = if egui::containers::menu::is_in_menu(ui) {
            crate::ui_icons::menu_item(ui, "Remove Transform", "")
        } else {
            ui.add(
                neoflash_ui::components::Button::new("Remove Transform")
                    .width(ui.available_width()),
            )
        };
        if reset.clicked() {
            self.reset_selection_transform(ui.ctx());
        }
    }
}

/// Rotate / flip commands in the order of the Transform panel icon group.
pub(super) const TRANSFORM_COMMANDS: [(&str, [f64; 6]); 4] = [
    ("Rotate 90° CCW", [0., -1., 1., 0., 0., 0.]),
    ("Rotate 90° CW", [0., 1., -1., 0., 0., 0.]),
    ("Flip Horizontal", [-1., 0., 0., 1., 0., 0.]),
    ("Flip Vertical", [1., 0., 0., -1., 0., 0.]),
];

pub(super) type EdgeIcons = [[(Icon, &'static str); 3]; 2];
pub(super) const ALIGN_ICONS: EdgeIcons = [
    [
        (i16::ALIGN_LEFT, "Left"),
        (i16::ALIGN_HCENTER, "Center"),
        (i16::ALIGN_RIGHT, "Right"),
    ],
    [
        (i16::ALIGN_TOP, "Top"),
        (i16::ALIGN_VMIDDLE, "Middle"),
        (i16::ALIGN_BOTTOM, "Bottom"),
    ],
];
pub(super) const DISTRIBUTE_ICONS: EdgeIcons = [
    [
        (i16::DIST_LEFT, "Left"),
        (i16::DIST_HCENTER, "Center"),
        (i16::DIST_RIGHT, "Right"),
    ],
    [
        (i16::DIST_TOP, "Top"),
        (i16::DIST_VMIDDLE, "Middle"),
        (i16::DIST_BOTTOM, "Bottom"),
    ],
];

/// Two `NF2/Icon Group`s (horizontal edges · vertical edges). Accessible names
/// stay "Align Left", "Distribute Top", ….
pub(super) fn alignment_groups(
    ui: &mut egui::Ui,
    operation: &str,
    icons: EdgeIcons,
    mut action: impl FnMut(&mut egui::Ui, usize, usize),
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::s2();
        for (axis, group) in icons.iter().enumerate() {
            let names = group.map(|(_, label)| format!("{operation} {label}"));
            let items: Vec<(Icon, &str)> = group
                .iter()
                .zip(&names)
                .map(|((icon, _), name)| (*icon, name.as_str()))
                .collect();
            if let Some(edge) = (IconGroup {
                items: &items,
                selected: None,
            })
            .show(ui)
            {
                action(ui, axis, edge);
            }
        }
    });
}

/// Inline row label (`NF2/Body`, secondary).
pub(super) fn caption_label(ui: &mut egui::Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    let color = if ui.is_enabled() {
        p.text_secondary
    } else {
        p.text_disabled
    };
    ui.label(rich(text, TextStyle::Body).color(color));
}

/// Two prefixed fields on the shared two-column grid.
pub(super) fn field_pair(
    ui: &mut egui::Ui,
    prefixes: [&str; 2],
    values: [&mut f64; 2],
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) -> [egui::Response; 2] {
    let [a, b] = values;
    let first_range = range.clone();
    let (first, second) = two_column_row(
        ui,
        |ui, width| number_field(ui, prefixes[0], a, first_range, 1., suffix, Some(width)),
        |ui, width| number_field(ui, prefixes[1], b, range, 1., suffix, Some(width)),
    );
    [first, second]
}

/// `Constrain proportions` toggle on the right side of a section header (v3 `Section Header` › `Action`).
pub(super) fn section_link_toggle(ui: &mut egui::Ui, linked: &mut bool) {
    if ui
        .add(
            IconButton::new(i16::LINK, "Constrain proportions")
                .selected(*linked)
                .side(crate::panels::properties::section_action_side()),
        )
        .clicked()
    {
        *linked = !*linked;
    }
}

/// Transform panel › `Scale` (W · H with the constrain toggle in its header) and `Rotate and skew`.
/// `values` = scale X %, scale Y %, rotation°, skew°. Each response comes with its index in
/// `values`; a collapsed section has none.
pub(super) fn scale_rotate_sections(
    ui: &mut egui::Ui,
    values: &mut [f64; 4],
    linked: &mut bool,
) -> Vec<(usize, egui::Response)> {
    let [sx, sy, rotation, skew] = values;
    let mut scale = Vec::new();
    crate::panels::properties::inspector_section_with_trailing(
        ui,
        "Scale",
        true,
        crate::panels::properties::section_action_side(),
        |ui| {
            scale.extend(field_pair(
                ui,
                ["W", "H"],
                [sx, sy],
                -1_000_000. ..=1_000_000.,
                " %",
            ));
        },
        |ui| section_link_toggle(ui, linked),
    );
    let mut turn = Vec::new();
    crate::panels::properties::inspector_section(ui, "Rotate and skew", |ui| {
        let (rotate, skew) = two_column_row(
            ui,
            |ui, width| {
                number_field(
                    ui,
                    "Rotation",
                    rotation,
                    -360_000. ..=360_000.,
                    1.,
                    "°",
                    Some(width),
                )
            },
            |ui, width| number_field(ui, "Skew", skew, -89. ..=89., 1., "°", Some(width)),
        );
        turn.extend([rotate, skew]);
    });
    scale
        .into_iter()
        .enumerate()
        .chain(turn.into_iter().enumerate().map(|(i, r)| (i + 2, r)))
        .collect()
}
