use crate::app::*;
use crate::{
    theme::{ThemeMode, UiPalette},
    timeline_paint::{ROW_HEIGHT, RULER_HEIGHT, paint_tween_arrow},
    ui_icons::{Icon, IconButton, inline_icon},
    workspace::{PanelId, PanelPlacement},
};
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use neoflash_core::*;

impl Editor {
    pub fn menu(&mut self, ctx: &egui::Context) {
        let clipboard_target = self.clipboard_target(ctx);
        let mut edit_open = false;
        egui::TopBottomPanel::top("menu")
            .exact_height(32.)
            .show(ctx, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.strong("NeoFlash");
                    ui.separator();
                    ui.menu_button("File", |ui| {
                        if ui.button("New                         ⌘N").clicked() {
                            self.request(Pending::New, ctx);
                            ui.close();
                        }
                        if ui.button("Open…                      ⌘O").clicked() {
                            self.request(Pending::Open, ctx);
                            ui.close();
                        }
                        if ui.button("Import legacy Flash…").clicked() {
                            self.request(Pending::Import, ctx);
                            ui.close();
                        }
                        if ui.button("Save                         ⌘S").clicked() {
                            self.save(false);
                            ui.close();
                        }
                        if ui.button("Save As…").clicked() {
                            self.save(true);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Publish HTML5…").clicked() {
                            self.export(false);
                            ui.close();
                        }
                        if ui.button("Open example").clicked() {
                            self.request(Pending::Demo, ctx);
                            ui.close();
                        }
                    });
                    ui.menu_button("Edit", |ui| {
                        edit_open = true;
                        if !self.clipboard.menu_open
                            || self.clipboard.menu_viewport != Some(ctx.viewport_id())
                        {
                            self.clipboard.menu_target = Some(clipboard_target.clone());
                        }
                        if ui
                            .add_enabled(
                                self.history.can_undo() || self.vector.draft.is_some(),
                                egui::Button::new("Undo     ⌘Z"),
                            )
                            .clicked()
                        {
                            self.undo();
                            ui.close();
                        }
                        if ui
                            .add_enabled(self.history.can_redo(), egui::Button::new("Redo     ⇧⌘Z"))
                            .clicked()
                        {
                            self.redo();
                            ui.close();
                        }
                        ui.separator();
                        let text_target = self
                            .clipboard
                            .menu_target
                            .as_ref()
                            .is_some_and(|target| target.is_text());
                        let can_copy = text_target || !self.selection.is_empty();
                        let can_paste = text_target || self.layer_editable();
                        for (label, command, enabled) in [
                            (
                                "Cut       ⌘X",
                                crate::clipboard::ClipboardCommand::Cut,
                                can_copy,
                            ),
                            (
                                "Copy      ⌘C",
                                crate::clipboard::ClipboardCommand::Copy,
                                can_copy,
                            ),
                            (
                                "Paste     ⌘V",
                                crate::clipboard::ClipboardCommand::Paste,
                                can_paste,
                            ),
                            (
                                "Paste in Place     ⇧⌘V",
                                crate::clipboard::ClipboardCommand::PasteInPlace,
                                !text_target && self.layer_editable(),
                            ),
                        ] {
                            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                                self.request_clipboard(ctx, command);
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("Select All     ⌘A").clicked() {
                            self.select_all();
                            ui.close();
                        }
                        if ui
                            .add_enabled(!self.selection.is_empty(), egui::Button::new("Deselect"))
                            .clicked()
                        {
                            self.finish_edits();
                            self.clear_selection();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                !self.selection.is_empty(),
                                egui::Button::new("Duplicate     ⌘D"),
                            )
                            .clicked()
                        {
                            self.duplicate();
                            ui.close();
                        }
                        if ui
                            .add_enabled(!self.selection.is_empty(), egui::Button::new("Delete"))
                            .clicked()
                        {
                            self.delete_selected();
                            ui.close();
                        }
                    });
                    ui.menu_button("Insert", |ui| {
                        if ui.button("Frame                  F5").clicked() {
                            self.finish_edits();
                            self.insert_frame();
                            ui.close();
                        }
                        if ui.button("Keyframe             F6").clicked() {
                            self.finish_edits();
                            self.make_key(false);
                            ui.close();
                        }
                        if ui.button("Blank Keyframe   F7").clicked() {
                            self.finish_edits();
                            self.make_key(true);
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.selection.len() == 1,
                                egui::Button::new("Convert to Symbol…  F8"),
                            )
                            .clicked()
                        {
                            self.convert_to_symbol();
                            ui.close();
                        }
                    });
                    ui.menu_button("Control", |ui| {
                        if ui
                            .button(if self.playing {
                                "Stop    Enter"
                            } else {
                                "Play    Enter"
                            })
                            .clicked()
                        {
                            self.set_playing(!self.playing);
                            ui.close();
                        }
                        if ui.button("Test Movie    ⌘Enter").clicked() {
                            self.export(true);
                            ui.close();
                        }
                        if ui.button("Rewind").clicked() {
                            self.finish_edits();
                            self.timeline_key_drag = None;
                            self.frame = 0;
                            ui.close();
                        }
                    });
                    ui.menu_button("Window", |ui| {
                        for (name, panel) in [
                            ("Timeline", PanelId::Timeline),
                            ("Tools", PanelId::Tools),
                            ("Library", PanelId::Library),
                            ("Properties", PanelId::Properties),
                            ("Actions    F9", PanelId::Actions),
                            ("Accessibility", PanelId::Accessibility),
                            ("Output", PanelId::Output),
                        ] {
                            let mut visible = self.workspace.is_panel_visible(panel);
                            if ui.checkbox(&mut visible, name).changed() {
                                self.set_panel_visible(panel, visible);
                                ui.close();
                            }
                        }
                        ui.separator();
                        for (name, panel) in [
                            ("Tools", PanelId::Tools),
                            ("Timeline", PanelId::Timeline),
                            ("Actions", PanelId::Actions),
                        ] {
                            let detached =
                                self.workspace.placement(panel) == PanelPlacement::Detached;
                            if ui
                                .button(format!(
                                    "{} {name}",
                                    if detached { "Dock" } else { "Detach" }
                                ))
                                .clicked()
                            {
                                self.set_panel_placement(
                                    ctx,
                                    panel,
                                    if detached {
                                        PanelPlacement::Docked
                                    } else {
                                        PanelPlacement::Detached
                                    },
                                );
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("Restore Workspace").clicked() {
                            self.restore_workspace(ctx);
                            ui.close();
                        }
                        ui.separator();
                        ui.label("Appearance");
                        for (name, theme) in
                            [("Light", ThemeMode::Light), ("Dark", ThemeMode::Dark)]
                        {
                            if ui.radio(self.workspace.theme == theme, name).clicked() {
                                self.set_theme(ctx, theme);
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("About NeoFlash").clicked() {
                            self.show_about = true;
                            ui.close();
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(IconButton::new(Icon::External, "Test Movie").text())
                            .clicked()
                        {
                            self.export(true);
                        }
                        ui.label(
                            egui::RichText::new("CLASSIC WORKSPACE")
                                .small()
                                .color(UiPalette::current(ctx).muted_text),
                        );
                    });
                });
            });
        if edit_open {
            self.clipboard.menu_open = true;
            self.clipboard.menu_viewport = Some(ctx.viewport_id());
        } else if self.clipboard.menu_viewport == Some(ctx.viewport_id()) {
            self.clipboard.menu_open = false;
            self.clipboard.menu_target = None;
            self.clipboard.menu_viewport = None;
        }
    }
    pub fn timeline(&mut self, ctx: &egui::Context) {
        if !self.workspace.is_panel_visible(PanelId::Timeline)
            || !self.workspace.is_panel_docked(PanelId::Timeline)
        {
            return;
        }
        let maximum = (ctx.content_rect().height() * 0.46).clamp(130., 380.);
        let panel = egui::TopBottomPanel::top("timeline")
            .default_height(self.workspace.sizes.timeline_height.min(maximum))
            .min_height(130.)
            .max_height(maximum)
            .resizable(true)
            .show(ctx, |ui| self.timeline_contents(ui));
        self.workspace.sizes.timeline_height = panel.response.rect.height();
    }
    pub(crate) fn timeline_contents(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let palette = UiPalette::current(&ctx);
        let id = (
            "neoflash-panel-content",
            PanelId::Timeline,
            self.clipboard.session.clone(),
            self.symbol_id().to_owned(),
        );
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("Timeline");
                ui.separator();
                for (index, id) in self.editing.clone().iter().enumerate() {
                    if index > 0 {
                        inline_icon(ui, Icon::ChevronRight, 12.);
                    }
                    let name = self
                        .project
                        .symbol(id)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    if ui
                        .selectable_label(index + 1 == self.editing.len(), name)
                        .clicked()
                    {
                        self.finish_edits();
                        self.timeline_key_drag = None;
                        self.editing.truncate(index + 1);
                        self.frame = 0;
                        self.layer = 0;
                        self.clear_selection();
                    }
                }
                ui.label(format!(
                    "Frame {} / {}   {:.2} s   {} fps",
                    self.frame + 1,
                    self.symbol().timeline.frame_count,
                    self.frame as f64 / self.project.fps as f64,
                    self.project.fps
                ));
            });
            ui.separator();
            // The frame canvas may be much wider than the viewport. Freeze the
            // toolbar width before allocating it so scrolling frames cannot
            // widen the controls. Small windows keep a bounded, scrollable footer.
            let timeline_width = ui.available_width();
            let desired_footer: f32 = if timeline_width < 1000. { 66. } else { 34. };
            let footer =
                desired_footer.min((ui.available_height() - RULER_HEIGHT - 25. - 10.).max(24.));
            let height =
                (ui.available_height() - footer - RULER_HEIGHT - 2. * ui.spacing().item_spacing.y)
                    .max(25.);
            let (header, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), RULER_HEIGHT),
                Sense::hover(),
            );
            let layers = self.symbol().timeline.layers.clone();
            let count = self.symbol().timeline.frame_count;
            let content_height = layers.len() as f32 * ROW_HEIGHT;
            let mut grid_rect = Rect::NOTHING;
            let mut grid_clip = Rect::NOTHING;
            egui::ScrollArea::vertical()
                .id_salt("timeline_rows")
                .min_scrolled_height(25.)
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(210., content_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(210.);
                                ui.spacing_mut().item_spacing.y = 0.;
                                for (idx, layer) in layers.iter().enumerate() {
                                    let (row_rect, _) = ui.allocate_exact_size(
                                        Vec2::new(210., ROW_HEIGHT),
                                        Sense::hover(),
                                    );
                                    ui.scope_builder(
                                        egui::UiBuilder::new().max_rect(row_rect).layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                        ),
                                        |ui| {
                                            ui.set_clip_rect(ui.clip_rect().intersect(row_rect));
                                            if ui
                                                .add_sized(
                                                    [146., ROW_HEIGHT],
                                                    egui::Button::new(&layer.name)
                                                        .selected(idx == self.layer)
                                                        .frame(false),
                                                )
                                                .clicked()
                                            {
                                                self.finish_edits();
                                                self.timeline_key_drag = None;
                                                self.layer = idx;
                                                self.clear_selection();
                                            }
                                            if ui
                                                .add(
                                                    IconButton::new(
                                                        if layer.locked {
                                                            Icon::Lock
                                                        } else {
                                                            Icon::Unlock
                                                        },
                                                        if layer.locked {
                                                            "Unlock layer"
                                                        } else {
                                                            "Lock layer"
                                                        },
                                                    )
                                                    .size(Vec2::splat(24.))
                                                    .flat(),
                                                )
                                                .clicked()
                                            {
                                                self.finish_edits();
                                                self.transact(|s| {
                                                    s.symbol_mut().timeline.layers[idx].locked =
                                                        !layer.locked
                                                });
                                            }
                                            if ui
                                                .add(
                                                    IconButton::new(
                                                        if layer.visible {
                                                            Icon::Visible
                                                        } else {
                                                            Icon::Hidden
                                                        },
                                                        if layer.visible {
                                                            "Hide layer"
                                                        } else {
                                                            "Show layer"
                                                        },
                                                    )
                                                    .size(Vec2::splat(24.))
                                                    .flat(),
                                                )
                                                .clicked()
                                            {
                                                self.finish_edits();
                                                self.transact(|s| {
                                                    s.symbol_mut().timeline.layers[idx].visible =
                                                        !layer.visible
                                                });
                                            }
                                        },
                                    );
                                }
                            },
                        );
                        egui::ScrollArea::horizontal()
                            .id_salt("frames")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let width = (count.max(72) as f32 * self.timeline_cell)
                                    .max(ui.available_width());
                                let (rect, response) = ui.allocate_exact_size(
                                    Vec2::new(width, content_height.max(height - 14.)),
                                    Sense::click_and_drag(),
                                );
                                grid_rect = rect;
                                grid_clip = ui.clip_rect().intersect(rect);
                                let painter = ui.painter_at(rect);
                                painter.rect_filled(rect, 0, palette.timeline_fill);
                                let first = (((grid_clip.left() - rect.left())
                                        / self.timeline_cell)
                                        .max(0.)
                                        as u32)
                                        .min(count);
                                let last =
                                    (((grid_clip.right() - rect.left()) / self.timeline_cell)
                                        .ceil()
                                        .max(0.) as u32)
                                        .min(count.max(72));
                                for f in first..last {
                                    let x = rect.left() + f as f32 * self.timeline_cell;
                                    let color = if f == self.frame {
                                        palette.timeline_active_frame
                                    } else if f % 5 == 4 {
                                        palette.timeline_major
                                    } else {
                                        palette.timeline_cell
                                    };
                                    painter.rect_filled(
                                        Rect::from_min_size(
                                            Pos2::new(x, rect.top()),
                                            Vec2::new(self.timeline_cell, rect.height()),
                                        ),
                                        0,
                                        color,
                                    );
                                    painter.line_segment(
                                        [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                                        Stroke::new(0.5_f32, palette.timeline_grid),
                                    );
                                }
                                for (li, layer) in layers.iter().enumerate() {
                                    let y = rect.top() + li as f32 * ROW_HEIGHT;
                                    let row = Rect::from_min_size(
                                        Pos2::new(rect.left(), y),
                                        Vec2::new(width, ROW_HEIGHT),
                                    );
                                    if !row.intersects(grid_clip) {
                                        continue;
                                    }
                                    let p = painter.with_clip_rect(grid_clip.intersect(row));
                                    if li == self.layer {
                                        p.rect_filled(row, 0, palette.timeline_active_layer);
                                    }
                                    for (ki, key) in layer.keyframes.iter().enumerate() {
                                        let next = layer
                                            .keyframes
                                            .get(ki + 1)
                                            .map(|k| k.frame)
                                            .unwrap_or(count);
                                        if next < first || key.frame > last {
                                            continue;
                                        }
                                        let x = rect.left() + key.frame as f32 * self.timeline_cell;
                                        if key.tween.is_some() && ki + 1 < layer.keyframes.len() {
                                            let end =
                                                rect.left() + next as f32 * self.timeline_cell;
                                            p.rect_filled(
                                                Rect::from_min_max(
                                                    Pos2::new(x, y + 1.),
                                                    Pos2::new(end, y + ROW_HEIGHT - 1.),
                                                ),
                                                0,
                                                palette.timeline_tween,
                                            );
                                            paint_tween_arrow(
                                                &p,
                                                Pos2::new(x + 12., y + ROW_HEIGHT / 2.),
                                                Pos2::new(end - 8., y + ROW_HEIGHT / 2.),
                                                row,
                                                Stroke::new(1.0_f32, palette.timeline_tween_stroke),
                                            );
                                        }
                                        p.circle(
                                            Pos2::new(
                                                x + self.timeline_cell / 2.,
                                                y + ROW_HEIGHT / 2.,
                                            ),
                                            3.,
                                            if key.elements.is_empty() {
                                                palette.timeline_blank_key
                                            } else {
                                                palette.timeline_key
                                            },
                                            Stroke::new(1.0_f32, palette.timeline_key),
                                        );
                                        if !key.label.is_empty() {
                                            p.text(
                                                Pos2::new(x + 8., y + 2.),
                                                Align2::LEFT_TOP,
                                                &key.label,
                                                FontId::proportional(9.),
                                                palette.timeline_label,
                                            );
                                        }
                                    }
                                    p.line_segment(
                                        [
                                            Pos2::new(rect.left(), y + ROW_HEIGHT),
                                            Pos2::new(rect.right(), y + ROW_HEIGHT),
                                        ],
                                        Stroke::new(1.0_f32, palette.timeline_row_separator),
                                    );
                                }
                                for script in &self.symbol().timeline.scripts {
                                    painter.text(
                                        Pos2::new(
                                            rect.left()
                                                + script.frame as f32 * self.timeline_cell
                                                + self.timeline_cell / 2.,
                                            rect.top() + 3.,
                                        ),
                                        Align2::CENTER_TOP,
                                        "a",
                                        FontId::monospace(9.),
                                        palette.timeline_script,
                                    );
                                }
                                let playx = rect.left()
                                    + self.frame as f32 * self.timeline_cell
                                    + self.timeline_cell / 2.;
                                painter.line_segment(
                                    [
                                        Pos2::new(playx, rect.top()),
                                        Pos2::new(playx, rect.bottom()),
                                    ],
                                    Stroke::new(1.5_f32, palette.playhead),
                                );
                                if (response.clicked() || response.dragged())
                                    && let Some(pos) = response.interact_pointer_pos()
                                {
                                    self.finish_edits();
                                    if self.playing {
                                        self.set_playing(false);
                                    }
                                    self.frame = (((pos.x - rect.left()) / self.timeline_cell)
                                        .floor()
                                        .max(0.)
                                        as u32)
                                        .min(count - 1);
                                    self.layer =
                                        (((pos.y - rect.top()) / ROW_HEIGHT).floor().max(0.)
                                            as usize)
                                            .min(layers.len() - 1);
                                    self.clear_selection();
                                }
                                if response.drag_started()
                                    && let Some(pos) = ui.input(|i| i.pointer.press_origin())
                                {
                                    let f = (((pos.x - rect.left()) / self.timeline_cell)
                                        .floor()
                                        .max(0.)
                                        as u32)
                                        .min(count - 1);
                                    let li = (((pos.y - rect.top()) / ROW_HEIGHT).floor().max(0.)
                                        as usize)
                                        .min(layers.len() - 1);
                                    if layers[li].keyframes.iter().any(|k| k.frame == f) {
                                        self.timeline_key_drag = Some((
                                            self.symbol_id().to_owned(),
                                            layers[li].id.clone(),
                                            f,
                                        ));
                                    }
                                }
                                if response.drag_stopped()
                                    && let Some((symbol, layer_id, from)) =
                                        self.timeline_key_drag.take()
                                    && symbol == self.symbol_id()
                                    && let Some(li) = self
                                        .symbol()
                                        .timeline
                                        .layers
                                        .iter()
                                        .position(|layer| layer.id == layer_id)
                                {
                                    let release = ui
                                        .input(|input| {
                                            input.events.iter().rev().find_map(
                                                |event| match event {
                                                    egui::Event::PointerButton {
                                                        pos,
                                                        button: egui::PointerButton::Primary,
                                                        pressed: false,
                                                        ..
                                                    } => Some(*pos),
                                                    _ => None,
                                                },
                                            )
                                        })
                                        .or_else(|| response.interact_pointer_pos())
                                        .or_else(|| ui.input(|i| i.pointer.latest_pos()));
                                    let to = release.map_or(self.frame, |pos| {
                                        (((pos.x - rect.left()) / self.timeline_cell)
                                            .floor()
                                            .max(0.)
                                            as u32)
                                            .min(count - 1)
                                    });
                                    self.move_key(li, from, to, ui.input(|i| i.modifiers.alt));
                                }
                                response.context_menu(|ui| {
                                    if ui.button("Insert Keyframe   F6").clicked() {
                                        self.finish_edits();
                                        self.make_key(false);
                                        ui.close();
                                    }
                                    if ui.button("Insert Blank Keyframe   F7").clicked() {
                                        self.finish_edits();
                                        self.make_key(true);
                                        ui.close();
                                    }
                                    if ui
                                        .add_enabled(
                                            self.layer_editable(),
                                            egui::Button::new("Create Classic Tween"),
                                        )
                                        .clicked()
                                    {
                                        self.finish_edits();
                                        self.transact(|s| {
                                            let f = s.frame;
                                            let l = s.layer;
                                            if let Some(k) =
                                                s.symbol_mut().timeline.layers[l].key_at_mut(f)
                                            {
                                                k.tween = Some(Easing::EaseInOut);
                                            }
                                        });
                                        ui.close();
                                    }
                                    if ui.button("Actions   F9").clicked() {
                                        self.show_panel(PanelId::Actions);
                                        self.script_target = ScriptTarget::Frame;
                                        ui.close();
                                    }
                                });
                            });
                    });
                });
            // Paint the fixed ruler after scrolling, using the same current X
            // origin as the frame canvas. Layer and grid rows share one Y scroll.
            if grid_rect.is_finite() {
                let ruler = Rect::from_min_max(
                    Pos2::new(grid_clip.left(), header.top()),
                    Pos2::new(grid_clip.right(), header.bottom()),
                );
                let p = ui.painter_at(ruler);
                p.rect_filled(ruler, 0, palette.timeline_fill);
                ui.painter().text(
                    Pos2::new(header.left() + 4., header.center().y),
                    Align2::LEFT_CENTER,
                    "Layers",
                    FontId::proportional(12.),
                    palette.muted_text,
                );
                let first = (((ruler.left() - grid_rect.left()) / self.timeline_cell).max(0.)
                    as u32)
                    .min(count);
                let last = (((ruler.right() - grid_rect.left()) / self.timeline_cell)
                    .ceil()
                    .max(0.) as u32)
                    .min(count.max(72));
                for f in first..last {
                    let x = grid_rect.left() + f as f32 * self.timeline_cell;
                    if f == 0 || (f + 1) % 5 == 0 {
                        p.text(
                            Pos2::new(x + self.timeline_cell / 2., ruler.center().y),
                            Align2::CENTER_CENTER,
                            (f + 1).to_string(),
                            FontId::proportional(10.),
                            palette.muted_text,
                        );
                    }
                }
                let response =
                    ui.interact(ruler, ui.id().with("ruler_seek"), Sense::click_and_drag());
                if (response.clicked() || response.dragged())
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    self.finish_edits();
                    self.timeline_key_drag = None;
                    self.set_playing(false);
                    self.frame = (((pos.x - grid_rect.left()) / self.timeline_cell)
                        .floor()
                        .max(0.) as u32)
                        .min(count - 1);
                    self.clear_selection();
                }
                let playx = grid_rect.left()
                    + self.frame as f32 * self.timeline_cell
                    + self.timeline_cell / 2.;
                p.line_segment(
                    [
                        Pos2::new(playx, ruler.top()),
                        Pos2::new(playx, ruler.bottom()),
                    ],
                    Stroke::new(1.5_f32, palette.playhead),
                );
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(playx - 4., ruler.top()), Vec2::new(8., 7.)),
                    0,
                    palette.playhead,
                );
                if self.onion {
                    let (start, end) = self.onion_range();
                    let left = grid_rect.left() + start as f32 * self.timeline_cell;
                    let right = grid_rect.left() + (end + 1) as f32 * self.timeline_cell;
                    p.line_segment(
                        [
                            Pos2::new(left, ruler.bottom() - 3.),
                            Pos2::new(right, ruler.bottom() - 3.),
                        ],
                        Stroke::new(3.0_f32, palette.onion_range),
                    );
                    for (is_start, x) in [(true, left), (false, right)] {
                        let r = Rect::from_center_size(
                            Pos2::new(x, ruler.bottom() - 7.),
                            Vec2::new(9., 14.),
                        );
                        p.rect_filled(r, 1, palette.onion_range);
                        let handle = ui.interact(
                            r.intersect(ruler),
                            ui.id().with(("onion", is_start)),
                            Sense::drag(),
                        );
                        if handle.dragged()
                            && let Some(pos) = handle.interact_pointer_pos()
                        {
                            let f = (((pos.x - grid_rect.left()) / self.timeline_cell)
                                .round()
                                .max(0.) as u32)
                                .min(count - 1);
                            if self.onion_anchor {
                                if is_start {
                                    self.anchored_range.0 = f.min(self.anchored_range.1);
                                } else {
                                    self.anchored_range.1 = f.max(self.anchored_range.0);
                                }
                            } else if is_start {
                                self.onion_before = self.frame.saturating_sub(f);
                            } else {
                                self.onion_after = f.saturating_sub(self.frame);
                            }
                        }
                    }
                }
            }
            ui.allocate_ui_with_layout(
                Vec2::new(timeline_width, footer),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_max_width(timeline_width);
                    ui.spacing_mut().scroll.floating = false;
                    egui::ScrollArea::vertical()
                        .id_salt("timeline_toolbar")
                        .max_height(footer)
                        .min_scrolled_height(footer)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_max_width(ui.available_width().min(timeline_width));
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .add(IconButton::new(Icon::Plus, "Add layer").text())
                                    .clicked()
                                {
                                    self.finish_edits();
                                    self.transact(|s| {
                                        let n = s.symbol().timeline.layers.len() + 1;
                                        s.symbol_mut().timeline.layers.push(Layer {
                                            id: new_id("layer"),
                                            name: format!("Layer {n}"),
                                            visible: true,
                                            locked: false,
                                            keyframes: vec![Keyframe {
                                                frame: 0,
                                                elements: vec![],
                                                tween: None,
                                                label: String::new(),
                                            }],
                                        });
                                        s.layer = s.symbol().timeline.layers.len() - 1;
                                    });
                                }
                                if ui
                                    .add_enabled(
                                        self.symbol().timeline.layers.len() > 1,
                                        IconButton::new(Icon::Trash, "Delete selected layer"),
                                    )
                                    .on_hover_text("Delete selected layer")
                                    .clicked()
                                {
                                    self.finish_edits();
                                    self.transact(|s| {
                                        let l = s.layer;
                                        s.symbol_mut().timeline.layers.remove(l);
                                        s.normalize_selection();
                                    });
                                }
                                ui.separator();
                                if ui.add(IconButton::new(Icon::Rewind, "Rewind")).clicked() {
                                    self.finish_edits();
                                    self.timeline_key_drag = None;
                                    self.frame = 0;
                                }
                                if ui
                                    .add(IconButton::new(
                                        if self.playing {
                                            Icon::Pause
                                        } else {
                                            Icon::Play
                                        },
                                        if self.playing {
                                            "Pause preview"
                                        } else {
                                            "Play preview"
                                        },
                                    ))
                                    .clicked()
                                {
                                    self.set_playing(!self.playing);
                                }
                                ui.separator();
                                ui.checkbox(&mut self.onion, "Onion Skin");
                                ui.checkbox(&mut self.onion_outlines, "Outlines");
                                ui.allocate_ui_with_layout(
                                    Vec2::new(92., 24.),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.label("Before");
                                        ui.add(
                                            egui::DragValue::new(&mut self.onion_before)
                                                .range(0..=120),
                                        );
                                    },
                                );
                                ui.allocate_ui_with_layout(
                                    Vec2::new(84., 24.),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.label("After");
                                        ui.add(
                                            egui::DragValue::new(&mut self.onion_after)
                                                .range(0..=120),
                                        );
                                    },
                                );
                                if ui.checkbox(&mut self.onion_anchor, "Anchor").changed() {
                                    self.anchored_range = (
                                        self.frame.saturating_sub(self.onion_before),
                                        (self.frame + self.onion_after)
                                            .min(self.symbol().timeline.frame_count - 1),
                                    );
                                }
                                // Slider's own label is a nested horizontal layout. Reserve
                                // the complete group so it wraps as one control, not midway.
                                ui.allocate_ui_with_layout(
                                    Vec2::new(190., 24.),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.spacing_mut().slider_width = 120.;
                                        ui.add(
                                            egui::Slider::new(&mut self.onion_opacity, 0.05..=0.7)
                                                .text("Opacity")
                                                .show_value(false),
                                        );
                                    },
                                );
                            });
                        });
                },
            );
        });
    }
    pub fn onion_range(&self) -> (u32, u32) {
        if self.onion_anchor {
            self.anchored_range
        } else {
            (
                self.frame.saturating_sub(self.onion_before),
                (self.frame + self.onion_after).min(self.symbol().timeline.frame_count - 1),
            )
        }
    }
    pub fn tools(&mut self, ctx: &egui::Context) {
        if !self.workspace.is_panel_visible(PanelId::Tools)
            || !self.workspace.is_panel_docked(PanelId::Tools)
        {
            return;
        }
        egui::SidePanel::left("tools")
            .exact_width(58.)
            .resizable(false)
            .show(ctx, |ui| self.tools_contents(ui));
    }
    pub(crate) fn tools_contents(&mut self, ui: &mut egui::Ui) {
        let id = (
            "neoflash-panel-content",
            PanelId::Tools,
            self.clipboard.session.clone(),
        );
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| {
            ui.small("Tools");
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("tool_controls")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (tool, icon, hint) in [
                        (Tool::Select, Icon::Select, "Selection (V)"),
                        (Tool::Transform, Icon::Transform, "Free Transform (Q)"),
                        (Tool::Subselect, Icon::Subselect, "Subselection (A)"),
                        (Tool::Pen, Icon::Pen, "Pen (P)"),
                        (Tool::Pencil, Icon::Pencil, "Pencil (Y)"),
                        (Tool::Rectangle, Icon::Rectangle, "Rectangle (R)"),
                        (Tool::Ellipse, Icon::Ellipse, "Oval (O)"),
                        (Tool::Text, Icon::Text, "Text (T)"),
                    ] {
                        if ui
                            .add(
                                IconButton::new(icon, hint)
                                    .selected(self.tool == tool)
                                    .size(Vec2::new(36., 34.)),
                            )
                            .clicked()
                        {
                            self.set_tool(tool);
                        }
                    }
                    ui.separator();
                    ui.small("Fill");
                    ui.color_edit_button_srgba_unmultiplied(&mut self.fill);
                    ui.add_space(8.);
                    ui.small("View");
                    if ui.add(IconButton::new(Icon::ZoomIn, "Zoom in")).clicked() {
                        self.finish_edits();
                        self.zoom = (self.zoom * 1.25).min(4.);
                    }
                    if ui.add(IconButton::new(Icon::ZoomOut, "Zoom out")).clicked() {
                        self.finish_edits();
                        self.zoom = (self.zoom / 1.25).max(0.1);
                    }
                    if ui.add(IconButton::new(Icon::Fit, "Fit stage")).clicked() {
                        self.finish_edits();
                        self.zoom = 0.;
                        self.pan = Vec2::ZERO;
                    }
                });
        });
    }
    pub fn library(&mut self, ctx: &egui::Context) {
        if !self.workspace.is_panel_visible(PanelId::Library) {
            return;
        }
        let maximum = (ctx.content_rect().width() * 0.35).clamp(190., 480.);
        let panel = egui::SidePanel::right("library")
            .default_width(self.workspace.sizes.library_width.min(maximum))
            .max_width(maximum)
            .min_width(190.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add_space(4.);
                ui.strong("Library");
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("library_contents")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.small(format!(
                            "{} symbols",
                            self.project.symbols.len().saturating_sub(1)
                        ));
                        let symbols = self.project.symbols.clone();
                        let root = self.project.root.clone();
                        for symbol in symbols.iter().filter(|s| s.id != root) {
                            let response = ui.add(
                                IconButton::new(Icon::Symbol, &symbol.name)
                                    .text()
                                    .flat()
                                    .selected(self.library_selected.as_ref() == Some(&symbol.id))
                                    .size(Vec2::new(ui.available_width(), 26.)),
                            );
                            if response.clicked() {
                                self.library_selected = Some(symbol.id.clone());
                            }
                            if response.double_clicked() {
                                self.enter_symbol(symbol.id.clone());
                            }
                        }
                        ui.add_space(12.);
                        if let Some(id) = self.library_selected.clone() {
                            if let Some(symbol) = self.project.symbol(&id) {
                                ui.label(format!(
                                    "{:?} · {} frames",
                                    symbol.kind, symbol.timeline.frame_count
                                ));
                            }
                            if ui.button("Place instance on stage").clicked() {
                                if id == self.symbol_id() {
                                    self.message("A symbol cannot contain itself.");
                                } else {
                                    self.finish_edits();
                                    self.add_element(
                                        ElementKind::Instance {
                                            symbol_id: id,
                                            playback: InstancePlayback::Independent,
                                        },
                                        120.,
                                        120.,
                                    );
                                }
                            }
                            if ui.button("Edit symbol").clicked() {
                                self.enter_symbol(self.library_selected.clone().unwrap());
                            }
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                self.selection.len() == 1,
                                egui::Button::new("Convert to Symbol   F8"),
                            )
                            .clicked()
                        {
                            self.convert_to_symbol();
                        }
                        self.resource_library(ui);
                        ui.add_space(18.);
                        ui.strong("Document");
                        ui.separator();
                        ui.label(&self.project.name);
                        ui.small(format!(
                            "{} × {} px · {} fps",
                            self.project.width, self.project.height, self.project.fps
                        ));
                        ui.small("Target: HTML5");
                        ui.small("Double-click a symbol to edit its timeline.");
                    });
            });
        self.workspace.sizes.library_width = panel.response.rect.width();
    }
    pub fn bottom_panel(&mut self, ctx: &egui::Context) {
        if !self.workspace.bottom_visible() {
            return;
        }
        let maximum = (ctx.content_rect().height() * 0.40).clamp(145., 420.);
        let panel = egui::TopBottomPanel::bottom("properties")
            .default_height(self.workspace.sizes.bottom_height.min(maximum))
            .min_height(145.)
            .max_height(maximum)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (name, id, tab) in [
                        ("Properties", PanelId::Properties, BottomTab::Properties),
                        ("Actions", PanelId::Actions, BottomTab::Actions),
                        (
                            "Accessibility",
                            PanelId::Accessibility,
                            BottomTab::Accessibility,
                        ),
                        ("Output", PanelId::Output, BottomTab::Output),
                    ] {
                        if self.workspace.is_panel_visible(id)
                            && self.workspace.is_panel_docked(id)
                            && ui.selectable_label(self.bottom == tab, name).clicked()
                        {
                            self.show_panel(id);
                        }
                    }
                });
                ui.separator();
                match self.bottom {
                    BottomTab::Properties => {
                        egui::ScrollArea::vertical()
                            .id_salt("property_contents")
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.properties(ui, ctx));
                    }
                    BottomTab::Actions => self.actions_contents(ui),
                    BottomTab::Accessibility => {
                        egui::ScrollArea::vertical()
                            .id_salt("accessibility_contents")
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.accessibility_panel(ui));
                    }
                    BottomTab::Output => {
                        egui::ScrollArea::vertical()
                            .id_salt("output_contents")
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                for line in &self.output {
                                    ui.monospace(line);
                                }
                            });
                    }
                }
            });
        self.workspace.sizes.bottom_height = panel.response.rect.height();
    }
    fn properties(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.selection.len() > 1 {
            ui.strong(format!("{} objects selected", self.selection.len()));
            ui.label("Select one object to edit its properties.");
            ui.small("V: move the selection · Q: scale and rotate · Shift-click: add or remove an object");
            return;
        }
        if let Some(mut element) = self.selected_sampled() {
            let original = element.clone();
            let mut edit_symbol = None;
            let mut convert_text = false;
            ui.add_enabled_ui(self.selected_editable(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Instance / object");
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut element.name).desired_width(180.));
                    ui.separator();
                    ui.label("Opacity");
                    ui.add(
                        egui::DragValue::new(&mut element.transform.alpha)
                            .range(0.0..=1.0)
                            .speed(0.01),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    for (name, value) in [
                        ("X", &mut element.transform.x),
                        ("Y", &mut element.transform.y),
                        ("Scale X", &mut element.transform.scale_x),
                        ("Scale Y", &mut element.transform.scale_y),
                        ("Rotation", &mut element.transform.rotation),
                    ] {
                        ui.label(name);
                        ui.add(
                            egui::DragValue::new(value)
                                .speed(if name.starts_with("Scale") { 0.01 } else { 1. })
                                .max_decimals(2),
                        );
                    }
                });
                match &mut element.kind {
                    ElementKind::Rectangle {
                        width,
                        height,
                        fill,
                        stroke,
                        stroke_width,
                    }
                    | ElementKind::Ellipse {
                        width,
                        height,
                        fill,
                        stroke,
                        stroke_width,
                    } => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Width");
                            ui.add(egui::DragValue::new(width).range(1.0..=10000.));
                            ui.label("Height");
                            ui.add(egui::DragValue::new(height).range(1.0..=10000.));
                            ui.label("Fill");
                            ui.color_edit_button_srgba_unmultiplied(fill);
                            ui.label("Stroke");
                            ui.color_edit_button_srgba_unmultiplied(stroke);
                            ui.add(egui::DragValue::new(stroke_width).range(0.0..=100.));
                        });
                    }
                    ElementKind::Text {
                        text,
                        font_family,
                        font_size,
                        color,
                    } => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Text");
                            ui.add(egui::TextEdit::singleline(text).desired_width(220.));
                            ui.label("Size");
                            ui.add(egui::DragValue::new(font_size).range(4.0..=512.));
                            ui.color_edit_button_srgba_unmultiplied(color);
                            egui::ComboBox::from_id_salt("font_family")
                                .selected_text(font_family.as_str())
                                .width(180.)
                                .show_ui(ui, |ui| {
                                    ui.text_edit_singleline(&mut self.font_filter);
                                    for family in &self.fonts.families {
                                        if self.font_filter.is_empty()
                                            || family
                                                .to_lowercase()
                                                .contains(&self.font_filter.to_lowercase())
                                        {
                                            ui.selectable_value(
                                                font_family,
                                                family.clone(),
                                                family,
                                            );
                                        }
                                    }
                                });
                        });
                        if let Err(e) = self.fonts.load(ctx, font_family) {
                            ui.colored_label(UiPalette::current(ctx).error_text, e);
                        }
                        if ui.button("Use text box").clicked() {
                            convert_text = true;
                        }
                    }
                    kind @ ElementKind::TextBox { .. } => self.text_box_properties(ui, ctx, kind),
                    ElementKind::Bitmap {
                        resource_id,
                        width,
                        height,
                        smoothing,
                    } => {
                        ui.horizontal_wrapped(|ui| {
                            let path = self
                                .project
                                .resources
                                .iter()
                                .find(|resource| resource.id == *resource_id)
                                .map(|resource| resource.path.as_str())
                                .unwrap_or("Missing image");
                            ui.label(format!("{path} · {width} × {height} px"));
                            ui.checkbox(smoothing, "Smoothing");
                        });
                    }
                    ElementKind::Path {
                        commands,
                        fill_rule,
                        fill,
                        stroke,
                        stroke_width,
                    } => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("Vector path · {} commands", commands.len()));
                            ui.label("Fill");
                            ui.color_edit_button_srgba_unmultiplied(fill);
                            ui.label("Stroke");
                            ui.color_edit_button_srgba_unmultiplied(stroke);
                            ui.add(egui::DragValue::new(stroke_width).range(0.0..=100.));
                            egui::ComboBox::from_id_salt("fill_rule")
                                .selected_text(if *fill_rule == FillRule::EvenOdd {
                                    "Even–odd"
                                } else {
                                    "Nonzero"
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(fill_rule, FillRule::NonZero, "Nonzero");
                                    ui.selectable_value(fill_rule, FillRule::EvenOdd, "Even–odd");
                                });
                            if ui.button("Edit points (A)").clicked() {
                                self.set_tool(Tool::Subselect);
                                self.select_single_id(original.id.clone());
                            }
                        });
                    }
                    ElementKind::Instance {
                        symbol_id,
                        playback,
                    } => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!(
                                "Symbol: {}",
                                self.project
                                    .symbol(symbol_id)
                                    .map(|s| s.name.as_str())
                                    .unwrap_or("Missing")
                            ));
                            egui::ComboBox::from_id_salt("playback")
                                .selected_text(format!("{playback:?}"))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        playback,
                                        InstancePlayback::Independent,
                                        "MovieClip · independent",
                                    );
                                    ui.selectable_value(
                                        playback,
                                        InstancePlayback::Loop,
                                        "Graphic · loop",
                                    );
                                    ui.selectable_value(
                                        playback,
                                        InstancePlayback::PlayOnce,
                                        "Graphic · play once",
                                    );
                                    ui.selectable_value(
                                        playback,
                                        InstancePlayback::SingleFrame(0),
                                        "Graphic · single frame",
                                    );
                                });
                            if ui.button("Edit").clicked() {
                                edit_symbol = Some(symbol_id.clone());
                            }
                        });
                    }
                }
            });
            if convert_text
                && let ElementKind::Text {
                    text,
                    font_family,
                    font_size,
                    color,
                } = &element.kind
            {
                let named = crate::fonts::styled_family(font_family, false, false);
                let family = if ctx.fonts(|fonts| fonts.families().contains(&named)) {
                    named
                } else {
                    egui::FontFamily::Proportional
                };
                let galley = ui.painter().layout_no_wrap(
                    text.clone(),
                    egui::FontId::new(*font_size as f32, family),
                    egui::Color32::WHITE,
                );
                let kind = ElementKind::TextBox {
                    text: text.clone(),
                    width: (galley.size().x as f64 + 4.).clamp(100., 1000000.),
                    height: (galley.size().y as f64 + 4.).clamp(100., 1000000.),
                    format: TextFormat {
                        font: font_family.clone(),
                        size: *font_size,
                        color: u32::from_be_bytes([0, color[0], color[1], color[2]]),
                        ..Default::default()
                    },
                    word_wrap: true,
                    multiline: true,
                };
                element.transform.alpha *= color[3] as f64 / 255.;
                element.kind = kind;
            }
            if original != element {
                if matches!(element.kind, ElementKind::TextBox { .. })
                    && let Some(field) = TextFieldState::from_kind(&element.kind)
                    && let Err(error) = field.validate()
                {
                    self.message(format!("Text box edit rejected: {error}"));
                    return;
                }
                self.finish_edits();
                self.select_single_id(original.id.clone());
                self.transact(|s| {
                    s.ensure_selected_key();
                    if let Some(e) = s.selected_element_mut() {
                        *e = element
                    }
                });
            }
            if let Some(id) = edit_symbol {
                self.enter_symbol(id);
            }
        } else {
            // Edit a draft so finishing an active gesture precedes any document mutation.
            // Apply only these fields: finishing a pen path can add new elements and keys.
            let mut document = self.project.clone();
            let symbol_id = self.symbol_id().to_owned();
            let layer_id = self.symbol().timeline.layers[self.layer].id.clone();
            let layer = self.layer;
            let frame = self.frame;
            ui.horizontal_wrapped(|ui| {
                ui.strong("Document");
                ui.add(egui::TextEdit::singleline(&mut document.name).desired_width(180.));
                ui.label("Size");
                ui.add(egui::DragValue::new(&mut document.width).range(1..=8192));
                ui.label("×");
                ui.add(egui::DragValue::new(&mut document.height).range(1..=8192));
                ui.label("FPS");
                ui.add(egui::DragValue::new(&mut document.fps).range(1..=120));
                ui.label("Background");
                ui.color_edit_button_srgba_unmultiplied(&mut document.background);
            });
            let timeline = &mut document.symbol_mut(&symbol_id).unwrap().timeline;
            ui.horizontal_wrapped(|ui| {
                ui.label("Layer");
                ui.add(
                    egui::TextEdit::singleline(&mut timeline.layers[layer].name)
                        .desired_width(160.),
                );
                let min = timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .map(|k| k.frame + 1)
                    .chain(timeline.scripts.iter().map(|s| s.frame + 1))
                    .max()
                    .unwrap_or(1);
                ui.label("Frames");
                ui.add(egui::DragValue::new(&mut timeline.frame_count).range(min..=10000));
                ui.add_enabled_ui(self.layer_editable(), |ui| {
                    if let Some(key) = timeline.layers[layer].key_at_mut(frame) {
                        ui.label("Label");
                        ui.add(egui::TextEdit::singleline(&mut key.label).desired_width(140.));
                        egui::ComboBox::from_id_salt("tween")
                            .selected_text(match key.tween {
                                None => "No tween",
                                Some(Easing::Linear) => "Linear",
                                Some(Easing::EaseIn) => "Ease in",
                                Some(Easing::EaseOut) => "Ease out",
                                Some(Easing::EaseInOut) => "Ease in/out",
                                Some(Easing::CubicBezier { .. }) => "Custom curve",
                            })
                            .show_ui(ui, |ui| {
                                for (label, value) in [
                                    ("No tween", None),
                                    ("Linear", Some(Easing::Linear)),
                                    ("Ease in", Some(Easing::EaseIn)),
                                    ("Ease out", Some(Easing::EaseOut)),
                                    ("Ease in/out", Some(Easing::EaseInOut)),
                                    (
                                        "Custom curve",
                                        Some(Easing::CubicBezier {
                                            x1: 0.25,
                                            y1: 0.1,
                                            x2: 0.25,
                                            y2: 1.,
                                        }),
                                    ),
                                ] {
                                    ui.selectable_value(&mut key.tween, value, label);
                                }
                            });
                    }
                });
            });
            ui.add_enabled_ui(self.layer_editable(), |ui| {
                if let Some(key) = timeline.layers[layer].key_at_mut(frame)
                    && let Some(Easing::CubicBezier { x1, y1, x2, y2 }) = &mut key.tween
                {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Bezier control points");
                        for v in [x1, y1, x2, y2] {
                            ui.add(egui::DragValue::new(v).range(0.0..=1.0).speed(0.01));
                        }
                    });
                }
            });
            if document != self.project {
                self.finish_edits();
                self.clear_selection();
                self.transact(|editor| {
                    editor.project.name = document.name.clone();
                    editor.project.width = document.width;
                    editor.project.height = document.height;
                    editor.project.fps = document.fps;
                    editor.project.background = document.background;
                    let draft = &document.symbol(&symbol_id).unwrap().timeline;
                    if let Some(symbol) = editor.project.symbol_mut(&symbol_id) {
                        let timeline = &mut symbol.timeline;
                        let min = timeline
                            .layers
                            .iter()
                            .flat_map(|l| &l.keyframes)
                            .map(|k| k.frame + 1)
                            .chain(timeline.scripts.iter().map(|s| s.frame + 1))
                            .max()
                            .unwrap_or(1);
                        timeline.frame_count = draft.frame_count.max(min);
                        if let Some(target) = timeline.layers.iter_mut().find(|l| l.id == layer_id)
                        {
                            target.name = draft.layers[layer].name.clone();
                            if target.visible
                                && !target.locked
                                && let Some(changed) = draft.layers[layer].key_at(frame)
                                && let Some(key) = target
                                    .keyframes
                                    .iter_mut()
                                    .find(|k| k.frame == changed.frame)
                            {
                                key.label = changed.label.clone();
                                key.tween = changed.tween;
                            }
                        }
                    }
                    editor.normalize_selection();
                });
            }
        }
    }
    pub(crate) fn actions_editor_id(&self) -> egui::Id {
        if self.script_target == ScriptTarget::Source {
            return egui::Id::new((
                "neoflash-project-source-editor",
                &self.clipboard.session,
                &self.selected_source,
            ));
        }
        let target = match self.script_target {
            ScriptTarget::Frame => "frame".to_owned(),
            ScriptTarget::Source => {
                format!("source:{}", self.selected_source.as_deref().unwrap_or(""))
            }
            ScriptTarget::Object => format!(
                "object:{}:{:?}",
                self.selected_element().map_or("", |e| e.id.as_str()),
                self.script_event
            ),
        };
        egui::Id::new((
            "neoflash-actions-editor",
            &self.clipboard.session,
            self.symbol_id(),
            self.frame,
            target,
        ))
    }
    pub(crate) fn actions_contents(&mut self, ui: &mut egui::Ui) {
        let id = (
            "neoflash-panel-content",
            PanelId::Actions,
            self.clipboard.session.clone(),
        );
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| self.actions_body(ui));
    }
    fn actions_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.script_target,
                ScriptTarget::Frame,
                format!("Frame {}", self.frame + 1),
            );
            ui.add_enabled_ui(self.selected_editable(), |ui| {
                ui.selectable_value(
                    &mut self.script_target,
                    ScriptTarget::Object,
                    "Selected object",
                );
            });
            ui.selectable_value(
                &mut self.script_target,
                ScriptTarget::Source,
                "Project files",
            );
            if self.script_target == ScriptTarget::Object {
                egui::ComboBox::from_id_salt("event")
                    .selected_text(format!("{:?}", self.script_event))
                    .show_ui(ui, |ui| {
                        for e in [
                            ObjectEvent::Click,
                            ObjectEvent::Load,
                            ObjectEvent::EnterFrame,
                        ] {
                            ui.selectable_value(&mut self.script_event, e, format!("{e:?}"));
                        }
                    });
            }
        });
        if self.script_target == ScriptTarget::Source {
            self.project_sources(ui);
            return;
        }
        if self.script_target == ScriptTarget::Object && !self.selected_editable() {
            if self.selection.len() > 1 {
                ui.strong(format!("{} objects selected", self.selection.len()));
            }
            ui.label("Select one editable object to edit its actions, or select Frame above.");
            return;
        }
        let object_id = self.selected_element().map(|element| element.id.clone());
        let original = match self.script_target {
            ScriptTarget::Source => unreachable!("Project files use their own editor"),
            ScriptTarget::Frame => self
                .symbol()
                .timeline
                .scripts
                .iter()
                .find(|s| s.frame == self.frame)
                .map(|s| s.script.clone()),
            ScriptTarget::Object => self
                .selected_element()
                .and_then(|e| e.scripts.iter().find(|s| s.event == self.script_event))
                .map(|s| s.script.clone()),
        }
        .unwrap_or_default();
        let mut script = original.clone();
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("language")
                .selected_text(format!("{:?}", script.language))
                .show_ui(ui, |ui| {
                    for (language, label) in [
                        (ScriptLanguage::JavaScript, "JavaScript"),
                        (ScriptLanguage::TypeScript, "TypeScript"),
                        (ScriptLanguage::ActionScript2, "AS2 · initial subset"),
                        (ScriptLanguage::ActionScript3, "AS3 · initial subset"),
                    ] {
                        ui.selectable_value(&mut script.language, language, label);
                    }
                });
            if ui.button("Check script").clicked() {
                self.check_action_source(&script);
            }
            ui.small("Test Movie runs code. Timeline preview does not.");
        });
        self.show_code_feedback(ui, &script);
        let editable = self.script_target == ScriptTarget::Frame || self.selected_editable();
        let editor_id = self.actions_editor_id();
        let mut text_editing = false;
        ui.add_enabled_ui(editable, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("actions_source_scroll", editor_id))
                .show(ui, |ui| {
                    let response = ui.add_sized(
                        [ui.available_width(), ui.available_height().max(60.)],
                        egui::TextEdit::multiline(&mut script.source)
                            .id(editor_id)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .hint_text(
                                "this.stop();\n// Use this.gotoAndPlay(1) to control a timeline.",
                            ),
                    );
                    text_editing = response.changed()
                        || response.gained_focus()
                        || (response.has_focus() && ui.input(|input| input.focused));
                });
        });
        if text_editing && self.playing {
            self.set_playing(false);
        }
        if script != original && editable {
            if self.playing {
                self.set_playing(false);
            }
            self.finish_edits();
            if self.script_target == ScriptTarget::Object
                && let Some(id) = object_id
            {
                self.select_single_id(id);
            }
            self.transact(|s| match s.script_target {
                ScriptTarget::Source => unreachable!("Project files use their own editor"),
                ScriptTarget::Frame => {
                    let frame = s.frame;
                    let scripts = &mut s.symbol_mut().timeline.scripts;
                    scripts.retain(|fs| fs.frame != frame);
                    if !script.source.trim().is_empty()
                        || script.language != ScriptLanguage::JavaScript
                    {
                        scripts.push(FrameScript { frame, script });
                    }
                }
                ScriptTarget::Object => {
                    let event = s.script_event;
                    s.ensure_selected_key();
                    if let Some(e) = s.selected_element_mut() {
                        e.scripts.retain(|os| os.event != event);
                        if !script.source.trim().is_empty()
                            || script.language != ScriptLanguage::JavaScript
                        {
                            e.scripts.push(ObjectScript { event, script });
                        }
                    }
                }
            });
        }
    }
}
