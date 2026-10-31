//! Configurable workspace docks. Geometry here is transient; the ordered
//! groups, tab selection and split weights live in WorkspacePrefs.
use crate::theme::{DesignUi as _, ScrollTheme as _};
use crate::{
    app::Editor,
    workspace::{
        DockDestination, DockEdge, DockGroup, DockLocation, DockSurface, FloatingNode, PanelId,
        PanelPlacement, SplitAxis,
    },
};
use eframe::egui;

const HEADER: f32 = crate::theme::HEADER_HEIGHT;
const SPLITTER: f32 = 6.;
const AREA_GRIP: f32 = 6.;
/// Drag strip above the docked tools rail (the rail itself has no header in NF2).
const TOOLS_GRIP: f32 = 14.;
const MIN_STAGE_HEIGHT: f32 = 240.;
const MIN_STAGE_WIDTH: f32 = 320.;

#[derive(Clone, Copy)]
struct AreaResize {
    size: f32,
    preference: f32,
    origin: egui::Pos2,
}

#[derive(Clone)]
pub(crate) struct DockGroupTarget {
    pub surface: DockSurface,
    pub edge: DockEdge,
    pub group: u32,
    pub rect: egui::Rect,
    pub header: egui::Rect,
    pub tabs: Vec<(PanelId, egui::Rect)>,
}

#[derive(Clone)]
pub(crate) struct DockHit {
    pub surface: DockSurface,
    pub destination: DockDestination,
    pub rect: egui::Rect,
    pub caption: String,
}

#[derive(Clone, Copy)]
pub(crate) struct DockViewportTarget {
    pub surface: DockSurface,
    pub viewport: egui::ViewportId,
    pub inner: egui::Rect,
    pub content: egui::Rect,
    pub zoom: f32,
    pub order: u64,
}

pub(crate) fn surface_viewport(surface: DockSurface) -> egui::ViewportId {
    match surface {
        DockSurface::Main => egui::ViewportId::ROOT,
        DockSurface::Floating(id) => {
            egui::ViewportId::from_hash_of(id.viewport_key().expect("window key"))
        }
    }
}

/// Large edge targets remain available even when an area is empty. Existing
/// tab headers take precedence, allowing reordering along a window edge.
pub(crate) fn edge_targets(root: egui::Rect, zoom: f32) -> [(DockEdge, egui::Rect); 4] {
    let x = (root.width() * 0.055)
        .clamp(44. * zoom, 80. * zoom)
        .min(root.width() * 0.22);
    let y = (root.height() * 0.055)
        .clamp(36. * zoom, 66. * zoom)
        .min(root.height() * 0.22);
    let top = root.top();
    [
        (
            DockEdge::Top,
            egui::Rect::from_min_max(
                egui::pos2(root.left() + x, top),
                egui::pos2(root.right() - x, top + y),
            ),
        ),
        (
            DockEdge::Bottom,
            egui::Rect::from_min_max(
                egui::pos2(root.left() + x, root.bottom() - y),
                egui::pos2(root.right() - x, root.bottom()),
            ),
        ),
        (
            DockEdge::Left,
            egui::Rect::from_min_max(
                egui::pos2(root.left(), top),
                egui::pos2(root.left() + x, root.bottom()),
            ),
        ),
        (
            DockEdge::Right,
            egui::Rect::from_min_max(egui::pos2(root.right() - x, top), root.max),
        ),
    ]
}

impl Editor {
    pub(crate) fn dock_grid(&mut self, ctx: &egui::Context) {
        let mut content = ctx.viewport_rect();
        content.min.y += neoflash_ui::metrics::TOP_BAR;
        content.max.y -= neoflash_ui::metrics::STATUS_BAR;
        self.register_dock_viewport(ctx, DockSurface::Main, content);
        self.continue_panel_drag(ctx);
        // NF2: the bottom dock spans the full window width; tools rail and inspector sit above it.
        for edge in [
            DockEdge::Top,
            DockEdge::Bottom,
            DockEdge::Left,
            DockEdge::Right,
        ] {
            self.dock_edge(ctx, edge);
        }
    }

    pub(crate) fn dock_edge(&mut self, ctx: &egui::Context, edge: DockEdge) {
        self.detached
            .dock_groups
            .retain(|target| target.surface != DockSurface::Main || target.edge != edge);
        let groups: Vec<_> = self
            .workspace
            .dock
            .area(edge)
            .groups
            .iter()
            .filter_map(|group| {
                let mut group = group.clone();
                group.tabs.retain(|p| self.workspace.is_panel_visible(*p));
                if group.tabs.is_empty() {
                    return None;
                }
                if !group.tabs.contains(&group.active) {
                    group.active = group.tabs[0];
                }
                Some(group)
            })
            .collect();
        if groups.is_empty() {
            return;
        }
        let size = self.workspace.dock.area(edge).size;
        let tools_only = groups.len() == 1 && groups[0].tabs == [PanelId::Tools];
        let available = ctx.available_rect();
        let (minimum, maximum) = if edge.vertical() {
            let min = if tools_only {
                crate::theme::TOOLS_WIDTH
                    + AREA_GRIP
                    + if available.height() < 472. { 12. } else { 0. }
            } else {
                248.
            };
            let reserve = if edge == DockEdge::Left && self.dock_edge_visible(DockEdge::Right) {
                self.workspace
                    .dock
                    .right
                    .size
                    .clamp(248., (available.width() - MIN_STAGE_WIDTH - min).max(248.))
            } else {
                0.
            };
            (
                min,
                (available.width() - MIN_STAGE_WIDTH - reserve).clamp(min, 1000.),
            )
        } else {
            let min = if groups.iter().any(|g| g.tabs.contains(&PanelId::Timeline)) {
                crate::theme::TIMELINE_MIN_HEIGHT
            } else {
                110.
            };
            let reserve = if edge == DockEdge::Top && self.dock_edge_visible(DockEdge::Bottom) {
                self.workspace.dock.bottom.size.clamp(
                    110.,
                    (available.height() - MIN_STAGE_HEIGHT - min).max(110.),
                )
            } else {
                0.
            };
            (
                min,
                (available.height() - MIN_STAGE_HEIGHT - reserve).clamp(min, 800.),
            )
        };
        let effective = size.clamp(minimum, maximum);
        if edge.vertical() {
            let panel = if edge == DockEdge::Left {
                egui::SidePanel::left(edge.panel_id())
            } else {
                egui::SidePanel::right(edge.panel_id())
            };
            panel
                .exact_width(effective)
                .resizable(false)
                .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(0.))
                .show(ctx, |ui| {
                    self.dock_area_contents(ui, edge, &groups, effective, minimum, maximum)
                });
        } else {
            let panel = if edge == DockEdge::Top {
                egui::TopBottomPanel::top(edge.panel_id())
            } else {
                egui::TopBottomPanel::bottom(edge.panel_id())
            };
            panel
                .exact_height(effective)
                .resizable(false)
                .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(0.))
                .show(ctx, |ui| {
                    self.dock_area_contents(ui, edge, &groups, effective, minimum, maximum)
                });
        }
    }

    fn dock_edge_visible(&self, edge: DockEdge) -> bool {
        self.workspace.dock.area(edge).groups.iter().any(|group| {
            group
                .tabs
                .iter()
                .any(|panel| self.workspace.is_panel_visible(*panel))
        })
    }

    fn dock_area_contents(
        &mut self,
        ui: &mut egui::Ui,
        edge: DockEdge,
        groups: &[DockGroup],
        effective: f32,
        minimum: f32,
        maximum: f32,
    ) {
        let full_rect = ui.max_rect();
        let mut rect = full_rect;
        let mut grip = full_rect;
        match edge {
            DockEdge::Top => {
                rect.max.y -= AREA_GRIP;
                grip.min.y = rect.max.y;
            }
            DockEdge::Bottom => {
                rect.min.y += AREA_GRIP;
                grip.max.y = rect.min.y;
            }
            DockEdge::Left => {
                rect.max.x -= AREA_GRIP;
                grip.min.x = rect.max.x;
            }
            DockEdge::Right => {
                rect.min.x += AREA_GRIP;
                grip.max.x = rect.min.x;
            }
        }
        let vertical = edge.vertical();
        let length = if vertical {
            rect.height()
        } else {
            rect.width()
        };
        let space = (length - SPLITTER * groups.len().saturating_sub(1) as f32).max(1.);
        let weight: f32 = groups.iter().map(|g| g.weight).sum();
        // Proportional split, but a stacked group never shrinks below its panel's working minimum
        // (e.g. Library keeps search, three rows and its footer) while the others can give space.
        let mut extents: Vec<f32> = groups.iter().map(|g| space * g.weight / weight).collect();
        if vertical {
            let minimums: Vec<f32> = groups
                .iter()
                .map(|g| g.active.min_stacked_height())
                .collect();
            for i in 0..extents.len() {
                let lack = minimums[i] - extents[i];
                if lack <= 0. {
                    continue;
                }
                let spare: f32 = (0..extents.len())
                    .filter(|j| *j != i)
                    .map(|j| (extents[j] - minimums[j]).max(0.))
                    .sum();
                let take = lack.min(spare);
                if take <= 0. {
                    continue;
                }
                for j in 0..extents.len() {
                    if j != i {
                        let give = (extents[j] - minimums[j]).max(0.) / spare * take;
                        extents[j] -= give;
                    }
                }
                extents[i] += take;
            }
        }
        let mut cursor = if vertical { rect.top() } else { rect.left() };
        for (index, group) in groups.iter().enumerate() {
            let end = if index + 1 == groups.len() {
                if vertical {
                    rect.bottom()
                } else {
                    rect.right()
                }
            } else {
                cursor + extents[index]
            };
            let group_rect = if vertical {
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), cursor),
                    egui::pos2(rect.right(), end),
                )
            } else {
                egui::Rect::from_min_max(
                    egui::pos2(cursor, rect.top()),
                    egui::pos2(end, rect.bottom()),
                )
            };
            self.dock_group_contents(ui, DockSurface::Main, edge, group, group_rect);
            if let Some(next) = groups.get(index + 1) {
                let separator = if vertical {
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left(), end),
                        egui::pos2(rect.right(), end + SPLITTER),
                    )
                } else {
                    egui::Rect::from_min_max(
                        egui::pos2(end, rect.top()),
                        egui::pos2(end + SPLITTER, rect.bottom()),
                    )
                };
                let response = ui.interact(
                    separator,
                    ui.id().with(("dock-split", group.id, next.id)),
                    egui::Sense::drag(),
                );
                // NF2 Splitter: panel surface with a 1 px separator, accent grip while hovered/dragged.
                ui.painter()
                    .rect_filled(separator, 0., neoflash_ui::palette(ui.ctx()).bg_panel);
                neoflash_ui::components::surface::splitter(
                    ui.painter(),
                    ui.ctx(),
                    separator,
                    !vertical,
                    &response,
                );
                if response.dragged() {
                    let delta = ui.input(|i| {
                        if vertical {
                            i.pointer.delta().y
                        } else {
                            i.pointer.delta().x
                        }
                    });
                    let total = group.weight + next.weight;
                    let minimum = (48. / space * weight).min(total * 0.45);
                    let first =
                        (group.weight + delta / space * weight).clamp(minimum, total - minimum);
                    let area = self.workspace.dock.area_mut(edge);
                    if let Some(stored) = area.groups.iter_mut().find(|g| g.id == group.id) {
                        stored.weight = first;
                    }
                    if let Some(stored) = area.groups.iter_mut().find(|g| g.id == next.id) {
                        stored.weight = total - first;
                    }
                }
            }
            cursor = end + SPLITTER;
        }
        self.dock_area_resize_grip(ui, edge, grip, effective, minimum, maximum);
        ui.expand_to_include_rect(full_rect);
    }

    /// The complete grab strip belongs to this dock, with content clipped
    /// away from it. A neighbouring Stage/ScrollArea cannot steal half of it.
    fn dock_area_resize_grip(
        &mut self,
        ui: &mut egui::Ui,
        edge: DockEdge,
        grip: egui::Rect,
        effective: f32,
        minimum: f32,
        maximum: f32,
    ) {
        let id = egui::Id::new(("dock-area-resize", edge));
        let response = ui.interact(grip, id, egui::Sense::drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                format!("Resize {} dock", edge.title().to_lowercase()),
            )
        });
        let state_id = id.with("origin");
        let mut saved = ui.ctx().data(|data| data.get_temp::<AreaResize>(state_id));
        let (press, release, mut pointer, escape) = ui.input(|input| {
            let mut press = None;
            let mut release = None;
            let mut pointer = input.pointer.latest_pos();
            for event in &input.events {
                match event {
                    egui::Event::PointerMoved(pos) => pointer = Some(*pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        ..
                    } => {
                        pointer = Some(*pos);
                        if *pressed {
                            press = Some(*pos);
                        } else {
                            release = Some(*pos);
                        }
                    }
                    _ => {}
                }
            }
            (
                press,
                release,
                pointer,
                input.key_pressed(egui::Key::Escape),
            )
        });
        // A complete native drag can arrive between two paints. Its press
        // event remains authoritative after egui clears pointer.press_origin.
        if let Some(origin) = press.filter(|origin| {
            ui.is_enabled()
                && self.symbol_conversion.is_none()
                && grip.contains(*origin)
                && ui.clip_rect().contains(*origin)
                && ui.ctx().layer_id_at(*origin) == Some(ui.layer_id())
        }) {
            saved = Some(AreaResize {
                size: effective,
                preference: self.workspace.dock.area(edge).size,
                origin,
            });
        }
        let mut released = release.is_some();
        if saved.is_some()
            && let Some((global, down)) = crate::native_screens::pointer_state()
            && (release.is_none() || down)
        {
            let inner = ui
                .ctx()
                .input(|input| input.viewport().inner_rect)
                .unwrap_or_else(|| ui.ctx().viewport_rect());
            pointer = Some(global / ui.ctx().zoom_factor() - inner.min.to_vec2());
            released = !down;
        }
        if escape || self.symbol_conversion.is_some() {
            if let Some(saved) = saved {
                self.workspace.dock.area_mut(edge).size = saved.preference;
            }
            saved = None;
        } else if let Some((saved, pointer)) = saved.zip(release.filter(|_| released).or(pointer)) {
            let delta = if edge.vertical() {
                pointer.x - saved.origin.x
            } else {
                pointer.y - saved.origin.y
            };
            let direction = if matches!(edge, DockEdge::Top | DockEdge::Left) {
                1.
            } else {
                -1.
            };
            self.workspace.dock.area_mut(edge).size =
                (saved.size + delta * direction).clamp(minimum, maximum);
            ui.ctx().request_repaint();
        }
        if released {
            saved = None;
        }
        ui.ctx().data_mut(|data| {
            if let Some(saved) = saved {
                data.insert_temp(state_id, saved);
            } else {
                data.remove::<AreaResize>(state_id);
            }
        });
        let active = response.hovered() || saved.is_some();
        if active {
            ui.ctx().set_cursor_icon(if edge.vertical() {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::ResizeVertical
            });
        }
        // NF2 Splitter between a dock and the canvas: 1 px separator, accent grip while active.
        let p = neoflash_ui::palette(ui.ctx());
        ui.painter().rect_filled(grip, 0., p.bg_panel);
        let _ = active;
        neoflash_ui::components::surface::splitter(
            ui.painter(),
            ui.ctx(),
            grip,
            edge.vertical(),
            &response,
        );
        response.on_hover_text(if edge.vertical() {
            "Drag to change panel width"
        } else {
            "Drag to change panel height"
        });
    }

    fn dock_group_contents(
        &mut self,
        parent: &mut egui::Ui,
        surface: DockSurface,
        edge: DockEdge,
        group: &DockGroup,
        rect: egui::Rect,
    ) {
        // NF2: horizontal docks use `Dock Tab` (40, icon + label); side columns use `Panel Tab` (36).
        // Side column with one panel = `Panel Header` (40, Title text, ghost ⋯); several panels =
        // `Panel Tab`s (36). The docked tools rail only gets a slim drag strip, like the Figma rail.
        let wide = matches!(edge, DockEdge::Top | DockEdge::Bottom);
        let single = !wide && group.tabs.len() == 1;
        let tools_only = group.tabs == [PanelId::Tools];
        let header_h = if tools_only {
            TOOLS_GRIP
        } else if wide {
            neoflash_ui::metrics::DOCK_TAB
        } else if single {
            neoflash_ui::metrics::PANEL_HEADER
        } else {
            HEADER
        };
        let header = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.right(), (rect.top() + header_h).min(rect.bottom())),
        );
        let padding = group.active.body_padding();
        let body = egui::Rect::from_min_max(egui::pos2(rect.left(), header.bottom()), rect.max)
            .shrink(padding);
        let ctx = parent.ctx().clone();
        let clipboard_target = self.clipboard_target(&ctx);
        let zoom = ctx.zoom_factor();
        let inner = ctx
            .input(|i| i.viewport().inner_rect)
            .unwrap_or_else(|| ctx.viewport_rect());
        if surface == DockSurface::Main {
            self.detached.root_inner = Some(inner * zoom);
        }
        let global = |r: egui::Rect| r.translate(inner.min.to_vec2()) * zoom;
        let mut target = DockGroupTarget {
            surface,
            edge,
            group: group.id,
            rect: global(rect),
            header: global(header),
            tabs: Vec::new(),
        };
        // Figma dock tabs start 8 pt in (first label at 24 pt with the 16 pt tab padding).
        let tabs_rect = egui::Rect::from_min_max(
            header.min + egui::vec2(8., 0.),
            header.max - egui::vec2(36., 0.),
        );
        let mut header_ui = parent.new_child(
            egui::UiBuilder::new()
                .id(("dock-tabs", surface, group.id))
                .max_rect(tabs_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        header_ui.set_clip_rect(parent.clip_rect().intersect(tabs_rect));
        if self.symbol_conversion.is_some() {
            header_ui.disable_semantic();
        }
        let nf = neoflash_ui::palette(&ctx);
        header_ui.painter().rect_filled(header, 0., nf.bg_panel);
        header_ui.painter().hline(
            header.x_range(),
            header.bottom() - 0.5,
            egui::Stroke::new(1.0_f32, nf.border_subtle),
        );
        egui::ScrollArea::horizontal()
            .id_salt(("dock-tab-scroll", surface, group.id))
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .themed()
            .show(&mut header_ui, |ui| {
                // ui.horizontal starts with interact_size.y (24), centering its
                // child two points down in this 28-point header. A 28-point tab
                // then overflows the header and clips its active marker.
                let row_size = egui::vec2(ui.available_width(), header_h);
                ui.allocate_ui_with_layout(
                    row_size,
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 0.;
                        for panel in &group.tabs {
                            let active = group.active == *panel;
                            let style = if wide {
                                neoflash_ui::TextStyle::Body
                            } else if single {
                                neoflash_ui::TextStyle::Title
                            } else {
                                neoflash_ui::TextStyle::SmallStrong
                            };
                            let label_w = ui.fonts_mut(|f| {
                                f.layout_no_wrap(
                                    panel.title().into(),
                                    neoflash_ui::typography::font(style),
                                    nf.text_primary,
                                )
                                .size()
                                .x
                            });
                            let pad = if wide { 16. } else { 12. };
                            let icon_w = if wide { 16. + 8. } else { 0. };
                            let (tab_rect, response) = ui.allocate_exact_size(
                                egui::vec2(label_w + icon_w + 2. * pad, header_h),
                                egui::Sense::click_and_drag(),
                            );
                            let visible_rect =
                                tab_rect.intersect(tabs_rect).intersect(ui.clip_rect());
                            if visible_rect.is_positive() {
                                target.tabs.push((*panel, global(visible_rect)));
                                let hovered = !single
                                    && (response.hovered() || response.is_pointer_button_down_on());
                                if hovered && !active {
                                    ui.painter().rect_filled(tab_rect, 0., nf.bg_hover);
                                }
                                let fg = if active || hovered {
                                    nf.text_primary
                                } else {
                                    nf.text_secondary
                                };
                                let mut x = tab_rect.left() + pad;
                                if wide {
                                    let icon_rect = egui::Rect::from_center_size(
                                        egui::pos2(x + 8., tab_rect.center().y),
                                        egui::vec2(16., 16.),
                                    );
                                    neoflash_ui::icons::paint(
                                        ui.painter(),
                                        ui.ctx(),
                                        icon_rect,
                                        panel.icon(),
                                        if active { nf.accent_default } else { fg },
                                    );
                                    x += icon_w;
                                }
                                if !tools_only {
                                    ui.painter().text(
                                        egui::pos2(x, tab_rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        panel.title(),
                                        neoflash_ui::typography::font(style),
                                        fg,
                                    );
                                }
                                if active && !single && !tools_only {
                                    // Keep the marker inside the tab's clip rect, not below it.
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::pos2(
                                                visible_rect.left(),
                                                visible_rect.bottom() - 2.,
                                            ),
                                            visible_rect.max,
                                        ),
                                        0.,
                                        nf.accent_default,
                                    );
                                }
                                if neoflash_ui::theme::keyboard_focus(ui, &response) {
                                    neoflash_ui::theme::focus_ring(ui, tab_rect.shrink(3.), 4.);
                                }
                            }
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::SelectableLabel,
                                    ui.is_enabled(),
                                    active,
                                    panel.title(),
                                )
                            });
                            ui.ctx().accesskit_node_builder(response.id, |node| {
                                node.set_role(egui::accesskit::Role::Tab);
                                node.clear_toggled();
                                node.set_selected(active);
                            });
                            let response = response.on_hover_text(panel.title());
                            if response.clicked() {
                                self.show_panel(*panel);
                            }
                            self.panel_drag_handle(ui, *panel, &response);
                            response.context_menu(|ui| {
                                self.dock_header_menu(ui, *panel, surface, &clipboard_target)
                            });
                        }
                    },
                );
            });
        let blank_start = target
            .tabs
            .last()
            .map_or(tabs_rect.left(), |(_, r)| r.right() / zoom - inner.left());
        let blank =
            egui::Rect::from_min_max(egui::pos2(blank_start, tabs_rect.top()), tabs_rect.max);
        if blank.width() > 6. {
            let response = header_ui.interact(
                blank,
                header_ui.id().with("header-grab"),
                egui::Sense::click_and_drag(),
            );
            self.panel_group_drag_handle(
                &header_ui,
                group.active,
                (surface, edge, group.id),
                &response,
            );
            response.context_menu(|ui| {
                crate::ui_icons::menu_contents(ui, 240., |ui| {
                    if group.tabs.len() > 1 {
                        ui.label(egui::RichText::new("Panels in this group").small());
                        for panel in &group.tabs {
                            let mut active = group.active == *panel;
                            if crate::ui_icons::menu_toggle(ui, &mut active, panel.title(), "")
                                .clicked()
                            {
                                self.show_panel(*panel);
                                ui.close();
                            }
                        }
                        ui.separator();
                    }
                    self.dock_header_commands(ui, group.active, surface, &clipboard_target)
                })
            });
        }
        // A small grip stays reachable when the tab strip is full. The blank
        // header uses the same gesture; tab labels always move a single panel.
        let grip =
            egui::Rect::from_min_max(header.min, egui::pos2(header.left() + 13., header.bottom()));
        let mut grip_ui = parent.new_child(
            egui::UiBuilder::new()
                .id(("dock-group-grip", surface, group.id))
                .max_rect(grip),
        );
        grip_ui.set_clip_rect(parent.clip_rect().intersect(grip));
        if self.symbol_conversion.is_some() {
            grip_ui.disable_semantic();
        }
        let response = grip_ui.interact(grip, grip_ui.id(), egui::Sense::click_and_drag());
        // NF2 headers show no permanent grip; it appears on hover (the whole blank header drags too).
        if response.hovered() || response.dragged() {
            for y in [-4., 0., 4.] {
                grip_ui.painter().circle_filled(
                    grip.center() + egui::vec2(0., y),
                    1.,
                    nf.text_tertiary,
                );
            }
        }
        if tools_only {
            neoflash_ui::icons::paint(
                parent.painter(),
                &ctx,
                egui::Rect::from_center_size(header.center(), egui::vec2(16., 16.)),
                neoflash_ui::icons::i16::GRIP_H,
                nf.text_tertiary,
            );
        }
        self.panel_group_drag_handle(&grip_ui, group.active, (surface, edge, group.id), &response);
        response
            .on_hover_text("Drag panel group")
            .context_menu(|ui| self.dock_header_menu(ui, group.active, surface, &clipboard_target));
        let menu_side = if tools_only { 0. } else { 28. };
        let menu_rect = egui::Rect::from_center_size(
            egui::pos2(header.right() - 6. - menu_side / 2., header.center().y),
            egui::vec2(menu_side, menu_side.min(header.height())),
        );
        let mut menu_ui = parent.new_child(
            egui::UiBuilder::new()
                .id(("dock-group-menu", surface, group.id))
                .max_rect(menu_rect),
        );
        menu_ui.set_clip_rect(parent.clip_rect().intersect(menu_rect));
        if self.symbol_conversion.is_some() {
            menu_ui.disable_semantic();
        }
        if !tools_only {
            let more = menu_ui.add(
                neoflash_ui::components::IconButton::new(
                    neoflash_ui::icons::i16::MORE,
                    "Panel commands and tabs",
                )
                .side(menu_side),
            );
            neoflash_ui::components::popup_below(&more, 240., |ui| {
                crate::ui_icons::menu_contents(ui, 240., |ui| {
                    if group.tabs.len() > 1 {
                        neoflash_ui::components::menu_section(ui, "Panels in this group");
                        for panel in &group.tabs {
                            let mut active = group.active == *panel;
                            if crate::ui_icons::menu_toggle(ui, &mut active, panel.title(), "")
                                .clicked()
                            {
                                self.show_panel(*panel);
                                ui.close();
                            }
                        }
                        ui.separator();
                    }
                    self.dock_header_commands(ui, group.active, surface, &clipboard_target)
                })
            });
        }
        if !tools_only {
            parent.painter().hline(
                header.x_range(),
                header.bottom() - 0.5,
                egui::Stroke::new(1.0_f32, nf.border_subtle),
            );
        }
        self.detached.dock_groups.push(target);
        // A tab drag may have moved this group during the header callback.
        // Its new owner renders it on the next frame, never a second copy here.
        let active = self
            .current_dock_group(surface, edge, group.id)
            .and_then(|g| {
                g.tabs
                    .iter()
                    .copied()
                    .find(|p| *p == g.active && self.workspace.is_panel_visible(*p))
                    .or_else(|| {
                        g.tabs
                            .iter()
                            .copied()
                            .find(|p| self.workspace.is_panel_visible(*p))
                    })
            });
        if let Some(panel) = active
            && body.is_positive()
        {
            let mut content = parent.new_child(
                egui::UiBuilder::new()
                    .id(("dock-body", panel))
                    .max_rect(body),
            );
            content.set_clip_rect(parent.clip_rect().intersect(body));
            content.set_min_size(body.size());
            self.dock_panel_contents(&mut content, panel);
        }
    }

    pub(crate) fn register_dock_viewport(
        &mut self,
        ctx: &egui::Context,
        surface: DockSurface,
        local_content: egui::Rect,
    ) {
        let zoom = ctx.zoom_factor();
        let inner = ctx
            .input(|i| i.viewport().inner_rect)
            .unwrap_or_else(|| ctx.viewport_rect());
        if surface == DockSurface::Main {
            self.detached.root_inner = Some(inner * zoom);
        }
        let previous = self
            .detached
            .dock_viewports
            .iter()
            .find(|v| v.surface == surface)
            .map_or(0, |v| v.order);
        let order = if ctx.input(|i| i.viewport().focused == Some(true) || i.pointer.any_pressed())
        {
            self.detached.activation += 1;
            self.detached.activation
        } else {
            previous
        };
        self.detached
            .dock_viewports
            .retain(|v| v.surface != surface);
        self.detached.dock_viewports.push(DockViewportTarget {
            surface,
            viewport: ctx.viewport_id(),
            inner: inner * zoom,
            content: local_content.translate(inner.min.to_vec2()) * zoom,
            zoom,
            order,
        });
    }

    pub(crate) fn current_dock_group(
        &self,
        surface: DockSurface,
        edge: DockEdge,
        group: u32,
    ) -> Option<&DockGroup> {
        match surface {
            DockSurface::Main => self
                .workspace
                .dock
                .area(edge)
                .groups
                .iter()
                .find(|g| g.id == group),
            DockSurface::Floating(id) => self
                .workspace
                .floating
                .iter()
                .find(|w| w.id == id)?
                .group(group),
        }
    }

    pub(crate) fn floating_dock_contents(&mut self, ui: &mut egui::Ui, window: PanelId) {
        let surface = DockSurface::Floating(window);
        self.register_dock_viewport(ui.ctx(), surface, ui.max_rect());
        self.detached.dock_groups.retain(|g| g.surface != surface);
        let root = self
            .workspace
            .floating
            .iter()
            .find(|w| w.id == window)
            .and_then(|w| w.root.clone());
        if let Some(root) = root {
            self.floating_node_contents(ui, window, &root, ui.max_rect(), &mut Vec::new());
        }
    }

    fn floating_node_visible(&self, node: &FloatingNode) -> bool {
        match node {
            FloatingNode::Group(group) => group
                .tabs
                .iter()
                .any(|p| self.workspace.is_panel_visible(*p)),
            FloatingNode::Split { first, second, .. } => {
                self.floating_node_visible(first) || self.floating_node_visible(second)
            }
        }
    }

    fn floating_node_contents(
        &mut self,
        ui: &mut egui::Ui,
        window: PanelId,
        node: &FloatingNode,
        rect: egui::Rect,
        path: &mut Vec<bool>,
    ) {
        match node {
            FloatingNode::Group(group) => {
                let mut visible = group.clone();
                visible.tabs.retain(|p| self.workspace.is_panel_visible(*p));
                if visible.tabs.is_empty() {
                    return;
                }
                if !visible.tabs.contains(&visible.active) {
                    visible.active = visible.tabs[0];
                }
                self.dock_group_contents(
                    ui,
                    DockSurface::Floating(window),
                    DockEdge::Left,
                    &visible,
                    rect,
                );
            }
            FloatingNode::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let a = self.floating_node_visible(first);
                let b = self.floating_node_visible(second);
                if !a || !b {
                    if a || b {
                        path.push(b);
                        self.floating_node_contents(
                            ui,
                            window,
                            if a { first } else { second },
                            rect,
                            path,
                        );
                        path.pop();
                    }
                    return;
                }
                let vertical = *axis == SplitAxis::Vertical;
                let extent = if vertical {
                    rect.height()
                } else {
                    rect.width()
                };
                let space = (extent - SPLITTER).max(1.);
                let minimum = (48. / space).clamp(0.05, 0.45);
                let ratio = ratio.clamp(minimum, 1. - minimum);
                let coordinate = if vertical { rect.top() } else { rect.left() } + space * ratio;
                let mut first_rect = rect;
                let mut second_rect = rect;
                let mut divider = rect;
                if vertical {
                    first_rect.max.y = coordinate;
                    second_rect.min.y = coordinate + SPLITTER;
                    divider.min.y = coordinate;
                    divider.max.y = coordinate + SPLITTER;
                } else {
                    first_rect.max.x = coordinate;
                    second_rect.min.x = coordinate + SPLITTER;
                    divider.min.x = coordinate;
                    divider.max.x = coordinate + SPLITTER;
                }
                path.push(false);
                self.floating_node_contents(ui, window, first, first_rect, path);
                path.pop();
                path.push(true);
                self.floating_node_contents(ui, window, second, second_rect, path);
                path.pop();
                let response = ui.interact(
                    divider,
                    egui::Id::new(("floating-divider", window, path.clone())),
                    egui::Sense::drag(),
                );
                ui.painter().rect_filled(
                    divider,
                    0.,
                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                );
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(if vertical {
                        egui::CursorIcon::ResizeVertical
                    } else {
                        egui::CursorIcon::ResizeHorizontal
                    });
                }
                if response.dragged() {
                    let delta = ui.input(|i| {
                        if vertical {
                            i.pointer.delta().y
                        } else {
                            i.pointer.delta().x
                        }
                    });
                    if let Some(window) =
                        self.workspace.floating.iter_mut().find(|w| w.id == window)
                    {
                        window
                            .set_ratio(path, (ratio + delta / space).clamp(minimum, 1. - minimum));
                    }
                }
            }
        }
    }

    pub(crate) fn dock_panel_contents(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        if self.symbol_conversion.is_some() {
            ui.disable_semantic();
        }
        self.detached.content_header = Some(panel);
        match panel {
            PanelId::Tools => self.tools_contents(ui),
            PanelId::Timeline => self.timeline_contents(ui),
            PanelId::Library => self.library_contents(ui),
            PanelId::Align => self.align_panel_ui(ui),
            PanelId::Transform => self.transform_panel_ui(ui),
            PanelId::Color => self.color_panel_ui(ui),
            _ => self.bottom_contents(ui, panel),
        }
        self.detached.content_header = None;
    }

    pub(crate) fn panel_dock_menu(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        let ctx = ui.ctx().clone();
        for edge in DockEdge::ALL {
            if crate::ui_icons::menu_item(ui, &format!("Dock {}", edge.title()), "").clicked() {
                self.dock_panel_at(&ctx, panel, DockDestination::Edge(edge));
                ui.close();
            }
        }
        ui.separator();
        if crate::ui_icons::menu_item(ui, "Detach", "").clicked() {
            self.set_panel_placement(&ctx, panel, PanelPlacement::Detached);
            ui.close();
        }
        if crate::ui_icons::menu_item(ui, "Hide", "").clicked() {
            self.set_panel_visible(panel, false);
            ui.close();
        }
    }

    fn dock_header_menu(
        &mut self,
        ui: &mut egui::Ui,
        panel: PanelId,
        surface: DockSurface,
        target: &crate::clipboard::ClipboardTarget,
    ) {
        crate::ui_icons::menu_contents(ui, 240., |ui| {
            self.dock_header_commands(ui, panel, surface, target)
        });
    }

    fn dock_header_commands(
        &mut self,
        ui: &mut egui::Ui,
        panel: PanelId,
        surface: DockSurface,
        target: &crate::clipboard::ClipboardTarget,
    ) {
        if panel == PanelId::Color {
            self.color_panel_menu(ui);
            ui.separator();
        }
        if let DockSurface::Floating(window) = surface {
            self.floating_panel_commands(ui, window, target);
        }
        self.panel_dock_menu(ui, panel);
    }

    pub(crate) fn dock_surface_at(&self, pointer: egui::Pos2) -> Option<DockViewportTarget> {
        let candidates: Vec<_> = self
            .detached
            .dock_viewports
            .iter()
            .copied()
            .filter(|target| {
                target.inner.contains(pointer)
                    && match target.surface {
                        DockSurface::Main => true,
                        DockSurface::Floating(id) => self.workspace.floating.iter().any(|window| {
                            window.id == id
                                && window
                                    .panels()
                                    .iter()
                                    .any(|panel| self.workspace.is_panel_visible(*panel))
                        }),
                    }
            })
            .collect();
        match crate::native_screens::front_window_at(pointer) {
            Some(Some(native)) => candidates
                .into_iter()
                .filter(|target| {
                    target.inner.min.distance(native.min) < 6.
                        && target.inner.max.distance(native.max) < 6.
                })
                .max_by_key(|target| {
                    (
                        target.order,
                        u8::from(matches!(target.surface, DockSurface::Floating(_))),
                    )
                }),
            Some(None) => None,
            None => candidates.into_iter().max_by_key(|target| {
                (
                    target.order,
                    u8::from(matches!(target.surface, DockSurface::Floating(_))),
                )
            }),
        }
    }

    pub(crate) fn dock_hit(&self, pointer: egui::Pos2, zoom: f32) -> Option<DockHit> {
        let view = self.dock_surface_at(pointer).or_else(|| {
            // Standalone header harnesses and an initial frame may precede a
            // complete grid paint. Native viewports otherwise own their targets.
            if !self.detached.dock_viewports.is_empty() {
                return None;
            }
            let inner = self.detached.root_inner?;
            inner.contains(pointer).then_some(DockViewportTarget {
                surface: DockSurface::Main,
                viewport: egui::ViewportId::ROOT,
                inner,
                content: egui::Rect::from_min_max(
                    inner.min + egui::vec2(0., 32. * zoom),
                    inner.max,
                ),
                zoom,
                order: 0,
            })
        })?;
        if !view.content.contains(pointer) {
            return None;
        }
        let groups: Vec<_> = self
            .detached
            .dock_groups
            .iter()
            .filter(|target| {
                target.surface == view.surface
                    && self
                        .current_dock_group(target.surface, target.edge, target.group)
                        .is_some()
            })
            .collect();
        let tabs_destination = |target: &DockGroupTarget, index| match target.surface {
            DockSurface::Main => DockDestination::Tabs(DockLocation {
                edge: target.edge,
                group: target.group,
                index,
            }),
            DockSurface::Floating(window) => DockDestination::FloatingTabs {
                window,
                group: target.group,
                index,
            },
        };
        if let Some(target) = groups.iter().find(|g| g.header.contains(pointer)) {
            let group = self.current_dock_group(target.surface, target.edge, target.group)?;
            let before = target
                .tabs
                .iter()
                .find(|(_, rect)| pointer.x < rect.center().x);
            let index = before
                .and_then(|(p, _)| group.tabs.iter().position(|tab| tab == p))
                .unwrap_or(group.tabs.len());
            let x = before.map_or_else(
                || {
                    target
                        .tabs
                        .last()
                        .map_or(target.header.left(), |(_, r)| r.right())
                },
                |(_, r)| r.left(),
            );
            return Some(DockHit {
                surface: view.surface,
                destination: tabs_destination(target, index),
                rect: egui::Rect::from_min_max(
                    egui::pos2(x - 3. * view.zoom, target.header.top()),
                    egui::pos2(x + 3. * view.zoom, target.header.bottom()),
                ),
                caption: "Place tab".into(),
            });
        }
        for (edge, rect) in edge_targets(view.content, view.zoom) {
            if rect.contains(pointer) {
                let destination = match view.surface {
                    DockSurface::Main => DockDestination::Edge(edge),
                    DockSurface::Floating(window) => DockDestination::FloatingSplit {
                        window,
                        group: None,
                        edge,
                    },
                };
                return Some(DockHit {
                    surface: view.surface,
                    destination,
                    rect,
                    caption: format!("Dock {}", edge.title()),
                });
            }
        }
        let target = groups.iter().find(|g| g.rect.contains(pointer))?;
        let split = match view.surface {
            DockSurface::Main => {
                let relative = if target.edge.vertical() {
                    (pointer.y - target.rect.top()) / target.rect.height()
                } else {
                    (pointer.x - target.rect.left()) / target.rect.width()
                };
                if !(0.25..=0.75).contains(&relative) {
                    Some(if target.edge.vertical() {
                        if relative < 0.5 {
                            DockEdge::Top
                        } else {
                            DockEdge::Bottom
                        }
                    } else if relative < 0.5 {
                        DockEdge::Left
                    } else {
                        DockEdge::Right
                    })
                } else {
                    None
                }
            }
            DockSurface::Floating(_) => {
                let x = (pointer.x - target.rect.left()) / target.rect.width();
                let y = (pointer.y - target.rect.top()) / target.rect.height();
                [
                    (DockEdge::Left, x),
                    (DockEdge::Right, 1. - x),
                    (DockEdge::Top, y),
                    (DockEdge::Bottom, 1. - y),
                ]
                .into_iter()
                .filter(|(_, distance)| *distance < 0.24)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(edge, _)| edge)
            }
        };
        if let Some(edge) = split {
            let mut rect = target.rect;
            match edge {
                DockEdge::Top => rect.max.y = rect.center().y,
                DockEdge::Bottom => rect.min.y = rect.center().y,
                DockEdge::Left => rect.max.x = rect.center().x,
                DockEdge::Right => rect.min.x = rect.center().x,
            }
            let destination = match view.surface {
                DockSurface::Main => DockDestination::Split {
                    edge: target.edge,
                    group: target.group,
                    before: matches!(edge, DockEdge::Top | DockEdge::Left),
                },
                DockSurface::Floating(window) => DockDestination::FloatingSplit {
                    window,
                    group: Some(target.group),
                    edge,
                },
            };
            return Some(DockHit {
                surface: view.surface,
                destination,
                rect,
                caption: format!("Split {}", edge.title().to_lowercase()),
            });
        }
        let group = self.current_dock_group(target.surface, target.edge, target.group)?;
        Some(DockHit {
            surface: view.surface,
            destination: tabs_destination(target, group.tabs.len()),
            rect: target.rect,
            caption: "Add as tab".into(),
        })
    }
}
