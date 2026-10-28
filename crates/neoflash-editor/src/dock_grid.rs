//! Configurable workspace docks. Geometry here is transient; the ordered
//! groups, tab selection and split weights live in WorkspacePrefs.
use crate::{
    app::Editor,
    workspace::{DockDestination, DockEdge, DockGroup, DockLocation, PanelId, PanelPlacement},
};
use eframe::egui;

const HEADER: f32 = 26.;
const SPLITTER: f32 = 5.;

#[derive(Clone)]
pub(crate) struct DockGroupTarget {
    pub edge: DockEdge,
    pub group: u32,
    pub rect: egui::Rect,
    pub header: egui::Rect,
    pub tabs: Vec<(PanelId, egui::Rect)>,
}

#[derive(Clone)]
pub(crate) struct DockHit {
    pub destination: DockDestination,
    pub rect: egui::Rect,
    pub caption: String,
}

/// Large edge targets remain available even when an area is empty. Existing
/// tab headers take precedence, allowing reordering along a window edge.
pub(crate) fn edge_targets(root: egui::Rect, zoom: f32) -> [(DockEdge, egui::Rect); 4] {
    let x = (root.width() * 0.055).clamp(44. * zoom, 80. * zoom);
    let y = (root.height() * 0.055).clamp(36. * zoom, 66. * zoom);
    let top = root.top() + 32. * zoom;
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
        // Side columns span alongside the bottom editors so a Library preview
        // and Properties remain usable together on smaller windows.
        for edge in [
            DockEdge::Top,
            DockEdge::Left,
            DockEdge::Right,
            DockEdge::Bottom,
        ] {
            self.dock_edge(ctx, edge);
        }
    }

    pub(crate) fn dock_edge(&mut self, ctx: &egui::Context, edge: DockEdge) {
        self.detached
            .dock_groups
            .retain(|target| target.edge != edge);
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
        let rect = if edge.vertical() {
            let min = if tools_only { 56. } else { 170. };
            let max = (available.width() - 160.).clamp(min, 1000.);
            let panel = if edge == DockEdge::Left {
                egui::SidePanel::left(edge.panel_id())
            } else {
                egui::SidePanel::right(edge.panel_id())
            };
            panel
                .default_width(size.clamp(min, max))
                .min_width(min)
                .max_width(max)
                .resizable(true)
                .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(0.))
                .show(ctx, |ui| self.dock_area_contents(ui, edge, &groups))
                .response
                .rect
        } else {
            let min = 110.;
            let max = (available.height() - 130.).clamp(min, 800.);
            let panel = if edge == DockEdge::Top {
                egui::TopBottomPanel::top(edge.panel_id())
            } else {
                egui::TopBottomPanel::bottom(edge.panel_id())
            };
            panel
                .default_height(size.clamp(min, max))
                .min_height(min)
                .max_height(max)
                .resizable(true)
                .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(0.))
                .show(ctx, |ui| self.dock_area_contents(ui, edge, &groups))
                .response
                .rect
        };
        self.workspace.dock.area_mut(edge).size = if edge.vertical() {
            rect.width()
        } else {
            rect.height()
        };
    }

    fn dock_area_contents(&mut self, ui: &mut egui::Ui, edge: DockEdge, groups: &[DockGroup]) {
        let rect = ui.max_rect();
        let vertical = edge.vertical();
        let length = if vertical {
            rect.height()
        } else {
            rect.width()
        };
        let space = (length - SPLITTER * groups.len().saturating_sub(1) as f32).max(1.);
        let weight: f32 = groups.iter().map(|g| g.weight).sum();
        let mut cursor = if vertical { rect.top() } else { rect.left() };
        for (index, group) in groups.iter().enumerate() {
            let end = if index + 1 == groups.len() {
                if vertical {
                    rect.bottom()
                } else {
                    rect.right()
                }
            } else {
                cursor + space * group.weight / weight
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
            self.dock_group_contents(ui, edge, group, group_rect);
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
                ui.painter().rect_filled(
                    separator,
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
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    fn dock_group_contents(
        &mut self,
        parent: &mut egui::Ui,
        edge: DockEdge,
        group: &DockGroup,
        rect: egui::Rect,
    ) {
        let header = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.right(), (rect.top() + HEADER).min(rect.bottom())),
        );
        let body =
            egui::Rect::from_min_max(egui::pos2(rect.left(), header.bottom()), rect.max).shrink(5.);
        let ctx = parent.ctx().clone();
        let zoom = ctx.zoom_factor();
        let inner = ctx
            .input(|i| i.viewport().inner_rect)
            .unwrap_or_else(|| ctx.viewport_rect());
        self.detached.root_inner = Some(inner * zoom);
        let global = |r: egui::Rect| r.translate(inner.min.to_vec2()) * zoom;
        let mut target = DockGroupTarget {
            edge,
            group: group.id,
            rect: global(rect),
            header: global(header),
            tabs: Vec::new(),
        };
        let mut header_ui = parent.new_child(
            egui::UiBuilder::new()
                .id(("dock-tabs", group.id))
                .max_rect(header)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        header_ui.set_clip_rect(parent.clip_rect().intersect(header));
        if self.symbol_conversion.is_some() {
            header_ui.disable();
        }
        header_ui
            .painter()
            .rect_filled(header, 0., header_ui.visuals().faint_bg_color);
        egui::ScrollArea::horizontal()
            .id_salt(("dock-tab-scroll", group.id))
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut header_ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.;
                    for panel in &group.tabs {
                        let response = ui.add(
                            egui::Button::new(egui::RichText::new(panel.title()).size(12.))
                                .selected(group.active == *panel)
                                .frame(true)
                                .sense(egui::Sense::click_and_drag()),
                        );
                        let visible_rect = response.rect.intersect(header);
                        if visible_rect.is_positive() {
                            target.tabs.push((*panel, global(visible_rect)));
                        }
                        if response.clicked() {
                            self.show_panel(*panel);
                        }
                        self.panel_drag_handle(ui, *panel, &response);
                        response.context_menu(|ui| self.panel_dock_menu(ui, *panel));
                    }
                });
            });
        let blank_start = target
            .tabs
            .last()
            .map_or(header.left(), |(_, r)| r.right() / zoom - inner.left());
        let blank = egui::Rect::from_min_max(egui::pos2(blank_start, header.top()), header.max);
        if blank.width() > 6. {
            let response = header_ui.interact(
                blank,
                header_ui.id().with("header-grab"),
                egui::Sense::click_and_drag(),
            );
            self.panel_drag_handle(&header_ui, group.active, &response);
        }
        parent.painter().hline(
            header.x_range(),
            header.bottom(),
            parent.visuals().widgets.noninteractive.bg_stroke,
        );
        self.detached.dock_groups.push(target);
        // A tab drag may have moved this group during the header callback.
        // Its new owner renders it on the next frame, never a second copy here.
        let active = self
            .workspace
            .dock
            .area(edge)
            .groups
            .iter()
            .find(|g| g.id == group.id)
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

    pub(crate) fn dock_panel_contents(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        if self.symbol_conversion.is_some() {
            ui.disable();
        }
        self.detached.content_header = Some(panel);
        match panel {
            PanelId::Tools => self.tools_contents(ui),
            PanelId::Timeline => self.timeline_contents(ui),
            PanelId::Library => self.library_contents(ui),
            _ => self.bottom_contents(ui, panel),
        }
        self.detached.content_header = None;
    }

    pub(crate) fn panel_dock_menu(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        let ctx = ui.ctx().clone();
        for edge in DockEdge::ALL {
            if ui.button(format!("Dock {}", edge.title())).clicked() {
                self.dock_panel_at(&ctx, panel, DockDestination::Edge(edge));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Detach").clicked() {
            self.set_panel_placement(&ctx, panel, PanelPlacement::Detached);
            ui.close();
        }
        if ui.button("Hide").clicked() {
            self.set_panel_visible(panel, false);
            ui.close();
        }
    }

    pub(crate) fn dock_hit(&self, pointer: egui::Pos2, zoom: f32) -> Option<DockHit> {
        let root = self.detached.root_inner?;
        if !root.contains(pointer) {
            return None;
        }
        let groups: Vec<_> = self
            .detached
            .dock_groups
            .iter()
            .filter(|target| {
                self.workspace
                    .dock
                    .area(target.edge)
                    .groups
                    .iter()
                    .any(|g| g.id == target.group)
            })
            .collect();
        // Exact tab insertion is independent of edge/split hot zones.
        if let Some(target) = groups.iter().find(|g| g.header.contains(pointer)) {
            let group = self
                .workspace
                .dock
                .area(target.edge)
                .groups
                .iter()
                .find(|g| g.id == target.group)?;
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
                destination: DockDestination::Tabs(DockLocation {
                    edge: target.edge,
                    group: target.group,
                    index,
                }),
                rect: egui::Rect::from_min_max(
                    egui::pos2(x - 3. * zoom, target.header.top()),
                    egui::pos2(x + 3. * zoom, target.header.bottom()),
                ),
                caption: "Place tab".into(),
            });
        }
        for (edge, rect) in edge_targets(root, zoom) {
            if rect.contains(pointer) {
                return Some(DockHit {
                    destination: DockDestination::Edge(edge),
                    rect,
                    caption: format!("Dock {}", edge.title()),
                });
            }
        }
        let target = groups.iter().find(|g| g.rect.contains(pointer))?;
        let vertical = target.edge.vertical();
        let length = if vertical {
            target.rect.height()
        } else {
            target.rect.width()
        };
        let distance = if vertical {
            pointer.y - target.rect.top()
        } else {
            pointer.x - target.rect.left()
        };
        let relative = distance / length;
        if !(0.25..=0.75).contains(&relative) {
            let before = relative < 0.5;
            let mut rect = target.rect;
            if vertical {
                if before {
                    rect.max.y = rect.center().y;
                } else {
                    rect.min.y = rect.center().y;
                }
            } else if before {
                rect.max.x = rect.center().x;
            } else {
                rect.min.x = rect.center().x;
            }
            return Some(DockHit {
                destination: DockDestination::Split {
                    edge: target.edge,
                    group: target.group,
                    before,
                },
                rect,
                caption: if vertical {
                    if before { "Split above" } else { "Split below" }
                } else if before {
                    "Split left"
                } else {
                    "Split right"
                }
                .into(),
            });
        }
        let group = self
            .workspace
            .dock
            .area(target.edge)
            .groups
            .iter()
            .find(|g| g.id == target.group)?;
        Some(DockHit {
            destination: DockDestination::Tabs(DockLocation {
                edge: target.edge,
                group: target.group,
                index: group.tabs.len(),
            }),
            rect: target.rect,
            caption: "Add as tab".into(),
        })
    }
}
