//! Timeline panel (NF2 Timeline 2.0, reference §5). The controls bar lives in `timeline_controls.rs`;
//! this file lays out the body: `TL Layers Header` + one `TL Layer` row per layer (280 pt column, user
//! resizable), the `TL Ruler` (32), and the frame area painted with the `TL Grid`, `TL Span`,
//! `TL Grid Lines`, `TL Marker` and `TL Frame Label` painters, plus `TL Playhead` and `TL Onion Range`.
//! Frame `n` (1-based) starts at `(n − 1) × frame width`; the editor keeps 0-based frames.
use crate::app::*;
use crate::theme::ScrollTheme as _;
use crate::{
    layer_controls::{layer_color, layer_depth, layer_kind, visible_layer_indices},
    theme::UiPalette,
    timeline_paint::{RULER_HEIGHT, frame_at_x, timeline_cell_at},
    workspace::PanelId,
};
use eframe::egui::{self, Align2, Pos2, Rect, Sense, Stroke, Vec2};
use neoflash_core::*;
use neoflash_ui::components as nf;
use neoflash_ui::components::timeline::{
    self as tl, FrameLabelKind, Geometry, GridKind, LayerRow, MarkerKind, SpanType, ToggleKind,
};
use neoflash_ui::metrics::LAYER_TOGGLE;
use neoflash_ui::tokens::space;
use neoflash_ui::{TextStyle, typography};

impl Editor {
    /// Test helper: paint the dock edge that currently holds the Timeline.
    #[cfg(test)]
    pub fn timeline(&mut self, ctx: &egui::Context) {
        let edge = self
            .workspace
            .dock
            .location(PanelId::Timeline)
            .map_or(crate::workspace::DockEdge::Bottom, |location| location.edge);
        self.dock_edge(ctx, edge);
    }
    pub(crate) fn timeline_contents(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let palette = UiPalette::current(&ctx);
        let p = neoflash_ui::palette(&ctx);
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
        let row_height = self.workspace.timeline.row_height();
        let timeline_width = ui.available_width();
        let maximum_layer_width = (timeline_width - 100.).clamp(140., 320.);
        let mut layer_column_width = self
            .workspace
            .sizes
            .layer_column_width
            .clamp(140., maximum_layer_width);
        ui.scope_builder(egui::UiBuilder::new().id(id), |ui| {
            // Keep a usable frame canvas even in the smallest native Timeline
            // window. Overflowing bar controls scroll; the onion group stays pinned.
            ui.spacing_mut().scroll.floating = false;
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let controls = self.timeline_controls_bar(
                ui,
                layer_column_width,
                timeline_width - layer_column_width - space::S4,
            );
            let zoom_changed = self.timeline_cell != previous_cell;
            if zoom_changed {
                self.timeline_key_drag = None;
            }
            let recenter = zoom_changed || controls.recenter;
            let view = self.workspace.timeline;
            let button_states = self.symbol().kind == SymbolKind::Button;
            let cell_width = if button_states { 64. } else { self.timeline_cell };
            let height = (ui.available_height() - RULER_HEIGHT).max(25.);
            let (header, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), RULER_HEIGHT),
                Sense::hover(),
            );
            let divider_x = header.left() + layer_column_width;
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
            // NF2/TL Layers Header: "LAYERS" + eye / lock / outline columns that toggle every row.
            let header_toggles = ui
                .scope_builder(
                    egui::UiBuilder::new().max_rect(Rect::from_min_size(header.min, Vec2::new(layer_column_width, RULER_HEIGHT))),
                    |ui| tl::layers_header(ui, layer_column_width),
                )
                .inner;
            for (clicked, column) in [
                (header_toggles.all_visibility, ToggleKind::Visibility),
                (header_toggles.all_lock, ToggleKind::Lock),
                (header_toggles.all_outline, ToggleKind::Outline),
            ] {
                if clicked {
                    self.toggle_all_layers(column);
                }
            }
            // Frame exposures are ordered by their frame, independent of JSON order.
            let layer_indices = visible_layer_indices(&self.symbol().timeline);
            if !layer_indices.contains(&self.layer) {
                self.layer = layer_indices.iter().copied().rfind(|index| *index < self.layer).unwrap_or(0);
                self.clear_selection();
            }
            let depths: Vec<usize> = {
                let timeline = &self.symbol().timeline;
                layer_indices.iter().map(|index| layer_depth(timeline, &timeline.layers[*index])).collect()
            };
            let mut layers: Vec<_> = layer_indices.iter().map(|index| self.symbol().timeline.layers[*index].clone()).collect();
            for layer in &mut layers {
                layer.keyframes.sort_by_key(|key| key.frame);
            }
            let count = self.symbol().timeline.frame_count;
            let script_frames: Vec<u32> = self.symbol().timeline.scripts.iter().map(|script| script.frame).collect();
            // Frame scripts belong to the whole timeline. Like Flash's actions layer, they show as
            // Action keyframes on the top non-folder row where it has a keyframe; otherwise on the ruler.
            let action_row = layers.iter().position(|layer| layer.properties.role != LayerRole::Folder);
            let content_height = layers.len() as f32 * row_height;
            // Solid scrollbars own a separate strip instead of covering the last
            // visible key. Content drags belong to frames, not kinetic scrolling.
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
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(layer_column_width, content_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(layer_column_width);
                                ui.spacing_mut().item_spacing.y = 0.;
                                for (row_index, layer) in layers.iter().enumerate() {
                                    let idx = layer_indices[row_index];
                                    let (row_rect, row_response) = ui.allocate_exact_size(
                                        Vec2::new(layer_column_width, row_height),
                                        Sense::click_and_drag(),
                                    );
                                    let renaming = rename.as_ref().is_some_and(|draft| draft.layer == layer.id);
                                    let row = LayerRow {
                                        label: if renaming { "" } else { &layer.name },
                                        kind: layer_kind(&self.symbol().timeline, layer),
                                        selected: idx == self.layer,
                                        indent: depths[row_index],
                                        editing: idx == self.layer,
                                        visible: layer.visible,
                                        locked: layer.locked,
                                        outline: layer.properties.outline,
                                        color: layer_color(layer),
                                    }
                                    .paint_at(ui, row_rect, row_response);
                                    if row.disclosure {
                                        self.toggle_layer_folder(idx);
                                    }
                                    if row.visibility {
                                        self.finish_edits();
                                        self.transact(|s| s.symbol_mut().timeline.layers[idx].visible = !layer.visible);
                                    }
                                    if row.lock {
                                        self.finish_edits();
                                        self.transact(|s| s.symbol_mut().timeline.layers[idx].locked = !layer.locked);
                                    }
                                    if row.outline {
                                        self.toggle_layer_outline(layer);
                                    }
                                    // The outline swatch keeps its right-click colour menu.
                                    let swatch = Rect::from_min_size(
                                        Pos2::new(row_rect.right() - space::S2 - LAYER_TOGGLE, row_rect.top()),
                                        Vec2::new(LAYER_TOGGLE, row_rect.height()),
                                    );
                                    let swatch = ui.interact(swatch, row.row.id.with(ToggleKind::Outline as u8), Sense::click());
                                    self.layer_outline_menu(&swatch, layer);
                                    if let Some(draft) = rename.as_mut().filter(|draft| draft.layer == layer.id) {
                                        let field = ui.put(row.label_rect,
                                            egui::TextEdit::singleline(&mut draft.text)
                                                .id(ui.id().with(("layer-name", &layer.id)))
                                                .font(typography::font(TextStyle::Body))
                                                .vertical_align(egui::Align::Center)
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
                                        continue;
                                    }
                                    // Name area (left of the column toggles) starts a reorder drag.
                                    let grab = Rect::from_min_max(row_rect.min, Pos2::new(row_rect.right() - space::S2 - 3. * LAYER_TOGGLE, row_rect.bottom()));
                                    if !cancelled && resize.is_none() && let Some(start)=pressed.filter(|point| grab.contains(*point) && ui.clip_rect().contains(*point) && ui.is_enabled() && ctx.layer_id_at(*point)==Some(ui.layer_id())) {
                                        self.finish_edits();self.timeline_key_drag=None;self.layer=idx;self.clear_selection();
                                        reorder=Some(LayerReorder {symbol:self.symbol_id().to_owned(),layer:layer.id.clone(),start,moved:false});
                                    }
                                    let response = row.row;
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
                                        if self.timeline_layer_menu(ui, idx) {
                                            rename = Some(LayerRename { layer: layer.id.clone(), text: layer.name.clone(), focus: true });
                                            ui.ctx().request_repaint();
                                        }
                                    });
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
                                self.paint_timeline_rows(&painter, rect, grid_clip, cell_width, &layers, action_row, &script_frames, view.tinted);
                                self.paint_frame_selection(ui, rect, cell_width, &layer_indices);
                                let end_x = rect.left() + count as f32 * cell_width;
                                let selected_cell = Rect::from_min_size(
                                    Pos2::new(rect.left() + self.frame as f32 * cell_width, rect.top() + layer_indices.iter().position(|i| *i == self.layer).unwrap_or(0) as f32 * row_height),
                                    Vec2::new(cell_width, row_height),
                                );
                                if response.hovered()
                                    && let Some(pos) = response.hover_pos()
                                {
                                    if let Some((frame, layer)) = timeline_cell_at(pos, rect.min, cell_width, count, layers.len(), row_height) {
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
                                    painter.text(Pos2::new(grid_clip.left() + space::S1, grid_clip.bottom() - space::S1), Align2::LEFT_BOTTOM, text, typography::font(TextStyle::Small), stroke.color);
                                }
                                if response.context_menu_opened()
                                    || response.interact_pointer_pos().is_some_and(|pos| timeline_cell_at(pos, rect.min, cell_width, count, layers.len(), row_height).is_some())
                                {
                                response.context_menu(|ui| {
                                    crate::ui_icons::menu_contents(ui, 280., |ui| self.frame_context_menu(ui));
                                });
                                }
                            });
                        if recenter {
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
            ui.painter().vline(divider_x, header.top()..=rows.inner_rect.bottom(), Stroke::new(if resize.is_some() {2.0_f32} else {1.0_f32}, if resize.is_some() {palette.accent} else {palette.divider}));
            if let Some(drag)=&mut reorder {
                if drag.symbol!=self.symbol_id() || !layers.iter().any(|layer| layer.id==drag.layer) {reorder=None;}
                else if let Some(position)=pointer {
                    drag.moved |= position.distance(drag.start)>=6.;
                    if drag.moved {
                        let names_view=Rect::from_min_max(Pos2::new(header.left(),rows.inner_rect.top()),Pos2::new(header.left()+layer_column_width,rows.inner_rect.bottom()));
                        let within=names_view.contains(position);
                        ctx.set_cursor_icon(if within {egui::CursorIcon::Grabbing} else {egui::CursorIcon::NotAllowed});
                        if within {
                            let boundary=(((position.y-grid_rect.top())/row_height).round().max(0.) as usize).min(layers.len());
                            let y=(grid_rect.top()+boundary as f32*row_height).clamp(names_view.top(),names_view.bottom());
                            nf::surface::drop_indicator(ui.painter(), &ctx, Pos2::new(names_view.left(),y), Pos2::new(rows.inner_rect.right(),y));
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
                let geo = Geometry { origin_x: grid_rect.left(), frame_width: cell_width, row_height };
                let end_x = grid_rect.left() + count as f32 * cell_width;
                let rp = ui.painter_at(ruler);
                rp.rect_filled(ruler, 0, p.bg_panel);
                rp.hline(ruler.x_range(), ruler.bottom() - 0.5, Stroke::new(1.0_f32, p.border_subtle));
                let seconds_row = if view.show_seconds && !button_states { RULER_HEIGHT * 0.45 } else { 0. };
                if button_states {
                    // Button symbols: Up · Over · Down · Hit instead of frame numbers.
                    for (frame, name) in crate::authoring_symbols::BUTTON_STATES.iter().enumerate().take(count as usize) {
                        let x = geo.frame_x(frame as u32 + 1);
                        rp.vline(x + 0.5, ruler.bottom() - 7.0..=ruler.bottom(), Stroke::new(1.0_f32, p.border_default));
                        rp.text(Pos2::new(x + cell_width / 2., ruler.center().y), Align2::CENTER_CENTER, *name, typography::font(TextStyle::MonoSmall), p.text_tertiary);
                    }
                } else {
                    // Ticks and numbers stop at the last frame; its number may overhang.
                    let digits = count.max(1).ilog10() as f32 + 1.;
                    let overhang = (digits * 7. - cell_width).max(0.);
                    // Seconds stop at the end too (no "5s" past frame 120 at 24 fps).
                    let full_ruler = Rect::from_min_max(Pos2::new(grid_rect.left(), ruler.top()), Pos2::new(end_x + overhang, ruler.bottom()));
                    let fps = self.project.fps.max(1) as f32;
                    let numbers = rp.with_clip_rect(Rect::from_min_max(Pos2::new(ruler.left(), ruler.top() + seconds_row), Pos2::new(end_x + overhang, ruler.bottom())));
                    tl::paint_ruler(&numbers, &ctx, full_ruler, &geo, fps, view.show_seconds);
                    let seconds = rp.with_clip_rect(Rect::from_min_max(ruler.min, Pos2::new(end_x, ruler.top() + seconds_row)));
                    tl::paint_ruler(&seconds, &ctx, full_ruler, &geo, fps, view.show_seconds);
                }
                for &frame in &script_frames {
                    let on_row = action_row.is_some_and(|row| layers[row].keyframes.iter().any(|key| key.frame == frame));
                    if !on_row && frame < count {
                        let cell = geo.cell(ruler, frame + 1);
                        rp.text(Pos2::new(cell.center().x, ruler.top() + seconds_row / 2. + space::S1), Align2::CENTER_CENTER, "a", typography::font(TextStyle::MonoSmall), palette.timeline_script);
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
                    if pos.x < end_x {
                        self.reset_authored_preview();
                        let frame = frame_at_x(pos.x, grid_rect.left(), cell_width, count);
                        self.frame = if view.snap_playhead && !ui.input(|i| i.modifiers.shift) {
                            self.snap_scrub_frame(frame, pos.x, grid_rect.left(), cell_width)
                        } else {
                            frame
                        };
                        self.select_frame_range(FrameRange { layers: vec![self.symbol().timeline.layers[self.layer].id.clone()], start: self.frame, end: self.frame });
                        self.script_target = ScriptTarget::Frame;
                        self.clear_selection();
                    }
                }
                if self.onion {
                    let (start, end) = self.onion_range();
                    let handles = tl::paint_onion_range(&rp, &ctx, ruler, geo.frame_x(start + 1), geo.frame_x(end + 2));
                    for (is_start, r) in [(true, handles.0), (false, handles.1)] {
                        let handle = ui.interact(
                            r.intersect(ruler),
                            ui.id().with(("onion", is_start)),
                            Sense::drag(),
                        );
                        if handle.hovered() || handle.dragged() {
                            ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        }
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
                // NF2/TL Playhead over ruler and rows: frame chip, time chip on the seconds row.
                let rows_bottom = (grid_rect.top() + content_height).min(grid_clip.bottom()).max(header.bottom());
                let full = Rect::from_min_max(ruler.min, Pos2::new(ruler.right(), rows_bottom));
                let time = (seconds_row > 0.).then(|| format!("{:.2}s", self.frame as f64 / self.project.fps.max(1) as f64));
                tl::paint_playhead(&ui.painter_at(full.expand2(Vec2::new(0., 1.))), &ctx, full, ruler, &geo, geo.frame_x(self.frame + 1), self.frame + 1, time.as_deref());
            }
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

    /// Rows of the frame area, back to front per row: `TL Grid` (empty frames, 5th shaded; folders
    /// a flat strip) → `TL Span`s → `TL Grid Lines` → selected frames (`TL Span` Selected) →
    /// `TL Marker`s (Tone = On Selection inside the selection) → `TL Frame Label`s.
    #[allow(clippy::too_many_arguments)]
    fn paint_timeline_rows(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        clip: Rect,
        cell: f32,
        layers: &[Layer],
        action_row: Option<usize>,
        script_frames: &[u32],
        tinted: bool,
    ) {
        let ctx = painter.ctx().clone();
        let p = neoflash_ui::palette(&ctx);
        let count = self.symbol().timeline.frame_count;
        let row_height = self.workspace.timeline.row_height();
        let geo = Geometry {
            origin_x: rect.left(),
            frame_width: cell,
            row_height,
        };
        let first = (((clip.left() - rect.left()) / cell).max(0.) as u32).min(count);
        let last = (((clip.right() - rect.left()) / cell).ceil().max(0.) as u32).min(count);
        let end_x = rect.left() + count as f32 * cell;
        let selection = self.frame_range();
        for (li, layer) in layers.iter().enumerate() {
            let y = rect.top() + li as f32 * row_height;
            let row =
                Rect::from_min_max(Pos2::new(rect.left(), y), Pos2::new(end_x, y + row_height));
            if !row.intersects(clip) {
                continue;
            }
            let rp = painter.with_clip_rect(clip.intersect(row));
            if layer.properties.role == LayerRole::Folder {
                tl::paint_grid(&rp, &ctx, row, &geo, GridKind::Folder);
                continue;
            }
            tl::paint_grid(&rp, &ctx, row, &geo, GridKind::Layer);
            let layer_end = layer.properties.end_frame.unwrap_or(count).min(count);
            // A motion tween is one span through its ◆ property keys (which have no span of
            // their own); classic and shape tweens without an end keyframe are broken.
            let span_end = |ki: usize| {
                let key = &layer.keyframes[ki];
                let motion = key.tween.is_some() && key.tween_kind == TweenKind::Motion;
                layer.keyframes[ki + 1..]
                    .iter()
                    .find(|k| !(motion && k.keyed_properties.is_some()))
                    .map(|k| k.frame)
            };
            let spans: Vec<(u32, u32, Option<SpanType>)> = layer
                .keyframes
                .iter()
                .enumerate()
                .filter_map(|(ki, key)| {
                    if key.keyed_properties.is_some() {
                        return Some((key.frame, key.frame, None));
                    }
                    let next_key = span_end(ki);
                    let next = next_key.unwrap_or(layer_end).min(count);
                    (next > key.frame).then(|| {
                        let kind = if key.tween.is_some() {
                            Some(match key.tween_kind {
                                TweenKind::Motion => SpanType::MotionTween,
                                _ if !next_key.is_some_and(|f| f < count) => SpanType::BrokenTween,
                                TweenKind::Shape => SpanType::ShapeTween,
                                TweenKind::Classic => SpanType::ClassicTween,
                            })
                        } else if !key.elements.is_empty() {
                            Some(SpanType::Static)
                        } else {
                            None
                        };
                        (key.frame, next, kind)
                    })
                })
                .collect();
            for &(start, next, kind) in &spans {
                if next < first || start > last {
                    continue;
                }
                if let Some(kind) = kind {
                    tl::paint_span(&rp, &ctx, geo.span(row, start + 1, next), kind, tinted);
                }
            }
            tl::paint_grid_lines(&rp, &ctx, row, &geo, p.timeline_grid);
            let selected = selection
                .as_ref()
                .filter(|range| range.layers.contains(&layer.id))
                .map(|range| (range.start, range.end));
            if let Some((start, end)) = selected {
                tl::paint_span(
                    &rp,
                    &ctx,
                    geo.span(row, start + 1, end + 1),
                    SpanType::Selected,
                    tinted,
                );
            }
            let on_selection =
                |frame: u32| selected.is_some_and(|(start, end)| start <= frame && frame <= end);
            for (ki, &(start, next, kind)) in spans.iter().enumerate() {
                if next < first || start > last.saturating_add(1) {
                    continue;
                }
                let key = &layer.keyframes[ki];
                let action = Some(li) == action_row && script_frames.contains(&start);
                let marker = match (key.elements.is_empty(), action) {
                    _ if key.keyed_properties.is_some() => MarkerKind::PropertyKeyframe,
                    (true, true) => MarkerKind::ActionBlankKeyframe,
                    (true, false) => MarkerKind::BlankKeyframe,
                    (false, true) => MarkerKind::ActionKeyframe,
                    (false, false) => MarkerKind::Keyframe,
                };
                tl::paint_marker(
                    &rp,
                    &ctx,
                    geo.cell(row, start + 1),
                    marker,
                    on_selection(start),
                );
                let tween = matches!(
                    kind,
                    Some(
                        SpanType::ClassicTween
                            | SpanType::ShapeTween
                            | SpanType::MotionTween
                            | SpanType::BrokenTween
                    )
                ) || key.keyed_properties.is_some();
                if !tween && next > start + 1 {
                    tl::paint_marker(
                        &rp,
                        &ctx,
                        geo.cell(row, next),
                        MarkerKind::SpanEnd,
                        on_selection(next - 1),
                    );
                }
                if !key.label.is_empty() {
                    let (kind, text) = match key.label.strip_prefix("//") {
                        Some(comment) => (FrameLabelKind::Comment, comment.trim_start()),
                        None => (FrameLabelKind::Label, key.label.as_str()),
                    };
                    tl::paint_frame_label(
                        &rp,
                        &ctx,
                        geo.cell(row, start + 1),
                        kind,
                        text,
                        geo.frame_x(next + 1),
                    );
                }
            }
        }
    }

    /// Frame (right-click) menu, design §8.
    fn frame_context_menu(&mut self, ui: &mut egui::Ui) {
        use crate::timeline_edit::FrameCommand as F;
        use crate::ui_icons::{menu_item, menu_item_enabled, menu_toggle};
        let editable = self.layer_editable();
        let tweenable = self.can_create_tween();
        for (label, kind) in [
            ("Create motion tween", TweenKind::Motion),
            ("Create shape tween", TweenKind::Shape),
            ("Create classic tween", TweenKind::Classic),
        ] {
            if menu_item_enabled(
                ui,
                tweenable && self.current_tween() != Some(kind),
                label,
                "",
            )
            .clicked()
            {
                self.create_tween(kind);
                ui.close();
            }
        }
        nf::menu_separator(ui);
        if menu_item_enabled(ui, editable, "Insert frame", "F5").clicked() {
            self.insert_frame();
            ui.close();
        }
        self.frame_command_item(ui, "Remove frames", "⇧F5", F::Remove);
        if menu_item_enabled(ui, editable, "Insert keyframe", "F6").clicked() {
            self.finish_edits();
            self.make_key(false);
            ui.close();
        }
        if menu_item_enabled(ui, editable, "Insert blank keyframe", "F7").clicked() {
            self.finish_edits();
            self.make_key(true);
            ui.close();
        }
        if menu_item_enabled(ui, editable, "Clear keyframe", "⇧F6").clicked() {
            self.clear_keyframe();
            ui.close();
        }
        if menu_item_enabled(ui, editable, "Convert to keyframes", "").clicked() {
            self.convert_to_keyframes(false);
            ui.close();
        }
        if menu_item_enabled(ui, editable, "Convert to blank keyframes", "").clicked() {
            self.convert_to_keyframes(true);
            ui.close();
        }
        nf::menu_separator(ui);
        self.frame_command_item(ui, "Cut frames", "⌥⌘X", F::Cut);
        self.frame_command_item(ui, "Copy frames", "⌥⌘C", F::Copy);
        self.frame_command_item(ui, "Paste frames", "⌥⌘V", F::Paste);
        self.frame_command_item(ui, "Clear frames", "⌥⌫", F::Clear);
        self.frame_command_item(ui, "Select all frames", "⌥⌘A", F::SelectAll);
        nf::menu_separator(ui);
        Self::unavailable_item(ui, "Copy motion", "");
        Self::unavailable_item(ui, "Paste motion", "");
        Self::unavailable_item(ui, "Paste motion special…", "");
        nf::menu_separator(ui);
        if menu_item_enabled(ui, self.can_reverse_frames(), "Reverse frames", "").clicked() {
            self.reverse_frames();
            ui.close();
        }
        Self::unavailable_item(ui, "Synchronise symbols", "");
        nf::menu_separator(ui);
        if menu_item(ui, "Actions", "F9").clicked() {
            self.open_frame_actions();
            ui.close();
        }
        if menu_item_enabled(ui, self.tween_span().is_some(), "Ease…", "").clicked() {
            self.show_panel(crate::workspace::PanelId::Ease);
            ui.close();
        }
        nf::menu_separator(ui);
        nf::menu_section(ui, "Options");
        menu_toggle(
            ui,
            &mut self.timeline_edit.include_scripts,
            "Include frame scripts",
            "",
        )
        .on_hover_text("Frame scripts belong to this timeline, not to a layer. Include them once in frame copy, move, insert and remove operations. Script collisions are rejected.");
    }

    /// One frame-range command row (Edit ▸ Timeline and the frame menu).
    pub(crate) fn frame_command_item(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        shortcut: &str,
        command: crate::timeline_edit::FrameCommand,
    ) {
        let enabled = self.frame_command_enabled(command);
        if crate::ui_icons::menu_item_enabled(ui, enabled, label, shortcut)
            .on_disabled_hover_text("Select frames in the Timeline first.")
            .clicked()
        {
            self.run_frame_command(ui.ctx(), command);
            ui.close();
        }
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
}
