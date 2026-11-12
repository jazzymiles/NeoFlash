//! Published HTML/DOM accessibility overlay generation and read-order synchronization (`T-234`).

use neoflash_core::{
    AccessibilityLivePoliteness, AccessibleRole, ContentAccessibleRole, Element, ElementKind,
    ExtendedAccessibility, LayerRole, Project, SymbolKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

/// A deterministic DOM accessibility node for the published HTML stage overlay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublishedAccessibilityOverlayNode {
    pub element_id: String,
    pub instance_name: String,
    pub dom_id: String,
    pub role: String,
    pub tab_index: i32,
    pub reading_order: u32,
    pub explicit_tab_index: Option<u32>,
    pub aria_label: Option<String>,
    pub aria_description: Option<String>,
    pub aria_live: Option<String>,
    pub aria_hidden: bool,
    pub aria_disabled: bool,
    pub aria_keyshortcuts: Option<String>,
    pub auto_label: bool,
    pub bounds: [f64; 4],
}

fn html_attr_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn element_stage_bounds(element: &Element, project: &Project) -> [f64; 4] {
    let tx = element.transform.x;
    let ty = element.transform.y;
    let sx = element.transform.scale_x.abs().max(0.001);
    let sy = element.transform.scale_y.abs().max(0.001);
    let (w, h) = match &element.kind {
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. }
        | ElementKind::Video { width, height }
        | ElementKind::TextBox { width, height, .. } => (*width * sx, *height * sy),
        ElementKind::Bitmap { width, height, .. } => ((*width as f64) * sx, (*height as f64) * sy),
        ElementKind::Text {
            text, font_size, ..
        } => {
            let approx_w = (text.chars().count().max(1) as f64) * (*font_size * 0.6) * sx;
            let approx_h = (*font_size * 1.2) * sy;
            (approx_w, approx_h)
        }
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for cmd in commands {
                match *cmd {
                    neoflash_core::PathCommand::MoveTo { x, y }
                    | neoflash_core::PathCommand::LineTo { x, y }
                    | neoflash_core::PathCommand::QuadraticTo { x, y, .. }
                    | neoflash_core::PathCommand::CubicTo { x, y, .. } => {
                        min_x = min_x.min(x);
                        min_y = min_y.min(y);
                        max_x = max_x.max(x);
                        max_y = max_y.max(y);
                    }
                    neoflash_core::PathCommand::Close => {}
                }
            }
            if min_x.is_finite() && max_x.is_finite() {
                (
                    ((max_x - min_x).max(1.0)) * sx,
                    ((max_y - min_y).max(1.0)) * sy,
                )
            } else {
                (24.0 * sx, 24.0 * sy)
            }
        }
        ElementKind::Instance { symbol_id, .. } => {
            let _ = project.symbol(symbol_id);
            (64.0 * sx, 64.0 * sy)
        }
    };
    [tx, ty, w.max(1.0), h.max(1.0)]
}

fn infer_role_and_label(
    element: &Element,
    ext: Option<&ExtendedAccessibility>,
    project: &Project,
) -> (String, Option<String>, bool) {
    let role = if let Some(ext_meta) = ext
        && ext_meta.role != ContentAccessibleRole::Auto
    {
        ext_meta.role.aria_role_str().to_string()
    } else if let Some(acc) = &element.accessibility {
        match acc.role {
            AccessibleRole::Button => "button".to_string(),
            AccessibleRole::Image => "img".to_string(),
        }
    } else {
        match &element.kind {
            ElementKind::Text { .. } => "note".to_string(),
            ElementKind::TextBox { format, .. } => {
                if let Some(opts) = format.field_options()
                    && opts.field_type == neoflash_core::TextFieldType::Input
                {
                    "textbox".to_string()
                } else {
                    "note".to_string()
                }
            }
            ElementKind::Instance { symbol_id, .. } => match project
                .symbol(symbol_id)
                .map(|s| s.kind)
                .unwrap_or(SymbolKind::Graphic)
            {
                SymbolKind::Button => "button".to_string(),
                SymbolKind::MovieClip => "group".to_string(),
                SymbolKind::Graphic => "img".to_string(),
            },
            _ => "img".to_string(),
        }
    };

    let auto_label = !ext.is_some_and(|x| x.no_auto_labeling);
    let explicit_name = element
        .accessibility
        .as_ref()
        .map(|a| a.name.trim())
        .unwrap_or("");
    let label = if !explicit_name.is_empty() {
        Some(explicit_name.to_string())
    } else if auto_label {
        match &element.kind {
            ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. }
                if !text.trim().is_empty() =>
            {
                Some(text.trim().to_string())
            }
            _ if !element.name.trim().is_empty() => Some(element.name.trim().to_string()),
            ElementKind::Instance { symbol_id, .. } => project
                .symbol(symbol_id)
                .map(|s| s.name.trim().to_string())
                .filter(|s| !s.is_empty()),
            _ => None,
        }
    } else if !element.name.trim().is_empty() {
        Some(element.name.trim().to_string())
    } else {
        None
    };

    (role, label, auto_label)
}

/// Resolves deterministic DOM accessibility overlay nodes for a symbol and frame in reading order.
/// Keeps `silent` (`aria-hidden="true"`) nodes synchronized in the overlay with `tab_index = -1`
/// so runtime mutations or authored hiding are explicitly represented in the DOM tree.
pub fn resolve_published_accessibility_overlay_nodes(
    project: &Project,
    symbol_id: Option<&str>,
    frame: u32,
) -> Vec<PublishedAccessibilityOverlayNode> {
    let target_id = symbol_id.unwrap_or(project.root.as_str());
    let Some(symbol) = project.symbol(target_id) else {
        return Vec::new();
    };

    struct RawCandidate {
        author_idx: usize,
        element_id: String,
        instance_name: String,
        role: String,
        explicit_tab_index: Option<u32>,
        aria_label: Option<String>,
        aria_description: Option<String>,
        aria_live: Option<String>,
        aria_hidden: bool,
        aria_disabled: bool,
        aria_keyshortcuts: Option<String>,
        auto_label: bool,
        bounds: [f64; 4],
    }

    let mut candidates = Vec::new();
    let mut author_idx = 0usize;

    for layer in symbol.timeline.layers.iter().rev() {
        if !layer.visible
            || matches!(
                layer.properties.role,
                LayerRole::Guide | LayerRole::Mask | LayerRole::Folder
            )
        {
            continue;
        }
        let Some(kf) = layer
            .keyframes
            .iter()
            .filter(|k| k.frame <= frame)
            .max_by_key(|k| k.frame)
        else {
            continue;
        };
        let ext_map: HashMap<&str, &ExtendedAccessibility> = kf
            .extended_accessibility
            .iter()
            .map(|e| (e.element_id.as_str(), e))
            .collect();
        let el_map: HashMap<&str, &Element> =
            kf.elements.iter().map(|e| (e.id.as_str(), e)).collect();

        for element in &kf.elements {
            if !element.visible {
                continue;
            }
            let ext = ext_map.get(element.id.as_str()).copied();
            let is_interactive_or_text = matches!(
                element.kind,
                ElementKind::Text { .. }
                    | ElementKind::TextBox { .. }
                    | ElementKind::Instance { .. }
            );
            let should_emit = element.accessibility.is_some()
                || ext.is_some_and(|x| !x.is_default())
                || is_interactive_or_text;
            if !should_emit {
                continue;
            }

            let silent = ext.is_some_and(|x| x.silent);
            let aria_hidden = silent;
            let (role, mut aria_label, auto_label) = infer_role_and_label(element, ext, project);
            let mut aria_description = element
                .accessibility
                .as_ref()
                .map(|a| a.description.trim())
                .filter(|d| !d.is_empty())
                .map(str::to_owned);

            if let Some(ext_meta) = ext {
                if aria_label.is_none()
                    && let Some(label_id) = &ext_meta.aria_labelled_by
                    && let Some(target_el) = el_map.get(label_id.as_str())
                {
                    aria_label = match &target_el.kind {
                        ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. }
                            if !text.trim().is_empty() =>
                        {
                            Some(text.trim().to_owned())
                        }
                        _ => target_el.accessibility.as_ref().map(|a| a.name.clone()),
                    };
                }
                if aria_description.is_none()
                    && let Some(desc_id) = &ext_meta.aria_described_by
                    && let Some(target_el) = el_map.get(desc_id.as_str())
                {
                    aria_description = match &target_el.kind {
                        ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. }
                            if !text.trim().is_empty() =>
                        {
                            Some(text.trim().to_owned())
                        }
                        _ => target_el
                            .accessibility
                            .as_ref()
                            .map(|a| a.description.clone())
                            .filter(|s| !s.is_empty()),
                    };
                }
            }

            let aria_live = match ext.map_or(AccessibilityLivePoliteness::Off, |x| x.live) {
                AccessibilityLivePoliteness::Off => None,
                AccessibilityLivePoliteness::Polite => Some("polite".to_string()),
                AccessibilityLivePoliteness::Assertive => Some("assertive".to_string()),
            };
            let aria_keyshortcuts = ext
                .map(|x| x.shortcut.trim())
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let explicit_tab_index = element
                .accessibility
                .as_ref()
                .and_then(|a| (a.tab_order > 0).then_some(a.tab_order))
                .or_else(|| ext.and_then(|x| x.reading_order).filter(|&ro| ro > 0));
            let aria_disabled = element.accessibility.as_ref().is_some_and(|a| a.disabled);
            let bounds = element_stage_bounds(element, project);

            candidates.push(RawCandidate {
                author_idx,
                element_id: element.id.clone(),
                instance_name: element.name.clone(),
                role,
                explicit_tab_index,
                aria_label,
                aria_description,
                aria_live,
                aria_hidden,
                aria_disabled,
                aria_keyshortcuts,
                auto_label,
                bounds,
            });
            author_idx += 1;
        }
    }

    // Sort active (`!aria_hidden`) nodes first in deterministic reading order:
    // 1. Explicit `tab_order > 0` ascending
    // 2. Spatial top-to-bottom (`y`), left-to-right (`x`)
    // 3. Authoring index
    // Silent (`aria_hidden`) nodes follow in deterministic authoring order with `tab_index = -1`.
    candidates.sort_by(|a, b| {
        a.aria_hidden
            .cmp(&b.aria_hidden)
            .then_with(|| match (a.explicit_tab_index, b.explicit_tab_index) {
                (Some(ta), Some(tb)) => ta.cmp(&tb),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.bounds[1].total_cmp(&b.bounds[1]))
            .then_with(|| a.bounds[0].total_cmp(&b.bounds[0]))
            .then_with(|| a.author_idx.cmp(&b.author_idx))
    });

    let mut out = Vec::with_capacity(candidates.len());
    let mut active_order = 0u32;
    for c in candidates {
        let (reading_order, tab_index) = if c.aria_hidden || c.aria_disabled {
            (0, -1)
        } else {
            active_order += 1;
            let ti = c.explicit_tab_index.unwrap_or(active_order) as i32;
            (active_order, ti)
        };
        let dom_id = format!("nf-a11y-{}", c.element_id);
        out.push(PublishedAccessibilityOverlayNode {
            element_id: c.element_id,
            instance_name: c.instance_name,
            dom_id,
            role: c.role,
            tab_index,
            reading_order,
            explicit_tab_index: c.explicit_tab_index,
            aria_label: c.aria_label,
            aria_description: c.aria_description,
            aria_live: c.aria_live,
            aria_hidden: c.aria_hidden,
            aria_disabled: c.aria_disabled,
            aria_keyshortcuts: c.aria_keyshortcuts,
            auto_label: c.auto_label,
            bounds: c.bounds,
        });
    }
    out
}

/// Validates authored `tab_order` (`tabIndex`) read order across exposed elements on `frame`,
/// reporting duplicate `tabIndex` conflicts and sequence gaps.
pub fn validate_published_accessibility_read_order(
    project: &Project,
    symbol_id: Option<&str>,
    frame: u32,
) -> Vec<String> {
    let nodes = resolve_published_accessibility_overlay_nodes(project, symbol_id, frame);
    let mut by_tab: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for node in &nodes {
        if node.aria_hidden {
            continue;
        }
        if let Some(ti) = node.explicit_tab_index {
            let label = if !node.instance_name.trim().is_empty() {
                node.instance_name.clone()
            } else if let Some(lbl) = &node.aria_label {
                lbl.clone()
            } else {
                node.element_id.clone()
            };
            by_tab.entry(ti).or_default().push(label);
        }
    }

    let mut diagnostics = Vec::new();
    for (ti, names) in &by_tab {
        if names.len() > 1 {
            diagnostics.push(format!(
                "Read-order conflict: tabIndex {ti} is assigned to multiple exposed elements ({})",
                names.join(", ")
            ));
        }
    }

    let indices: Vec<u32> = by_tab.keys().copied().collect();
    if let Some(&first) = indices.first()
        && first > 1
    {
        diagnostics.push(format!(
            "Read-order gap: explicit tabIndex sequence starts at {first} instead of 1"
        ));
    }
    for pair in indices.windows(2) {
        if pair[1] > pair[0] + 1 {
            diagnostics.push(format!(
                "Read-order gap: missing tabIndex between {} and {}",
                pair[0], pair[1]
            ));
        }
    }
    diagnostics
}

/// Renders deterministic HTML markup for the published DOM accessibility overlay.
/// Pointer events pass through to the Canvas stage (`pointer-events: none` on the overlay),
/// keeping pointer interaction, keyboard focus (`data-focus-reason`), and authored ARIA semantics distinct.
pub fn render_accessibility_overlay_nodes_html(
    nodes: &[PublishedAccessibilityOverlayNode],
) -> String {
    let mut html = String::new();
    let _ = writeln!(
        html,
        "<div id=\"stage-a11y-overlay\" class=\"neoflash-a11y-overlay\" role=\"region\" aria-label=\"Stage accessibility overlay\" data-focus-policy=\"keyboard-distinct\" style=\"position:absolute;inset:0;pointer-events:none;overflow:hidden;\">"
    );
    for node in nodes {
        let [x, y, w, h] = node.bounds;
        let mut attrs = format!(
            "id=\"{}\" data-element-id=\"{}\" data-neoflash-id=\"{}\" role=\"{}\" tabindex=\"{}\"",
            html_attr_escape(&node.dom_id),
            html_attr_escape(&node.element_id),
            html_attr_escape(&node.element_id),
            html_attr_escape(&node.role),
            node.tab_index
        );
        if !node.instance_name.trim().is_empty() {
            let _ = write!(
                attrs,
                " data-instance-name=\"{}\"",
                html_attr_escape(node.instance_name.trim())
            );
        }
        if node.reading_order > 0 {
            let _ = write!(attrs, " data-reading-order=\"{}\"", node.reading_order);
        }
        if let Some(label) = &node.aria_label {
            let _ = write!(attrs, " aria-label=\"{}\"", html_attr_escape(label));
        }
        if let Some(desc) = &node.aria_description {
            let _ = write!(attrs, " aria-description=\"{}\"", html_attr_escape(desc));
        }
        if let Some(live) = &node.aria_live {
            let _ = write!(attrs, " aria-live=\"{}\"", html_attr_escape(live));
        }
        if node.aria_hidden {
            attrs.push_str(" aria-hidden=\"true\"");
        }
        if node.aria_disabled {
            attrs.push_str(" aria-disabled=\"true\"");
        }
        if let Some(shortcut) = &node.aria_keyshortcuts {
            let _ = write!(
                attrs,
                " aria-keyshortcuts=\"{}\"",
                html_attr_escape(shortcut)
            );
        }
        let _ = writeln!(
            html,
            "  <div {attrs} style=\"position:absolute;left:{x:.2}px;top:{y:.2}px;width:{w:.2}px;height:{h:.2}px;pointer-events:none;\"></div>"
        );
    }
    html.push_str("</div>\n");
    html
}

/// Resolves published accessibility overlay nodes for the root symbol at frame 0.
pub fn collect_project_accessibility_items(
    project: &Project,
) -> Vec<PublishedAccessibilityOverlayNode> {
    resolve_published_accessibility_overlay_nodes(project, None, 0)
}

/// Resolves and renders the published DOM accessibility overlay HTML for `project` at `(symbol_id, frame)`.
pub fn render_accessibility_overlay_html(
    project: &Project,
    symbol_id: Option<&str>,
    frame: u32,
) -> String {
    let nodes = resolve_published_accessibility_overlay_nodes(project, symbol_id, frame);
    render_accessibility_overlay_nodes_html(&nodes)
}

#[cfg(test)]
mod html_tests {
    use super::*;
    use neoflash_core::{Accessibility, Transform};

    #[test]
    fn published_accessibility_overlay_emits_deterministic_tab_order_and_aria_attributes() {
        let mut project = Project::blank();
        let kf = &mut project.symbols[0].timeline.layers[0].keyframes[0];
        kf.elements.clear();
        kf.extended_accessibility.clear();

        let second = Element {
            id: "el-second".into(),
            name: "submit_btn".into(),
            transform: Transform {
                x: 20.0,
                y: 20.0,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 120.0,
                height: 40.0,
                fill: [40, 80, 160, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            accessibility: Some(Accessibility {
                name: "Submit Form".into(),
                description: "Submits the current score".into(),
                role: AccessibleRole::Button,
                tab_order: 2,
                disabled: false,
            }),
            ..Element::default()
        };
        let first = Element {
            id: "el-first".into(),
            name: "header_banner".into(),
            transform: Transform {
                x: 20.0,
                y: 120.0,
                ..Transform::default()
            },
            kind: ElementKind::Text {
                text: "Leaderboard".into(),
                font_family: "Inter".into(),
                font_size: 24.0,
                color: [255, 255, 255, 255],
            },
            accessibility: Some(Accessibility {
                name: "Leaderboard Heading".into(),
                description: "Top player rankings".into(),
                role: AccessibleRole::Image,
                tab_order: 1,
                disabled: false,
            }),
            ..Element::default()
        };
        let silent_el = Element {
            id: "el-silent".into(),
            name: "bg_deco".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 300.0,
                height: 200.0,
                fill: [10, 10, 10, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            accessibility: None,
            ..Element::default()
        };

        kf.elements.push(second);
        kf.elements.push(first);
        kf.elements.push(silent_el);
        kf.extended_accessibility.push(ExtendedAccessibility {
            element_id: "el-second".into(),
            role: ContentAccessibleRole::Button,
            shortcut: "Enter".into(),
            live: AccessibilityLivePoliteness::Polite,
            ..ExtendedAccessibility::default()
        });
        kf.extended_accessibility.push(ExtendedAccessibility {
            element_id: "el-first".into(),
            role: ContentAccessibleRole::Heading,
            live: AccessibilityLivePoliteness::Assertive,
            ..ExtendedAccessibility::default()
        });
        kf.extended_accessibility.push(ExtendedAccessibility {
            element_id: "el-silent".into(),
            silent: true,
            ..ExtendedAccessibility::default()
        });

        let nodes = resolve_published_accessibility_overlay_nodes(&project, None, 0);
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].element_id, "el-first");
        assert_eq!(nodes[0].tab_index, 1);
        assert_eq!(nodes[0].reading_order, 1);
        assert_eq!(nodes[0].role, "heading");
        assert_eq!(nodes[0].aria_label.as_deref(), Some("Leaderboard Heading"));
        assert_eq!(
            nodes[0].aria_description.as_deref(),
            Some("Top player rankings")
        );
        assert_eq!(nodes[0].aria_live.as_deref(), Some("assertive"));
        assert!(!nodes[0].aria_hidden);

        assert_eq!(nodes[1].element_id, "el-second");
        assert_eq!(nodes[1].tab_index, 2);
        assert_eq!(nodes[1].reading_order, 2);
        assert_eq!(nodes[1].role, "button");
        assert_eq!(nodes[1].aria_live.as_deref(), Some("polite"));
        assert_eq!(nodes[1].aria_keyshortcuts.as_deref(), Some("Enter"));

        assert_eq!(nodes[2].element_id, "el-silent");
        assert!(nodes[2].aria_hidden);
        assert_eq!(nodes[2].tab_index, -1);

        let html = render_accessibility_overlay_nodes_html(&nodes);
        assert!(html.contains("data-focus-policy=\"keyboard-distinct\""));
        assert!(html.contains("aria-description=\"Top player rankings\""));
        assert!(html.contains("aria-live=\"assertive\""));
        assert!(html.contains("aria-hidden=\"true\""));

        let diags = validate_published_accessibility_read_order(&project, None, 0);
        assert!(diags.is_empty(), "expected clean read order, got {diags:?}");
    }

    #[test]
    fn published_accessibility_read_order_reports_conflicts_and_gaps() {
        let mut project = Project::blank();
        let elements = &mut project.symbols[0].timeline.layers[0].keyframes[0].elements;
        elements.clear();

        for (id, name, tab_order) in [
            ("a", "btn_a", 2u32),
            ("b", "btn_b", 2u32),
            ("c", "btn_c", 5u32),
        ] {
            elements.push(Element {
                id: id.into(),
                name: name.into(),
                kind: ElementKind::Rectangle {
                    width: 40.0,
                    height: 20.0,
                    fill: [200, 200, 200, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                accessibility: Some(Accessibility {
                    name: name.into(),
                    description: String::new(),
                    role: AccessibleRole::Button,
                    tab_order,
                    disabled: false,
                }),
                ..Element::default()
            });
        }

        let diags = validate_published_accessibility_read_order(&project, None, 0);
        assert_eq!(diags.len(), 3);
        assert!(diags[0].contains("conflict") && diags[0].contains("tabIndex 2"));
        assert!(diags[1].contains("starts at 2"));
        assert!(diags[2].contains("between 2 and 5"));
    }
}
