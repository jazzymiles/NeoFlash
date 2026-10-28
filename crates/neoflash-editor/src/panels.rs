use crate::app::*;
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::{
    theme::{ThemeMode, UiPalette},
    timeline_paint::{ROW_HEIGHT, RULER_HEIGHT, frame_at_x, paint_tween_arrow, timeline_cell_at},
    ui_icons::{Icon, IconButton, inline_icon},
    workspace::{PanelId, PanelPlacement},
};
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use neoflash_core::*;

impl Editor {
    pub fn menu(&mut self, ctx: &egui::Context) {
        use crate::ui_icons::{menu_item, menu_item_enabled, menu_popup, menu_toggle};
        let clipboard_target = self.clipboard_target(ctx);
        let mut edit_open = false;
        egui::TopBottomPanel::top("menu")
            .exact_height(28.)
            .show(ctx, |ui| {
                egui::MenuBar::new()
                    .config(
                        egui::containers::menu::MenuConfig::new()
                            .style(crate::ui_icons::menu_style),
                    )
                    .ui(ui, |ui| {
                        ui.strong("NeoFlash");
                        ui.separator();
                        menu_popup(ui, "File", "Import graphics to Stage…", "⌘S", |ui| {
                            if menu_item(ui, "New", "⌘N").clicked() {
                                self.request(Pending::New, ctx);
                                ui.close();
                            }
                            if menu_item(ui, "Open…", "⌘O").clicked() {
                                self.request(Pending::Open, ctx);
                                ui.close();
                            }
                            if menu_item(ui, "Import legacy Flash…", "").clicked() {
                                self.request(Pending::Import, ctx);
                                ui.close();
                            }
                            if menu_item(ui, "Import graphics to Stage…", "").clicked() {
                                self.import_graphics_dialog();
                                ui.close();
                            }
                            if menu_item(ui, "Save", "⌘S").clicked() {
                                self.save(false);
                                ui.close();
                            }
                            if menu_item(ui, "Save As…", "").clicked() {
                                self.save(true);
                                ui.close();
                            }
                            ui.separator();
                            if menu_item(ui, "Publish HTML5…", "").clicked() {
                                self.export(false);
                                ui.close();
                            }
                            if menu_item(ui, "Open example", "").clicked() {
                                self.request(Pending::Demo, ctx);
                                ui.close();
                            }
                        });
                        menu_popup(ui, "Edit", "Paste in Place", "Shift+⌘V", |ui| {
                            edit_open = true;
                            if !self.clipboard.menu_open
                                || self.clipboard.menu_viewport != Some(ctx.viewport_id())
                            {
                                self.clipboard.menu_target = Some(clipboard_target.clone());
                            }
                            if menu_item_enabled(
                                ui,
                                self.history.can_undo() || self.vector.draft.is_some(),
                                "Undo",
                                "⌘Z",
                            )
                            .clicked()
                            {
                                self.undo();
                                ui.close();
                            }
                            if menu_item_enabled(ui, self.history.can_redo(), "Redo", "Shift+⌘Z")
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
                            for (label, shortcut, command, enabled) in [
                                (
                                    "Cut",
                                    "⌘X",
                                    crate::clipboard::ClipboardCommand::Cut,
                                    can_copy,
                                ),
                                (
                                    "Copy",
                                    "⌘C",
                                    crate::clipboard::ClipboardCommand::Copy,
                                    can_copy,
                                ),
                                (
                                    "Paste",
                                    "⌘V",
                                    crate::clipboard::ClipboardCommand::Paste,
                                    can_paste,
                                ),
                                (
                                    "Paste in Place",
                                    "Shift+⌘V",
                                    crate::clipboard::ClipboardCommand::PasteInPlace,
                                    !text_target && self.layer_editable(),
                                ),
                            ] {
                                if menu_item_enabled(ui, enabled, label, shortcut).clicked() {
                                    self.request_clipboard(ctx, command);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if menu_item(ui, "Select All", "⌘A").clicked() {
                                self.select_all();
                                ui.close();
                            }
                            if menu_item_enabled(ui, !self.selection.is_empty(), "Deselect", "")
                                .clicked()
                            {
                                self.finish_edits();
                                self.clear_selection();
                                ui.close();
                            }
                            if menu_item_enabled(ui, !self.selection.is_empty(), "Duplicate", "⌘D")
                                .clicked()
                            {
                                self.duplicate();
                                ui.close();
                            }
                            if menu_item_enabled(ui, !self.selection.is_empty(), "Delete", "")
                                .clicked()
                            {
                                self.delete_selected();
                                ui.close();
                            }
                        });
                        self.modify_menu_ui(ui);
                        menu_popup(ui, "Insert", "Convert to Symbol…", "⌘F8", |ui| {
                            if menu_item(ui, "New Symbol…", "⌘F8").clicked() {
                                self.begin_new_symbol();
                                ui.close();
                            }
                            ui.separator();
                            if menu_item_enabled(ui, self.layer_editable(), "Video placeholder", "")
                                .clicked()
                            {
                                self.finish_edits();
                                self.add_element(
                                    ElementKind::Video {
                                        width: 320.,
                                        height: 240.,
                                    },
                                    80.,
                                    80.,
                                );
                                ui.close();
                            }
                            ui.separator();
                            if menu_item(ui, "Frame", "F5").clicked() {
                                self.finish_edits();
                                self.insert_frame();
                                ui.close();
                            }
                            if menu_item(ui, "Keyframe", "F6").clicked() {
                                self.finish_edits();
                                self.make_key(false);
                                ui.close();
                            }
                            if menu_item(ui, "Blank Keyframe", "F7").clicked() {
                                self.finish_edits();
                                self.make_key(true);
                                ui.close();
                            }
                            if menu_item_enabled(
                                ui,
                                self.can_convert_selection(),
                                "Convert to Symbol…",
                                "F8",
                            )
                            .clicked()
                            {
                                self.convert_to_symbol();
                                ui.close();
                            }
                        });
                        menu_popup(ui, "Control", "Test Movie", "⌘Enter", |ui| {
                            if menu_item(ui, if self.playing { "Stop" } else { "Play" }, "Enter")
                                .clicked()
                            {
                                self.set_playing(!self.playing);
                                ui.close();
                            }
                            if menu_item(ui, "Test Movie", "⌘Enter").clicked() {
                                self.export(true);
                                ui.close();
                            }
                            if menu_item(ui, "Rewind", "").clicked() {
                                self.finish_edits();
                                self.timeline_key_drag = None;
                                self.reset_authored_preview();
                                self.frame = 0;
                                ui.close();
                            }
                        });
                        menu_popup(ui, "Window", "Restore Workspace", "F9", |ui| {
                            let panels = [
                                ("Timeline", PanelId::Timeline),
                                ("Tools", PanelId::Tools),
                                ("Library", PanelId::Library),
                                ("Properties", PanelId::Properties),
                                ("Actions", PanelId::Actions),
                                ("Accessibility", PanelId::Accessibility),
                                ("Output", PanelId::Output),
                                ("Align", PanelId::Align),
                                ("Transform", PanelId::Transform),
                                ("Color", PanelId::Color),
                            ];
                            for (name, panel) in panels {
                                let mut visible = self.workspace.is_panel_visible(panel);
                                if menu_toggle(
                                    ui,
                                    &mut visible,
                                    name,
                                    if panel == PanelId::Actions { "F9" } else { "" },
                                )
                                .changed()
                                {
                                    self.set_panel_visible(panel, visible);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            for (label, placement, target) in [
                                (
                                    "Detach Panel",
                                    PanelPlacement::Docked,
                                    PanelPlacement::Detached,
                                ),
                                (
                                    "Dock Panel",
                                    PanelPlacement::Detached,
                                    PanelPlacement::Docked,
                                ),
                            ] {
                                let any = panels.iter().any(|(_, panel)| {
                                    self.workspace.placement(*panel) == placement
                                });
                                ui.add_enabled_ui_semantic(any, |ui| {
                                    menu_popup(ui, label, "Accessibility", "", |ui| {
                                        for (name, panel) in panels {
                                            if self.workspace.placement(panel) == placement
                                                && menu_item(ui, name, "").clicked()
                                            {
                                                self.set_panel_placement(ctx, panel, target);
                                                ui.close();
                                            }
                                        }
                                    })
                                });
                            }
                            if menu_item(ui, "Restore Workspace", "").clicked() {
                                self.restore_workspace(ctx);
                                ui.close();
                            }
                            ui.separator();
                            menu_popup(ui, "Appearance", "Light", "", |ui| {
                                for (name, theme) in
                                    [("Light", ThemeMode::Light), ("Dark", ThemeMode::Dark)]
                                {
                                    let mut selected = self.workspace.theme == theme;
                                    if menu_toggle(ui, &mut selected, name, "").clicked() {
                                        self.set_theme(ctx, theme);
                                        ui.close();
                                    }
                                }
                            });
                            if menu_item(ui, "About NeoFlash", "").clicked() {
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
    #[cfg(test)]
    pub fn timeline(&mut self, ctx: &egui::Context) {
        self.dock_edge(ctx, crate::workspace::DockEdge::Top);
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
        #[derive(Clone)]
        struct LayerRename {
            layer: String,
            text: String,
            focus: bool,
        }
        #[derive(Clone)]
        struct LayerReorder {
            symbol: String,
            layer: String,
            start: Pos2,
            moved: bool,
        }
        #[derive(Clone)]
        struct ColumnResize {
            start: Pos2,
            width: f32,
            saved: f32,
            moved: bool,
        }
        let reorder_id = egui::Id::new((&id, "layer-reorder"));
        let resize_id = egui::Id::new((&id, "layer-column-resize"));
        let mut reorder = ctx.data(|data| data.get_temp::<LayerReorder>(reorder_id));
        let mut resize = ctx.data(|data| data.get_temp::<ColumnResize>(resize_id));
        let mut commit_reorder = None;
        let (pressed, released, pointer, down, cancelled) = ctx.input(|input| {
            let mut pressed = None;
            let mut released = None;
            for event in &input.events {
                if let egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: held,
                    ..
                } = event
                {
                    if *held {
                        pressed = Some(*pos);
                        released = None;
                    } else {
                        released = Some(*pos);
                    }
                }
            }
            (
                pressed,
                released,
                input.pointer.latest_pos().or(released),
                input.pointer.primary_down(),
                input.key_pressed(egui::Key::Escape) || !input.focused,
            )
        });
        if cancelled {
            if let Some(resize) = resize.take() {
                self.workspace.sizes.layer_column_width = resize.saved;
            }
            reorder = None;
        }
        let rename_id = egui::Id::new((&id, "layer-rename"));
        let mut rename = ctx.data(|data| data.get_temp::<LayerRename>(rename_id));
        let mut commit_rename = None;
        let mut cancel_rename = false;
        let previous_cell = self.timeline_cell;
        let timeline_width = ui.available_width();
        let maximum_layer_width = (timeline_width - 100.).clamp(140., 320.);
        let mut layer_column_width = self
            .workspace
            .sizes
            .layer_column_width
            .clamp(140., maximum_layer_width);
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| {
            // Keep a usable frame canvas and footer even in the smallest native
            // Timeline window. Overflowing header controls remain scrollable.
            ui.spacing_mut().scroll.floating = false;
            ui.spacing_mut().item_spacing = Vec2::new(4., 0.);
            ui.style_mut().override_font_id = Some(FontId::proportional(crate::theme::PANEL_FONT));
            egui::ScrollArea::horizontal()
                .id_salt("timeline_header")
                .auto_shrink([false, true])
                .themed().show(ui, |ui| {
            ui.horizontal(|ui| {
                self.panel_title(ui, PanelId::Timeline, "Timeline");
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
                        self.reset_authored_preview();
                        self.frame = 0;
                        self.layer = 0;
                        self.clear_selection();
                    }
                }
                self.timeline_timing_ui(ui);
                ui.separator();
                crate::theme::timeline_controls(ui, |ui| {
                    ui.spacing_mut().slider_width = 84.;
                    if ui.small_button("−").on_hover_text("Show more frames").clicked() {
                        self.timeline_cell = (self.timeline_cell / 1.4).max(3.);
                    }
                    ui.add(egui::Slider::new(&mut self.timeline_cell, 3.0..=52.0)
                        .logarithmic(true).show_value(false).text("Zoom"))
                        .on_hover_text("Change the width of timeline cells without changing the animation");
                    if ui.small_button("+").on_hover_text("Widen frame cells").clicked() {
                        self.timeline_cell = (self.timeline_cell * 1.4).min(52.);
                    }
                    if ui.small_button("Fit").on_hover_text("Fit the whole timeline").clicked() {
                        self.timeline_cell = ((timeline_width - layer_column_width - 30.).max(1.)
                            / self.symbol().timeline.frame_count.max(1) as f32).clamp(3., 52.);
                    }
                });
            });
                });
            let zoom_changed = self.timeline_cell != previous_cell;
            if zoom_changed {
                self.timeline_key_drag = None;
            }
            let button_states = self.symbol().kind == SymbolKind::Button;
            let cell_width = if button_states { 64. } else { self.timeline_cell };
            // The frame canvas may be much wider than the viewport. Freeze the
            // toolbar width before allocating it so scrolling frames cannot
            // widen the controls. Small windows keep a bounded, scrollable footer.
            let timeline_width = ui.available_width();
            let desired_footer: f32 = crate::theme::FOOTER_HEIGHT;
            let footer =
                desired_footer.min((ui.available_height() - RULER_HEIGHT - 25. - 10.).max(24.));
            let height =
                (ui.available_height() - footer - RULER_HEIGHT - 2. * ui.spacing().item_spacing.y)
                    .max(25.);
            let (header, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), RULER_HEIGHT),
                Sense::hover(),
            );
            let divider_x = header.left() + layer_column_width + ui.spacing().item_spacing.x / 2.;
            let divider_rect = Rect::from_min_max(Pos2::new(divider_x - 3., header.top()), Pos2::new(divider_x + 3., header.bottom() + height));
            let divider = ui.interact(divider_rect.intersect(ui.clip_rect()), ui.id().with("layer-column-divider"), Sense::click_and_drag());
            divider.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Resize layer column"));
            if !cancelled && let Some(start)=pressed.filter(|point| divider_rect.contains(*point) && ui.clip_rect().contains(*point) && ui.is_enabled() && ctx.layer_id_at(*point)==Some(ui.layer_id())) {
                resize=Some(ColumnResize {start,width:layer_column_width,saved:self.workspace.sizes.layer_column_width,moved:false});
                reorder=None;
                self.timeline_key_drag=None;
            }
            if divider.hovered() || resize.is_some() {ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);}
            if let Some(drag)=&mut resize && let Some(position)=pointer {
                drag.moved |= (position.x-drag.start.x).abs()>0.5;
                if drag.moved {
                    layer_column_width=(drag.width + position.x - drag.start.x).clamp(140.,maximum_layer_width);
                    self.workspace.sizes.layer_column_width=layer_column_width;
                    ctx.request_repaint();
                }
            }
            let layer_name_width = layer_column_width - 3. * (crate::timeline_paint::LAYER_CONTROL_WIDTH + crate::timeline_paint::LAYER_COLUMN_GAP);
            // Frame exposures are ordered by their frame, independent of JSON order.
            let layer_indices = crate::layer_controls::visible_layer_indices(&self.symbol().timeline);
            if !layer_indices.contains(&self.layer) {
                self.layer = layer_indices.iter().copied().rfind(|index| *index < self.layer).unwrap_or(0);
                self.clear_selection();
            }
            let mut layers: Vec<_> = layer_indices.iter().map(|index| self.symbol().timeline.layers[*index].clone()).collect();
            for layer in &mut layers {
                layer.keyframes.sort_by_key(|key| key.frame);
            }
            let count = self.symbol().timeline.frame_count;
            let content_height = layers.len() as f32 * ROW_HEIGHT;
            // Solid scrollbars own a separate strip instead of covering the last
            // visible key. Content drags belong to frames, not kinetic scrolling.
            ui.spacing_mut().scroll.floating = false;
            let scroll_source = egui::scroll_area::ScrollSource::SCROLL_BAR
                | egui::scroll_area::ScrollSource::MOUSE_WHEEL;
            let mut grid_rect = Rect::NOTHING;
            let mut grid_clip = Rect::NOTHING;
            let rows = egui::ScrollArea::vertical()
                .id_salt("timeline_rows")
                .scroll_source(scroll_source)
                .min_scrolled_height(25.)
                .max_height(height)
                .auto_shrink([false, false])
                .themed().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(layer_column_width, content_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(layer_column_width);
                                ui.spacing_mut().item_spacing.y = 0.;
                                for (row_index, layer) in layers.iter().enumerate() {
                                    let idx = layer_indices[row_index];
                                    let (row_rect, _) = ui.allocate_exact_size(
                                        Vec2::new(layer_column_width, ROW_HEIGHT),
                                        Sense::hover(),
                                    );
                                    ui.scope_builder(
                                        egui::UiBuilder::new().max_rect(row_rect).layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                        ),
                                        |ui| {
                                            ui.set_clip_rect(ui.clip_rect().intersect(row_rect));
                                            if idx == self.layer {
                                                ui.painter().rect_filled(row_rect, 0, palette.timeline_active_layer);
                                            }
                                            ui.spacing_mut().item_spacing.x = crate::timeline_paint::LAYER_COLUMN_GAP;
                                            let (name_rect, _) = ui.allocate_exact_size(Vec2::new(layer_name_width, ROW_HEIGHT), Sense::hover());
                                            if let Some(draft) = rename.as_mut().filter(|draft| draft.layer == layer.id) {
                                                let field = ui.put(name_rect,
                                                    egui::TextEdit::singleline(&mut draft.text)
                                                        .id(ui.id().with(("layer-name", &layer.id)))
                                                        .font(FontId::proportional(crate::theme::PANEL_FONT))
                                                        .hint_text("Layer name"));
                                                if draft.focus {
                                                    field.request_focus();
                                                    if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), field.id) {
                                                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                                                            egui::text::CCursor::new(0),
                                                            egui::text::CCursor::new(draft.text.chars().count()),
                                                        )));
                                                        state.store(ui.ctx(), field.id);
                                                    }
                                                    draft.focus = false;
                                                }
                                                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                                    cancel_rename = true;
                                                    field.surrender_focus();
                                                } else if field.lost_focus()
                                                    || ui.input(|i| i.key_pressed(egui::Key::Enter))
                                                {
                                                    commit_rename = Some((draft.layer.clone(), draft.text.trim().to_owned()));
                                                    field.surrender_focus();
                                                }
                                            } else {
                                                let indent = if layer.properties.parent.is_some() { 8. } else { 0. };
                                                let role_rect = Rect::from_center_size(Pos2::new(name_rect.left() + 7. + indent, name_rect.center().y), Vec2::splat(14.));
                                                crate::layer_controls::paint_layer_role(ui.painter(), role_rect, layer, palette.muted_text);
                                                if layer.properties.role == LayerRole::Folder {
                                                    let role = ui.interact(role_rect, ui.id().with(("folder-fold", &layer.id)), Sense::click());
                                                    role.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), if layer.properties.collapsed { "Expand folder" } else { "Collapse folder" }));
                                                    if role.clicked() { self.toggle_layer_folder(idx); }
                                                }
                                                let label_rect = Rect::from_min_max(name_rect.min + Vec2::new(15. + indent, 0.), name_rect.max);
                                                let response = ui.put(label_rect,
                                                    egui::Button::new((egui::RichText::new(&layer.name).size(crate::theme::PANEL_FONT), egui::Atom::grow()))
                                                        .min_size(label_rect.size()).truncate().selected(idx == self.layer).frame(false).sense(Sense::click_and_drag()));
                                                if !cancelled && resize.is_none() && let Some(start)=pressed.filter(|point| label_rect.contains(*point) && ui.clip_rect().contains(*point) && ui.is_enabled() && ctx.layer_id_at(*point)==Some(ui.layer_id())) {
                                                    self.finish_edits();self.timeline_key_drag=None;self.layer=idx;self.clear_selection();
                                                    reorder=Some(LayerReorder {symbol:self.symbol_id().to_owned(),layer:layer.id.clone(),start,moved:false});
                                                }
                                                if response.clicked() || response.secondary_clicked() {
                                                    self.finish_edits();
                                                    self.timeline_key_drag = None;
                                                    self.layer = idx;
                                                    self.clear_selection();
                                                }
                                                if response.double_clicked() {
                                                    reorder=None;
                                                    rename = Some(LayerRename { layer: layer.id.clone(), text: layer.name.clone(), focus: true });
                                                    ui.ctx().request_repaint();
                                                }
                                                response.on_hover_text("Drag to reorder. Double-click to rename layer").context_menu(|ui| {
                                    crate::ui_icons::menu_contents(ui, 280., |ui| {
                                                    if self.timeline_layer_menu(ui, idx) {
                                                        rename = Some(LayerRename { layer: layer.id.clone(), text: layer.name.clone(), focus: true });
                                                        ui.ctx().request_repaint();
                                                    }

                                    });
                                });
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
                                                    .size(Vec2::new(crate::timeline_paint::LAYER_CONTROL_WIDTH, ROW_HEIGHT))
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
                                                    .size(Vec2::new(crate::timeline_paint::LAYER_CONTROL_WIDTH, ROW_HEIGHT))
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
                                            self.layer_outline_control(ui, layer);
                                        },
                                    );
                                    ui.painter().line_segment(
                                        [row_rect.left_bottom(), row_rect.right_bottom()],
                                        Stroke::new(1.0_f32, palette.timeline_row_separator),
                                    );
                                }
                            },
                        );
                        let frames = egui::ScrollArea::horizontal()
                            .id_salt("frames")
                            .scroll_source(scroll_source)
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                            .auto_shrink([false, false])
                            .themed().show(ui, |ui| {
                                let width = ((if button_states { count.saturating_add(1) } else { count.saturating_add(4).max(72) }) as f32 * cell_width)
                                    .max(ui.available_width());
                                let (rect, response) = ui.allocate_exact_size(
                                    Vec2::new(width, (content_height + 8.).max(height - 18.)),
                                    Sense::click_and_drag(),
                                );
                                grid_rect = rect;
                                grid_clip = ui.clip_rect().intersect(rect);
                                self.timeline_grid_interact(ui, &response, rect, grid_clip, cell_width, count, &layer_indices, resize.is_some() || reorder.is_some());
                                let painter = ui.painter_at(rect);
                                painter.rect_filled(rect, 0, palette.panel_fill);
                                let first = (((grid_clip.left() - rect.left())
                                        / cell_width)
                                        .max(0.)
                                        as u32)
                                        .min(count);
                                let last =
                                    (((grid_clip.right() - rect.left()) / cell_width)
                                        .ceil()
                                        .max(0.) as u32)
                                        .min(count);
                                for f in first..last {
                                    let x = rect.left() + f as f32 * cell_width;
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
                                            Vec2::new(cell_width, rect.height()),
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
                                    if layer_indices[li] == self.layer {
                                        p.rect_filled(row, 0, palette.timeline_active_layer);
                                    }
                                    for (ki, key) in layer.keyframes.iter().enumerate() {
                                        let next = layer
                                            .keyframes
                                            .get(ki + 1)
                                            .map(|k| k.frame)
                                            .unwrap_or(layer.properties.end_frame.unwrap_or(count));
                                        if next < first || key.frame > last {
                                            continue;
                                        }
                                        let x = rect.left() + key.frame as f32 * cell_width;
                                        let end = rect.left() + next.min(count) as f32 * cell_width;
                                        let span = Rect::from_min_max(Pos2::new(x, y + 1.), Pos2::new(end, y + ROW_HEIGHT - 1.));
                                        p.rect_filled(span, 0, if key.elements.is_empty() {
                                            palette.timeline_blank_key
                                        } else {
                                            palette.timeline_major
                                        });
                                        p.line_segment([span.left_top(), span.right_top()], Stroke::new(0.5_f32, palette.timeline_grid));
                                        p.line_segment([span.left_bottom(), span.right_bottom()], Stroke::new(0.5_f32, palette.timeline_grid));
                                        for frame in key.frame.max(first)..next.min(last) {
                                            let cell_x = rect.left() + frame as f32 * cell_width;
                                            p.line_segment([Pos2::new(cell_x, y + 1.), Pos2::new(cell_x, y + ROW_HEIGHT - 1.)], Stroke::new(0.5_f32, palette.timeline_grid));
                                        }
                                        if next > key.frame + 1 && !key.elements.is_empty() {
                                            let last_x = end - cell_width / 2.;
                                            p.rect_stroke(Rect::from_center_size(Pos2::new(last_x, y + ROW_HEIGHT / 2.), Vec2::new(4., 6.)), 0, Stroke::new(1.0_f32, palette.timeline_key), egui::StrokeKind::Inside);
                                        }
                                        if key.tween.is_some() && ki + 1 < layer.keyframes.len() {
                                            let end =
                                                rect.left() + next as f32 * cell_width;
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
                                                x + cell_width / 2.,
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
                                                FontId::proportional(crate::theme::SECONDARY_FONT),
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
                                self.paint_frame_selection(ui, rect, cell_width, &layer_indices);
                                let selected_cell = Rect::from_min_size(
                                    Pos2::new(rect.left() + self.frame as f32 * cell_width, rect.top() + layer_indices.iter().position(|i| *i == self.layer).unwrap_or(0) as f32 * ROW_HEIGHT),
                                    Vec2::new(cell_width, ROW_HEIGHT),
                                );
                                painter.rect_stroke(selected_cell, 0, Stroke::new(1.5_f32, palette.selection_stroke), egui::StrokeKind::Inside);
                                let end_x = rect.left() + count as f32 * cell_width;
                                painter.line_segment([Pos2::new(end_x, rect.top()), Pos2::new(end_x, rect.top() + content_height)], Stroke::new(1.5_f32, palette.border));
                                if response.hovered()
                                    && let Some(pos) = response.hover_pos()
                                {
                                    if let Some((frame, layer)) = timeline_cell_at(pos, rect.min, cell_width, count, layers.len()) {
                                        let key = layers[layer].key_at(frame);
                                        let label = match key {
                                            Some(key) if key.frame == frame && key.elements.is_empty() => "Blank keyframe".to_owned(),
                                            Some(key) if key.frame == frame => "Keyframe · drag to move, Option-drag to copy".to_owned(),
                                            Some(key) if key.elements.is_empty() => format!("Empty frames from {}", key.frame + 1),
                                            Some(key) => format!("Content held from keyframe {}", key.frame + 1),
                                            None => "Empty before the first keyframe".to_owned(),
                                        };
                                        if key.is_some_and(|key| key.frame == frame) && !layers[layer].locked && layers[layer].visible {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                        }
                                        response.clone().on_hover_text(format!("{} · Frame {}\n{}\nShift-click/drag selects a frame range. Drag a selected range to move; Option-drag copies.\nRight-click for frame commands and Actions", layers[layer].name, frame + 1, label));
                                    } else if pos.x >= end_x {
                                        response.clone().on_hover_text(format!("Timeline ends at frame {}. Insert frames (F5) to extend it.", count));
                                    }
                                }
                                if response.dragged_by(egui::PointerButton::Primary)
                                    && let Some((symbol, layer_id, from)) = &self.timeline_key_drag
                                    && symbol == self.symbol_id()
                                    && let Some(layer) = layers.iter().find(|layer| &layer.id == layer_id)
                                {
                                    let blocked = self.frame != *from && layer.keyframes.iter().any(|key| key.frame == self.frame);
                                    let stroke = Stroke::new(2_f32, if blocked { palette.error_text } else { palette.accent });
                                    painter.rect_stroke(selected_cell.shrink(1.), 0, stroke, egui::StrokeKind::Inside);
                                    ui.ctx().set_cursor_icon(if blocked { egui::CursorIcon::NotAllowed } else { egui::CursorIcon::Grabbing });
                                    let text = if blocked { "Destination already has a keyframe".to_owned() } else { format!("{} key {} → {}", if ui.input(|i| i.modifiers.alt) { "Copy" } else { "Move" }, from + 1, self.frame + 1) };
                                    painter.text(Pos2::new(grid_clip.left() + 5., grid_clip.bottom() - 3.), Align2::LEFT_BOTTOM, text, FontId::proportional(11.), stroke.color);
                                }
                                let playx = rect.left()
                                    + self.frame as f32 * cell_width
                                    + cell_width / 2.;
                                painter.line_segment(
                                    [
                                        Pos2::new(playx, rect.top()),
                                        Pos2::new(playx, rect.bottom()),
                                    ],
                                    Stroke::new(1.5_f32, palette.playhead),
                                );
                                if response.context_menu_opened()
                                    || response.interact_pointer_pos().is_some_and(|pos| timeline_cell_at(pos, rect.min, cell_width, count, layers.len()).is_some())
                                {
                                response.context_menu(|ui| {
                                    crate::ui_icons::menu_contents(ui, 280., |ui| {
                                    ui.label(format!("{} · Frame {}", self.symbol().timeline.layers[self.layer].name, self.frame + 1));
                                    ui.separator();
                                    self.frame_range_menu(ui);
                                    if ui.add_enabled_semantic(self.layer_editable(), egui::Button::new("Insert Keyframe").shortcut_text("F6")).clicked() {
                                        self.finish_edits();
                                        self.make_key(false);
                                        ui.close();
                                    }
                                    if ui.add_enabled_semantic(self.layer_editable(), egui::Button::new("Insert Blank Keyframe").shortcut_text("F7")).clicked() {
                                        self.finish_edits();
                                        self.make_key(true);
                                        ui.close();
                                    }
                                    if ui
                                        .add_enabled_semantic(
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
                                    if ui.add(egui::Button::new("Actions").shortcut_text("F9")).clicked() {
                                        self.open_frame_actions();
                                        ui.close();
                                    }

                                    });
                                });
                                }
                            });
                        if zoom_changed {
                            let mut state = frames.state;
                            let maximum = (frames.content_size.x - frames.inner_rect.width()).max(0.);
                            state.offset.x = ((self.frame as f32 + 0.5) * cell_width
                                - frames.inner_rect.width() * 0.5).clamp(0., maximum);
                            state.store(ui.ctx(), frames.id);
                            ui.ctx().request_repaint();
                        }
                        // Continue an existing key drag at the horizontal viewport edge;
                        // never start a drag from a scrollbar or an out-of-range cell.
                        if self.timeline_edit.dragging()
                            && ui.input(|i| i.pointer.primary_down() && i.pointer.is_decidedly_dragging())
                            && let Some(pos) = ui.input(|i| i.pointer.latest_pos())
                            && pos.y >= frames.inner_rect.top()
                            && pos.y < frames.inner_rect.bottom()
                        {
                            let margin = 24.;
                            let direction = if pos.x < frames.inner_rect.left() + margin { -1. } else if pos.x > frames.inner_rect.right() - margin { 1. } else { 0. };
                            if direction != 0. {
                                let mut state = frames.state;
                                let dt = ui.input(|i| i.stable_dt).min(0.05);
                                let maximum = (frames.content_size.x - frames.inner_rect.width()).max(0.);
                                let next = (state.offset.x + direction * 360. * dt).clamp(0., maximum);
                                if next != state.offset.x {
                                    state.offset.x = next;
                                    state.store(ui.ctx(), frames.id);
                                    ui.ctx().request_repaint();
                                }
                            }
                        }
                    });
                });
            let divider_x = header.left() + layer_column_width + ui.spacing().item_spacing.x / 2.;
            ui.painter().line_segment([Pos2::new(divider_x, header.top()), Pos2::new(divider_x, rows.inner_rect.bottom())], Stroke::new(if resize.is_some() {2.0_f32} else {1.0_f32}, if resize.is_some() {palette.accent} else {palette.border}));
            if let Some(drag)=&mut reorder {
                if drag.symbol!=self.symbol_id() || !layers.iter().any(|layer| layer.id==drag.layer) {reorder=None;}
                else if let Some(position)=pointer {
                    drag.moved |= position.distance(drag.start)>=6.;
                    if drag.moved {
                        let names_view=Rect::from_min_max(Pos2::new(header.left(),rows.inner_rect.top()),Pos2::new(header.left()+layer_column_width,rows.inner_rect.bottom()));
                        let within=names_view.contains(position);
                        ctx.set_cursor_icon(if within {egui::CursorIcon::Grabbing} else {egui::CursorIcon::NotAllowed});
                        if within {
                            let boundary=(((position.y-grid_rect.top())/ROW_HEIGHT).round().max(0.) as usize).min(layers.len());
                            let y=(grid_rect.top()+boundary as f32*ROW_HEIGHT).clamp(names_view.top(),names_view.bottom());
                            ui.painter().line_segment([Pos2::new(names_view.left(),y),Pos2::new(rows.inner_rect.right(),y)],Stroke::new(2.0_f32,palette.accent));
                            if released.is_some() {commit_reorder=Some((drag.layer.clone(),layers.get(boundary).map(|layer|layer.id.clone())));}
                            if down {
                                let direction=if position.y<names_view.top()+18. {-1.} else if position.y>names_view.bottom()-18. {1.} else {0.};
                                if direction!=0. {
                                    let mut state=rows.state;
                                    let maximum=(rows.content_size.y-rows.inner_rect.height()).max(0.);
                                    state.offset.y=(state.offset.y+direction*220.*ui.input(|input|input.stable_dt).min(0.05)).clamp(0.,maximum);
                                    state.store(ui.ctx(),rows.id);
                                    ctx.request_repaint();
                                }
                            }
                        }
                    }
                }
            }
            if divider.hovered() || resize.is_some() {ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);}
            if released.is_some() || !down {reorder=None;resize=None;}
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
                for (column, icon, label) in [(0, Icon::Lock, "Layer lock"), (1, Icon::Visible, "Layer visibility")] {
                    let center = Pos2::new(header.left() + layer_name_width
                        + crate::timeline_paint::LAYER_COLUMN_GAP
                        + crate::timeline_paint::LAYER_CONTROL_WIDTH / 2.
                        + column as f32 * (crate::timeline_paint::LAYER_CONTROL_WIDTH + crate::timeline_paint::LAYER_COLUMN_GAP), header.center().y);
                    let rect = Rect::from_center_size(center, Vec2::splat(crate::theme::ICON_SIZE));
                    crate::ui_icons::paint_icon(ui.painter(), rect, icon, Stroke::new(crate::theme::ICON_STROKE, palette.muted_text));
                    ui.interact(rect, ui.id().with(("layer-column", column)), Sense::hover()).on_hover_text(label);
                }
                let outline_header = Rect::from_center_size(Pos2::new(header.left() + layer_column_width - crate::timeline_paint::LAYER_CONTROL_WIDTH / 2., header.center().y), Vec2::splat(crate::theme::ICON_SIZE));
                ui.painter().rect_stroke(outline_header, 0, Stroke::new(crate::theme::ICON_STROKE, palette.muted_text), egui::StrokeKind::Inside);
                ui.interact(outline_header, ui.id().with("layer-outline-column"), Sense::hover()).on_hover_text("Layer outline and color");
                let first = (((ruler.left() - grid_rect.left()) / cell_width).max(0.)
                    as u32)
                    .min(count);
                let last = (((ruler.right() - grid_rect.left()) / cell_width)
                    .ceil()
                    .max(0.) as u32)
                    .min(count);
                let label_step = crate::timeline_paint::ruler_label_step(cell_width, count);
                for f in first..last {
                    let x = grid_rect.left() + f as f32 * cell_width;
                    if f == 0 || (f + 1) % label_step == 0 {
                        p.text(
                            Pos2::new(x + cell_width / 2., ruler.bottom() - 6.),
                            Align2::CENTER_CENTER,
                            if button_states { crate::authoring_symbols::BUTTON_STATES.get(f as usize).unwrap_or(&"—").to_string() } else { (f + 1).to_string() },
                            FontId::proportional(crate::theme::SECONDARY_FONT),
                            palette.muted_text,
                        );
                    }
                }
                let fps = self.project.fps.max(1);
                let seconds_step = ((46. / (cell_width * fps as f32)).ceil() as u32).max(1);
                for f in first..last {
                    if !button_states && f.is_multiple_of(fps) && (f / fps).is_multiple_of(seconds_step) {
                        let x = grid_rect.left() + f as f32 * cell_width;
                        p.text(Pos2::new(x + 3., ruler.top() + 5.), Align2::LEFT_CENTER, format!("{}s", f / fps), FontId::proportional(crate::theme::SECONDARY_FONT), palette.muted_text);
                    }
                }
                // Frame scripts belong to the whole timeline, so their markers
                // live in the ruler rather than in the first authored layer.
                for script in &self.symbol().timeline.scripts {
                    let x = grid_rect.left() + (script.frame as f32 + 0.5) * cell_width;
                    p.text(Pos2::new(x, ruler.top() + 1.), Align2::CENTER_TOP, "a", FontId::monospace(crate::theme::SECONDARY_FONT), palette.timeline_script);
                }
                let end_x = grid_rect.left() + count as f32 * cell_width;
                if end_x >= ruler.left() && end_x <= ruler.right() {
                    p.line_segment([Pos2::new(end_x, ruler.top()), Pos2::new(end_x, ruler.bottom())], Stroke::new(1.5_f32, palette.border));
                    p.text(Pos2::new(end_x + 5., ruler.center().y), Align2::LEFT_CENTER, "End", FontId::proportional(crate::theme::SECONDARY_FONT), palette.muted_text);
                }
                let response =
                    ui.interact(ruler, ui.id().with("ruler_seek"), Sense::click_and_drag());
                if (response.clicked() || response.dragged())
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    self.finish_edits();
                    self.timeline_key_drag = None;
                    self.set_playing(false);
                    if pos.x < grid_rect.left() + count as f32 * cell_width {
                        self.reset_authored_preview();
                        self.frame = frame_at_x(pos.x, grid_rect.left(), cell_width, count);
                        self.select_frame_range(FrameRange { layers: vec![self.symbol().timeline.layers[self.layer].id.clone()], start: self.frame, end: self.frame });
                        self.script_target = ScriptTarget::Frame;
                        self.clear_selection();
                    }
                }
                let playx = grid_rect.left()
                    + self.frame as f32 * cell_width
                    + cell_width / 2.;
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
                    let left = grid_rect.left() + start as f32 * cell_width;
                    let right = grid_rect.left() + (end + 1) as f32 * cell_width;
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
                            let f = (((pos.x - grid_rect.left()) / cell_width)
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
                    egui::ScrollArea::horizontal()
                        .id_salt("timeline_toolbar")
                        .max_height(footer)
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .auto_shrink([false, false])
                        .themed().show(ui, |ui| {
                            ui.set_max_width(ui.available_width().min(timeline_width));
                            ui.horizontal(|ui| {
                                if ui
                                    .add(IconButton::new(Icon::Plus, "Add layer"))
                                    .clicked()
                                {
                                    self.finish_edits();
                                    self.transact(|s| {
                                        let n = s.symbol().timeline.layers.len() + 1;
                                        s.symbol_mut().timeline.layers.push(Layer {
                                            properties: Default::default(),
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
                                    .add_enabled_semantic(
                                        self.symbol().timeline.layers.len() > 1,
                                        IconButton::new(Icon::Trash, "Delete selected layer"),
                                    )
                                    .on_hover_text("Delete selected layer")
                                    .clicked()
                                {
                                    self.delete_timeline_layer(self.layer);
                                }
                                ui.separator();
                                if ui.add(IconButton::new(Icon::Rewind, "Rewind")).clicked() {
                                    self.finish_edits();
                                    self.timeline_key_drag = None;
                                    self.reset_authored_preview();
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
                                if ui.button("Actions").on_hover_text("Edit the selected frame script").clicked() {
                                    self.open_frame_actions();
                                }
                                ui.separator();
                                ui.checkbox(&mut self.onion, if timeline_width < 960. { "Onion" } else { "Onion Skin" });
                                if timeline_width < 960. {
                                    let options = ui.menu_button("…", |ui| {
                    crate::ui_icons::menu_contents(ui, 240., |ui| {


                                        ui.checkbox(&mut self.onion_outlines, "Outlines");
                                        crate::paint_controls::property_row(ui, "Before", |ui| { ui.add(egui::DragValue::new(&mut self.onion_before).range(0..=120).suffix(" fr")); });
                                        crate::paint_controls::property_row(ui, "After", |ui| { ui.add(egui::DragValue::new(&mut self.onion_after).range(0..=120).suffix(" fr")); });
                                        if ui.checkbox(&mut self.onion_anchor, "Anchor").changed() {
                                            self.anchored_range = (self.frame.saturating_sub(self.onion_before), (self.frame+self.onion_after).min(self.symbol().timeline.frame_count-1));
                                        }
                                        crate::paint_controls::property_row(ui, "Opacity", |ui| {
                                            crate::theme::timeline_controls(ui, |ui| {
                                                ui.spacing_mut().slider_width=96.;
                                                let response = ui.add(egui::Slider::new(&mut self.onion_opacity,0.05..=0.7).show_value(true));
                                                response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), self.onion_opacity as f64, "Onion opacity"));
                                            });
                                        });

                    });
                }).response.on_hover_text("Onion skin: outlines, before/after range, anchor and opacity");
                                    options.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Onion skin options"));
                                } else {
ui.checkbox(&mut self.onion_outlines, "Outlines");
                                ui.allocate_ui_with_layout(
                                    Vec2::new(76., 24.),
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
                                    Vec2::new(68., 24.),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.label("After");
                                        ui.add(
                                            egui::DragValue::new(&mut self.onion_after)
                                                .range(0..=120),
                                        );
                                    },
                                );
                                if crate::theme::timeline_controls(ui, |ui| {
                                    ui.checkbox(&mut self.onion_anchor, "Anchor")
                                }).inner.changed() {
                                    self.anchored_range = (
                                        self.frame.saturating_sub(self.onion_before),
                                        (self.frame + self.onion_after)
                                            .min(self.symbol().timeline.frame_count - 1),
                                    );
                                }
                                // Slider's own label is a nested horizontal layout. Reserve
                                // the complete group so it wraps as one control, not midway.
                                ui.allocate_ui_with_layout(
                                    Vec2::new(140., 24.),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        crate::theme::timeline_controls(ui, |ui| {
                                            ui.spacing_mut().slider_width = 76.;
                                            ui.add(
                                                egui::Slider::new(&mut self.onion_opacity, 0.05..=0.7)
                                                    .text("Opacity")
                                                    .show_value(false),
                                            );
                                        });
                                    },
                                );
                                }
                            });
                        });
                },
            );
        });
        if let Some((layer, before)) = commit_reorder {
            self.reorder_timeline_layer_visual(&layer, before.as_deref());
        }
        ctx.data_mut(|data| {
            if let Some(drag) = reorder {
                data.insert_temp(reorder_id, drag);
            } else {
                data.remove::<LayerReorder>(reorder_id);
            }
            if let Some(drag) = resize {
                data.insert_temp(resize_id, drag);
            } else {
                data.remove::<ColumnResize>(resize_id);
            }
        });
        if cancel_rename {
            rename = None;
        } else if let Some((layer_id, name)) = commit_rename {
            if !name.is_empty()
                && let Some(index) = self
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .position(|layer| layer.id == layer_id)
                && self.symbol().timeline.layers[index].name != name
            {
                self.finish_edits();
                self.transact(|s| s.symbol_mut().timeline.layers[index].name = name);
            }
            rename = None;
        }
        ctx.data_mut(|data| {
            if let Some(rename) = rename.filter(|draft| {
                self.symbol()
                    .timeline
                    .layers
                    .iter()
                    .any(|layer| layer.id == draft.layer)
            }) {
                data.insert_temp(rename_id, rename);
            } else {
                data.remove::<LayerRename>(rename_id);
            }
        });
        self.layer_outline_color_dialog(&ctx);
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
    #[cfg(test)]
    pub fn tools(&mut self, ctx: &egui::Context) {
        self.dock_edge(ctx, crate::workspace::DockEdge::Left);
    }
    pub(crate) fn tools_contents(&mut self, ui: &mut egui::Ui) {
        let id = (
            "neoflash-panel-content",
            PanelId::Tools,
            self.clipboard.session.clone(),
        );
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(4.);
            ui.spacing_mut().scroll.floating = false;
            // Keep paint defaults reachable while only the tool grid scrolls.
            let grid_height = (ui.available_height() - 84.).max(24.);
            egui::ScrollArea::vertical()
                .id_salt("tool_controls")
                .max_height(grid_height)
                .min_scrolled_height(grid_height)
                .auto_shrink([false, false])
                .themed()
                .show(ui, |ui| {
                    type ToolEntry = (Tool, Icon, &'static str);
                    type ToolGroup = (&'static str, &'static [ToolEntry]);
                    let groups: &[ToolGroup] = &[
                        (
                            "Select",
                            &[
                                (Tool::Select, Icon::Select, "Selection (V)"),
                                (Tool::Subselect, Icon::Subselect, "Subselection (A)"),
                                (Tool::Transform, Icon::Transform, "Free Transform (Q)"),
                                (Tool::Lasso, Icon::Lasso, "Lasso (L)"),
                            ],
                        ),
                        (
                            "Draw",
                            &[
                                (Tool::Pen, Icon::Pen, "Pen (P)"),
                                (Tool::Text, Icon::Text, "Text (T)"),
                                (Tool::Line, Icon::Line, "Line (N)"),
                                (Tool::Rectangle, Icon::Rectangle, "Rectangle (R)"),
                                (Tool::Ellipse, Icon::Ellipse, "Oval (O)"),
                                (Tool::Pencil, Icon::Pencil, "Pencil (Y)"),
                                (Tool::Brush, Icon::Brush, "Brush (B)"),
                                (Tool::Ink, Icon::Ink, "Ink Bottle (S)"),
                                (Tool::Bucket, Icon::Bucket, "Paint Bucket (K)"),
                                (Tool::Eyedropper, Icon::Eyedropper, "Eyedropper (I)"),
                                (Tool::Eraser, Icon::Eraser, "Eraser (E)"),
                                (Tool::Gradient, Icon::Gradient, "Gradient Transform (F)"),
                            ],
                        ),
                        (
                            "View",
                            &[
                                (Tool::Hand, Icon::Hand, "Hand (H / hold Space)"),
                                (Tool::Zoom, Icon::ZoomIn, "Zoom (Z / M)"),
                            ],
                        ),
                    ];
                    for (label, tools) in groups {
                        ui.label(
                            egui::RichText::new(*label)
                                .size(crate::theme::SECONDARY_FONT)
                                .color(UiPalette::current(ui.ctx()).muted_text),
                        );
                        for row in tools.chunks(2) {
                            ui.horizontal(|ui| {
                                for &(tool, icon, hint) in row {
                                    if ui
                                        .add(
                                            IconButton::new(icon, hint)
                                                .selected(self.tool == tool)
                                                .size(Vec2::splat(crate::theme::CONTROL_HEIGHT)),
                                        )
                                        .clicked()
                                    {
                                        self.set_tool(tool);
                                    }
                                }
                            });
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                IconButton::new(Icon::ZoomOut, "Zoom out")
                                    .size(Vec2::splat(crate::theme::CONTROL_HEIGHT)),
                            )
                            .clicked()
                        {
                            self.finish_edits();
                            self.zoom = (self.zoom / 1.25).max(0.1);
                        }
                        if ui
                            .add(
                                IconButton::new(Icon::Fit, "Fit stage")
                                    .size(Vec2::splat(crate::theme::CONTROL_HEIGHT)),
                            )
                            .clicked()
                        {
                            self.finish_edits();
                            self.zoom = 0.;
                            self.pan = Vec2::ZERO;
                        }
                    });
                });
            ui.separator();
            ui.horizontal(|ui| {
                for (name, color) in [("Fill", &mut self.fill), ("Stroke", &mut self.stroke)] {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(name).size(crate::theme::SECONDARY_FONT));
                        let mut rgb = [color[0], color[1], color[2]];
                        let response = ui.color_edit_button_srgb(&mut rgb).on_hover_text(format!(
                            "{name} color; alpha and width in Tool properties"
                        ));
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::ColorButton,
                                ui.is_enabled(),
                                format!("Tool {name} color"),
                            )
                        });
                        if response.changed() {
                            color[..3].copy_from_slice(&rgb);
                        }
                    });
                }
            });
            if ui
                .add_sized(
                    [ui.available_width(), crate::theme::CONTROL_HEIGHT],
                    egui::Button::new("Options"),
                )
                .on_hover_text("Tool properties: paint alpha, stroke width and tool options")
                .clicked()
            {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(egui::Id::new("properties-tool-tab"), true));
                self.show_panel(PanelId::Properties);
            }
        });
    }
    #[cfg(test)]
    pub fn library(&mut self, ctx: &egui::Context) {
        self.dock_edge(ctx, crate::workspace::DockEdge::Right);
    }
    pub(crate) fn library_contents(&mut self, ui: &mut egui::Ui) {
        self.library_body(ui);
    }
    #[cfg(test)]
    pub fn bottom_panel(&mut self, ctx: &egui::Context) {
        self.dock_edge(ctx, crate::workspace::DockEdge::Bottom);
    }
    pub(crate) fn bottom_contents(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        let ctx = ui.ctx().clone();
        match panel {
            PanelId::Properties => {
                egui::ScrollArea::vertical()
                    .id_salt("property_contents")
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, |ui| self.properties(ui, &ctx));
            }
            PanelId::Actions => self.actions_contents(ui),
            PanelId::Accessibility => {
                egui::ScrollArea::vertical()
                    .id_salt("accessibility_contents")
                    .auto_shrink([false, false])
                    .themed()
                    .show(ui, |ui| self.accessibility_panel(ui));
            }
            PanelId::Output => {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .id_salt("output_contents")
                    .stick_to_bottom(true)
                    .themed()
                    .show(ui, |ui| {
                        for line in &self.output {
                            ui.add(egui::Label::new(
                                egui::RichText::new(line)
                                    .monospace()
                                    .size(crate::theme::PANEL_FONT),
                            ));
                        }
                    });
            }
            _ => {}
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
        let has_frame_script =
            self.symbol().timeline.scripts.iter().any(|script| {
                script.frame == self.frame && !script.script.source.trim().is_empty()
            });
        let object_id = self.selected_element().map(|element| element.id.clone());
        let target_before = self.script_target;
        let event_before = self.script_event;
        let original = match self.script_target {
            ScriptTarget::Source => None,
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
        let context = match self.script_target {
            ScriptTarget::Frame => format!(
                "{} · Frame {}{}",
                self.symbol().name,
                self.frame + 1,
                if has_frame_script { " · script" } else { "" }
            ),
            ScriptTarget::Object => format!(
                "{} · {} · {:?}",
                self.symbol().name,
                self.selected_element()
                    .map_or("Object", |e| e.name.as_str()),
                self.script_event
            ),
            ScriptTarget::Source => "Project files".to_owned(),
        };
        ui.horizontal(|ui| {
            let language_width = 82.;
            let command_width = crate::theme::CONTROL_HEIGHT;
            let gap = ui.spacing().item_spacing.x;
            let target_width = if self.script_target == ScriptTarget::Source {
                ui.available_width()
            } else {
                (ui.available_width() - language_width - command_width - 2. * gap).max(40.)
            };
            // ComboBox::width is a minimum, not a maximum: constrain each cell
            // before laying out long target or language names.
            ui.allocate_ui_with_layout(
                Vec2::new(target_width, crate::theme::CONTROL_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(target_width);
                    egui::ComboBox::from_id_salt("actions_target")
                        .width(target_width)
                        .truncate()
                        .selected_text(&context)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.script_target,
                                ScriptTarget::Frame,
                                format!("Frame {}", self.frame + 1),
                            );
                            ui.add_enabled_ui_semantic(self.selected_editable(), |ui| {
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
                        })
                        .response
                        .on_hover_text(&context);
                },
            );
            if self.script_target != ScriptTarget::Source {
                let label = crate::sources::source_language_name(script.language);
                ui.allocate_ui_with_layout(
                    Vec2::new(language_width, crate::theme::CONTROL_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_width(language_width);
                        egui::ComboBox::from_id_salt("language")
                            .width(language_width)
                            .truncate()
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                for language in [
                                    ScriptLanguage::JavaScript,
                                    ScriptLanguage::TypeScript,
                                    ScriptLanguage::ActionScript1,
                                    ScriptLanguage::ActionScript2,
                                    ScriptLanguage::ActionScript3,
                                ] {
                                    ui.selectable_value(
                                        &mut script.language,
                                        language,
                                        crate::sources::source_language_name(language),
                                    );
                                }
                            });
                    },
                );
                ui.allocate_ui_with_layout(
                    Vec2::splat(command_width),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_width(command_width);
                        // Icon-sized command trigger; popup content restores the
                        // standard menu padding through menu_contents.
                        ui.spacing_mut().button_padding = Vec2::new(4., 0.);
                        let commands = ui
                            .menu_button("…", |ui| {
                                crate::ui_icons::menu_contents(ui, 240., |ui| {
                                    if crate::ui_icons::menu_item(ui, "Check script", "").clicked()
                                    {
                                        self.check_action_source(&script);
                                        ui.close();
                                    }
                                    if self.script_target == ScriptTarget::Object {
                                        crate::ui_icons::menu_popup(
                                            ui,
                                            "Object event",
                                            "EnterFrame",
                                            "",
                                            |ui| {
                                                for event in [
                                                    ObjectEvent::Click,
                                                    ObjectEvent::Load,
                                                    ObjectEvent::EnterFrame,
                                                ] {
                                                    if ui
                                                        .selectable_value(
                                                            &mut self.script_event,
                                                            event,
                                                            format!("{event:?}"),
                                                        )
                                                        .clicked()
                                                    {
                                                        ui.close();
                                                    }
                                                }
                                            },
                                        );
                                    }
                                    ui.separator();
                                    ui.label("Test Movie runs scripts in the browser.");
                                });
                            })
                            .response
                            .on_hover_text("Actions commands");
                        commands.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                "Actions commands",
                            )
                        });
                    },
                );
            }
        });
        if target_before != self.script_target || event_before != self.script_event {
            ui.ctx().request_repaint();
            return;
        }
        if self.script_target == ScriptTarget::Source {
            self.project_sources(ui);
            return;
        }
        if self.script_target == ScriptTarget::Object && !self.selected_editable() {
            ui.label("Select one editable object to edit its actions, or choose Frame.");
            return;
        }
        if self.symbol().kind == SymbolKind::Button
            && self.script_target == ScriptTarget::Frame
            && !has_frame_script
        {
            ui.label("Button states contain artwork. Attach actions to the named Button instance on its parent timeline, or use its linked class in Project files.");
            return;
        }
        self.show_code_feedback(ui, &script);
        let editable = self.script_target == ScriptTarget::Frame || self.selected_editable();
        let editor_id = self.actions_editor_id();
        let mut text_editing = false;
        let mut code_layout = crate::sources::code_layout;
        ui.add_enabled_ui_semantic(editable, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("actions_source_scroll", editor_id))
                .themed()
                .show(ui, |ui| {
                    let response = ui.add_sized(
                        [ui.available_width(), ui.available_height().max(60.)],
                        egui::TextEdit::multiline(&mut script.source)
                            .id(editor_id)
                            .font(egui::FontId::monospace(12.))
                            .layouter(&mut code_layout)
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
