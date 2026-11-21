use super::*;

/// Window placement belongs to the workspace, never to the authored Project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelPlacement {
    #[default]
    Docked,
    Detached,
}

/// Magnetic positions for the lone floating Tools window around the Stage workspace.
/// Side-centre anchors use the compact vertical rail; the other anchors use the
/// horizontal tool layout. This is workspace state, never authored document data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolsStageAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}
impl ToolsStageAnchor {
    pub const ALL: [Self; 8] = [
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::MiddleLeft,
        Self::MiddleRight,
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
    ];

    pub fn vertical(self) -> bool {
        matches!(self, Self::MiddleLeft | Self::MiddleRight)
    }

    pub fn bottom_aligned(self) -> bool {
        matches!(
            self,
            Self::BottomLeft | Self::BottomCenter | Self::BottomRight
        )
    }

    /// Popovers should open toward the Stage, away from the closest outer edge.
    pub fn expands_right(self) -> bool {
        !matches!(self, Self::TopRight | Self::MiddleRight | Self::BottomRight)
    }
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
            | PanelId::Color
            | PanelId::Ease
            | PanelId::Scene
            | PanelId::History
            | PanelId::Swatches
            | PanelId::Info
            | PanelId::Project
            | PanelId::Filters
            | PanelId::CodeSnippets
            | PanelId::MovieExplorer
            | PanelId::Brushes => Self::Right,
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
    /// Split just the target leaf on the chosen side of a main-window area.
    SplitGroup {
        area: DockEdge,
        group: u32,
        side: DockEdge,
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

/// Binary split geometry shared by floating windows and nested main-window areas.
/// Main leaves reference the tab records by ID; floating leaves own their tab records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "content", rename_all = "snake_case")]
pub enum DockNode<G> {
    Group(G),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<Self>,
        second: Box<Self>,
    },
}
pub type FloatingNode = DockNode<DockGroup>;
pub type MainDockNode = DockNode<u32>;

impl<G> DockNode<G> {
    fn branch(axis: SplitAxis, ratio: f32, first: Self, second: Self) -> Self {
        Self::Split {
            axis,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    fn beside(self, side: DockEdge, added: G) -> Self {
        let axis = if side.vertical() {
            SplitAxis::Horizontal
        } else {
            SplitAxis::Vertical
        };
        if matches!(side, DockEdge::Top | DockEdge::Left) {
            Self::branch(axis, 0.5, Self::Group(added), self)
        } else {
            Self::branch(axis, 0.5, self, Self::Group(added))
        }
    }

    fn leaf_mut(&mut self, matches: &impl Fn(&G) -> bool) -> Option<&mut Self> {
        match self {
            Self::Group(g) if matches(g) => Some(self),
            Self::Group(_) => None,
            Self::Split { first, second, .. } => {
                first.leaf_mut(matches).or_else(|| second.leaf_mut(matches))
            }
        }
    }

    fn split_leaf(&mut self, matches: impl Fn(&G) -> bool, side: DockEdge, new: G)
    where
        G: Clone,
    {
        if let Some(leaf) = self.leaf_mut(&matches) {
            *leaf = leaf.clone().beside(side, new);
        }
    }

    /// Preserve the structure of surviving leaves and collapse a branch if one side vanishes.
    fn map_groups<T>(&self, visit: &mut impl FnMut(&G) -> Option<T>) -> Option<DockNode<T>> {
        match self {
            Self::Group(group) => visit(group).map(DockNode::Group),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let ratio = if ratio.is_finite() {
                    ratio.clamp(0.001, 0.999)
                } else {
                    0.5
                };
                DockNode::join(
                    *axis,
                    ratio,
                    first.map_groups(visit),
                    second.map_groups(visit),
                )
            }
        }
    }

    fn set_ratio(&mut self, path: &[bool], value: f32) {
        let mut node = self;
        for side in path {
            let Self::Split { first, second, .. } = node else {
                return;
            };
            node = if *side { second } else { first };
        }
        if let Self::Split { ratio, .. } = node
            && value.is_finite()
        {
            *ratio = value.clamp(0.05, 0.95);
        }
    }

    fn join(
        axis: SplitAxis,
        ratio: f32,
        first: Option<Self>,
        second: Option<Self>,
    ) -> Option<Self> {
        match (first, second) {
            (Some(first), Some(second)) => Some(Self::branch(axis, ratio, first, second)),
            (first, second) => first.or(second),
        }
    }
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
    pub(super) fn location(&self, panel: PanelId) -> Option<(u32, usize)> {
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
    pub(super) fn remove(self, panel: PanelId, keep: Option<u32>) -> Option<Self> {
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
    pub(super) fn split(mut self, target: Option<u32>, edge: DockEdge, group: DockGroup) -> Self {
        if let Some(id) = target {
            self.split_leaf(|g| g.id == id, edge, group);
            self
        } else {
            self.beside(edge, group)
        }
    }
    pub fn minimum_size(&self) -> [f32; 2] {
        match self {
            Self::Group(group) => {
                if group.tabs.is_empty() {
                    return [80., 80.];
                }
                group.tabs.iter().fold([0.0_f32, 0.0_f32], |size, p| {
                    let minimum = minimum_window_size(*p);
                    [size[0].max(minimum[0]), size[1].max(minimum[1])]
                })
            }
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
    pub fn maximum_size(&self) -> [f32; 2] {
        match self {
            Self::Group(group) => {
                if group.tabs.is_empty() {
                    return [4096., 2160.];
                }
                group.tabs.iter().fold([0.0_f32, 0.0_f32], |size, p| {
                    let maximum = maximum_window_size(*p);
                    [size[0].max(maximum[0]), size[1].max(maximum[1])]
                })
            }
            Self::Split {
                axis,
                first,
                second,
                ..
            } => {
                let a = first.maximum_size();
                let b = second.maximum_size();
                match axis {
                    SplitAxis::Horizontal => {
                        [(a[0] + b[0] + 5.).min(4096.), a[1].max(b[1]).min(2160.)]
                    }
                    SplitAxis::Vertical => {
                        [a[0].max(b[0]).min(4096.), (a[1] + b[1] + 5.).min(2160.)]
                    }
                }
            }
        }
    }
    pub fn normalize_active(&mut self, panels: PanelVisibility) -> bool {
        match self {
            Self::Group(group) => group.normalize_active(panels),
            Self::Split { first, second, .. } => {
                first.normalize_active(panels) | second.normalize_active(panels)
            }
        }
    }
    pub(super) fn sanitize(
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_anchor: Option<ToolsStageAnchor>,
    pub(super) next_group: u32,
}
impl FloatingDock {
    pub(super) fn new(id: PanelId, panel: PanelId, geometry: Option<WindowGeometry>) -> Self {
        Self {
            id,
            root: Some(FloatingNode::Group(DockGroup {
                id: 1,
                tabs: vec![panel],
                active: panel,
                weight: 1.,
            })),
            geometry,
            tools_anchor: None,
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
    pub(super) fn fresh_group(&mut self, panel: PanelId) -> DockGroup {
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
        if let Some(root) = &mut self.root {
            root.set_ratio(path, value);
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
impl DockGroup {
    pub fn active_visible(&self, panels: PanelVisibility) -> Option<PanelId> {
        self.tabs
            .iter()
            .copied()
            .find(|&p| p == self.active && panels.is_visible(p))
            .or_else(|| self.tabs.iter().copied().find(|&p| panels.is_visible(p)))
    }
    pub fn normalize_active(&mut self, panels: PanelVisibility) -> bool {
        if let Some(active) = self.active_visible(panels)
            && self.active != active
        {
            self.active = active;
            return true;
        }
        false
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockArea {
    pub size: f32,
    pub groups: Vec<DockGroup>,
    /// Absent on original stacked areas. Tab records remain in `groups` for both forms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<MainDockNode>,
}
/// Identifies the sibling subtree of a detached leaf by its first and last group.
/// This remains usable if another part of the layout is resized while the panel floats.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct DockReturnSplit {
    sibling: [u32; 2],
    side: DockEdge,
    ratio: f32,
}

impl MainDockNode {
    fn ends(&self) -> [u32; 2] {
        match self {
            Self::Group(id) => [*id; 2],
            Self::Split { first, second, .. } => [first.ends()[0], second.ends()[1]],
        }
    }

    pub(super) fn return_split(&self, group: u32) -> Option<DockReturnSplit> {
        let Self::Split {
            first,
            second,
            axis,
            ratio,
        } = self
        else {
            return None;
        };
        let (leading, trailing) = match axis {
            SplitAxis::Horizontal => (DockEdge::Left, DockEdge::Right),
            SplitAxis::Vertical => (DockEdge::Top, DockEdge::Bottom),
        };
        for (child, sibling, side) in [(first, second, leading), (second, first, trailing)] {
            if matches!(**child, Self::Group(id) if id == group) {
                return Some(DockReturnSplit {
                    sibling: sibling.ends(),
                    side,
                    ratio: *ratio,
                });
            }
        }
        first
            .return_split(group)
            .or_else(|| second.return_split(group))
    }

    fn reattach(&mut self, id: u32, slot: DockReturnSplit) -> bool {
        if self.ends() == slot.sibling {
            let mut split = self.clone().beside(slot.side, id);
            if let Self::Split { ratio, .. } = &mut split {
                *ratio = slot.ratio;
            }
            *self = split;
            return true;
        }
        match self {
            Self::Group(_) => false,
            Self::Split { first, second, .. } => {
                first.reattach(id, slot) || second.reattach(id, slot)
            }
        }
    }

    fn from_stack(groups: &[DockGroup], axis: SplitAxis) -> Option<Self> {
        match groups {
            [] => None,
            [only] => Some(Self::Group(only.id)),
            _ => {
                let (left, right) = groups.split_at(groups.len() / 2);
                let total: f32 = groups.iter().map(|g| g.weight).sum();
                let leading: f32 = left.iter().map(|g| g.weight).sum();
                Self::join(
                    axis,
                    leading / total,
                    Self::from_stack(left, axis),
                    Self::from_stack(right, axis),
                )
            }
        }
    }

    fn share(&self, group: u32) -> f32 {
        match self {
            Self::Group(id) => {
                if *id == group {
                    1.
                } else {
                    0.
                }
            }
            Self::Split {
                ratio,
                first,
                second,
                ..
            } => first.share(group) * ratio + second.share(group) * (1. - ratio),
        }
    }

    fn apply_weights(&mut self, groups: &[DockGroup]) -> f32 {
        match self {
            Self::Group(id) => groups.iter().find(|g| g.id == *id).map_or(0., |g| g.weight),
            Self::Split {
                ratio,
                first,
                second,
                ..
            } => {
                let a = first.apply_weights(groups);
                let b = second.apply_weights(groups);
                if a + b > 0. {
                    *ratio = a / (a + b);
                }
                a + b
            }
        }
    }
}

impl DockArea {
    /// Resolved groups for the shared split renderer, including temporarily hidden leaves.
    pub fn nested_layout(&self) -> Option<FloatingNode> {
        self.root
            .as_ref()?
            .map_groups(&mut |id| self.groups.iter().find(|g| g.id == *id).cloned())
    }

    pub fn set_ratio(&mut self, path: &[bool], value: f32) {
        if let Some(root) = &mut self.root {
            root.set_ratio(path, value);
        }
    }

    /// Recreate the former leaf at its sibling subtree. Keep the normal docking fallback
    /// when that subtree has been reorganised or removed since detaching.
    pub(super) fn restore_leaf(&mut self, group: u32, slot: DockReturnSplit) {
        if !slot.ratio.is_finite() || !(0.0..1.0).contains(&slot.ratio) {
            return;
        }
        let previous = if let Some(root) = &self.root {
            root.map_groups(&mut |id| (*id != group).then_some(*id))
        } else if self.groups.len() == 2 {
            self.groups
                .iter()
                .find(|g| g.id != group)
                .map(|g| MainDockNode::Group(g.id))
        } else {
            None
        };
        if let Some(mut tree) = previous
            && tree.reattach(group, slot)
        {
            self.root = Some(tree);
            // Keep the tab registry in the same reading order as the restored tree.
            if let Some(index) = self.groups.iter().position(|g| g.id == group) {
                let restored = self.groups.remove(index);
                let after = matches!(slot.side, DockEdge::Right | DockEdge::Bottom);
                let anchor = slot.sibling[usize::from(after)];
                let neighbour = self
                    .groups
                    .iter()
                    .position(|g| g.id == anchor)
                    .expect("restored sibling");
                self.groups.insert(neighbour + usize::from(after), restored);
            }
        }
    }

    fn prune_root(&mut self) {
        if let Some(root) = self.root.take() {
            let mut remaining: std::collections::HashSet<_> =
                self.groups.iter().map(|g| g.id).collect();
            let cleaned = root.map_groups(&mut |id| remaining.remove(id).then_some(*id));
            // An incomplete stored tree cannot position all panels: fall back to their intact stack.
            if remaining.is_empty() && self.groups.len() > 1 {
                self.root = cleaned;
            }
        }
    }

    fn push_group(&mut self, edge: DockEdge, group: DockGroup) {
        if let Some(root) = self.root.take() {
            let side = if edge.vertical() {
                DockEdge::Bottom
            } else {
                DockEdge::Right
            };
            self.root = Some(root.beside(side, group.id));
        }
        self.groups.push(group);
    }

    fn split_group(&mut self, edge: DockEdge, target: u32, side: DockEdge, mut group: DockGroup) {
        let Some(index) = self.groups.iter().position(|g| g.id == target) else {
            self.push_group(edge, group);
            return;
        };
        // Keep the former stack geometry when the first explicit split builds a tree.
        if self.root.is_none() {
            let axis = if edge.vertical() {
                SplitAxis::Vertical
            } else {
                SplitAxis::Horizontal
            };
            self.root = MainDockNode::from_stack(&self.groups, axis);
        }
        if let Some(root) = self.root.as_mut() {
            root.split_leaf(|id| *id == target, side, group.id);
        }
        self.groups[index].weight *= 0.5;
        group.weight = self.groups[index].weight;
        let after = matches!(side, DockEdge::Right | DockEdge::Bottom);
        self.groups.insert(index + usize::from(after), group);
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DockLayout {
    pub top: DockArea,
    pub bottom: DockArea,
    pub left: DockArea,
    pub right: DockArea,
    pub(super) next_group: u32,
}
impl DockLayout {
    /// Share of a stacked side-dock area the priority group gets (R-054 Inspector / Library priority).
    pub const PRIORITY_WEIGHT: f32 = 0.68;

    /// Give the stacked group holding `panel` priority in its side area (the others share the rest).
    /// Returns false when the panel is not in a stacked left / right area.
    pub fn give_priority(&mut self, panel: PanelId) -> bool {
        for area in [&mut self.left, &mut self.right] {
            if area.groups.len() < 2 || !area.groups.iter().any(|g| g.tabs.contains(&panel)) {
                continue;
            }
            let others = (1. - Self::PRIORITY_WEIGHT) / (area.groups.len() - 1) as f32;
            for group in &mut area.groups {
                group.weight = if group.tabs.contains(&panel) {
                    Self::PRIORITY_WEIGHT
                } else {
                    others
                };
            }
            if let Some(root) = &mut area.root {
                root.apply_weights(&area.groups);
            }
            return true;
        }
        false
    }

    /// Whether the stacked group holding `panel` currently has the larger share of its area.
    pub fn has_priority(&self, panel: PanelId) -> bool {
        [&self.left, &self.right].into_iter().any(|area| {
            if let Some(root) = &area.root {
                return area
                    .groups
                    .iter()
                    .find(|g| g.tabs.contains(&panel))
                    .is_some_and(|g| root.share(g.id) > 0.5);
            }
            let total: f32 = area.groups.iter().map(|g| g.weight).sum();
            area.groups.len() > 1
                && area
                    .groups
                    .iter()
                    .any(|g| g.tabs.contains(&panel) && g.weight > total / 2.)
        })
    }
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
                root: None,
                size: crate::theme::timeline_h(),
                groups: vec![],
            },
            bottom: DockArea {
                root: None,
                size: neoflash_ui::metrics::BOTTOM_DOCK,
                groups: vec![group(
                    2,
                    vec![
                        PanelId::Timeline,
                        PanelId::Actions,
                        PanelId::Output,
                        PanelId::Accessibility,
                        PanelId::MotionEditor,
                        PanelId::Debugger,
                        PanelId::CompilerErrors,
                    ],
                )],
            },
            // Base Tools layout is two columns; one column (with a "…" overflow) is the narrow variant.
            left: DockArea {
                root: None,
                size: neoflash_ui::metrics::tools_two_columns() + 6.,
                groups: vec![group(3, vec![PanelId::Tools])],
            },
            right: DockArea {
                root: None,
                // `width/inspector` panel + the 6 pt resize strip.
                size: neoflash_ui::metrics::inspector() + 6.,
                groups: vec![
                    DockGroup {
                        // Inspector priority (R-054): Properties over a Library that stays usable.
                        weight: DockLayout::PRIORITY_WEIGHT,
                        ..group(
                            5,
                            vec![
                                PanelId::Properties,
                                PanelId::Filters,
                                PanelId::Align,
                                PanelId::Transform,
                                PanelId::Color,
                                PanelId::Swatches,
                                PanelId::Info,
                                PanelId::Ease,
                                PanelId::History,
                                PanelId::Scene,
                                PanelId::Project,
                                PanelId::CodeSnippets,
                                PanelId::MovieExplorer,
                                PanelId::Brushes,
                            ],
                        )
                    },
                    DockGroup {
                        weight: 1. - DockLayout::PRIORITY_WEIGHT,
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
    pub(super) fn remove(&mut self, panel: PanelId, keep_group: Option<u32>) {
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
            area.prune_root();
        }
    }
    pub(super) fn fresh_group(&mut self, panel: PanelId) -> DockGroup {
        let id = self.next_group;
        self.next_group += 1;
        DockGroup {
            id,
            tabs: vec![panel],
            active: panel,
            weight: 1.,
        }
    }
    pub(super) fn insert(&mut self, panel: PanelId, destination: DockDestination) {
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
                    self.next_group = self.next_group.max(group.id + 1);
                    self.area_mut(location.edge)
                        .push_group(location.edge, group);
                }
            }
            DockDestination::SplitGroup { area, group, side } => {
                if self
                    .area(area)
                    .groups
                    .iter()
                    .find(|g| g.id == group)
                    .is_none_or(|g| g.tabs == [panel])
                {
                    return;
                }
                self.remove(panel, None);
                let added = self.fresh_group(panel);
                self.area_mut(area).split_group(area, group, side, added);
            }
            DockDestination::Edge(edge) => {
                self.remove(panel, None);
                let mut group = self.fresh_group(panel);
                let area = self.area_mut(edge);
                group.weight = area.groups.last().map_or(1., |g| g.weight);
                area.push_group(edge, group);
            }
            DockDestination::FloatingTabs { .. } | DockDestination::FloatingSplit { .. } => {
                unreachable!("floating destinations are routed by WorkspacePrefs")
            }
        }
    }
    pub(super) fn select(&mut self, panel: PanelId) -> bool {
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
    pub(super) fn sanitize(&mut self, detached: DetachedPanels) {
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
            area.prune_root();
        }
        let minimum_id = ids.into_iter().max().unwrap_or(0) + 1;
        self.next_group = self.next_group.clamp(minimum_id, 1_000_008);
        let active: Vec<_> = DockEdge::ALL
            .into_iter()
            .flat_map(|edge| {
                self.area(edge)
                    .groups
                    .iter()
                    .map(move |group| (edge, group.id, group.active))
            })
            .collect();
        for panel in PanelId::DETACHABLE {
            if detached
                .get(panel)
                .is_some_and(|w| w.placement == PanelPlacement::Docked)
                && self.location(panel).is_none()
            {
                self.insert_at_default(panel);
            }
        }
        // A newly available utility panel is hidden by default. Loading an older workspace
        // must not select that new tab or replace the user's remembered active tab.
        for (edge, id, panel) in active {
            if let Some(group) = self
                .area_mut(edge)
                .groups
                .iter_mut()
                .find(|group| group.id == id)
            {
                group.active = panel;
            }
        }
    }
    /// Dock a panel that is in no group (new utility panels, layouts saved before they existed):
    /// utility panels join the Properties group, the Motion editor the Timeline group, anything
    /// else the first group of its default edge.
    pub fn insert_at_default(&mut self, panel: PanelId) {
        let edge = DockEdge::default_for(panel);
        if panel == PanelId::Swatches
            && let Some(g) = self
                .area(edge)
                .groups
                .iter()
                .find(|group| group.tabs.contains(&PanelId::Color))
        {
            self.insert(
                panel,
                DockDestination::Tabs(DockLocation {
                    edge,
                    group: g.id,
                    index: g.tabs.len(),
                }),
            );
            return;
        }
        let companion = match panel {
            PanelId::MotionEditor => Some(PanelId::Timeline),
            PanelId::Debugger | PanelId::CompilerErrors => Some(PanelId::Output),
            p if PanelId::UTILITY.contains(&p) => Some(PanelId::Properties),
            _ => None,
        };
        let destination = self
            .area(edge)
            .groups
            .iter()
            .find(|group| companion.is_some_and(|c| group.tabs.contains(&c)))
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
    pub(super) fn sanitized(self, panel: PanelId) -> Option<Self> {
        let max = maximum_window_size(panel);
        self.sanitized_with_bounds(minimum_window_size(panel), [max[0] * 2.0, max[1] * 2.0])
    }
    pub(crate) fn sanitized_with_bounds(self, min: [f32; 2], max: [f32; 2]) -> Option<Self> {
        if !self
            .outer_position
            .into_iter()
            .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
            || !self.inner_size.into_iter().all(|v| v.is_finite() && v > 0.)
        {
            return None;
        }
        let max_w = max[0].max(min[0]);
        let max_h = max[1].max(min[1]);
        Some(Self {
            outer_position: self.outer_position,
            inner_size: [
                self.inner_size[0].clamp(min[0], max_w),
                self.inner_size[1].clamp(min[1], max_h),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) return_split: Option<DockReturnSplit>,
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
    pub ease: PanelWindowPrefs,
    pub scene: PanelWindowPrefs,
    pub motion_editor: PanelWindowPrefs,
    pub debugger: PanelWindowPrefs,
    pub history: PanelWindowPrefs,
    pub swatches: PanelWindowPrefs,
    pub info: PanelWindowPrefs,
    pub project: PanelWindowPrefs,
    pub filters: PanelWindowPrefs,
    pub compiler_errors: PanelWindowPrefs,
    pub code_snippets: PanelWindowPrefs,
    pub movie_explorer: PanelWindowPrefs,
    pub brushes: PanelWindowPrefs,
}
impl DetachedPanels {
    pub(super) fn get(&self, panel: PanelId) -> Option<&PanelWindowPrefs> {
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
            PanelId::Ease => Some(&self.ease),
            PanelId::Scene => Some(&self.scene),
            PanelId::MotionEditor => Some(&self.motion_editor),
            PanelId::Debugger => Some(&self.debugger),
            PanelId::History => Some(&self.history),
            PanelId::Swatches => Some(&self.swatches),
            PanelId::Info => Some(&self.info),
            PanelId::Project => Some(&self.project),
            PanelId::Filters => Some(&self.filters),
            PanelId::CompilerErrors => Some(&self.compiler_errors),
            PanelId::CodeSnippets => Some(&self.code_snippets),
            PanelId::MovieExplorer => Some(&self.movie_explorer),
            PanelId::Brushes => Some(&self.brushes),
        }
    }
    pub(super) fn get_mut(&mut self, panel: PanelId) -> Option<&mut PanelWindowPrefs> {
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
            PanelId::Ease => Some(&mut self.ease),
            PanelId::Scene => Some(&mut self.scene),
            PanelId::MotionEditor => Some(&mut self.motion_editor),
            PanelId::Debugger => Some(&mut self.debugger),
            PanelId::History => Some(&mut self.history),
            PanelId::Swatches => Some(&mut self.swatches),
            PanelId::Info => Some(&mut self.info),
            PanelId::Project => Some(&mut self.project),
            PanelId::Filters => Some(&mut self.filters),
            PanelId::CompilerErrors => Some(&mut self.compiler_errors),
            PanelId::CodeSnippets => Some(&mut self.code_snippets),
            PanelId::MovieExplorer => Some(&mut self.movie_explorer),
            PanelId::Brushes => Some(&mut self.brushes),
        }
    }
    pub(super) fn sanitize(&mut self) {
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
    pub(super) fn center(self) -> [f32; 2] {
        [
            self.origin[0] + self.size[0] * 0.5,
            self.origin[1] + self.size[1] * 0.5,
        ]
    }
    pub(super) fn distance_squared(self, point: [f32; 2]) -> f64 {
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

/// Minimum logical width of a standalone `Tools` panel window: the one-column rail
/// (`width/tools-1`), so the 1-column layout is reachable when resizing narrow.
pub fn minimum_tools_window_width() -> f32 {
    neoflash_ui::metrics::tools_rail()
}

/// Value of the density metric `name` (a `neoflash_ui::metrics` function) in `density`, whatever
/// profile the UI thread currently uses.
pub(super) fn density_metric(name: &str, density: neoflash_ui::metrics::Density) -> f32 {
    let column = neoflash_ui::metrics::Density::ALL
        .iter()
        .position(|candidate| *candidate == density)
        .unwrap_or(0);
    neoflash_ui::metrics::DENSITY_TABLE
        .iter()
        .find(|(metric, _)| *metric == name)
        .map_or(0., |(_, values)| values[column])
}

/// A width tuned for the former 320 pt inspector, carried to the current density
/// (`old × inspector() / 320`, on the 4 pt grid).
pub(super) fn inspector_scaled(old: f32) -> f32 {
    (old * neoflash_ui::metrics::inspector() / 320. / 4.).round() * 4.
}

/// Smallest useful window (and dock) size of a panel. Every inspector-family panel shares the
/// `inspector()` minimum width, so a side dock has one minimum whichever of them is active; only
/// panels that need more room (timeline, debugger, compiler errors, ease, motion editor) differ.
pub fn minimum_window_size(panel: PanelId) -> [f32; 2] {
    let inspector = neoflash_ui::metrics::inspector();
    match panel {
        PanelId::Tools => [minimum_tools_window_width(), 160.],
        PanelId::Timeline => [inspector_scaled(320.), 180.],
        PanelId::Library => [inspector, 300.],
        PanelId::Align
        | PanelId::Transform
        | PanelId::Color
        | PanelId::Scene
        | PanelId::History
        | PanelId::Info => [inspector, 190.],
        PanelId::Swatches => [inspector, 360.],
        PanelId::Project | PanelId::CodeSnippets | PanelId::MovieExplorer => [inspector, 280.],
        PanelId::Filters => [inspector, 260.],
        PanelId::Debugger => [inspector_scaled(620.), 220.],
        PanelId::CompilerErrors => [inspector_scaled(360.), 180.],
        PanelId::Ease => [inspector_scaled(320.), 420.],
        PanelId::MotionEditor => [inspector_scaled(640.), 260.],
        // Properties, Actions, Accessibility, Output, Brushes.
        _ => [inspector, 220.],
    }
}
pub fn maximum_window_size(panel: PanelId) -> [f32; 2] {
    match panel {
        PanelId::Tools => [360., 960.],
        PanelId::Align | PanelId::Transform | PanelId::Color | PanelId::Scene | PanelId::Info => {
            [640., 900.]
        }
        PanelId::Swatches | PanelId::Filters | PanelId::Brushes => [680., 960.],
        PanelId::Ease => [720., 960.],
        _ => [4096., 2160.],
    }
}
/// Size of a freshly detached panel window. Side panels open at the `inspector()` width, Tools
/// at its two-column width; wide bottom-dock panels keep their working width.
pub fn default_window_size(panel: PanelId) -> [f32; 2] {
    let inspector = neoflash_ui::metrics::inspector();
    match panel {
        PanelId::Tools => [neoflash_ui::metrics::tools_two_columns(), 600.],
        PanelId::Brushes => [inspector, 650.],
        PanelId::Timeline => [900., 280.],
        PanelId::Library => [inspector, 600.],
        PanelId::Properties => [inspector, 560.],
        PanelId::Align | PanelId::Transform | PanelId::Color => [inspector, 440.],
        PanelId::Swatches => [inspector, 490.],
        PanelId::Filters => [inspector, 460.],
        PanelId::Info => [inspector, 220.],
        PanelId::History => [inspector, 320.],
        PanelId::Project => [inspector, 484.],
        PanelId::CodeSnippets | PanelId::MovieExplorer => [inspector, 480.],
        PanelId::Debugger => [960., 320.],
        PanelId::CompilerErrors => [inspector_scaled(560.), 260.],
        PanelId::Scene => [inspector, 360.],
        PanelId::Ease => [inspector_scaled(376.), 640.],
        PanelId::MotionEditor => [960., 520.],
        // Actions, Accessibility, Output.
        _ => [680., 440.],
    }
}

/// Recover only a detached panel, preserving partially offscreen windows whose
/// titlebar still has an 80x22-point draggable area on ANY connected monitor.
/// Pure bounded calculation; the caller applies the result once on creation or
/// topology changes, not every paint. Retain live user movement after that.
#[cfg_attr(not(test), allow(dead_code))]
pub fn recover_geometry(
    panel: PanelId,
    saved: Option<WindowGeometry>,
    monitors: &[ScreenRect],
    fallback: ScreenRect,
) -> WindowGeometry {
    recover_geometry_with_bounds(
        panel,
        saved,
        minimum_window_size(panel),
        maximum_window_size(panel),
        monitors,
        fallback,
    )
}

pub(crate) fn recover_geometry_with_bounds(
    panel: PanelId,
    saved: Option<WindowGeometry>,
    min_size: [f32; 2],
    max_size: [f32; 2],
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
    let saved = saved.and_then(|geometry| geometry.sanitized_with_bounds(min_size, max_size));
    let mut geometry = saved.unwrap_or_else(|| {
        let def = default_window_size(panel);
        let max_w = max_size[0].max(min_size[0]);
        let max_h = max_size[1].max(min_size[1]);
        WindowGeometry {
            outer_position: [fallback.origin[0] + 40., fallback.origin[1] + 40.],
            inner_size: [
                def[0].clamp(min_size[0], max_w),
                def[1].clamp(min_size[1], max_h),
            ],
        }
    });
    let reachable = screens.iter().copied().find(|screen| {
        let overlap = |axis: usize, length: f32| {
            (geometry.outer_position[axis] + length).min(screen.origin[axis] + screen.size[axis])
                - geometry.outer_position[axis].max(screen.origin[axis])
        };
        let min_x_overlap = 80.0_f32.min(geometry.inner_size[0] + CHROME[0]);
        overlap(0, geometry.inner_size[0] + CHROME[0]) >= min_x_overlap && overlap(1, 28.) >= 22.
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
