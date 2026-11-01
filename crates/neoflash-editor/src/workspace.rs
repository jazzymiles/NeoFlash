//! Application preferences, deliberately separate from Project and its history.
use crate::{app::BottomTab, theme::ThemeMode};
use serde::{Deserialize, Serialize};

/// v2 = NF2 layout (bottom dock with Timeline · Actions · Output · Accessibility). Layouts saved
/// under v1 are not migrated: the NF2 default replaces them once.
pub const PREFS_KEY: &str = "neoflash.workspace.v2";
pub const MAX_PREFS_BYTES: usize = 16 * 1024;
const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelId {
    Timeline,
    Tools,
    Library,
    Properties,
    Actions,
    Accessibility,
    Output,
    Align,
    Transform,
    Color,
}
impl PanelId {
    pub fn title(self) -> &'static str {
        match self {
            Self::Timeline => "Timeline",
            Self::Tools => "Tools",
            Self::Library => "Library",
            Self::Properties => "Properties",
            Self::Actions => "Actions",
            Self::Accessibility => "Accessibility",
            Self::Output => "Output",
            Self::Align => "Align",
            Self::Transform => "Transform",
            Self::Color => "Color",
        }
    }
    /// Body padding of the docked panel, from the Figma outlines: inspector 16, Library 8,
    /// floating-style utility panels 12; Tools, Timeline and Actions draw edge-to-edge bars.
    pub fn body_padding(self) -> f32 {
        use neoflash_ui::tokens::space;
        match self {
            // Properties (16) and Accessibility (12) pad inside their scroll areas, see panels/actions.rs.
            Self::Tools
            | Self::Timeline
            | Self::Actions
            | Self::Properties
            | Self::Accessibility => 0.,
            Self::Library => space::S2,
            _ => space::S3,
        }
    }
    /// Smallest height when stacked in a side column: the panel header plus its first control row
    /// (Figma editor-main keeps Library at 100 pt: header + search). Users drag the splitter for more.
    pub fn min_stacked_height(self) -> f32 {
        match self {
            Self::Library => 100.,
            _ => 120.,
        }
    }
    /// NF2 icon shown in dock tabs.
    pub fn icon(self) -> neoflash_ui::Icon {
        use neoflash_ui::icons::i16;
        match self {
            Self::Timeline => i16::FILM,
            Self::Tools => i16::SELECT,
            Self::Library => i16::SYMBOL,
            Self::Properties => i16::SETTINGS,
            Self::Actions => i16::CODE,
            Self::Accessibility => i16::ACCESS,
            Self::Output => i16::TERMINAL,
            Self::Align => i16::ALIGN_LEFT,
            Self::Transform => i16::TRANSFORM,
            Self::Color => i16::COLORWHEEL,
        }
    }
    pub const DETACHABLE: [Self; 10] = [
        Self::Tools,
        Self::Timeline,
        Self::Library,
        Self::Properties,
        Self::Actions,
        Self::Accessibility,
        Self::Output,
        Self::Align,
        Self::Transform,
        Self::Color,
    ];
    pub fn detachable(self) -> bool {
        Self::DETACHABLE.contains(&self)
    }
    /// Stable identifiers are not serialized egui memory or document IDs.
    pub fn viewport_key(self) -> Option<&'static str> {
        match self {
            Self::Tools => Some("neoflash.panel.tools"),
            Self::Timeline => Some("neoflash.panel.timeline"),
            Self::Actions => Some("neoflash.panel.actions"),
            Self::Library => Some("neoflash.panel.library"),
            Self::Properties => Some("neoflash.panel.properties"),
            Self::Accessibility => Some("neoflash.panel.accessibility"),
            Self::Output => Some("neoflash.panel.output"),
            Self::Align => Some("neoflash.panel.align"),
            Self::Transform => Some("neoflash.panel.transform"),
            Self::Color => Some("neoflash.panel.color"),
        }
    }
    #[cfg(test)]
    pub const ALL: [Self; 10] = [
        Self::Timeline,
        Self::Tools,
        Self::Library,
        Self::Properties,
        Self::Actions,
        Self::Accessibility,
        Self::Output,
        Self::Align,
        Self::Transform,
        Self::Color,
    ];
    pub const BOTTOM: [Self; 4] = [
        Self::Properties,
        Self::Actions,
        Self::Accessibility,
        Self::Output,
    ];
    pub fn bottom_tab(self) -> Option<BottomTab> {
        match self {
            Self::Properties => Some(BottomTab::Properties),
            Self::Actions => Some(BottomTab::Actions),
            Self::Accessibility => Some(BottomTab::Accessibility),
            Self::Output => Some(BottomTab::Output),
            _ => None,
        }
    }
    pub fn from_bottom_tab(tab: BottomTab) -> Self {
        match tab {
            BottomTab::Properties => Self::Properties,
            BottomTab::Actions => Self::Actions,
            BottomTab::Accessibility => Self::Accessibility,
            BottomTab::Output => Self::Output,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelVisibility {
    pub timeline: bool,
    pub tools: bool,
    pub library: bool,
    pub properties: bool,
    pub actions: bool,
    pub accessibility: bool,
    pub output: bool,
    pub align: bool,
    pub transform: bool,
    pub color: bool,
}
impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            timeline: true,
            tools: true,
            library: true,
            properties: true,
            actions: true,
            accessibility: true,
            output: true,
            align: false,
            transform: false,
            color: false,
        }
    }
}
impl PanelVisibility {
    fn slot(&mut self, panel: PanelId) -> &mut bool {
        match panel {
            PanelId::Timeline => &mut self.timeline,
            PanelId::Tools => &mut self.tools,
            PanelId::Library => &mut self.library,
            PanelId::Properties => &mut self.properties,
            PanelId::Actions => &mut self.actions,
            PanelId::Accessibility => &mut self.accessibility,
            PanelId::Output => &mut self.output,
            PanelId::Align => &mut self.align,
            PanelId::Transform => &mut self.transform,
            PanelId::Color => &mut self.color,
        }
    }
    pub fn is_visible(self, panel: PanelId) -> bool {
        match panel {
            PanelId::Timeline => self.timeline,
            PanelId::Tools => self.tools,
            PanelId::Library => self.library,
            PanelId::Properties => self.properties,
            PanelId::Actions => self.actions,
            PanelId::Accessibility => self.accessibility,
            PanelId::Output => self.output,
            PanelId::Align => self.align,
            PanelId::Transform => self.transform,
            PanelId::Color => self.color,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelSizes {
    pub timeline_height: f32,
    pub bottom_height: f32,
    pub library_width: f32,
    pub layer_column_width: f32,
}
impl Default for PanelSizes {
    fn default() -> Self {
        Self {
            timeline_height: crate::theme::TIMELINE_HEIGHT,
            bottom_height: 190.,
            library_width: 280.,
            layer_column_width: neoflash_ui::metrics::LAYER_COLUMN,
        }
    }
}
impl PanelSizes {
    /// Stored preferences have static safety limits. Rendering additionally
    /// clamps these against the current viewport and space reserved for Stage.
    fn sanitize(&mut self) {
        let default = Self::default();
        let size = |value: f32, fallback: f32, min: f32, max: f32| {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                fallback
            }
        };
        self.timeline_height = size(
            self.timeline_height,
            default.timeline_height,
            crate::theme::TIMELINE_MIN_HEIGHT,
            380.,
        );
        self.bottom_height = size(self.bottom_height, default.bottom_height, 145., 420.);
        self.library_width = size(self.library_width, default.library_width, 248., 360.);
        self.layer_column_width = size(
            self.layer_column_width,
            default.layer_column_width,
            140.,
            320.,
        );
    }
}

/// Window placement belongs to the workspace, never to the authored Project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelPlacement {
    #[default]
    Docked,
    Detached,
}

/// A workspace is a central Stage surrounded by four independently sized
/// areas. Each area contains split groups; each group contains ordered tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockEdge {
    Top,
    Bottom,
    Left,
    Right,
}
impl DockEdge {
    pub const ALL: [Self; 4] = [Self::Top, Self::Bottom, Self::Left, Self::Right];
    pub fn vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
    pub fn panel_id(self) -> &'static str {
        match self {
            Self::Top => "dock-edge-top",
            Self::Bottom => "dock-edge-bottom",
            Self::Left => "dock-edge-left",
            Self::Right => "dock-edge-right",
        }
    }
    pub fn default_for(panel: PanelId) -> Self {
        match panel {
            PanelId::Timeline => Self::Top,
            PanelId::Tools => Self::Left,
            PanelId::Library
            | PanelId::Properties
            | PanelId::Align
            | PanelId::Transform
            | PanelId::Color => Self::Right,
            _ => Self::Bottom,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockLocation {
    pub edge: DockEdge,
    pub group: u32,
    pub index: usize,
}

/// Drop positions are transient; only the resulting layout is persisted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockDestination {
    Edge(DockEdge),
    Tabs(DockLocation),
    Split {
        edge: DockEdge,
        group: u32,
        before: bool,
    },
    FloatingTabs {
        window: PanelId,
        group: u32,
        index: usize,
    },
    FloatingSplit {
        window: PanelId,
        group: Option<u32>,
        edge: DockEdge,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockSurface {
    Main,
    Floating(PanelId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

/// Floating windows use the same tab groups as the main grid, with a small
/// split tree so every side can accept a panel without reserving an empty Stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "content", rename_all = "snake_case")]
pub enum FloatingNode {
    Group(DockGroup),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<Self>,
        second: Box<Self>,
    },
}
impl FloatingNode {
    pub fn panels(&self, out: &mut Vec<PanelId>) {
        match self {
            Self::Group(group) => out.extend(&group.tabs),
            Self::Split { first, second, .. } => {
                first.panels(out);
                second.panels(out);
            }
        }
    }
    pub fn group(&self, id: u32) -> Option<&DockGroup> {
        match self {
            Self::Group(group) => (group.id == id).then_some(group),
            Self::Split { first, second, .. } => first.group(id).or_else(|| second.group(id)),
        }
    }
    pub fn group_mut(&mut self, id: u32) -> Option<&mut DockGroup> {
        match self {
            Self::Group(group) => (group.id == id).then_some(group),
            Self::Split { first, second, .. } => {
                first.group_mut(id).or_else(|| second.group_mut(id))
            }
        }
    }
    fn location(&self, panel: PanelId) -> Option<(u32, usize)> {
        match self {
            Self::Group(group) => group
                .tabs
                .iter()
                .position(|p| *p == panel)
                .map(|index| (group.id, index)),
            Self::Split { first, second, .. } => {
                first.location(panel).or_else(|| second.location(panel))
            }
        }
    }
    fn remove(self, panel: PanelId, keep: Option<u32>) -> Option<Self> {
        match self {
            Self::Group(mut group) => {
                group.tabs.retain(|p| *p != panel);
                if !group.tabs.contains(&group.active)
                    && let Some(first) = group.tabs.first()
                {
                    group.active = *first;
                }
                (!group.tabs.is_empty() || keep == Some(group.id)).then_some(Self::Group(group))
            }
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => Self::join(
                axis,
                ratio,
                first.remove(panel, keep),
                second.remove(panel, keep),
            ),
        }
    }
    fn join(
        axis: SplitAxis,
        ratio: f32,
        first: Option<Self>,
        second: Option<Self>,
    ) -> Option<Self> {
        match (first, second) {
            (Some(first), Some(second)) => Some(Self::Split {
                axis,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
            }),
            (first, second) => first.or(second),
        }
    }
    fn split(self, target: Option<u32>, edge: DockEdge, group: DockGroup) -> Self {
        if target.is_none() || matches!(&self, Self::Group(old) if Some(old.id) == target) {
            let axis = if edge.vertical() {
                SplitAxis::Horizontal
            } else {
                SplitAxis::Vertical
            };
            let added = Box::new(Self::Group(group));
            let existing = Box::new(self);
            return if matches!(edge, DockEdge::Top | DockEdge::Left) {
                Self::Split {
                    axis,
                    ratio: 0.5,
                    first: added,
                    second: existing,
                }
            } else {
                Self::Split {
                    axis,
                    ratio: 0.5,
                    first: existing,
                    second: added,
                }
            };
        }
        match self {
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                if target.is_some_and(|id| first.group(id).is_some()) {
                    Self::Split {
                        axis,
                        ratio,
                        first: Box::new(first.split(target, edge, group)),
                        second,
                    }
                } else {
                    Self::Split {
                        axis,
                        ratio,
                        first,
                        second: Box::new(second.split(target, edge, group)),
                    }
                }
            }
            node => node.split(None, edge, group),
        }
    }
    pub fn minimum_size(&self) -> [f32; 2] {
        match self {
            Self::Group(group) => group.tabs.iter().fold([80_f32, 80_f32], |size, p| {
                let minimum = minimum_window_size(*p);
                [size[0].max(minimum[0]), size[1].max(minimum[1])]
            }),
            Self::Split {
                axis,
                first,
                second,
                ..
            } => {
                let a = first.minimum_size();
                let b = second.minimum_size();
                match axis {
                    SplitAxis::Horizontal => [a[0] + b[0] + 5., a[1].max(b[1])],
                    SplitAxis::Vertical => [a[0].max(b[0]), a[1] + b[1] + 5.],
                }
            }
        }
    }
    fn sanitize(
        self,
        seen: &mut std::collections::HashSet<PanelId>,
        ids: &mut std::collections::HashSet<u32>,
        next: &mut u32,
        detached: DetachedPanels,
    ) -> Option<Self> {
        match self {
            Self::Group(mut group) => {
                group.tabs.retain(|p| {
                    detached
                        .get(*p)
                        .is_some_and(|w| w.placement == PanelPlacement::Detached)
                        && seen.insert(*p)
                });
                if group.tabs.is_empty() {
                    return None;
                }
                if !group.tabs.contains(&group.active) {
                    group.active = group.tabs[0];
                }
                group.weight = 1.;
                if group.id == 0 || group.id > 1_000_000 || !ids.insert(group.id) {
                    while ids.contains(next) {
                        *next += 1;
                    }
                    group.id = *next;
                    ids.insert(*next);
                }
                Some(Self::Group(group))
            }
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let ratio = if ratio.is_finite() {
                    ratio.clamp(0.05, 0.95)
                } else {
                    0.5
                };
                let first = first.sanitize(seen, ids, next, detached);
                let second = second.sanitize(seen, ids, next, detached);
                Self::join(axis, ratio, first, second)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FloatingDock {
    /// Identity belongs to the window, not its selected tab. At most ten
    /// windows exist, so the panel-key pool gives stable native viewport IDs.
    pub id: PanelId,
    pub root: Option<FloatingNode>,
    pub geometry: Option<WindowGeometry>,
    next_group: u32,
}
impl FloatingDock {
    fn new(id: PanelId, panel: PanelId, geometry: Option<WindowGeometry>) -> Self {
        Self {
            id,
            root: Some(FloatingNode::Group(DockGroup {
                id: 1,
                tabs: vec![panel],
                active: panel,
                weight: 1.,
            })),
            geometry,
            next_group: 2,
        }
    }
    pub fn panels(&self) -> Vec<PanelId> {
        let mut panels = Vec::new();
        if let Some(root) = &self.root {
            root.panels(&mut panels);
        }
        panels
    }
    pub fn contains(&self, panel: PanelId) -> bool {
        self.root
            .as_ref()
            .is_some_and(|root| root.location(panel).is_some())
    }
    pub fn group(&self, id: u32) -> Option<&DockGroup> {
        self.root.as_ref()?.group(id)
    }
    pub fn select(&mut self, panel: PanelId) -> bool {
        let Some((id, _)) = self.root.as_ref().and_then(|root| root.location(panel)) else {
            return false;
        };
        let group = self
            .root
            .as_mut()
            .and_then(|root| root.group_mut(id))
            .expect("located floating group");
        let changed = group.active != panel;
        group.active = panel;
        changed
    }
    fn fresh_group(&mut self, panel: PanelId) -> DockGroup {
        let id = self.next_group;
        self.next_group += 1;
        DockGroup {
            id,
            tabs: vec![panel],
            active: panel,
            weight: 1.,
        }
    }
    pub fn set_ratio(&mut self, path: &[bool], value: f32) {
        let Some(mut node) = self.root.as_mut() else {
            return;
        };
        for side in path {
            let FloatingNode::Split { first, second, .. } = node else {
                return;
            };
            node = if *side { second } else { first };
        }
        if let FloatingNode::Split { ratio, .. } = node {
            *ratio = value.clamp(0.05, 0.95);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockGroup {
    pub id: u32,
    pub tabs: Vec<PanelId>,
    pub active: PanelId,
    /// Relative size within its area, independent of viewport dimensions.
    pub weight: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockArea {
    pub size: f32,
    pub groups: Vec<DockGroup>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DockLayout {
    pub top: DockArea,
    pub bottom: DockArea,
    pub left: DockArea,
    pub right: DockArea,
    next_group: u32,
}
impl Default for DockLayout {
    fn default() -> Self {
        let group = |id, tabs: Vec<PanelId>| DockGroup {
            id,
            active: tabs[0],
            tabs,
            weight: 1.,
        };
        // NF2 main window (§4): tools rail left, inspector right (Properties over Library),
        // one full-width bottom dock with Timeline · Actions · Output · Accessibility.
        Self {
            top: DockArea {
                size: crate::theme::TIMELINE_HEIGHT,
                groups: vec![],
            },
            bottom: DockArea {
                size: neoflash_ui::metrics::BOTTOM_DOCK,
                groups: vec![group(
                    2,
                    vec![
                        PanelId::Timeline,
                        PanelId::Actions,
                        PanelId::Output,
                        PanelId::Accessibility,
                    ],
                )],
            },
            // Base Tools layout is two columns; one column (with a "…" overflow) is the narrow variant.
            left: DockArea {
                size: neoflash_ui::metrics::TOOLS_TWO_COLUMNS + 6.,
                groups: vec![group(3, vec![PanelId::Tools])],
            },
            right: DockArea {
                // 320 pt panel + the 6 pt resize strip, so rows keep the Figma 288 pt width.
                size: neoflash_ui::metrics::INSPECTOR + 6.,
                groups: vec![
                    DockGroup {
                        weight: 0.83, // Figma editor-main: Properties 488 of 588 pt, Library 100.
                        ..group(
                            5,
                            vec![
                                PanelId::Properties,
                                PanelId::Align,
                                PanelId::Transform,
                                PanelId::Color,
                            ],
                        )
                    },
                    DockGroup {
                        weight: 0.17,
                        ..group(4, vec![PanelId::Library])
                    },
                ],
            },
            next_group: 6,
        }
    }
}
impl DockLayout {
    pub fn area(&self, edge: DockEdge) -> &DockArea {
        match edge {
            DockEdge::Top => &self.top,
            DockEdge::Bottom => &self.bottom,
            DockEdge::Left => &self.left,
            DockEdge::Right => &self.right,
        }
    }
    pub fn area_mut(&mut self, edge: DockEdge) -> &mut DockArea {
        match edge {
            DockEdge::Top => &mut self.top,
            DockEdge::Bottom => &mut self.bottom,
            DockEdge::Left => &mut self.left,
            DockEdge::Right => &mut self.right,
        }
    }
    pub fn location(&self, panel: PanelId) -> Option<DockLocation> {
        for edge in DockEdge::ALL {
            for group in &self.area(edge).groups {
                if let Some(index) = group.tabs.iter().position(|p| *p == panel) {
                    return Some(DockLocation {
                        edge,
                        group: group.id,
                        index,
                    });
                }
            }
        }
        None
    }
    fn remove(&mut self, panel: PanelId, keep_group: Option<u32>) {
        for edge in DockEdge::ALL {
            let area = self.area_mut(edge);
            for group in &mut area.groups {
                group.tabs.retain(|p| *p != panel);
                if !group.tabs.contains(&group.active)
                    && let Some(first) = group.tabs.first()
                {
                    group.active = *first;
                }
            }
            area.groups
                .retain(|g| !g.tabs.is_empty() || keep_group == Some(g.id));
        }
    }
    fn fresh_group(&mut self, panel: PanelId) -> DockGroup {
        let id = self.next_group;
        self.next_group += 1;
        DockGroup {
            id,
            tabs: vec![panel],
            active: panel,
            weight: 1.,
        }
    }
    fn insert(&mut self, panel: PanelId, destination: DockDestination) {
        let previous = self.location(panel);
        match destination {
            DockDestination::Tabs(mut location) => {
                if previous
                    .is_some_and(|old| old.group == location.group && old.index < location.index)
                {
                    location.index = location.index.saturating_sub(1);
                }
                self.remove(panel, Some(location.group));
                if let Some(group) = self
                    .area_mut(location.edge)
                    .groups
                    .iter_mut()
                    .find(|g| g.id == location.group)
                {
                    group
                        .tabs
                        .insert(location.index.min(group.tabs.len()), panel);
                    group.active = panel;
                } else {
                    let mut group = self.fresh_group(panel);
                    if location.group > 0
                        && location.group <= 1_000_000
                        && DockEdge::ALL.into_iter().all(|edge| {
                            self.area(edge)
                                .groups
                                .iter()
                                .all(|g| g.id != location.group)
                        })
                    {
                        group.id = location.group;
                    }
                    self.area_mut(location.edge).groups.push(group);
                }
            }
            DockDestination::Split {
                edge,
                group,
                before,
            } => {
                if self
                    .area(edge)
                    .groups
                    .iter()
                    .any(|g| g.id == group && g.tabs == [panel])
                {
                    return;
                }
                self.remove(panel, None);
                let mut new_group = self.fresh_group(panel);
                let area = self.area_mut(edge);
                let index = area.groups.iter().position(|g| g.id == group);
                if let Some(index) = index {
                    let target = &mut area.groups[index];
                    target.weight *= 0.5;
                    new_group.weight = target.weight;
                    area.groups.insert(index + usize::from(!before), new_group);
                } else {
                    area.groups.push(new_group);
                }
            }
            DockDestination::Edge(edge) => {
                self.remove(panel, None);
                let mut group = self.fresh_group(panel);
                let area = self.area_mut(edge);
                group.weight = area.groups.last().map_or(1., |g| g.weight);
                area.groups.push(group);
            }
            DockDestination::FloatingTabs { .. } | DockDestination::FloatingSplit { .. } => {
                unreachable!("floating destinations are routed by WorkspacePrefs")
            }
        }
    }
    fn select(&mut self, panel: PanelId) -> bool {
        let Some(location) = self.location(panel) else {
            return false;
        };
        let group = self
            .area_mut(location.edge)
            .groups
            .iter_mut()
            .find(|g| g.id == location.group)
            .expect("located group");
        let changed = group.active != panel;
        group.active = panel;
        changed
    }
    fn sanitize(&mut self, detached: DetachedPanels) {
        let default = Self::default();
        let mut panels = std::collections::HashSet::new();
        // Assign bounded, unique group IDs on malformed input only. Tab and
        // group counts can never exceed the available panels.
        let mut ids = std::collections::HashSet::new();
        let mut replacement = 1;
        for edge in DockEdge::ALL {
            let area = self.area_mut(edge);
            area.size = if area.size.is_finite() {
                area.size.clamp(56., 1600.)
            } else {
                default.area(edge).size
            };
            for group in &mut area.groups {
                group.tabs.retain(|p| {
                    detached
                        .get(*p)
                        .is_some_and(|w| w.placement == PanelPlacement::Docked)
                        && panels.insert(*p)
                });
                if !group.tabs.contains(&group.active)
                    && let Some(panel) = group.tabs.first()
                {
                    group.active = *panel;
                }
                group.weight = if group.weight.is_finite() {
                    group.weight.clamp(0.001, 1000.)
                } else {
                    1.
                };
                if group.id == 0 || group.id > 1_000_000 || !ids.insert(group.id) {
                    while ids.contains(&replacement) {
                        replacement += 1;
                    }
                    group.id = replacement;
                    ids.insert(replacement);
                }
            }
            area.groups.retain(|g| !g.tabs.is_empty());
        }
        let minimum_id = ids.into_iter().max().unwrap_or(0) + 1;
        self.next_group = self.next_group.clamp(minimum_id, 1_000_008);
        for panel in PanelId::DETACHABLE {
            if detached
                .get(panel)
                .is_some_and(|w| w.placement == PanelPlacement::Docked)
                && self.location(panel).is_none()
            {
                let edge = DockEdge::default_for(panel);
                let destination = self
                    .area(edge)
                    .groups
                    .iter()
                    .find(|group| {
                        matches!(panel, PanelId::Align | PanelId::Transform | PanelId::Color)
                            && group.tabs.contains(&PanelId::Properties)
                    })
                    .or_else(|| self.area(edge).groups.first())
                    .map_or(DockDestination::Edge(edge), |g| {
                        DockDestination::Tabs(DockLocation {
                            edge,
                            group: g.id,
                            index: g.tabs.len(),
                        })
                    });
                self.insert(panel, destination);
            }
        }
    }
}

/// OS logical points in a global top-left-origin desktop coordinate system.
/// On macOS these are Cocoa points with Y flipped around the primary screen.
/// Multiply egui ViewportInfo coordinates by ctx.zoom_factor() when capturing;
/// divide by the same factor when supplying a ViewportBuilder. Do not multiply
/// by the Retina backing scale. Position is outer; size excludes window chrome.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub outer_position: [f32; 2],
    pub inner_size: [f32; 2],
}
impl WindowGeometry {
    fn sanitized(self, panel: PanelId) -> Option<Self> {
        if !self
            .outer_position
            .into_iter()
            .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
            || !self.inner_size.into_iter().all(|v| v.is_finite() && v > 0.)
        {
            return None;
        }
        let min = minimum_window_size(panel);
        Some(Self {
            outer_position: self.outer_position,
            inner_size: [
                self.inner_size[0].clamp(min[0], 4096.),
                self.inner_size[1].clamp(min[1], 2160.),
            ],
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelWindowPrefs {
    pub placement: PanelPlacement,
    pub geometry: Option<WindowGeometry>,
    pub last_dock: Option<DockLocation>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetachedPanels {
    pub tools: PanelWindowPrefs,
    pub timeline: PanelWindowPrefs,
    pub actions: PanelWindowPrefs,
    pub library: PanelWindowPrefs,
    pub properties: PanelWindowPrefs,
    pub accessibility: PanelWindowPrefs,
    pub output: PanelWindowPrefs,
    pub align: PanelWindowPrefs,
    pub transform: PanelWindowPrefs,
    pub color: PanelWindowPrefs,
}
impl DetachedPanels {
    fn get(&self, panel: PanelId) -> Option<&PanelWindowPrefs> {
        match panel {
            PanelId::Tools => Some(&self.tools),
            PanelId::Timeline => Some(&self.timeline),
            PanelId::Actions => Some(&self.actions),
            PanelId::Library => Some(&self.library),
            PanelId::Properties => Some(&self.properties),
            PanelId::Accessibility => Some(&self.accessibility),
            PanelId::Output => Some(&self.output),
            PanelId::Align => Some(&self.align),
            PanelId::Transform => Some(&self.transform),
            PanelId::Color => Some(&self.color),
        }
    }
    fn get_mut(&mut self, panel: PanelId) -> Option<&mut PanelWindowPrefs> {
        match panel {
            PanelId::Tools => Some(&mut self.tools),
            PanelId::Timeline => Some(&mut self.timeline),
            PanelId::Actions => Some(&mut self.actions),
            PanelId::Library => Some(&mut self.library),
            PanelId::Properties => Some(&mut self.properties),
            PanelId::Accessibility => Some(&mut self.accessibility),
            PanelId::Output => Some(&mut self.output),
            PanelId::Align => Some(&mut self.align),
            PanelId::Transform => Some(&mut self.transform),
            PanelId::Color => Some(&mut self.color),
        }
    }
    fn sanitize(&mut self) {
        for panel in PanelId::DETACHABLE {
            let window = self.get_mut(panel).expect("supported panel");
            window.geometry = window
                .geometry
                .and_then(|geometry| geometry.sanitized(panel));
        }
    }
}

/// A native monitor's usable area (excluding menu bar/Dock), in OS logical
/// points. A root-window rectangle can be supplied as fallback when enumeration
/// is unavailable; it is only a recovery target, never a window to be moved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenRect {
    pub origin: [f32; 2],
    pub size: [f32; 2],
}
impl ScreenRect {
    pub fn is_valid(self) -> bool {
        self.origin
            .into_iter()
            .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
            && self
                .size
                .into_iter()
                .all(|v| v.is_finite() && (120. ..=32768.).contains(&v))
    }
    fn center(self) -> [f32; 2] {
        [
            self.origin[0] + self.size[0] * 0.5,
            self.origin[1] + self.size[1] * 0.5,
        ]
    }
    fn distance_squared(self, point: [f32; 2]) -> f64 {
        (0..2)
            .map(|axis| {
                let distance = f64::from(
                    point[axis].clamp(self.origin[axis], self.origin[axis] + self.size[axis])
                        - point[axis],
                );
                distance * distance
            })
            .sum()
    }
}

pub fn minimum_window_size(panel: PanelId) -> [f32; 2] {
    match panel {
        PanelId::Tools => [88., 300.],
        PanelId::Timeline => [320., 180.],
        PanelId::Library => [248., 300.],
        PanelId::Align | PanelId::Transform | PanelId::Color => [248., 200.],
        _ => [320., 220.],
    }
}
pub fn default_window_size(panel: PanelId) -> [f32; 2] {
    match panel {
        PanelId::Tools => [96., 600.],
        PanelId::Timeline => [900., 280.],
        PanelId::Library => [300., 600.],
        PanelId::Align | PanelId::Transform | PanelId::Color => [320., 440.],
        _ => [680., 440.],
    }
}

/// Recover only a detached panel, preserving partially offscreen windows whose
/// titlebar still has an 80x22-point draggable area on ANY connected monitor.
/// Pure bounded calculation; the caller applies the result once on creation or
/// topology changes, not every paint. Retain live user movement after that.
pub fn recover_geometry(
    panel: PanelId,
    saved: Option<WindowGeometry>,
    monitors: &[ScreenRect],
    fallback: ScreenRect,
) -> WindowGeometry {
    const CHROME: [f32; 2] = [16., 40.];
    let fallback = if fallback.is_valid() {
        fallback
    } else {
        ScreenRect {
            origin: [40., 40.],
            size: [1280., 800.],
        }
    };
    let valid: Vec<_> = monitors
        .iter()
        .copied()
        .take(64)
        .filter(|screen| screen.is_valid())
        .collect();
    let screens = if valid.is_empty() {
        std::slice::from_ref(&fallback)
    } else {
        valid.as_slice()
    };
    let saved = saved.and_then(|geometry| geometry.sanitized(panel));
    let mut geometry = saved.unwrap_or(WindowGeometry {
        outer_position: [fallback.origin[0] + 40., fallback.origin[1] + 40.],
        inner_size: default_window_size(panel),
    });
    let reachable = screens.iter().copied().find(|screen| {
        let overlap = |axis: usize, length: f32| {
            (geometry.outer_position[axis] + length).min(screen.origin[axis] + screen.size[axis])
                - geometry.outer_position[axis].max(screen.origin[axis])
        };
        overlap(0, geometry.inner_size[0] + CHROME[0]) >= 80. && overlap(1, 28.) >= 22.
    });
    let reference = saved.map_or(fallback.center(), |g| g.outer_position);
    let screen = reachable.unwrap_or_else(|| {
        *screens
            .iter()
            .min_by(|a, b| {
                a.distance_squared(reference)
                    .total_cmp(&b.distance_squared(reference))
            })
            .expect("fallback screen")
    });
    let old_size = geometry.inner_size;
    for (axis, chrome) in CHROME.into_iter().enumerate() {
        geometry.inner_size[axis] = geometry.inner_size[axis].min(screen.size[axis] - chrome);
    }
    if reachable.is_none() || saved.is_none() || geometry.inner_size != old_size {
        for (axis, chrome) in CHROME.into_iter().enumerate() {
            geometry.outer_position[axis] = geometry.outer_position[axis].clamp(
                screen.origin[axis],
                screen.origin[axis]
                    + (screen.size[axis] - geometry.inner_size[axis] - chrome).max(0.),
            );
        }
    }
    geometry
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePrefs {
    pub version: u32,
    pub theme: ThemeMode,
    pub panels: PanelVisibility,
    pub active_bottom: BottomTab,
    pub sizes: PanelSizes,
    pub detached: DetachedPanels,
    pub dock: DockLayout,
    pub floating: Vec<FloatingDock>,
    /// Template the current layout started from (shown in the top-bar workspace select).
    pub template: WorkspaceTemplate,
    /// Timeline frame view and playback options (Frame view ⋯, Control menu).
    pub timeline: TimelineView,
    /// Rulers, guides & grid settings saved with "Save as default" (View settings popover); every
    /// session starts from them. `None` = NF2 defaults.
    pub view_defaults: Option<crate::stage_view::ViewPrefs>,
}

/// Presentation and playback options of the Timeline. Workspace-only, never in the document.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineView {
    /// Off = spans are not colour-coded, only arrows (§5.4 "Tinted frames").
    pub tinted: bool,
    pub show_seconds: bool,
    /// 18 pt layer rows (Frame view ▸ Short).
    pub short_rows: bool,
    /// Ruler scrubbing lands on the current layer's keyframes (⇧ suspends it).
    pub snap_playhead: bool,
    /// Off = editor playback stops on the last frame instead of wrapping.
    pub loop_playback: bool,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            tinted: true,
            show_seconds: true,
            short_rows: false,
            snap_playhead: false,
            loop_playback: true,
        }
    }
}

impl TimelineView {
    /// `NF2/TL Layer` row height for the current frame view.
    pub fn row_height(&self) -> f32 {
        if self.short_rows {
            neoflash_ui::metrics::LAYER_ROW_SHORT
        } else {
            neoflash_ui::metrics::LAYER_ROW
        }
    }
}
impl Default for WorkspacePrefs {
    fn default() -> Self {
        Self {
            version: VERSION,
            theme: ThemeMode::default(),
            panels: PanelVisibility::default(),
            active_bottom: BottomTab::Properties,
            sizes: PanelSizes::default(),
            detached: DetachedPanels::default(),
            dock: DockLayout::default(),
            floating: Vec::new(),
            template: WorkspaceTemplate::Classic,
            timeline: TimelineView::default(),
            view_defaults: None,
        }
    }
}

/// Built-in workspace templates (Window ▸ Workspace and the top-bar select). Each one is a complete
/// dock layout + panel visibility; applying it closes floating panel windows. Documents, history,
/// theme and drafts are never touched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTemplate {
    /// NF2 main window: tools left, Properties over Library, bottom dock Timeline · Actions · Output · Accessibility.
    #[default]
    Classic,
    /// Tall timeline for keyframing; Transform next to Properties.
    Animator,
    /// Drawing and layout: Properties, Color, Align, Transform visible; shorter timeline.
    Designer,
    /// Scripting: tall Actions editor with Output, timeline as a tab.
    Developer,
    /// Small screens: one-column tools, one inspector group, compact bottom dock.
    Essentials,
}

impl WorkspaceTemplate {
    pub const ALL: [Self; 5] = [
        Self::Classic,
        Self::Animator,
        Self::Designer,
        Self::Developer,
        Self::Essentials,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Animator => "Animator",
            Self::Designer => "Designer",
            Self::Developer => "Developer",
            Self::Essentials => "Essentials",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Classic => {
                "Tools, Properties over Library, Timeline with Actions and Output below."
            }
            Self::Animator => "Tall timeline for keyframing, Transform beside Properties.",
            Self::Designer => "Properties, Color, Align and Transform open; shorter timeline.",
            Self::Developer => "Tall Actions editor with Output; Timeline as a tab.",
            Self::Essentials => "One-column tools and a compact dock for small screens.",
        }
    }

    /// Dock layout, visibility and active bottom tab of this template.
    pub fn layout(self) -> (DockLayout, PanelVisibility, BottomTab) {
        use PanelId::*;
        let group = |id, tabs: Vec<PanelId>, weight: f32| DockGroup {
            id,
            active: tabs[0],
            tabs,
            weight,
        };
        let area = |size: f32, groups: Vec<DockGroup>| DockArea { size, groups };
        let tools = |size: f32| area(size + 6., vec![group(3, vec![Tools], 1.)]);
        let two_columns = neoflash_ui::metrics::TOOLS_TWO_COLUMNS;
        let inspector = neoflash_ui::metrics::INSPECTOR + 6.;
        let mut visible = PanelVisibility::default();
        let (dock, bottom) = match self {
            Self::Classic => (DockLayout::default(), BottomTab::Properties),
            Self::Animator => {
                visible.transform = true;
                let dock = DockLayout {
                    top: area(crate::theme::TIMELINE_HEIGHT, vec![]),
                    bottom: area(
                        520.,
                        vec![group(2, vec![Timeline, Actions, Output, Accessibility], 1.)],
                    ),
                    left: tools(two_columns),
                    right: area(
                        inspector,
                        vec![
                            group(5, vec![Properties, Transform, Align, Color], 0.55),
                            group(4, vec![Library], 0.45),
                        ],
                    ),
                    next_group: 6,
                };
                (dock, BottomTab::Properties)
            }
            Self::Designer => {
                visible.align = true;
                visible.transform = true;
                visible.color = true;
                let dock = DockLayout {
                    top: area(crate::theme::TIMELINE_HEIGHT, vec![]),
                    bottom: area(
                        300.,
                        vec![group(2, vec![Timeline, Actions, Output, Accessibility], 1.)],
                    ),
                    left: tools(two_columns),
                    right: area(
                        inspector,
                        vec![
                            group(5, vec![Properties, Library], 0.5),
                            group(6, vec![Color, Align, Transform], 0.5),
                        ],
                    ),
                    next_group: 7,
                };
                (dock, BottomTab::Properties)
            }
            Self::Developer => {
                let mut bottom = group(2, vec![Actions, Output, Timeline, Accessibility], 1.);
                bottom.active = Actions;
                let dock = DockLayout {
                    top: area(crate::theme::TIMELINE_HEIGHT, vec![]),
                    bottom: area(560., vec![bottom]),
                    left: tools(two_columns),
                    right: area(
                        inspector,
                        vec![
                            group(5, vec![Properties, Align, Transform, Color], 0.5),
                            group(4, vec![Library], 0.5),
                        ],
                    ),
                    next_group: 6,
                };
                (dock, BottomTab::Actions)
            }
            Self::Essentials => {
                let dock = DockLayout {
                    top: area(crate::theme::TIMELINE_HEIGHT, vec![]),
                    bottom: area(
                        300.,
                        vec![group(2, vec![Timeline, Actions, Output, Accessibility], 1.)],
                    ),
                    left: tools(neoflash_ui::metrics::TOOLS_RAIL),
                    right: area(
                        280.,
                        vec![group(
                            5,
                            vec![Properties, Library, Align, Transform, Color],
                            1.,
                        )],
                    ),
                    next_group: 6,
                };
                (dock, BottomTab::Properties)
            }
        };
        (dock, visible, bottom)
    }
}
impl WorkspacePrefs {
    /// Missing, malformed, oversized or future-version preferences fail soft.
    /// Unknown fields within this version are ignored and never written back.
    pub fn from_json(json: &str) -> Self {
        if json.len() > MAX_PREFS_BYTES {
            return Self::default();
        }
        let Ok(mut prefs) = serde_json::from_str::<Self>(json) else {
            return Self::default();
        };
        if prefs.version != VERSION {
            return Self::default();
        }
        // Earlier preferences stored fixed panel sizes and detached flags.
        // Migrate once; thereafter the grid is the authoritative dock layout.
        let has_grid = serde_json::from_str::<serde_json::Value>(json)
            .ok()
            .is_some_and(|value| value.get("dock").is_some());
        if !has_grid {
            prefs.dock.top.size = prefs.sizes.timeline_height;
            prefs.dock.bottom.size = prefs.sizes.bottom_height;
            prefs.dock.right.size = prefs.sizes.library_width;
            prefs
                .dock
                .select(PanelId::from_bottom_tab(prefs.active_bottom));
        }
        prefs.sanitize();
        prefs
    }
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        storage
            .and_then(|s| s.get_string(PREFS_KEY))
            .map_or_else(Self::default, |json| Self::from_json(&json))
    }
    /// Choose a compact default once, never rewrite a saved user layout on resize.
    pub fn load_for_viewport(storage: Option<&dyn eframe::Storage>, size: [f32; 2]) -> Self {
        let mut prefs = Self::load(storage);
        if storage.and_then(|s| s.get_string(PREFS_KEY)).is_none() {
            prefs.compact_default(size);
        }
        prefs
    }
    pub fn compact_default(&mut self, size: [f32; 2]) {
        if size[0] <= 1120. || size[1] <= 800. {
            let tabs = self
                .dock
                .right
                .groups
                .iter()
                .flat_map(|g| g.tabs.iter().copied())
                .collect();
            self.dock.right.groups = vec![DockGroup {
                id: 4,
                tabs,
                active: PanelId::Properties,
                weight: 1.,
            }];
            self.dock.bottom.size = 300.;
        }
    }
    /// Only this small JSON key is written. Eframe owns flushing the storage;
    /// this never serializes Editor, Project, clipboard or arbitrary egui memory.
    pub fn save(&self, storage: &mut dyn eframe::Storage) {
        let mut normalized = self.clone();
        normalized.sanitize();
        if let Ok(json) = serde_json::to_string(&normalized) {
            storage.set_string(PREFS_KEY, json);
        }
    }
    pub fn sanitize(&mut self) {
        self.version = VERSION;
        if let Some(view) = &mut self.view_defaults {
            view.sanitize();
        }
        self.sizes.sanitize();
        self.detached.sanitize();
        self.dock.sanitize(self.detached);
        let mut seen = std::collections::HashSet::new();
        let mut windows = std::collections::HashSet::new();
        self.floating.retain_mut(|window| {
            if !windows.insert(window.id) {
                return false;
            }
            let mut ids = std::collections::HashSet::new();
            let mut next = 1;
            window.root = window
                .root
                .take()
                .and_then(|root| root.sanitize(&mut seen, &mut ids, &mut next, self.detached));
            let minimum = ids.into_iter().max().unwrap_or(0) + 1;
            window.next_group = window.next_group.clamp(minimum, 1_000_008);
            if let Some(panel) = window.panels().first() {
                window.geometry = window
                    .geometry
                    .and_then(|geometry| geometry.sanitized(*panel));
            }
            window.root.is_some()
        });
        // Old single-window preferences migrate into one floating tab group.
        for panel in PanelId::DETACHABLE {
            if self.placement(panel) == PanelPlacement::Detached
                && self.floating_window(panel).is_none()
            {
                let geometry = self.detached.get(panel).and_then(|p| p.geometry);
                let visible = self.is_panel_visible(panel);
                self.float_panel(panel, geometry);
                *self.panels.slot(panel) = visible;
            }
        }
        self.normalize_active_bottom();
    }
    pub fn is_panel_visible(&self, panel: PanelId) -> bool {
        self.panels.is_visible(panel)
    }
    #[cfg(test)]
    pub fn bottom_visible(&self) -> bool {
        self.dock
            .bottom
            .groups
            .iter()
            .any(|group| group.tabs.iter().any(|panel| self.is_panel_visible(*panel)))
    }
    /// Revealing a bottom panel also selects it. F9 and error-reveal commands
    /// must use this even when a dock/tab was previously hidden.
    pub fn show_panel(&mut self, panel: PanelId) -> bool {
        let mut changed = !*self.panels.slot(panel);
        *self.panels.slot(panel) = true;
        changed |= self.dock.select(panel);
        for window in &mut self.floating {
            changed |= window.select(panel);
        }
        if let Some(tab) = panel.bottom_tab().filter(|_| self.is_panel_docked(panel)) {
            changed |= self.active_bottom != tab;
            self.active_bottom = tab;
        }
        changed
    }
    pub fn set_panel_visible(&mut self, panel: PanelId, visible: bool) -> bool {
        if visible {
            return self.show_panel(panel);
        }
        let changed = *self.panels.slot(panel);
        *self.panels.slot(panel) = false;
        let before = self.active_bottom;
        self.normalize_active_bottom();
        changed || before != self.active_bottom
    }
    /// Reset layout only. Theme is a separate user preference. The caller may
    /// remove known egui PanelState IDs, but must preserve authoring/input state.
    /// Replace the layout with a built-in template, keeping theme and remembered window geometry.
    pub fn apply_template(&mut self, template: WorkspaceTemplate) {
        let (dock, panels, active_bottom) = template.layout();
        *self = Self {
            theme: self.theme,
            view_defaults: self.view_defaults.take(),
            dock,
            panels,
            active_bottom,
            template,
            ..Self::default()
        };
        self.sanitize();
    }
    pub fn placement(&self, panel: PanelId) -> PanelPlacement {
        if self.dock.location(panel).is_some() {
            PanelPlacement::Docked
        } else {
            PanelPlacement::Detached
        }
    }
    pub fn is_panel_docked(&self, panel: PanelId) -> bool {
        self.placement(panel) == PanelPlacement::Docked
    }
    /// A placement command reveals the panel. Native close should only hide it,
    /// retaining placement and geometry so Window/F9 can reopen the same panel.
    pub fn set_placement(&mut self, panel: PanelId, placement: PanelPlacement) -> bool {
        let old = self.placement(panel);
        let grouped = self
            .floating_window(panel)
            .is_some_and(|window| window.panels().len() > 1);
        if old != placement || (grouped && placement == PanelPlacement::Detached) {
            if placement == PanelPlacement::Detached {
                let geometry = self.detached_geometry(panel);
                self.float_panel(panel, geometry);
            } else {
                let destination = self.detached.get(panel).and_then(|w| w.last_dock).map_or(
                    DockDestination::Edge(DockEdge::default_for(panel)),
                    DockDestination::Tabs,
                );
                self.dock_panel(panel, destination);
            }
        }
        let shown = self.show_panel(panel);
        self.normalize_active_bottom();
        old != placement || grouped || shown
    }
    pub fn dock_panel(&mut self, panel: PanelId, destination: DockDestination) {
        match destination {
            DockDestination::FloatingTabs {
                window,
                group,
                mut index,
            } => {
                let Some(target) = self.floating.iter().find(|w| w.id == window) else {
                    return;
                };
                if target.group(group).is_none() {
                    return;
                }
                if let Some((old_group, old_index)) =
                    target.root.as_ref().and_then(|root| root.location(panel))
                    && old_group == group
                    && old_index < index
                {
                    index = index.saturating_sub(1);
                }
                self.leave_root_dock(panel);
                self.remove_floating_panel(panel, Some((window, group)));
                let target = self
                    .floating
                    .iter_mut()
                    .find(|w| w.id == window)
                    .expect("retained target window");
                let group = target
                    .root
                    .as_mut()
                    .and_then(|root| root.group_mut(group))
                    .expect("retained target group");
                group.tabs.insert(index.min(group.tabs.len()), panel);
                group.active = panel;
                self.detached.get_mut(panel).expect("known panel").placement =
                    PanelPlacement::Detached;
                self.show_panel(panel);
                self.normalize_active_bottom();
                return;
            }
            DockDestination::FloatingSplit {
                window,
                group,
                edge,
            } => {
                let Some(target) = self.floating.iter().find(|w| w.id == window) else {
                    return;
                };
                if group.is_some_and(|id| target.group(id).is_none()) {
                    return;
                }
                if target.panels() == [panel]
                    || group.is_some_and(|id| target.group(id).is_some_and(|g| g.tabs == [panel]))
                {
                    return;
                }
                self.leave_root_dock(panel);
                self.remove_floating_panel(panel, None);
                let target = self
                    .floating
                    .iter_mut()
                    .find(|w| w.id == window)
                    .expect("nonempty target window");
                let new_group = target.fresh_group(panel);
                target.root = target
                    .root
                    .take()
                    .map(|root| root.split(group, edge, new_group));
                self.detached.get_mut(panel).expect("known panel").placement =
                    PanelPlacement::Detached;
                self.show_panel(panel);
                self.normalize_active_bottom();
                return;
            }
            _ => {}
        }
        self.remove_floating_panel(panel, None);
        self.dock.insert(panel, destination);
        let location = self.dock.location(panel);
        let window = self.detached.get_mut(panel).expect("known panel");
        window.placement = PanelPlacement::Docked;
        window.last_dock = location;
        self.show_panel(panel);
        self.normalize_active_bottom();
    }
    /// Move a tab group as one unit. Hidden tabs keep their visibility, and the
    /// selected tab survives moves between the main and native-window layouts.
    pub fn dock_group(
        &mut self,
        panels: &[PanelId],
        active: PanelId,
        destination: DockDestination,
    ) {
        let Some(&first) = panels.first() else { return };
        let same_group = match destination {
            DockDestination::Tabs(location) => self
                .dock
                .area(location.edge)
                .groups
                .iter()
                .find(|g| g.id == location.group)
                .map(|g| &g.tabs),
            DockDestination::Split { edge, group, .. } => self
                .dock
                .area(edge)
                .groups
                .iter()
                .find(|g| g.id == group)
                .map(|g| &g.tabs),
            DockDestination::FloatingTabs { window, group, .. }
            | DockDestination::FloatingSplit {
                window,
                group: Some(group),
                ..
            } => self
                .floating
                .iter()
                .find(|w| w.id == window)
                .and_then(|w| w.group(group))
                .map(|g| &g.tabs),
            _ => None,
        }
        .is_some_and(|tabs| tabs.len() == panels.len() && panels.iter().all(|p| tabs.contains(p)));
        let same_window = match destination {
            DockDestination::FloatingSplit {
                window,
                group: None,
                ..
            } => self
                .floating
                .iter()
                .find(|w| w.id == window)
                .is_some_and(|w| {
                    let tabs = w.panels();
                    tabs.len() == panels.len() && panels.iter().all(|p| tabs.contains(p))
                }),
            _ => false,
        };
        if same_group || same_window {
            return;
        }
        let visibility: Vec<_> = panels
            .iter()
            .map(|p| (*p, self.is_panel_visible(*p)))
            .collect();
        self.dock_panel(first, destination);
        for (offset, panel) in panels.iter().enumerate().skip(1) {
            let destination = if let Some(location) = self.dock.location(first) {
                DockDestination::Tabs(DockLocation {
                    index: location.index + offset,
                    ..location
                })
            } else {
                let window = self.floating_window(first).expect("moved group window");
                let (group, index) = window
                    .root
                    .as_ref()
                    .and_then(|root| root.location(first))
                    .expect("moved group tab");
                DockDestination::FloatingTabs {
                    window: window.id,
                    group,
                    index: index + offset,
                }
            };
            self.dock_panel(*panel, destination);
        }
        for (panel, visible) in visibility {
            *self.panels.slot(panel) = visible;
        }
        self.show_panel(active);
    }

    pub fn float_group(
        &mut self,
        panels: &[PanelId],
        active: PanelId,
        geometry: WindowGeometry,
    ) -> PanelId {
        let first = panels[0];
        if let Some(window) = self.floating.iter().find(|w| {
            let tabs = w.panels();
            tabs.len() == panels.len() && panels.iter().all(|p| tabs.contains(p))
        }) {
            let id = window.id;
            self.remember_floating_geometry(id, geometry);
            return id;
        }
        let visible = self.is_panel_visible(first);
        let window = self.float_panel(first, Some(geometry));
        *self.panels.slot(first) = visible;
        let group = self
            .floating_window(first)
            .and_then(|w| w.root.as_ref())
            .and_then(|root| root.location(first))
            .expect("new floating group")
            .0;
        self.dock_group(
            panels,
            active,
            DockDestination::FloatingTabs {
                window,
                group,
                index: 0,
            },
        );
        window
    }
    pub fn detached_geometry(&self, panel: PanelId) -> Option<WindowGeometry> {
        self.floating_window(panel)
            .and_then(|window| window.geometry)
            .or_else(|| self.detached.get(panel).and_then(|p| p.geometry))
    }
    pub fn floating_window(&self, panel: PanelId) -> Option<&FloatingDock> {
        self.floating.iter().find(|window| window.contains(panel))
    }
    pub fn panel_surface(&self, panel: PanelId) -> DockSurface {
        self.floating_window(panel)
            .map_or(DockSurface::Main, |window| DockSurface::Floating(window.id))
    }
    fn leave_root_dock(&mut self, panel: PanelId) {
        if let Some(location) = self.dock.location(panel) {
            self.detached.get_mut(panel).expect("known panel").last_dock = Some(location);
            self.dock.remove(panel, None);
        }
    }
    fn remove_floating_panel(&mut self, panel: PanelId, keep: Option<(PanelId, u32)>) {
        for window in &mut self.floating {
            let keep_group = keep
                .filter(|(id, _)| *id == window.id)
                .map(|(_, group)| group);
            window.root = window
                .root
                .take()
                .and_then(|root| root.remove(panel, keep_group));
        }
        self.floating.retain(|window| window.root.is_some());
    }
    /// Tear one tab into its own native window. Existing standalone windows keep
    /// identity and size; group membership never determines viewport identity.
    pub fn float_panel(&mut self, panel: PanelId, geometry: Option<WindowGeometry>) -> PanelId {
        if let Some(window) = self
            .floating
            .iter_mut()
            .find(|window| window.panels() == [panel])
        {
            if let Some(geometry) = geometry.and_then(|geometry| geometry.sanitized(panel)) {
                window.geometry = Some(geometry);
            }
            return window.id;
        }
        self.leave_root_dock(panel);
        self.remove_floating_panel(panel, None);
        let id = if self.floating.iter().all(|window| window.id != panel) {
            panel
        } else {
            PanelId::DETACHABLE
                .into_iter()
                .find(|id| self.floating.iter().all(|window| window.id != *id))
                .expect("at most ten windows")
        };
        let geometry = geometry.and_then(|geometry| geometry.sanitized(panel));
        self.floating.push(FloatingDock::new(id, panel, geometry));
        self.detached.get_mut(panel).expect("known panel").placement = PanelPlacement::Detached;
        self.show_panel(panel);
        self.normalize_active_bottom();
        id
    }
    pub fn remember_floating_geometry(&mut self, id: PanelId, geometry: WindowGeometry) -> bool {
        let Some(window) = self.floating.iter_mut().find(|window| window.id == id) else {
            return false;
        };
        let panels = window.panels();
        let Some(panel) = panels.first() else {
            return false;
        };
        let Some(geometry) = geometry.sanitized(*panel) else {
            return false;
        };
        let changed = window.geometry != Some(geometry);
        window.geometry = Some(geometry);
        for panel in panels {
            self.detached.get_mut(panel).expect("known panel").geometry = Some(geometry);
        }
        changed
    }
    /// Minimized/missing/nonfinite native snapshots must not erase good geometry.
    #[cfg(test)]
    pub fn remember_geometry(&mut self, panel: PanelId, geometry: WindowGeometry) -> bool {
        if let Some(id) = self.floating_window(panel).map(|window| window.id) {
            return self.remember_floating_geometry(id, geometry);
        }
        let Some(geometry) = geometry.sanitized(panel) else {
            return false;
        };
        let Some(window) = self.detached.get_mut(panel) else {
            return false;
        };
        let changed = window.geometry != Some(geometry);
        window.geometry = Some(geometry);
        changed
    }
    fn normalize_active_bottom(&mut self) {
        let active = PanelId::from_bottom_tab(self.active_bottom);
        if (!self.is_panel_visible(active) || !self.is_panel_docked(active))
            && let Some(tab) = PanelId::BOTTOM
                .into_iter()
                .find(|&panel| self.is_panel_visible(panel) && self.is_panel_docked(panel))
                .and_then(PanelId::bottom_tab)
        {
            self.active_bottom = tab;
        }
        // With every tab hidden, keep the last selection until an explicit show.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Storage {
        values: BTreeMap<String, String>,
        flushes: usize,
    }
    impl eframe::Storage for Storage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.values.insert(key.into(), value);
        }
        fn flush(&mut self) {
            self.flushes += 1;
        }
    }
    #[test]
    fn workspace_roundtrip_writes_only_its_own_bounded_key() {
        let mut prefs = WorkspacePrefs {
            theme: ThemeMode::Dark,
            ..Default::default()
        };
        prefs.set_panel_visible(PanelId::Timeline, false);
        prefs.set_panel_visible(PanelId::Properties, false);
        prefs.show_panel(PanelId::Accessibility);
        prefs.sizes.library_width = 310.;
        let mut storage = Storage::default();
        storage.values.insert("unrelated".into(), "keep me".into());
        prefs.save(&mut storage);
        assert_eq!(WorkspacePrefs::load(Some(&storage)), prefs);
        assert_eq!(storage.values.len(), 2);
        assert_eq!(storage.values["unrelated"], "keep me");
        assert_eq!(storage.flushes, 0);
        assert!(storage.values[PREFS_KEY].len() < MAX_PREFS_BYTES);
        assert!(!storage.values[PREFS_KEY].contains("project"));
    }
    #[test]
    fn malformed_large_unknown_version_and_unknown_theme_preferences_fail_soft() {
        let default = WorkspacePrefs::default();
        assert_eq!(WorkspacePrefs::load(None), default);
        for input in [
            "",
            "{",
            "null",
            r#"{"version":2,"theme":"dark"}"#,
            r#"{"theme":"system"}"#,
            r#"{"sizes":{"timeline_height":null}}"#,
        ] {
            assert_eq!(WorkspacePrefs::from_json(input), default, "{input}");
        }
        assert_eq!(
            WorkspacePrefs::from_json(&" ".repeat(MAX_PREFS_BYTES + 1)),
            default
        );
        let partial = WorkspacePrefs::from_json(
            r#"{"theme":"dark","panels":{"library":false},"unknown":"ignored"}"#,
        );
        assert_eq!(partial.theme, ThemeMode::Dark);
        assert!(!partial.panels.library);
        assert!(partial.panels.tools);
        assert_eq!(partial.sizes, PanelSizes::default());
    }
    #[test]
    fn hidden_tabs_reopen_and_active_tab_falls_back_without_losing_dock_state() {
        let mut prefs = WorkspacePrefs::default();
        prefs.show_panel(PanelId::Actions);
        assert_eq!(prefs.active_bottom, BottomTab::Actions);
        prefs.set_panel_visible(PanelId::Actions, false);
        assert_eq!(prefs.active_bottom, BottomTab::Properties);
        for panel in PanelId::ALL {
            prefs.set_panel_visible(panel, false);
        }
        assert!(!prefs.bottom_visible());
        assert!(PanelId::ALL.into_iter().all(|p| !prefs.is_panel_visible(p)));
        assert!(prefs.show_panel(PanelId::Actions));
        assert!(prefs.bottom_visible());
        assert_eq!(prefs.active_bottom, BottomTab::Actions);
        assert!(!prefs.panels.properties);
        assert!(!prefs.show_panel(PanelId::Actions));
        for panel in PanelId::ALL {
            prefs.show_panel(panel);
            assert!(prefs.is_panel_visible(panel));
        }
    }
    #[test]
    fn every_template_is_a_valid_layout_that_keeps_every_panel_reachable() {
        for template in WorkspaceTemplate::ALL {
            let mut prefs = WorkspacePrefs {
                theme: ThemeMode::Light,
                ..Default::default()
            };
            prefs.set_placement(PanelId::Library, PanelPlacement::Detached);
            prefs.apply_template(template);
            assert_eq!(prefs.template, template);
            assert_eq!(
                prefs.theme,
                ThemeMode::Light,
                "{template:?} keeps the theme"
            );
            assert!(
                prefs.floating.is_empty(),
                "{template:?} closes floating windows"
            );
            let mut seen = std::collections::HashSet::new();
            for edge in [
                DockEdge::Top,
                DockEdge::Bottom,
                DockEdge::Left,
                DockEdge::Right,
            ] {
                for group in &prefs.dock.area(edge).groups {
                    assert!(group.tabs.contains(&group.active));
                    for panel in &group.tabs {
                        assert!(seen.insert(*panel), "{template:?}: {panel:?} docked twice");
                    }
                }
            }
            for panel in PanelId::ALL {
                assert!(
                    prefs.is_panel_docked(panel),
                    "{template:?}: {panel:?} has a place"
                );
            }
            let mut storage = Storage::default();
            prefs.save(&mut storage);
            assert_eq!(
                WorkspacePrefs::load(Some(&storage)),
                prefs,
                "{template:?} round-trips"
            );
        }
    }
    #[test]
    fn restore_layout_preserves_theme_and_resets_visibility_sizes_and_active_tab() {
        let mut prefs = WorkspacePrefs {
            theme: ThemeMode::Dark,
            ..Default::default()
        };
        for panel in PanelId::ALL {
            prefs.set_panel_visible(panel, false);
        }
        prefs.sizes = PanelSizes {
            timeline_height: 380.,
            bottom_height: 420.,
            library_width: 360.,
            layer_column_width: 420.,
        };
        prefs.apply_template(WorkspaceTemplate::Classic);
        assert_eq!(
            prefs,
            WorkspacePrefs {
                theme: ThemeMode::Dark,
                ..Default::default()
            }
        );
    }
    #[test]
    fn dimensions_are_normalized_before_persistence_without_mutating_live_preferences() {
        let prefs = WorkspacePrefs {
            sizes: PanelSizes {
                timeline_height: f32::NAN,
                bottom_height: f32::INFINITY,
                library_width: -100.,
                layer_column_width: f32::NAN,
            },
            ..Default::default()
        };
        let mut storage = Storage::default();
        prefs.save(&mut storage);
        let loaded = WorkspacePrefs::load(Some(&storage));
        assert_eq!(
            loaded.sizes,
            PanelSizes {
                timeline_height: crate::theme::TIMELINE_HEIGHT,
                bottom_height: 190.,
                library_width: 248.,
                layer_column_width: neoflash_ui::metrics::LAYER_COLUMN,
            }
        );
        assert!(prefs.sizes.timeline_height.is_nan());
        let large = WorkspacePrefs::from_json(
            r#"{"sizes":{"timeline_height":999999,"bottom_height":-1,"library_width":999999}}"#,
        );
        assert_eq!(
            large.sizes,
            PanelSizes {
                timeline_height: 380.,
                bottom_height: 145.,
                library_width: 360.,
                layer_column_width: neoflash_ui::metrics::LAYER_COLUMN,
            }
        );
    }
    #[test]
    fn restoring_preferences_cannot_select_a_hidden_tab_unless_all_tabs_are_hidden() {
        let prefs = WorkspacePrefs::from_json(
            r#"{"active_bottom":"Output","panels":{"output":false,"properties":false}}"#,
        );
        assert_eq!(prefs.active_bottom, BottomTab::Actions);
        let prefs = WorkspacePrefs::from_json(
            r#"{"active_bottom":"Output","panels":{"timeline":false,"properties":false,"actions":false,"accessibility":false,"output":false}}"#,
        );
        assert_eq!(prefs.active_bottom, BottomTab::Output);
        assert!(!prefs.bottom_visible());
    }
    fn screen(origin: [f32; 2], size: [f32; 2]) -> ScreenRect {
        ScreenRect { origin, size }
    }
    fn window(position: [f32; 2], size: [f32; 2]) -> WindowGeometry {
        WindowGeometry {
            outer_position: position,
            inner_size: size,
        }
    }
    #[test]
    fn detached_actions_is_one_panel_and_reopens_in_the_same_placement() {
        let mut prefs = WorkspacePrefs::default();
        assert!(prefs.set_placement(PanelId::Actions, PanelPlacement::Detached));
        assert!(prefs.panels.actions);
        assert_eq!(prefs.active_bottom, BottomTab::Properties);
        assert!(!prefs.is_panel_docked(PanelId::Actions));
        assert!(prefs.set_placement(PanelId::Library, PanelPlacement::Detached));
        assert!(!prefs.is_panel_docked(PanelId::Library));
        // NF2: Timeline shares the bottom dock with Actions, Output and Accessibility.
        for panel in [
            PanelId::Timeline,
            PanelId::Properties,
            PanelId::Accessibility,
            PanelId::Output,
        ] {
            prefs.set_panel_visible(panel, false);
        }
        assert!(!prefs.bottom_visible());
        let geometry = window([-600., 70.], [640., 430.]);
        assert!(prefs.remember_geometry(PanelId::Actions, geometry));
        prefs.set_panel_visible(PanelId::Actions, false);
        assert!(prefs.show_panel(PanelId::Actions));
        assert_eq!(prefs.placement(PanelId::Actions), PanelPlacement::Detached);
        assert_eq!(prefs.detached_geometry(PanelId::Actions), Some(geometry));
        assert!(!prefs.bottom_visible());
        prefs.set_placement(PanelId::Actions, PanelPlacement::Docked);
        assert_eq!(prefs.active_bottom, BottomTab::Actions);
        assert!(prefs.bottom_visible());
    }
    #[test]
    fn detached_preferences_roundtrip_migrate_and_restore_without_changing_theme() {
        let old = WorkspacePrefs::from_json(r#"{"version":1,"theme":"dark"}"#);
        assert_eq!(old.detached, DetachedPanels::default());
        for panel in [PanelId::Align, PanelId::Transform, PanelId::Color] {
            assert!(!old.is_panel_visible(panel));
            assert!(old.is_panel_docked(panel));
        }
        let mut prefs = old;
        // A real workspace arrangement: merge two tabs, reorder them, split
        // Properties back out, and move Timeline to a different edge.
        let library = prefs.dock.location(PanelId::Library).unwrap();
        prefs.dock_panel(
            PanelId::Properties,
            DockDestination::Tabs(DockLocation {
                index: 1,
                ..library
            }),
        );
        prefs.dock_panel(
            PanelId::Properties,
            DockDestination::Tabs(DockLocation {
                index: 0,
                ..library
            }),
        );
        assert_eq!(
            prefs
                .dock
                .right
                .groups
                .iter()
                .find(|g| g.tabs.contains(&PanelId::Library))
                .unwrap()
                .tabs,
            [PanelId::Properties, PanelId::Library]
        );
        prefs.dock_panel(
            PanelId::Properties,
            DockDestination::Split {
                edge: DockEdge::Right,
                group: library.group,
                before: true,
            },
        );
        assert_eq!(
            prefs
                .dock
                .right
                .groups
                .iter()
                .filter(|group| { group.tabs.iter().any(|p| prefs.is_panel_visible(*p)) })
                .count(),
            2
        );
        prefs.dock.right.groups[0].weight = 0.35;
        prefs.dock.right.groups[1].weight = 0.65;
        prefs.dock_panel(PanelId::Timeline, DockDestination::Edge(DockEdge::Bottom));
        let timeline_location = prefs.dock.location(PanelId::Timeline).unwrap();
        prefs.set_placement(PanelId::Timeline, PanelPlacement::Detached);
        prefs.set_placement(PanelId::Timeline, PanelPlacement::Docked);
        assert_eq!(
            prefs.dock.location(PanelId::Timeline).unwrap().edge,
            timeline_location.edge
        );
        let mut arranged_storage = Storage::default();
        prefs.save(&mut arranged_storage);
        assert_eq!(WorkspacePrefs::load(Some(&arranged_storage)), prefs);
        let mut legacy = serde_json::to_value(&prefs).unwrap();
        for key in ["align", "transform", "color"] {
            legacy["panels"].as_object_mut().unwrap().remove(key);
            legacy["detached"].as_object_mut().unwrap().remove(key);
        }
        for edge in ["top", "bottom", "left", "right"] {
            for group in legacy["dock"][edge]["groups"].as_array_mut().unwrap() {
                group["tabs"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|tab| !matches!(tab.as_str(), Some("align" | "transform" | "color")));
            }
        }
        let migrated = WorkspacePrefs::from_json(&legacy.to_string());
        for panel in [PanelId::Align, PanelId::Transform, PanelId::Color] {
            assert!(!migrated.is_panel_visible(panel));
            assert_eq!(
                migrated.dock.location(panel).unwrap().group,
                migrated.dock.location(PanelId::Properties).unwrap().group
            );
        }
        for panel in PanelId::DETACHABLE {
            prefs.set_placement(panel, PanelPlacement::Detached);
            prefs.remember_geometry(panel, window([1800., -200.], default_window_size(panel)));
        }
        // Native-window groups survive reordering, two-axis splitting, hiding,
        // and moving the panel that originally supplied the window identity.
        prefs.dock_panel(
            PanelId::Properties,
            DockDestination::FloatingTabs {
                window: PanelId::Library,
                group: 1,
                index: 1,
            },
        );
        prefs.dock_panel(
            PanelId::Properties,
            DockDestination::FloatingTabs {
                window: PanelId::Library,
                group: 1,
                index: 0,
            },
        );
        prefs.dock_panel(
            PanelId::Actions,
            DockDestination::FloatingSplit {
                window: PanelId::Library,
                group: None,
                edge: DockEdge::Right,
            },
        );
        prefs.dock_panel(
            PanelId::Tools,
            DockDestination::FloatingSplit {
                window: PanelId::Library,
                group: None,
                edge: DockEdge::Top,
            },
        );
        prefs.dock_panel(PanelId::Library, DockDestination::Edge(DockEdge::Left));
        assert_eq!(
            prefs.panel_surface(PanelId::Properties),
            DockSurface::Floating(PanelId::Library)
        );
        prefs.dock_panel(
            PanelId::Output,
            DockDestination::FloatingTabs {
                window: PanelId::Library,
                group: 1,
                index: 1,
            },
        );
        prefs.set_panel_visible(PanelId::Properties, false);
        let floating_window = prefs
            .floating
            .iter_mut()
            .find(|window| window.id == PanelId::Library)
            .unwrap();
        floating_window.set_ratio(&[], 0.35);
        assert_eq!(
            floating_window.group(1).unwrap().tabs,
            [PanelId::Properties, PanelId::Output]
        );
        let all_panels: Vec<_> = prefs
            .floating
            .iter()
            .flat_map(FloatingDock::panels)
            .collect();
        assert_eq!(
            all_panels
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            all_panels.len()
        );
        let mut storage = Storage::default();
        prefs.save(&mut storage);
        assert_eq!(WorkspacePrefs::load(Some(&storage)), prefs);
        assert!(storage.values[PREFS_KEY].len() < MAX_PREFS_BYTES);
        assert!(
            PanelId::DETACHABLE
                .into_iter()
                .all(|p| p.viewport_key().is_some())
        );
        assert_ne!(
            PanelId::Tools.viewport_key(),
            PanelId::Timeline.viewport_key()
        );
        // A header moves its entire tab group, preserving hidden utility tabs,
        // while all four root/native edges remain valid destinations.
        let tabs = [
            PanelId::Properties,
            PanelId::Align,
            PanelId::Transform,
            PanelId::Color,
        ];
        for edge in DockEdge::ALL {
            let mut grouped = WorkspacePrefs::default();
            grouped.dock_group(&tabs, PanelId::Properties, DockDestination::Edge(edge));
            let location = grouped.dock.location(PanelId::Properties).unwrap();
            assert_eq!(location.edge, edge);
            assert!(
                tabs.iter()
                    .all(|p| grouped.dock.location(*p).unwrap().group == location.group)
            );
            let floating = grouped.float_group(
                &tabs,
                PanelId::Properties,
                window([300., 200.], [640., 480.]),
            );
            assert_eq!(
                grouped
                    .floating_window(PanelId::Properties)
                    .unwrap()
                    .panels(),
                tabs
            );
            assert_eq!(
                grouped.float_group(
                    &tabs,
                    PanelId::Properties,
                    window([320., 200.], [640., 480.])
                ),
                floating
            );
            let other = grouped.float_panel(PanelId::Library, None);
            grouped.dock_group(
                &tabs,
                PanelId::Properties,
                DockDestination::FloatingSplit {
                    window: other,
                    group: None,
                    edge,
                },
            );
            assert_eq!(
                grouped.panel_surface(PanelId::Properties),
                DockSurface::Floating(other)
            );
            let tree = grouped
                .floating_window(PanelId::Properties)
                .unwrap()
                .root
                .as_ref()
                .unwrap();
            let group = tree.location(PanelId::Properties).unwrap().0;
            assert_eq!(tree.group(group).unwrap().tabs, tabs);
            assert_eq!(tree.group(group).unwrap().active, PanelId::Properties);
            for panel in &tabs[1..] {
                assert!(!grouped.is_panel_visible(*panel));
            }
            grouped.dock_group(&tabs, PanelId::Properties, DockDestination::Edge(edge));
            let encoded = serde_json::to_string(&grouped).unwrap();
            assert_eq!(WorkspacePrefs::from_json(&encoded), grouped);
        }
        prefs.apply_template(WorkspaceTemplate::Classic);
        assert_eq!(prefs.theme, ThemeMode::Dark);
        assert_eq!(prefs.detached, DetachedPanels::default());
    }
    #[test]
    fn invalid_geometry_cannot_overwrite_last_good_snapshot() {
        let mut prefs = WorkspacePrefs::default();
        let good = window([70., 80.], [680., 440.]);
        prefs.remember_geometry(PanelId::Actions, good);
        for bad in [
            window([f32::NAN, 0.], [680., 440.]),
            window([0., 0.], [0., 440.]),
            window([0., 0.], [f32::INFINITY, 440.]),
        ] {
            assert!(!prefs.remember_geometry(PanelId::Actions, bad));
            assert_eq!(prefs.detached_geometry(PanelId::Actions), Some(good));
        }
        assert!(prefs.remember_geometry(PanelId::Library, good));
        let parsed = WorkspacePrefs::from_json(
            r#"{"theme":"dark","detached":{"actions":{"placement":"detached","geometry":{"outer_position":[1e30,0],"inner_size":[600,400]}}}}"#,
        );
        assert_eq!(parsed.theme, ThemeMode::Dark);
        assert_eq!(parsed.placement(PanelId::Actions), PanelPlacement::Detached);
        assert_eq!(parsed.detached_geometry(PanelId::Actions), None);
    }
    #[test]
    fn recovery_preserves_connected_secondary_and_partial_windows() {
        let main = screen([0., 24.], [1440., 826.]);
        let left = screen([-1920., -180.], [1920., 1080.]);
        for geometry in [
            window([-1800., -100.], [680., 440.]),
            window([1400., 60.], [680., 440.]),
            window([-500., 60.], [680., 440.]),
        ] {
            let recovered = recover_geometry(
                PanelId::Actions,
                Some(geometry),
                &[main, left, screen([1440., 0.], [1920., 1080.])],
                main,
            );
            assert_eq!(recovered, geometry);
        }
    }
    #[test]
    fn recovery_requires_a_reachable_titlebar_and_fits_removed_monitor_geometry() {
        let main = screen([0., 24.], [1440., 826.]);
        for geometry in [
            window([-1800., -100.], [680., 440.]),
            window([100., -300.], [680., 440.]),
            window([1430., 50.], [680., 440.]),
        ] {
            let recovered = recover_geometry(PanelId::Actions, Some(geometry), &[main], main);
            assert!(recovered.outer_position[0] >= 0.);
            assert!(recovered.outer_position[1] >= 24.);
            assert!(recovered.outer_position[0] + recovered.inner_size[0] + 16. <= 1440.);
            assert!(recovered.outer_position[1] + recovered.inner_size[1] + 40. <= 850.);
        }
        // A sliver of content below the menu bar cannot make its titlebar usable.
        let titlebar_hidden = window([100., 0.], [680., 440.]);
        assert_eq!(
            recover_geometry(PanelId::Actions, Some(titlebar_hidden), &[main], main).outer_position
                [1],
            24.
        );
    }
    #[test]
    fn recovery_bounds_huge_sizes_and_invalid_screen_input() {
        let small = screen([20., 30.], [500., 400.]);
        let recovered = recover_geometry(
            PanelId::Actions,
            Some(window([50., 50.], [1e20, 1e20])),
            &[small],
            small,
        );
        assert_eq!(recovered, window([20., 30.], [484., 360.]));
        let invalid = screen([f32::NAN, 0.], [0., f32::INFINITY]);
        let recovered = recover_geometry(PanelId::Tools, None, &[invalid], invalid);
        assert!(recovered.outer_position.into_iter().all(f32::is_finite));
        assert!(
            recovered
                .inner_size
                .into_iter()
                .all(|v| v.is_finite() && v > 0.)
        );
        let tiny = screen([999_999., 0.], [120.1, 120.1]);
        let recovered = recover_geometry(
            PanelId::Actions,
            Some(window([999_999., 0.], [680., 440.])),
            &[tiny],
            tiny,
        );
        assert!(recovered.inner_size[0] < 120.1 && recovered.inner_size[1] < 120.1);
    }
}
