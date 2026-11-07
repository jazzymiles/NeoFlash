//! Extended WAI-ARIA role mapping, live regions, keyboard focus groups, automatic label
//! association, reading-order resolution, and accessibility tree diagnostics (`T-117`).

use crate::{
    AccessibleRole, Element, ElementKind, Id, Keyframe, LayerRole, Project, SymbolKind, Timeline,
    path_bounds, sample_layer_in_at_fps,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Rich WAI-ARIA role taxonomy for published HTML5/DOM accessibility overlays (`T-117`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentAccessibleRole {
    #[default]
    Auto,
    Button,
    CheckBox,
    RadioButton,
    ComboBox,
    List,
    ListItem,
    Menu,
    MenuItem,
    Slider,
    ProgressBar,
    Tab,
    TabPanel,
    Dialog,
    Heading,
    Link,
    Image,
    Group,
    Region,
    Application,
    Presentation,
}

impl ContentAccessibleRole {
    pub fn is_interactive(self) -> bool {
        matches!(
            self,
            Self::Button
                | Self::CheckBox
                | Self::RadioButton
                | Self::ComboBox
                | Self::MenuItem
                | Self::Slider
                | Self::Tab
                | Self::Link
        )
    }

    pub fn aria_role_str(self) -> &'static str {
        match self {
            Self::Auto => "generic",
            Self::Button => "button",
            Self::CheckBox => "checkbox",
            Self::RadioButton => "radio",
            Self::ComboBox => "combobox",
            Self::List => "list",
            Self::ListItem => "listitem",
            Self::Menu => "menu",
            Self::MenuItem => "menuitem",
            Self::Slider => "slider",
            Self::ProgressBar => "progressbar",
            Self::Tab => "tab",
            Self::TabPanel => "tabpanel",
            Self::Dialog => "dialog",
            Self::Heading => "heading",
            Self::Link => "link",
            Self::Image => "img",
            Self::Group => "group",
            Self::Region => "region",
            Self::Application => "application",
            Self::Presentation => "presentation",
        }
    }
}

/// ARIA live-region politeness level (`aria-live`) for dynamic status or score readouts (`T-117`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessibilityLivePoliteness {
    #[default]
    Off,
    Polite,
    Assertive,
}

const fn bool_true() -> bool {
    true
}

fn default_lang_en() -> String {
    "en".into()
}

/// Document-level accessibility configuration (Flash Accessibility panel + WAI-ARIA metadata, `T-117`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentAccessibility {
    #[serde(default = "bool_true")]
    pub auto_label: bool,
    #[serde(default = "bool_true")]
    pub accessible: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_lang_en")]
    pub language: String,
    #[serde(default = "bool_true")]
    pub reading_order_top_down: bool,
    #[serde(default = "bool_true")]
    pub focus_ring_visible: bool,
}

impl Default for DocumentAccessibility {
    fn default() -> Self {
        Self {
            auto_label: true,
            accessible: true,
            title: String::new(),
            description: String::new(),
            language: "en".into(),
            reading_order_top_down: true,
            focus_ring_visible: true,
        }
    }
}

impl DocumentAccessibility {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Per-element extended accessibility metadata stored on a [`Keyframe`] (`T-117`, format 9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtendedAccessibility {
    pub element_id: Id,
    #[serde(default)]
    pub role: ContentAccessibleRole,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shortcut: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub force_simple: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_auto_labeling: bool,
    #[serde(default = "bool_true")]
    pub tab_children: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub silent: bool,
    #[serde(default)]
    pub live: AccessibilityLivePoliteness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aria_labelled_by: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aria_described_by: Option<Id>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aria_controls: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading_order: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_group: Option<String>,
}

impl Default for ExtendedAccessibility {
    fn default() -> Self {
        Self {
            element_id: String::new(),
            role: ContentAccessibleRole::Auto,
            shortcut: String::new(),
            force_simple: false,
            no_auto_labeling: false,
            tab_children: true,
            silent: false,
            live: AccessibilityLivePoliteness::Off,
            aria_labelled_by: None,
            aria_described_by: None,
            aria_controls: Vec::new(),
            reading_order: None,
            focus_group: None,
        }
    }
}

impl ExtendedAccessibility {
    pub fn is_default(&self) -> bool {
        self.role == ContentAccessibleRole::Auto
            && self.shortcut.is_empty()
            && !self.force_simple
            && !self.no_auto_labeling
            && self.tab_children
            && !self.silent
            && self.live == AccessibilityLivePoliteness::Off
            && self.aria_labelled_by.is_none()
            && self.aria_described_by.is_none()
            && self.aria_controls.is_empty()
            && self.reading_order.is_none()
            && self.focus_group.is_none()
    }
}

/// Resolved accessibility node in the frame's reading/focus tree (`T-117`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityTreeNode {
    pub element_id: Id,
    pub role: ContentAccessibleRole,
    pub name: String,
    pub description: String,
    pub shortcut: String,
    pub tab_index: Option<i32>,
    pub reading_order: u32,
    pub focus_group: Option<String>,
    pub live: AccessibilityLivePoliteness,
    pub bounds: Option<[f64; 4]>,
    pub children: Vec<AccessibilityTreeNode>,
}

/// Authoring diagnostic emitted when validating a frame's accessibility tree (`T-117`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibilityDiagnostic {
    pub code: String,
    pub element_id: Option<Id>,
    pub message: String,
}

type OrderedAccessibilityNode = (Option<u32>, Option<i32>, [f64; 2], AccessibilityTreeNode);

fn estimate_element_bounds(el: &Element) -> [f64; 4] {
    let tx = el.transform.x;
    let ty = el.transform.y;
    let sx = el.transform.scale_x.abs().max(0.01);
    let sy = el.transform.scale_y.abs().max(0.01);
    match &el.kind {
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. }
        | ElementKind::Video { width, height } => [tx, ty, width * sx, height * sy],
        ElementKind::Bitmap { width, height, .. } => {
            [tx, ty, (*width as f64) * sx, (*height as f64) * sy]
        }
        ElementKind::TextBox { width, height, .. } => [tx, ty, width * sx, height * sy],
        ElementKind::Text {
            text, font_size, ..
        } => {
            let w = (text.chars().count().max(1) as f64) * font_size * 0.6 * sx;
            let h = font_size * 1.2 * sy;
            [tx, ty, w, h]
        }
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            if let Ok(Some(b)) = path_bounds(commands) {
                [
                    tx + b.x_min * sx,
                    ty + b.y_min * sy,
                    (b.x_max - b.x_min).max(1.0) * sx,
                    (b.y_max - b.y_min).max(1.0) * sy,
                ]
            } else {
                [tx, ty, 40.0 * sx, 40.0 * sy]
            }
        }
        ElementKind::Instance { .. } => [tx, ty, 100.0 * sx, 40.0 * sy],
    }
}

fn element_text_content(el: &Element) -> Option<String> {
    match &el.kind {
        ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        _ => None,
    }
}

fn infer_role(
    project: &Project,
    el: &Element,
    ext: Option<&ExtendedAccessibility>,
) -> ContentAccessibleRole {
    if let Some(ext) = ext
        && ext.role != ContentAccessibleRole::Auto
    {
        return ext.role;
    }
    if let Some(acc) = &el.accessibility {
        return match acc.role {
            AccessibleRole::Button => ContentAccessibleRole::Button,
            AccessibleRole::Image => ContentAccessibleRole::Image,
        };
    }
    match &el.kind {
        ElementKind::Instance { symbol_id, .. } => {
            match project.symbol(symbol_id).map(|s| s.kind) {
                Some(SymbolKind::Button) => ContentAccessibleRole::Button,
                Some(SymbolKind::MovieClip) => ContentAccessibleRole::Group,
                Some(SymbolKind::Graphic) | None => ContentAccessibleRole::Image,
            }
        }
        ElementKind::Bitmap { .. } => ContentAccessibleRole::Image,
        ElementKind::Text { .. } | ElementKind::TextBox { .. } => ContentAccessibleRole::Region,
        _ => ContentAccessibleRole::Presentation,
    }
}

fn collect_frame_elements_and_ext<'a>(
    project: &Project,
    timeline: &'a Timeline,
    frame: u32,
) -> (
    Vec<Element>,
    HashMap<String, &'a ExtendedAccessibility>,
    HashMap<String, u32>,
) {
    let mut elements = Vec::new();
    let mut ext_map = HashMap::new();
    let mut child_frames = HashMap::new();
    for layer in &timeline.layers {
        if !crate::authored_layers::layer_effectively_visible(timeline, layer)
            || matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder)
        {
            continue;
        }
        if let Some(key) = layer.key_at(frame) {
            for ext in &key.extended_accessibility {
                ext_map.insert(ext.element_id.clone(), ext);
            }
        }
        for el in sample_layer_in_at_fps(timeline, layer, frame, f64::from(project.fps)) {
            if el.visible {
                if let ElementKind::Instance {
                    symbol_id,
                    playback,
                } = &el.kind
                    && let Some(child) = project.symbol(symbol_id)
                {
                    let count = child.timeline.frame_count.max(1);
                    let age = if crate::authored_layers::layer_effectively_synced(timeline, layer) {
                        frame
                    } else {
                        frame.saturating_sub(crate::evaluate::placement_start(layer, frame, &el.id))
                    };
                    let child_frame = if child.kind == SymbolKind::Button {
                        0
                    } else {
                        crate::evaluate::instance_frame_from_age(*playback, age, count)
                    };
                    child_frames.insert(el.id.clone(), child_frame);
                }
                elements.push(el);
            }
        }
    }
    (elements, ext_map, child_frames)
}

/// Resolve the ordered accessibility tree for `timeline` at `frame` (`T-117`).
pub fn resolve_frame_accessibility_tree(
    project: &Project,
    timeline: &Timeline,
    frame: u32,
) -> Vec<AccessibilityTreeNode> {
    resolve_tree_recursive(project, timeline, frame, 0)
}

fn resolve_tree_recursive(
    project: &Project,
    timeline: &Timeline,
    frame: u32,
    depth: usize,
) -> Vec<AccessibilityTreeNode> {
    if depth > 8 {
        return Vec::new();
    }
    let doc_acc = project.document_accessibility();
    if !doc_acc.accessible {
        return Vec::new();
    }

    let (elements, ext_map, child_frames) =
        collect_frame_elements_and_ext(project, timeline, frame);
    let mut text_candidates: Vec<(String, String, [f64; 2])> = Vec::new();
    let mut element_by_id: HashMap<String, &Element> = HashMap::new();

    for el in &elements {
        element_by_id.insert(el.id.clone(), el);
        if let Some(txt) = element_text_content(el) {
            let b = estimate_element_bounds(el);
            text_candidates.push((el.id.clone(), txt, [b[0] + b[2] * 0.5, b[1] + b[3] * 0.5]));
        }
    }

    let mut nodes: Vec<OrderedAccessibilityNode> = Vec::new();

    for el in &elements {
        let ext = ext_map.get(&el.id).copied();
        if ext.is_some_and(|x| x.silent) {
            continue;
        }
        if el.accessibility.as_ref().is_some_and(|a| a.disabled) {
            continue;
        }

        let role = infer_role(project, el, ext);
        if role == ContentAccessibleRole::Presentation
            && el.accessibility.is_none()
            && ext.is_none()
        {
            continue;
        }

        let bounds = estimate_element_bounds(el);
        let center = [bounds[0] + bounds[2] * 0.5, bounds[1] + bounds[3] * 0.5];

        let mut name = el
            .accessibility
            .as_ref()
            .map(|a| a.name.trim().to_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| element_text_content(el))
            .unwrap_or_default();

        let mut description = el
            .accessibility
            .as_ref()
            .map(|a| a.description.trim().to_owned())
            .unwrap_or_default();

        if let Some(ext_meta) = ext {
            if name.is_empty()
                && let Some(label_id) = &ext_meta.aria_labelled_by
                && let Some(target_el) = element_by_id.get(label_id)
            {
                if let Some(txt) = element_text_content(target_el) {
                    name = txt;
                } else if let Some(acc) = &target_el.accessibility {
                    name = acc.name.clone();
                }
            }
            if description.is_empty()
                && let Some(desc_id) = &ext_meta.aria_described_by
                && let Some(target_el) = element_by_id.get(desc_id)
            {
                if let Some(txt) = element_text_content(target_el) {
                    description = txt;
                } else if let Some(acc) = &target_el.accessibility {
                    description = acc.description.clone();
                }
            }
        }

        // Auto-label interactive controls from nearby static text labels when enabled.
        let allow_auto_label = doc_acc.auto_label && !ext.is_some_and(|x| x.no_auto_labeling);
        if name.is_empty()
            && allow_auto_label
            && (role.is_interactive() || matches!(el.kind, ElementKind::Instance { .. }))
        {
            let mut best: Option<(f64, &str)> = None;
            for (tid, txt, tcenter) in &text_candidates {
                if tid == &el.id {
                    continue;
                }
                let dist = (tcenter[0] - center[0]).hypot(tcenter[1] - center[1]);
                if dist <= 240.0 && best.is_none_or(|(bdist, _)| dist < bdist) {
                    best = Some((dist, txt.as_str()));
                }
            }
            if let Some((_, matched_text)) = best {
                name = matched_text.to_owned();
            } else if !el.name.trim().is_empty() {
                name = el.name.trim().to_owned();
            }
        }

        let tab_index = el.accessibility.as_ref().and_then(|a| {
            if a.tab_order > 0 {
                Some(a.tab_order as i32)
            } else {
                None
            }
        });
        let explicit_ro = ext.and_then(|x| x.reading_order);
        let shortcut = ext.map(|x| x.shortcut.clone()).unwrap_or_default();
        let focus_group = ext.and_then(|x| x.focus_group.clone());
        let live = ext
            .map(|x| x.live)
            .unwrap_or(AccessibilityLivePoliteness::Off);

        let mut children = Vec::new();
        if let ElementKind::Instance { symbol_id, .. } = &el.kind {
            let force_simple = ext.is_some_and(|x| x.force_simple);
            let tab_children = ext.map(|x| x.tab_children).unwrap_or(true);
            if !force_simple
                && tab_children
                && let Some(sym) = project.symbol(symbol_id)
                && sym.kind != SymbolKind::Button
            {
                let child_frame = child_frames
                    .get(&el.id)
                    .copied()
                    .unwrap_or(frame % sym.timeline.frame_count.max(1));
                children = resolve_tree_recursive(project, &sym.timeline, child_frame, depth + 1);
            }
        }

        nodes.push((
            explicit_ro,
            tab_index,
            [bounds[0], bounds[1]],
            AccessibilityTreeNode {
                element_id: el.id.clone(),
                role,
                name,
                description,
                shortcut,
                tab_index,
                reading_order: 0,
                focus_group,
                live,
                bounds: Some(bounds),
                children,
            },
        ));
    }

    nodes.sort_by(|a, b| {
        match (a.0, b.0) {
            (Some(ra), Some(rb)) if ra != rb => return ra.cmp(&rb),
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        match (a.1, b.1) {
            (Some(ta), Some(tb)) if ta != tb => return ta.cmp(&tb),
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        if doc_acc.reading_order_top_down {
            a.2[1]
                .total_cmp(&b.2[1])
                .then_with(|| a.2[0].total_cmp(&b.2[0]))
        } else {
            a.2[0]
                .total_cmp(&b.2[0])
                .then_with(|| a.2[1].total_cmp(&b.2[1]))
        }
    });

    nodes
        .into_iter()
        .enumerate()
        .map(|(idx, (_, _, _, mut node))| {
            node.reading_order = node.reading_order.max((idx + 1) as u32);
            node
        })
        .collect()
}

/// Validate a frame's accessibility configuration and resolved tree for duplicate tab indices,
/// missing accessible names on interactive controls, and broken ARIA target IDs (`T-117`).
pub fn validate_accessibility_tree(
    project: &Project,
    timeline: &Timeline,
    frame: u32,
) -> Vec<AccessibilityDiagnostic> {
    let mut diagnostics = Vec::new();
    let (elements, ext_map, _) = collect_frame_elements_and_ext(project, timeline, frame);
    let valid_ids: HashSet<&str> = elements.iter().map(|e| e.id.as_str()).collect();

    for (eid, ext) in &ext_map {
        if let Some(label_id) = &ext.aria_labelled_by
            && !valid_ids.contains(label_id.as_str())
        {
            diagnostics.push(AccessibilityDiagnostic {
                code: "broken-aria-reference".into(),
                element_id: Some(eid.clone()),
                message: format!(
                    "aria_labelled_by references missing element '{label_id}' on frame {}",
                    frame + 1
                ),
            });
        }
        if let Some(desc_id) = &ext.aria_described_by
            && !valid_ids.contains(desc_id.as_str())
        {
            diagnostics.push(AccessibilityDiagnostic {
                code: "broken-aria-reference".into(),
                element_id: Some(eid.clone()),
                message: format!(
                    "aria_described_by references missing element '{desc_id}' on frame {}",
                    frame + 1
                ),
            });
        }
        for ctrl_id in &ext.aria_controls {
            if !valid_ids.contains(ctrl_id.as_str()) {
                diagnostics.push(AccessibilityDiagnostic {
                    code: "broken-aria-reference".into(),
                    element_id: Some(eid.clone()),
                    message: format!(
                        "aria_controls references missing element '{ctrl_id}' on frame {}",
                        frame + 1
                    ),
                });
            }
        }
    }

    let tree = resolve_frame_accessibility_tree(project, timeline, frame);
    let mut seen_tab_indices: HashMap<i32, String> = HashMap::new();
    validate_nodes_recursive(&tree, &mut seen_tab_indices, &mut diagnostics);
    diagnostics
}

fn validate_nodes_recursive(
    nodes: &[AccessibilityTreeNode],
    seen_tab_indices: &mut HashMap<i32, String>,
    diagnostics: &mut Vec<AccessibilityDiagnostic>,
) {
    for node in nodes {
        if let Some(ti) = node.tab_index
            && ti > 0
        {
            if let Some(prev_id) = seen_tab_indices.get(&ti) {
                diagnostics.push(AccessibilityDiagnostic {
                    code: "duplicate-tab-index".into(),
                    element_id: Some(node.element_id.clone()),
                    message: format!(
                        "Duplicate positive tab_index {ti} shared by '{}' and '{}'",
                        prev_id, node.element_id
                    ),
                });
            } else {
                seen_tab_indices.insert(ti, node.element_id.clone());
            }
        }
        if node.role.is_interactive() && node.name.trim().is_empty() {
            diagnostics.push(AccessibilityDiagnostic {
                code: "missing-accessible-name".into(),
                element_id: Some(node.element_id.clone()),
                message: format!(
                    "Interactive {:?} element '{}' has no accessible name or nearby auto-label",
                    node.role, node.element_id
                ),
            });
        }
        validate_nodes_recursive(&node.children, seen_tab_indices, diagnostics);
    }
}

impl Keyframe {
    pub fn extended_accessibility(&self, element_id: &str) -> Option<&ExtendedAccessibility> {
        self.extended_accessibility
            .iter()
            .find(|a| a.element_id == element_id)
    }

    pub fn set_extended_accessibility(&mut self, ext: ExtendedAccessibility) {
        if ext.is_default() {
            self.extended_accessibility
                .retain(|a| a.element_id != ext.element_id);
            return;
        }
        if let Some(slot) = self
            .extended_accessibility
            .iter_mut()
            .find(|a| a.element_id == ext.element_id)
        {
            *slot = ext;
        } else {
            self.extended_accessibility.push(ext);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Accessibility, InstancePlayback, Layer, LayerProperties, Symbol, Transform};

    #[test]
    fn accessibility_tree_auto_labels_buttons_orders_tabs_and_reports_diagnostics() {
        let btn_sym = Symbol {
            id: "sym-btn".into(),
            name: "SubmitBtn".into(),
            folder: String::new(),
            kind: SymbolKind::Button,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "btn-layer".into(),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe::default()],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        };

        let btn1 = Element {
            id: "btn-play".into(),
            name: String::new(),
            transform: Transform {
                x: 100.0,
                y: 200.0,
                ..Default::default()
            },
            kind: ElementKind::Instance {
                symbol_id: "sym-btn".into(),
                playback: InstancePlayback::Independent,
            },
            accessibility: Some(Accessibility {
                name: String::new(),
                description: "Starts playback".into(),
                role: AccessibleRole::Button,
                tab_order: 2,
                disabled: false,
            }),
            ..Default::default()
        };

        let label1 = Element {
            id: "txt-play".into(),
            name: "label".into(),
            transform: Transform {
                x: 110.0,
                y: 205.0,
                ..Default::default()
            },
            kind: ElementKind::Text {
                text: "Play Game".into(),
                font_family: "sans-serif".into(),
                font_size: 16.0,
                color: [0, 0, 0, 255],
            },
            ..Default::default()
        };

        let btn2_unlabeled = Element {
            id: "btn-mute".into(),
            name: String::new(),
            transform: Transform {
                x: 500.0,
                y: 50.0,
                ..Default::default()
            },
            kind: ElementKind::Rectangle {
                width: 40.0,
                height: 40.0,
                fill: [100, 100, 100, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            accessibility: Some(Accessibility {
                name: String::new(),
                description: String::new(),
                role: AccessibleRole::Button,
                tab_order: 2, // Duplicate tab index!
                disabled: false,
            }),
            ..Default::default()
        };

        let mut key = Keyframe {
            frame: 0,
            elements: vec![btn1, label1, btn2_unlabeled],
            ..Default::default()
        };
        key.set_extended_accessibility(ExtendedAccessibility {
            element_id: "btn-mute".into(),
            role: ContentAccessibleRole::Button,
            no_auto_labeling: true,
            aria_labelled_by: Some("nonexistent-id".into()),
            ..Default::default()
        });

        let root_sym = Symbol {
            id: "root".into(),
            name: "Main".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "l1".into(),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![key],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        };

        let project = Project {
            format_version: 9,
            name: "AccTest".into(),
            width: 800,
            height: 600,
            fps: 24,
            background: [255, 255, 255, 255],
            root: "root".into(),
            symbols: vec![root_sym, btn_sym],
            library_folders: Vec::new(),
            sources: Vec::new(),
            resources: Vec::new(),
            document_class: None,
            scenes: Vec::new(),
        };

        let tree = resolve_frame_accessibility_tree(&project, &project.symbols[0].timeline, 0);
        let play_node = tree
            .iter()
            .find(|n| n.element_id == "btn-play")
            .expect("btn-play in tree");
        assert_eq!(
            play_node.name, "Play Game",
            "nearby text should auto-label button"
        );

        let diags = validate_accessibility_tree(&project, &project.symbols[0].timeline, 0);
        assert!(diags.iter().any(|d| d.code == "duplicate-tab-index"));
        assert!(diags.iter().any(|d| d.code == "missing-accessible-name"));
        assert!(diags.iter().any(|d| d.code == "broken-aria-reference"));
    }

    #[test]
    fn nested_accessibility_follows_instance_playback_and_folder_visibility() {
        let text = |id: &str, value: &str| Element {
            id: id.into(),
            kind: ElementKind::Text {
                text: value.into(),
                font_family: "sans-serif".into(),
                font_size: 16.0,
                color: [0, 0, 0, 255],
            },
            ..Element::default()
        };
        let child = Symbol {
            id: "child".into(),
            name: "Child".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 3,
                layers: vec![Layer {
                    id: "child-layer".into(),
                    name: "Child layer".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            elements: vec![text("frame-zero", "Zero")],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 2,
                            elements: vec![text("frame-two", "Two")],
                            ..Keyframe::default()
                        },
                    ],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        };
        let root = Symbol {
            id: "root".into(),
            name: "Root".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![
                    Layer {
                        id: "content".into(),
                        name: "Content".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![Element {
                                id: "child-instance".into(),
                                kind: ElementKind::Instance {
                                    symbol_id: "child".into(),
                                    playback: InstancePlayback::SingleFrame(2),
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        }],
                        properties: LayerProperties {
                            parent: Some("folder".into()),
                            ..LayerProperties::default()
                        },
                    },
                    Layer {
                        id: "folder".into(),
                        name: "Folder".into(),
                        visible: true,
                        locked: false,
                        keyframes: Vec::new(),
                        properties: LayerProperties {
                            role: LayerRole::Folder,
                            ..LayerProperties::default()
                        },
                    },
                ],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        };
        let mut project = Project {
            format_version: 9,
            name: "Nested accessibility".into(),
            width: 640,
            height: 480,
            fps: 24,
            background: [255; 4],
            root: "root".into(),
            symbols: vec![root, child],
            library_folders: Vec::new(),
            sources: Vec::new(),
            resources: Vec::new(),
            document_class: None,
            scenes: Vec::new(),
        };

        let tree = resolve_frame_accessibility_tree(&project, &project.symbols[0].timeline, 0);
        assert_eq!(tree[0].children[0].element_id, "frame-two");

        project.symbols[0].timeline.layers[1].visible = false;
        assert!(
            resolve_frame_accessibility_tree(&project, &project.symbols[0].timeline, 0).is_empty()
        );
    }
}
