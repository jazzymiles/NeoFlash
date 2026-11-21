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
use neoflash_ui::components as nf;

const SPLITTER: f32 = 6.;
const AREA_GRIP: f32 = 6.;
const MIN_STAGE_HEIGHT: f32 = 360.;
const MIN_STAGE_WIDTH: f32 = 320.;
const COLLAPSED_GROUP_WEIGHT: f32 = 0.001;

pub(crate) struct VerticalDockLayout {
    pub(crate) collapsed: Vec<bool>,
    pub(crate) extents: Vec<f32>,
}

/// Smallest height of a stacked side-dock group: the largest `min_stacked_height` of its (visible)
/// tabs, never just the active one, so switching tabs cannot move the splitter between groups.
pub(crate) fn group_min_stacked_height(group: &DockGroup) -> f32 {
    group
        .tabs
        .iter()
        .map(|panel| panel.min_stacked_height())
        .fold(group.active.min_stacked_height(), f32::max)
}

/// Allocates heights for a stacked vertical side dock (R-077): every expanded group keeps at
/// least its tabs' useful minimum height ([`group_min_stacked_height`]); groups dragged below their
/// minimum or crowded out when the side dock is shorter than all minima collapse to a single
/// `PanelHeader` strip without a splitter grip.
pub(crate) fn compute_vertical_dock_layout(
    groups: &[DockGroup],
    length: f32,
    collapsed_h: f32,
) -> VerticalDockLayout {
    let n = groups.len();
    if n == 0 {
        return VerticalDockLayout {
            collapsed: Vec::new(),
            extents: Vec::new(),
        };
    }
    if n == 1 {
        return VerticalDockLayout {
            collapsed: vec![false],
            extents: vec![length.max(0.)],
        };
    }
    let minimums: Vec<f32> = groups.iter().map(group_min_stacked_height).collect();
    let mut collapsed: Vec<bool> = groups
        .iter()
        .map(|g| g.weight <= COLLAPSED_GROUP_WEIGHT * 2.)
        .collect();
    if collapsed.iter().all(|c| *c)
        && let Some(best) = (0..n).max_by(|&a, &b| {
            groups[a]
                .weight
                .total_cmp(&groups[b].weight)
                .then_with(|| a.cmp(&b).reverse())
        })
    {
        collapsed[best] = false;
    }

    let splitter_count = |collapsed: &[bool]| -> usize {
        (0..n.saturating_sub(1))
            .filter(|&i| !collapsed[i] && !collapsed[i + 1])
            .count()
    };

    loop {
        let open_count = collapsed.iter().filter(|c| !**c).count();
        if open_count <= 1 {
            break;
        }
        let collapsed_count = n - open_count;
        let reserved =
            collapsed_count as f32 * collapsed_h + splitter_count(&collapsed) as f32 * SPLITTER;
        let open_min: f32 = (0..n).filter(|&i| !collapsed[i]).map(|i| minimums[i]).sum();
        if length - reserved + 0.5 >= open_min {
            break;
        }
        if let Some(victim) = (0..n).filter(|&i| !collapsed[i]).min_by(|&a, &b| {
            groups[a]
                .weight
                .total_cmp(&groups[b].weight)
                .then_with(|| b.cmp(&a))
        }) {
            collapsed[victim] = true;
        } else {
            break;
        }
    }

    let open_count = collapsed.iter().filter(|c| !**c).count();
    let collapsed_count = n - open_count;
    let splitters = splitter_count(&collapsed) as f32 * SPLITTER;
    let space = (length - splitters).max(0.);
    let reserved_collapsed = collapsed_count as f32 * collapsed_h;
    let mut extents = vec![collapsed_h; n];

    if reserved_collapsed >= space {
        let scale = if reserved_collapsed > 0. {
            space / reserved_collapsed
        } else {
            0.
        };
        for i in 0..n {
            extents[i] = if collapsed[i] {
                collapsed_h * scale
            } else {
                0.
            };
        }
        return VerticalDockLayout { collapsed, extents };
    }

    let open_space = (space - reserved_collapsed).max(0.);
    if open_count == 1 {
        if let Some(open_idx) = (0..n).position(|i| !collapsed[i]) {
            extents[open_idx] = open_space;
        }
        return VerticalDockLayout { collapsed, extents };
    }

    let open_weight: f32 = (0..n)
        .filter(|&i| !collapsed[i])
        .map(|i| groups[i].weight.max(0.01))
        .sum();
    for i in 0..n {
        if !collapsed[i] {
            extents[i] = open_space * groups[i].weight.max(0.01) / open_weight;
        }
    }
    for i in 0..n {
        if collapsed[i] {
            continue;
        }
        let lack = minimums[i] - extents[i];
        if lack <= 0. {
            continue;
        }
        let spare: f32 = (0..n)
            .filter(|&j| j != i && !collapsed[j])
            .map(|j| (extents[j] - minimums[j]).max(0.))
            .sum();
        let take = lack.min(spare);
        if take <= 0. {
            continue;
        }
        for j in 0..n {
            if j != i && !collapsed[j] {
                let give = (extents[j] - minimums[j]).max(0.) / spare * take;
                extents[j] -= give;
            }
        }
        extents[i] += take;
    }

    VerticalDockLayout { collapsed, extents }
}

/// Content width limits `(minimum, maximum)` of a side dock: the largest minimum and the largest
/// maximum over every visible tab of every group (the union rule of `FloatingNode::minimum_size`
/// / `maximum_size`). `None` when no tab is visible.
pub(crate) fn side_dock_content_limits(
    groups: &[DockGroup],
    visible: impl Fn(PanelId) -> bool,
) -> Option<(f32, f32)> {
    groups
        .iter()
        .flat_map(|group| group.tabs.iter().copied())
        .filter(|panel| visible(*panel))
        .map(|panel| {
            (
                crate::workspace::minimum_window_size(panel)[0],
                crate::workspace::maximum_window_size(panel)[0],
            )
        })
        .reduce(|a, b| (a.0.max(b.0), a.1.max(b.1)))
}

/// On the split axis, child extents add up. Across it, both children share the
/// same space. A hidden subtree does not reserve space on either axis.
fn nested_limits(
    node: &FloatingNode,
    direction: SplitAxis,
    measure: impl Fn(&DockGroup) -> Option<(f32, f32)> + Copy,
) -> Option<(f32, f32)> {
    match node {
        FloatingNode::Group(group) => measure(group),
        FloatingNode::Split {
            axis,
            first,
            second,
            ..
        } => {
            let first = nested_limits(first, direction, measure);
            let second = nested_limits(second, direction, measure);
            match (first, second) {
                (Some(a), Some(b)) if *axis == direction => {
                    Some((a.0 + b.0 + SPLITTER, a.1 + b.1 + SPLITTER))
                }
                (Some(a), Some(b)) => Some((a.0.max(b.0), a.1.max(b.1))),
                (a, b) => a.or(b),
            }
        }
    }
}

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

/// Crisp edge targets remain available even when an area is empty. Existing
/// tab headers take precedence, allowing reordering along a window edge.
pub(crate) fn edge_targets(root: egui::Rect, zoom: f32) -> [(DockEdge, egui::Rect); 4] {
    let x = (root.width() * 0.024)
        .clamp(28. * zoom, 36. * zoom)
        .min(root.width() * 0.15);
    let y = (root.height() * 0.028)
        .clamp(28. * zoom, 34. * zoom)
        .min(root.height() * 0.15);
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

/// Pointer-only hit testing uses a slightly wider inward halo than the visible
/// NF2 target while staying within a crisp 28..44 pt band. Never painted.
pub(crate) fn edge_acceptance_targets(root: egui::Rect, zoom: f32) -> [(DockEdge, egui::Rect); 4] {
    let x = (root.width() * 0.03)
        .clamp(32. * zoom, 44. * zoom)
        .min(root.width() * 0.15);
    let y = (root.height() * 0.04)
        .clamp(32. * zoom, 42. * zoom)
        .min(root.height() * 0.15);
    [
        (
            DockEdge::Top,
            egui::Rect::from_min_max(root.min, egui::pos2(root.right(), root.top() + y)),
        ),
        (
            DockEdge::Bottom,
            egui::Rect::from_min_max(egui::pos2(root.left(), root.bottom() - y), root.max),
        ),
        (
            DockEdge::Left,
            egui::Rect::from_min_max(root.min, egui::pos2(root.left() + x, root.bottom())),
        ),
        (
            DockEdge::Right,
            egui::Rect::from_min_max(egui::pos2(root.right() - x, root.top()), root.max),
        ),
    ]
}

impl Editor {
    pub(crate) fn dock_grid(&mut self, ctx: &egui::Context) {
        let mut content = ctx.viewport_rect();
        content.min.y += neoflash_ui::metrics::top_bar();
        content.max.y -= neoflash_ui::metrics::status_bar();
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
        let available = ctx.available_rect();
        let (minimum, maximum) = if edge.vertical() {
            self.side_dock_width_limits(edge, available.size())
        } else {
            let min = if groups.iter().any(|g| g.tabs.contains(&PanelId::Timeline)) {
                crate::theme::TIMELINE_MIN_HEIGHT
            } else {
                110.
            };
            let doc_bar = if self.has_document() {
                neoflash_ui::metrics::panel_header()
            } else {
                0.
            };
            let reserve = if edge == DockEdge::Top && self.dock_edge_visible(DockEdge::Bottom) {
                self.workspace.dock.bottom.size.clamp(
                    110.,
                    (available.height() - doc_bar - MIN_STAGE_HEIGHT - min).max(110.),
                )
            } else {
                0.
            };
            let budget = (available.height() - doc_bar - MIN_STAGE_HEIGHT - reserve).max(min);
            if let Some(root) = self.workspace.dock.area(edge).nested_layout() {
                let height_limits = self.main_node_limits(&root, SplitAxis::Vertical);
                let (ideal_min, ideal_max) = height_limits.unwrap_or((min, budget));
                let min = (ideal_min + AREA_GRIP).clamp(min, budget);
                (min, (ideal_max + AREA_GRIP).clamp(min, budget))
            } else {
                (min, budget.min(800.))
            }
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

    /// Minimum and maximum width of the side dock on `edge` inside `available` (the window area
    /// left for side docks and the Stage). Both come from every visible tab on the edge, never
    /// from the active one, so switching tabs cannot resize the dock.
    pub(crate) fn side_dock_width_limits(
        &self,
        edge: DockEdge,
        available: egui::Vec2,
    ) -> (f32, f32) {
        let visible = |panel: PanelId| self.workspace.is_panel_visible(panel);
        let limits = |edge: DockEdge| {
            let area = self.workspace.dock.area(edge);
            if let Some(root) = area.nested_layout() {
                self.main_node_limits(&root, SplitAxis::Horizontal)
            } else {
                side_dock_content_limits(&area.groups, visible)
            }
        };
        let Some((content_min, content_max)) = limits(edge) else {
            let floor = neoflash_ui::metrics::inspector() + AREA_GRIP;
            return (floor, floor);
        };
        let tools_only = self
            .workspace
            .dock
            .area(edge)
            .groups
            .iter()
            .flat_map(|group| group.tabs.iter().copied())
            .filter(|panel| visible(*panel))
            .all(|panel| panel == PanelId::Tools);
        let min = if tools_only {
            crate::theme::tools_w() + AREA_GRIP + if available.y < 472. { 12. } else { 0. }
        } else {
            content_min.max(neoflash_ui::metrics::inspector()) + AREA_GRIP
        };
        let reserve = if edge == DockEdge::Left {
            limits(DockEdge::Right).map_or(0., |(right_min, _)| {
                let right_min = right_min.max(neoflash_ui::metrics::inspector()) + AREA_GRIP;
                self.workspace.dock.right.size.clamp(
                    right_min,
                    (available.x - MIN_STAGE_WIDTH - min).max(right_min),
                )
            })
        } else {
            0.
        };
        // With many horizontal splits the ideal content minimum may exceed the entire
        // window. Keep the Stage usable and clip the individual leaf bodies in that case.
        let budget = (available.x - MIN_STAGE_WIDTH - reserve).max(AREA_GRIP);
        let nested = self.workspace.dock.area(edge).root.is_some();
        let min = if nested { min.min(budget) } else { min };
        let cap = if nested { budget } else { 1000. };
        let max = (content_max + AREA_GRIP).min(cap).min(budget).max(min);
        (min, max)
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
        if let Some(root) = self.workspace.dock.area(edge).nested_layout() {
            self.dock_node_contents(ui, (DockSurface::Main, edge), &root, rect, &mut Vec::new());
            self.dock_area_resize_grip(ui, edge, grip, effective, minimum, maximum);
            ui.expand_to_include_rect(full_rect);
            return;
        }
        let vertical = edge.vertical();
        let length = if vertical {
            rect.height()
        } else {
            rect.width()
        };
        let collapsed_h = neoflash_ui::metrics::panel_header();
        let space = (length - SPLITTER * groups.len().saturating_sub(1) as f32).max(1.);
        let weight: f32 = groups.iter().map(|g| g.weight).sum();
        let (collapsed, extents) = if vertical {
            let layout = compute_vertical_dock_layout(groups, length, collapsed_h);
            (layout.collapsed, layout.extents)
        } else {
            (
                vec![false; groups.len()],
                groups.iter().map(|g| space * g.weight / weight).collect(),
            )
        };
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
            let is_collapsed = collapsed[index];
            self.paint_dock_group_contents(
                ui,
                DockSurface::Main,
                group,
                (edge, group_rect),
                None,
                is_collapsed,
            );
            let next = groups.get(index + 1);
            let has_splitter = next.is_some() && !(is_collapsed || collapsed[index + 1]);
            if let Some(next) = next
                && has_splitter
            {
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
                let response = nf::tooltip(
                    ui.interact(
                        separator,
                        ui.id().with(("dock-split", group.id, next.id)),
                        egui::Sense::click_and_drag(),
                    ),
                    "Drag to resize · double-click to swap priority",
                    None,
                );
                // Double-click swaps priority between the two groups (R-054).
                if response.double_clicked()
                    && let Some(&panel) = (if group.weight < next.weight {
                        group
                    } else {
                        next
                    })
                    .tabs
                    .first()
                {
                    self.workspace.dock.give_priority(panel);
                    self.persist_workspace_requested = true;
                }
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
                let splitter_hovered = response.hovered()
                    && !self.has_modal_dialog_in(ui.ctx())
                    && !crate::panels::dialog_helpers::is_editor_popup_open_on_viewport(
                        self,
                        ui.ctx(),
                        ui.ctx().viewport_id(),
                    )
                    && ui.ctx().pointer_hover_pos().is_some_and(|pos| {
                        separator.contains(pos)
                            && ui.clip_rect().contains(pos)
                            && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
                    });
                if splitter_hovered || response.dragged() {
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
                    let (first, second) = if vertical {
                        let pair_extent = extents[index] + extents[index + 1];
                        let min_a = group_min_stacked_height(group);
                        let min_b = group_min_stacked_height(next);
                        let target_a = extents[index] + delta;
                        let target_b = pair_extent - target_a;
                        if target_a < min_a {
                            (
                                COLLAPSED_GROUP_WEIGHT,
                                (total - COLLAPSED_GROUP_WEIGHT).max(COLLAPSED_GROUP_WEIGHT),
                            )
                        } else if target_b < min_b {
                            (
                                (total - COLLAPSED_GROUP_WEIGHT).max(COLLAPSED_GROUP_WEIGHT),
                                COLLAPSED_GROUP_WEIGHT,
                            )
                        } else {
                            let share = (target_a / pair_extent.max(1.)).clamp(0.01, 0.99);
                            (total * share, total * (1. - share))
                        }
                    } else {
                        let minimum = (48. / space * weight).min(total * 0.45);
                        let first =
                            (group.weight + delta / space * weight).clamp(minimum, total - minimum);
                        (first, total - first)
                    };
                    let area = self.workspace.dock.area_mut(edge);
                    if let Some(stored) = area.groups.iter_mut().find(|g| g.id == group.id) {
                        stored.weight = first;
                    }
                    if let Some(stored) = area.groups.iter_mut().find(|g| g.id == next.id) {
                        stored.weight = second;
                    }
                    self.persist_workspace_requested = true;
                }
            }
            cursor = if has_splitter { end + SPLITTER } else { end };
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
                && !self.has_modal_dialog_in(ui.ctx())
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
        if saved.is_some() {
            if let Some((global, down)) = crate::native_screens::pointer_state() {
                if release.is_none() || down {
                    let inner = ui
                        .ctx()
                        .input(|input| input.viewport().inner_rect)
                        .unwrap_or_else(|| ui.ctx().viewport_rect());
                    pointer = Some(global / ui.ctx().zoom_factor() - inner.min.to_vec2());
                    released = !down;
                }
            } else if press.is_none() && !ui.input(|i| i.pointer.primary_down()) {
                released = true;
            }
        }
        if escape || self.has_modal_dialog_in(ui.ctx()) {
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
            let next_size = (saved.size + delta * direction).clamp(minimum, maximum);
            if (self.workspace.dock.area(edge).size - next_size).abs() > f32::EPSILON {
                self.workspace.dock.area_mut(edge).size = next_size;
                self.persist_workspace_requested = true;
            }
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
        let genuinely_hovered = response.hovered()
            && !self.has_modal_dialog_in(ui.ctx())
            && !crate::panels::dialog_helpers::is_editor_popup_open_on_viewport(
                self,
                ui.ctx(),
                ui.ctx().viewport_id(),
            )
            && ui.ctx().pointer_hover_pos().is_some_and(|pos| {
                grip.contains(pos)
                    && ui.clip_rect().contains(pos)
                    && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
            });
        let active = genuinely_hovered || response.dragged() || saved.is_some();
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
        nf::tooltip(
            response,
            if edge.vertical() {
                "Drag to change panel width"
            } else {
                "Drag to change panel height"
            },
            None,
        );
    }

    fn dock_group_contents(
        &mut self,
        parent: &mut egui::Ui,
        surface: DockSurface,
        edge: DockEdge,
        group: &DockGroup,
        rect: egui::Rect,
    ) {
        self.paint_dock_group_contents(parent, surface, group, (edge, rect), None, false);
    }

    fn paint_dock_group_contents(
        &mut self,
        parent: &mut egui::Ui,
        surface: DockSurface,
        group: &DockGroup,
        layout: (DockEdge, egui::Rect),
        preview_grab: Option<egui::Vec2>,
        collapsed: bool,
    ) {
        let (edge, rect) = layout;
        let first_shape = parent.ctx().graphics(|graphics| {
            graphics
                .get(parent.layer_id())
                .map_or(0, |list| list.next_idx().0)
        });
        // Pressing an inactive tab previews that panel during its drag. Keep
        // the persisted active tab untouched so Escape restores the group.
        let mut group = group.clone();
        if let Some(panel) = self.dragged_panel_in_group(surface, &group) {
            group.active = panel;
        }
        let group = &group;
        let header_id = |name: &str| {
            if preview_grab.is_some() {
                (format!("{name}-drag-preview"), surface, group.id)
            } else {
                (name.to_owned(), surface, group.id)
            }
        };
        // v3 `Tab`: every dock header is one `size/tab` strip of Body Strong labels padded by
        // `pad/panel`, with one tab or several, so adding or removing a tab never moves the body.
        // Horizontal docks add the panel icon; a single side panel is a title without tab chrome.
        let wide = matches!(edge, DockEdge::Top | DockEdge::Bottom);
        let single = !wide && (group.tabs.len() == 1 || collapsed);
        let header_h = neoflash_ui::metrics::panel_tab();
        let tab_style = neoflash_ui::TextStyle::BodyStrong;
        let tab_pad = neoflash_ui::tokens::space::s4();
        let header = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.right(), (rect.top() + header_h).min(rect.bottom())),
        );
        let padding = group.active.body_padding();
        let body = if collapsed {
            egui::Rect::NOTHING
        } else {
            egui::Rect::from_min_max(egui::pos2(rect.left(), header.bottom()), rect.max)
                .shrink(padding)
        };
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
        let menu_side = neoflash_ui::metrics::control();
        let menu_rect = egui::Rect::from_center_size(
            egui::pos2(
                header.right() - neoflash_ui::tokens::space::s1() - menu_side / 2.,
                header.center().y,
            ),
            egui::vec2(menu_side, menu_side.min(header.height())),
        );
        let expand_rect = collapsed.then(|| {
            egui::Rect::from_center_size(
                egui::pos2(
                    menu_rect.left() - neoflash_ui::tokens::space::s1() - menu_side / 2.,
                    header.center().y,
                ),
                egui::vec2(menu_side, menu_side.min(header.height())),
            )
        });
        let trailing_left = expand_rect.map_or(menu_rect.left(), |r| r.left());
        // Tabs start at the header edge; the tab's own `pad/panel` puts the first label on the
        // panel's content axis.
        let tabs_rect = egui::Rect::from_min_max(
            header.min,
            egui::pos2(
                (trailing_left - neoflash_ui::tokens::space::s1()).max(header.left()),
                header.bottom(),
            ),
        );
        let mut header_ui = parent.new_child(
            egui::UiBuilder::new()
                .id(header_id("dock-tabs"))
                .max_rect(tabs_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        header_ui.set_clip_rect(parent.clip_rect().intersect(tabs_rect));
        if self.has_modal_dialog_in(&ctx) {
            header_ui.disable_semantic();
        }
        let nf = neoflash_ui::palette(&ctx);
        parent.painter().rect_filled(header, 0., nf.bg_panel);
        parent.painter().hline(
            header.x_range(),
            header.bottom() - 0.5,
            egui::Stroke::new(1.0_f32, nf.border_subtle),
        );
        egui::ScrollArea::horizontal()
            .id_salt(header_id("dock-tab-scroll"))
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
                            if collapsed && *panel != group.active {
                                continue;
                            }
                            let active = group.active == *panel;
                            let style = tab_style;
                            let label_w = ui.fonts_mut(|f| {
                                f.layout_no_wrap(
                                    panel.title().into(),
                                    neoflash_ui::typography::font(style),
                                    nf.text_primary,
                                )
                                .size()
                                .x
                            });
                            let pad = tab_pad;
                            let icon_w = if wide {
                                neoflash_ui::metrics::ICON_UI + neoflash_ui::tokens::space::s2()
                            } else {
                                0.
                            };
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
                                // A lone title is never painted clipped (the narrow Tools dock):
                                // without room for all of it the header is a plain drag strip
                                // that keeps the tab's tooltip and menu.
                                if !single || x + label_w <= tabs_rect.right() + 0.5 {
                                    ui.painter().text(
                                        egui::pos2(x, tab_rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        panel.title(),
                                        neoflash_ui::typography::font(style),
                                        fg,
                                    );
                                }
                                if active && !single {
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
                            let hint = if collapsed {
                                format!("Expand {}", panel.title())
                            } else {
                                panel.title().to_owned()
                            };
                            let response = nf::tooltip(response, &hint, None);
                            if response.clicked() {
                                if collapsed {
                                    self.give_dock_priority(*panel);
                                } else {
                                    self.show_panel(*panel);
                                }
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
        let blank = egui::Rect::from_min_max(
            egui::pos2(blank_start, header.top()),
            egui::pos2(trailing_left.max(blank_start), header.bottom()),
        );
        if blank.width() > 0.5 {
            let mut blank_ui = parent.new_child(
                egui::UiBuilder::new()
                    .id(header_id("dock-header-blank"))
                    .max_rect(blank),
            );
            blank_ui.set_clip_rect(parent.clip_rect().intersect(blank));
            if self.has_modal_dialog_in(&ctx) {
                blank_ui.disable_semantic();
            }
            let response = blank_ui.interact(
                blank,
                blank_ui.id().with("header-grab"),
                egui::Sense::click_and_drag(),
            );
            if collapsed && response.clicked() && !self.panel_drag_moved() {
                self.give_dock_priority(group.active);
            }
            self.panel_group_drag_handle_anchored(
                &blank_ui,
                group.active,
                (surface, edge, group.id),
                header.min,
                true,
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
                        neoflash_ui::components::menu_separator(ui);
                    }
                    self.dock_header_commands(ui, group.active, surface, &clipboard_target)
                })
            });
        }
        // A small grip stays reachable when the tab strip is full. The blank
        // header uses the same gesture; tab labels always move a single panel.
        // The grip covers only the first tab's leading padding, never its label.
        let grip = egui::Rect::from_min_max(
            header.min,
            egui::pos2(header.left() + tab_pad, header.bottom()),
        );
        let mut grip_ui = parent.new_child(
            egui::UiBuilder::new()
                .id(header_id("dock-group-grip"))
                .max_rect(grip),
        );
        grip_ui.set_clip_rect(parent.clip_rect().intersect(grip));
        if self.has_modal_dialog_in(&ctx) {
            grip_ui.disable_semantic();
        }
        let response = grip_ui.interact(
            grip,
            grip_ui.id().with("grip"),
            egui::Sense::click_and_drag(),
        );
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
        self.panel_group_drag_handle(&grip_ui, group.active, (surface, edge, group.id), &response);
        nf::tooltip(response, "Drag panel group", None)
            .context_menu(|ui| self.dock_header_menu(ui, group.active, surface, &clipboard_target));
        if let Some(expand_rect) = expand_rect {
            let mut expand_ui = parent.new_child(
                egui::UiBuilder::new()
                    .id(header_id("dock-group-expand"))
                    .max_rect(expand_rect),
            );
            expand_ui.set_clip_rect(parent.clip_rect().intersect(expand_rect));
            if self.has_modal_dialog_in(&ctx) {
                expand_ui.disable_semantic();
            }
            let tip = format!("Expand {}", group.active.title());
            let expand = expand_ui
                .add(nf::IconButton::new(neoflash_ui::icons::i16::CHEVUP, &tip).side(menu_side));
            if expand.clicked() {
                self.give_dock_priority(group.active);
            }
        }
        let mut menu_ui = parent.new_child(
            egui::UiBuilder::new()
                .id(header_id("dock-group-menu"))
                .max_rect(menu_rect),
        );
        menu_ui.set_clip_rect(parent.clip_rect().intersect(menu_rect));
        if self.has_modal_dialog_in(&ctx) {
            menu_ui.disable_semantic();
        }
        let more = menu_ui.add(
            nf::IconButton::new(neoflash_ui::icons::i16::MORE, "Panel commands and tabs")
                .side(menu_side),
        );
        self.panel_group_drag_handle_anchored(
            &menu_ui,
            group.active,
            (surface, edge, group.id),
            header.min,
            false,
            &more,
        );
        if !self.panel_drag_moved() {
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
                        neoflash_ui::components::menu_separator(ui);
                    }
                    self.dock_header_commands(ui, group.active, surface, &clipboard_target)
                })
            });
        }
        parent.painter().hline(
            header.x_range(),
            header.bottom() - 0.5,
            egui::Stroke::new(1.0_f32, nf.border_subtle),
        );
        if preview_grab.is_none() {
            self.detached.dock_groups.push(target);
            if let Some((floating_group, size, grab_offset)) =
                self.compact_panel_drag_layout(surface, edge, group, rect, zoom)
            {
                // Lay out the body once at its floating size, outside the
                // viewport, then replay its paint output at the pointer. The
                // original header remains the drop target; no widgets run twice.
                let preview_rect = egui::Rect::from_min_size(ctx.viewport_rect().max, size);
                let layer = egui::LayerId::new(
                    egui::Order::Middle,
                    egui::Id::new(("panel-drag-layout", surface, group.id)),
                );
                let mut preview_ui = egui::Ui::new(
                    ctx.clone(),
                    layer.id,
                    egui::UiBuilder::new()
                        .layer_id(layer)
                        .max_rect(preview_rect),
                );
                self.paint_dock_group_contents(
                    &mut preview_ui,
                    surface,
                    &floating_group,
                    (DockEdge::Left, preview_rect),
                    Some(grab_offset),
                    false,
                );
                ctx.graphics_mut(|graphics| *graphics.entry(layer) = Default::default());
                return;
            }
        }
        // A tab drag may have moved this group during the header callback.
        // Its new owner renders it on the next frame, never a second copy here.
        let active = self.dragged_panel_in_group(surface, group).or_else(|| {
            self.current_dock_group(surface, edge, group.id)
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
                })
        });
        if !collapsed
            && let Some(panel) = active
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
        self.remember_panel_drag_preview(parent, surface, group, rect, (first_shape, preview_grab));
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
        self.sync_command_modifier_from_viewport(ctx, surface);
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
            self.dock_node_contents(
                ui,
                (surface, DockEdge::Left),
                &root,
                ui.max_rect(),
                &mut Vec::new(),
            );
        }
    }

    /// A one-group native window uses the locked `FloatingPanelHeader` as its only
    /// chrome. Render just the selected panel body here while retaining the same
    /// dock/drop target and active-tab semantics.
    pub(crate) fn floating_single_group_body_contents(
        &mut self,
        ui: &mut egui::Ui,
        window: PanelId,
        group: &DockGroup,
        header: egui::Rect,
        header_tabs: &[(PanelId, egui::Rect)],
    ) {
        let surface = DockSurface::Floating(window);
        let full = egui::Rect::from_min_max(header.min, ui.max_rect().max);
        self.register_dock_viewport(ui.ctx(), surface, full);
        self.detached.dock_groups.retain(|g| g.surface != surface);
        let mut visible = group.clone();
        visible
            .tabs
            .retain(|panel| self.workspace.is_panel_visible(*panel));
        if visible.tabs.is_empty() {
            return;
        }
        if !visible.tabs.contains(&visible.active) {
            visible.active = visible.tabs[0];
        }
        let ctx = ui.ctx().clone();
        let zoom = ctx.zoom_factor();
        let inner = ctx
            .input(|input| input.viewport().inner_rect)
            .unwrap_or_else(|| ctx.viewport_rect());
        let global = |rect: egui::Rect| rect.translate(inner.min.to_vec2()) * zoom;
        self.detached.dock_groups.push(DockGroupTarget {
            surface,
            edge: DockEdge::Left,
            group: visible.id,
            rect: global(full),
            header: global(header),
            tabs: header_tabs
                .iter()
                .map(|(panel, rect)| (*panel, global(*rect)))
                .collect(),
        });
        let body = ui.max_rect().shrink(visible.active.body_padding());
        if body.is_positive() {
            let mut content = ui.new_child(
                egui::UiBuilder::new()
                    .id(("floating-single-body", window, visible.id, visible.active))
                    .max_rect(body),
            );
            content.set_clip_rect(ui.clip_rect().intersect(body));
            content.set_min_size(body.size());
            self.dock_panel_contents(&mut content, visible.active);
        }
    }

    fn main_node_limits(&self, node: &FloatingNode, axis: SplitAxis) -> Option<(f32, f32)> {
        nested_limits(node, axis, |group| {
            group
                .tabs
                .iter()
                .copied()
                .filter(|panel| self.workspace.is_panel_visible(*panel))
                .map(|p| match axis {
                    SplitAxis::Horizontal => (
                        crate::workspace::minimum_window_size(p)[0],
                        crate::workspace::maximum_window_size(p)[0],
                    ),
                    SplitAxis::Vertical => (
                        p.min_stacked_height(),
                        crate::workspace::maximum_window_size(p)[1],
                    ),
                })
                .reduce(|a, b| (a.0.max(b.0), a.1.max(b.1)))
        })
    }

    fn dock_node_visible(&self, node: &FloatingNode) -> bool {
        match node {
            FloatingNode::Group(group) => group
                .tabs
                .iter()
                .any(|p| self.workspace.is_panel_visible(*p)),
            FloatingNode::Split { first, second, .. } => {
                self.dock_node_visible(first) || self.dock_node_visible(second)
            }
        }
    }

    fn dock_node_contents(
        &mut self,
        ui: &mut egui::Ui,
        location: (DockSurface, DockEdge),
        node: &FloatingNode,
        rect: egui::Rect,
        path: &mut Vec<bool>,
    ) {
        if !rect.is_positive() {
            return;
        }
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
                self.dock_group_contents(ui, location.0, location.1, &visible, rect);
            }
            FloatingNode::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let minimum = |node: &FloatingNode| match location.0 {
                    DockSurface::Main => self.main_node_limits(node, *axis).map(|bounds| bounds.0),
                    DockSurface::Floating(_) => self.dock_node_visible(node).then_some(48.),
                };
                let (first_min, second_min) = (minimum(first), minimum(second));
                let (Some(first_min), Some(second_min)) = (first_min, second_min) else {
                    if first_min.is_some() || second_min.is_some() {
                        path.push(second_min.is_some());
                        self.dock_node_contents(
                            ui,
                            location,
                            if first_min.is_some() { first } else { second },
                            rect,
                            path,
                        );
                        path.pop();
                    }
                    return;
                };
                let vertical = *axis == SplitAxis::Vertical;
                let extent = if vertical {
                    rect.height()
                } else {
                    rect.width()
                };
                // A deep split in a very narrow window can be smaller than the grip.
                // Keep both child rectangles inside their parent even then.
                let grip = SPLITTER.min((extent - 2.).max(0.));
                let space = extent - grip;
                let (low, high) = match location.0 {
                    DockSurface::Main if first_min + second_min <= space => {
                        let low = first_min / space;
                        (low, ((space - second_min) / space).max(low))
                    }
                    // In a smaller window both minima cannot fit. Honor the stored share
                    // temporarily; the next larger frame recovers the preferred layout.
                    DockSurface::Main => (0.05, 0.95),
                    DockSurface::Floating(_) => {
                        let low = (first_min / space).clamp(0.05, 0.45);
                        (low, 1. - low)
                    }
                };
                let ratio = ratio.clamp(low, high);
                let coordinate = if vertical { rect.top() } else { rect.left() } + space * ratio;
                let mut first_rect = rect;
                let mut second_rect = rect;
                let mut divider = rect;
                if vertical {
                    first_rect.max.y = coordinate;
                    second_rect.min.y = coordinate + grip;
                    divider.min.y = coordinate;
                    divider.max.y = coordinate + grip;
                } else {
                    first_rect.max.x = coordinate;
                    second_rect.min.x = coordinate + grip;
                    divider.min.x = coordinate;
                    divider.max.x = coordinate + grip;
                }
                path.push(false);
                self.dock_node_contents(ui, location, first, first_rect, path);
                path.pop();
                path.push(true);
                self.dock_node_contents(ui, location, second, second_rect, path);
                path.pop();
                let response = match location.0 {
                    DockSurface::Main => {
                        let response = nf::tooltip(
                            ui.interact(
                                divider,
                                ui.id().with(("dock-divider", location.1, &*path)),
                                egui::Sense::click_and_drag(),
                            ),
                            "Drag to resize · double-click to swap priority",
                            None,
                        );
                        ui.painter().rect_filled(
                            divider,
                            0.,
                            neoflash_ui::palette(ui.ctx()).bg_panel,
                        );
                        nf::surface::splitter(
                            ui.painter(),
                            ui.ctx(),
                            divider,
                            !vertical,
                            &response,
                        );
                        response
                    }
                    DockSurface::Floating(window) => {
                        let response = ui.interact(
                            divider,
                            egui::Id::new(("floating-divider", window, &*path)),
                            egui::Sense::drag(),
                        );
                        ui.painter().rect_filled(
                            divider,
                            0.,
                            ui.visuals().widgets.noninteractive.bg_stroke.color,
                        );
                        response
                    }
                };
                let divider_hovered = response.hovered()
                    && !self.has_modal_dialog_in(ui.ctx())
                    && !crate::panels::dialog_helpers::is_editor_popup_open_on_viewport(
                        self,
                        ui.ctx(),
                        ui.ctx().viewport_id(),
                    )
                    && !(ui.ctx().viewport_id() != egui::ViewportId::ROOT
                        && self.detached.context_menu_open)
                    && ui.ctx().pointer_hover_pos().is_some_and(|pos| {
                        divider.contains(pos)
                            && ui.clip_rect().contains(pos)
                            && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
                    });
                if divider_hovered || response.dragged() {
                    ui.ctx().set_cursor_icon(if vertical {
                        egui::CursorIcon::ResizeVertical
                    } else {
                        egui::CursorIcon::ResizeHorizontal
                    });
                }
                let next_ratio = if location.0 == DockSurface::Main && response.double_clicked() {
                    let priority = crate::workspace::DockLayout::PRIORITY_WEIGHT;
                    Some(if ratio > 0.5 { 1. - priority } else { priority })
                } else if response.dragged() {
                    let delta = ui.input(|i| {
                        if vertical {
                            i.pointer.delta().y
                        } else {
                            i.pointer.delta().x
                        }
                    });
                    Some((ratio + delta / space).clamp(low, high))
                } else {
                    None
                };
                if let Some(value) = next_ratio {
                    match location.0 {
                        DockSurface::Main => self
                            .workspace
                            .dock
                            .area_mut(location.1)
                            .set_ratio(path, value),
                        DockSurface::Floating(id) => {
                            if let Some(window) =
                                self.workspace.floating.iter_mut().find(|w| w.id == id)
                            {
                                window.set_ratio(path, value);
                            }
                        }
                    }
                    self.persist_workspace_requested = true;
                }
            }
        }
    }

    pub(crate) fn has_active_dock_or_window_drag(
        &self,
        ctx: &egui::Context,
        exclude_widget: Option<egui::Id>,
    ) -> bool {
        if self.has_active_panel_or_window_drag()
            || ctx
                .dragged_id()
                .is_some_and(|dragged| Some(dragged) != exclude_widget)
        {
            return true;
        }
        [
            DockEdge::Top,
            DockEdge::Bottom,
            DockEdge::Left,
            DockEdge::Right,
        ]
        .into_iter()
        .any(|edge| {
            let state_id = egui::Id::new(("dock-area-resize", edge)).with("origin");
            ctx.data(|data| data.get_temp::<AreaResize>(state_id).is_some())
        })
    }

    pub(crate) fn dock_panel_contents(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        if self.has_modal_dialog_in(ui.ctx()) {
            ui.disable_semantic();
        }
        self.detached.content_header = Some(panel);
        match panel {
            PanelId::Tools => self.tools_contents(ui),
            PanelId::Brushes => self.brushes_contents(ui),
            PanelId::Timeline => self.timeline_contents(ui),
            PanelId::Library => self.library_contents(ui),
            PanelId::Align => self.align_panel_ui(ui),
            PanelId::Transform => self.transform_panel_ui(ui),
            PanelId::Color => self.color_panel_ui(ui),
            PanelId::Ease => self.ease_panel_ui(ui),
            PanelId::Scene => self.scene_panel_ui(ui),
            PanelId::MotionEditor => self.motion_editor_panel_ui(ui),
            PanelId::History => self.history_panel_ui(ui),
            PanelId::Swatches => self.swatches_panel_ui(ui),
            PanelId::Info => self.info_panel_ui(ui),
            PanelId::Project => self.project_panel_ui(ui),
            PanelId::Filters => self.filters_panel_ui(ui),
            PanelId::Debugger => {
                let _ = self.debugger_contents(ui);
            }
            PanelId::CompilerErrors => self.compiler_errors_panel_ui(ui),
            PanelId::CodeSnippets => self.code_snippets_panel_ui(ui),
            PanelId::MovieExplorer => self.movie_explorer_panel_ui(ui),
            PanelId::Properties | PanelId::Actions | PanelId::Accessibility | PanelId::Output => {
                self.bottom_contents(ui, panel)
            }
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
        neoflash_ui::components::menu_separator(ui);
        if crate::ui_icons::menu_item(ui, "Detach", "").clicked() {
            self.set_panel_placement(&ctx, panel, PanelPlacement::Detached);
            ui.close();
        }
        if crate::ui_icons::menu_item(ui, "Hide", "").clicked() {
            self.set_panel_visible(panel, false);
            ui.close();
        }
    }

    pub(crate) fn dock_header_menu(
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

    pub(crate) fn dock_header_commands(
        &mut self,
        ui: &mut egui::Ui,
        panel: PanelId,
        surface: DockSurface,
        target: &crate::clipboard::ClipboardTarget,
    ) {
        if panel == PanelId::Color {
            self.color_panel_menu(ui);
            neoflash_ui::components::menu_separator(ui);
        }
        if let DockSurface::Floating(window) = surface {
            self.floating_panel_commands(ui, window, target);
        }
        self.panel_dock_menu(ui, panel);
    }

    pub(crate) fn dock_surface_at(&self, pointer: egui::Pos2) -> Option<DockViewportTarget> {
        let excluded_window = self.dragged_whole_floating_window();
        let excluded_title =
            excluded_window.and_then(|window_id| self.floating_window_title(window_id));
        let zoom = self
            .detached
            .dock_viewports
            .first()
            .map_or(1.0, |v| v.zoom.max(0.5));
        let mut excluded_rects = Vec::with_capacity(3);
        if let Some(window_id) = excluded_window {
            if let Some(viewport_rect) = self
                .detached
                .dock_viewports
                .iter()
                .find(|target| target.surface == DockSurface::Floating(window_id))
                .map(|target| target.inner)
            {
                excluded_rects.push(viewport_rect);
            }
            if let Some(saved) = self.workspace.detached_geometry(window_id) {
                excluded_rects.push(egui::Rect::from_min_size(
                    egui::pos2(saved.outer_position[0], saved.outer_position[1]),
                    egui::vec2(saved.inner_size[0], saved.inner_size[1]),
                ));
            }
            if let Some(live) = self.live_dragged_floating_window_rect(window_id, zoom) {
                excluded_rects.push(live);
            }
        }
        let other_visible_floating = self.workspace.floating.iter().any(|window| {
            Some(window.id) != excluded_window
                && window
                    .panels()
                    .iter()
                    .any(|panel| self.workspace.is_panel_visible(*panel))
        });
        let exclude_all_floating = excluded_window.is_some() && !other_visible_floating;
        let candidates: Vec<_> =
            self.detached
                .dock_viewports
                .iter()
                .copied()
                .filter(|target| {
                    target.inner.contains(pointer)
                        && match target.surface {
                            DockSurface::Main => true,
                            DockSurface::Floating(id) => {
                                excluded_window != Some(id)
                                    && self.workspace.floating.iter().any(|window| {
                                        window.id == id
                                            && window.panels().iter().any(|panel| {
                                                self.workspace.is_panel_visible(*panel)
                                            })
                                    })
                            }
                        }
                })
                .collect();
        match crate::native_screens::front_dock_window_at(
            pointer,
            excluded_title.as_deref(),
            &excluded_rects,
            exclude_all_floating,
        ) {
            Some(Some(crate::native_screens::NativeDockWindow::Main(_))) => candidates
                .into_iter()
                .find(|target| target.surface == DockSurface::Main),
            Some(Some(crate::native_screens::NativeDockWindow::Floating { rect, title })) => {
                candidates
                    .into_iter()
                    .filter(|target| match target.surface {
                        DockSurface::Main => false,
                        DockSurface::Floating(id) => {
                            (title.is_some() && self.floating_window_title(id) == title)
                                || (target.inner.min.distance(rect.min) < 16.
                                    && (target.inner.width() - rect.width()).abs() < 16.)
                        }
                    })
                    .max_by_key(|target| target.order)
            }
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
        if self.panel_drag_suppresses_dock() {
            return None;
        }
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
                    inner.min + egui::vec2(0., neoflash_ui::metrics::top_bar() * zoom),
                    inner.max,
                ),
                zoom,
                order: 0,
            })
        })?;
        let pointer = if view.content.contains(pointer)
            || self
                .detached
                .dock_groups
                .iter()
                .any(|g| g.surface == view.surface && g.header.contains(pointer))
        {
            pointer
        } else if view.surface == DockSurface::Main && view.inner.contains(pointer) {
            let side_band = (view.content.width() * 0.03)
                .clamp(32. * view.zoom, 44. * view.zoom)
                .min(view.content.width() * 0.15);
            if pointer.y < view.content.top()
                && (pointer.x <= view.content.left() + side_band
                    || pointer.x >= view.content.right() - side_band)
            {
                egui::pos2(
                    pointer.x.clamp(view.content.left(), view.content.right()),
                    (view.content.top() + 96. * view.zoom).min(view.content.center().y),
                )
            } else if pointer.y > view.content.bottom() {
                egui::pos2(
                    pointer.x.clamp(view.content.left(), view.content.right()),
                    (view.content.bottom() - 1.).max(view.content.top()),
                )
            } else {
                return None;
            }
        } else {
            return None;
        };
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
        let edge_zones = edge_acceptance_targets(view.content, view.zoom);
        let closest_edge = edge_zones
            .into_iter()
            .filter(|(_, rect)| rect.contains(pointer))
            .min_by(|(a, _), (b, _)| {
                let distance = |edge| match edge {
                    DockEdge::Top => pointer.y - view.content.top(),
                    DockEdge::Bottom => view.content.bottom() - pointer.y,
                    DockEdge::Left => pointer.x - view.content.left(),
                    DockEdge::Right => view.content.right() - pointer.x,
                };
                distance(*a).total_cmp(&distance(*b))
            })
            .map(|(edge, _)| edge);
        if let Some(edge) = closest_edge {
            let rect = edge_targets(view.content, view.zoom)
                .into_iter()
                .find_map(|(candidate, rect)| (candidate == edge).then_some(rect))
                .expect("all dock edges have a visible target");
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
        let target = groups.iter().find(|g| g.rect.contains(pointer))?;
        // Headers insert tabs; the outer window bands dock an entire edge. Calculate a
        // main leaf's four local split zones within its remaining body so even a narrow
        // Tools rail or a short group has reachable targets on each exposed side.
        let body = match view.surface {
            DockSurface::Main => {
                let [(_, top), (_, bottom), (_, left), (_, right)] = edge_zones;
                target.rect.intersect(egui::Rect::from_min_max(
                    egui::pos2(left.right(), top.bottom().max(target.header.bottom())),
                    egui::pos2(right.left(), bottom.top()),
                ))
            }
            DockSurface::Floating(_) => target.rect,
        };
        let x = (pointer.x - body.left()) / body.width().max(1.);
        let y = (pointer.y - body.top()) / body.height().max(1.);
        let split = [
            (DockEdge::Left, x),
            (DockEdge::Right, 1. - x),
            (DockEdge::Top, y),
            (DockEdge::Bottom, 1. - y),
        ]
        .into_iter()
        .filter(|(_, distance)| *distance < 0.24)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(edge, _)| edge);
        if let Some(edge) = split {
            let mut rect = target.rect;
            match edge {
                DockEdge::Top => rect.max.y = rect.center().y,
                DockEdge::Bottom => rect.min.y = rect.center().y,
                DockEdge::Left => rect.max.x = rect.center().x,
                DockEdge::Right => rect.min.x = rect.center().x,
            }
            let destination = match view.surface {
                DockSurface::Main => DockDestination::SplitGroup {
                    area: target.edge,
                    group: target.group,
                    side: edge,
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
