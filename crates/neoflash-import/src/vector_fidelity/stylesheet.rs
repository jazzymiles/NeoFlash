//! Resolve static SVG stylesheet selectors against the source XML, before collecting geometry.
use roxmltree::{Node, NodeId};
use simplecss::{AttributeOperator, Declaration, Element, PseudoClass, StyleSheet};
use std::collections::HashMap;

#[derive(Clone, Copy)]
struct SvgNode<'a, 'input>(Node<'a, 'input>);

impl Element for SvgNode<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(Self)
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        self.0.prev_sibling_element().map(Self)
    }
    fn has_local_name(&self, name: &str) -> bool {
        self.0.tag_name().name() == name
    }
    fn attribute_matches(&self, name: &str, operator: AttributeOperator<'_>) -> bool {
        self.0.attribute(name).is_some_and(|v| operator.matches(v))
    }
    fn pseudo_class_matches(&self, class: PseudoClass<'_>) -> bool {
        matches!(class, PseudoClass::FirstChild) && self.prev_sibling_element().is_none()
    }
}

pub(super) struct SvgStyles<'a>(HashMap<NodeId, HashMap<&'a str, Declaration<'a>>>);

impl<'a> SvgStyles<'a> {
    pub(super) fn new(root: Node<'a, 'a>) -> Self {
        let mut sheet = StyleSheet::new();
        for text in root
            .descendants()
            .filter(|n| n.has_tag_name("style"))
            .filter_map(|n| n.text())
        {
            sheet.parse_more(text);
        }
        let mut nodes = HashMap::new();
        for n in root.descendants().filter(Node::is_element) {
            let mut properties = HashMap::<_, Declaration<'a>>::new();
            // simplecss orders selectors by specificity and document order. Inline rules follow.
            for decl in sheet
                .rules
                .iter()
                .filter(|r| r.selector.matches(&SvgNode(n)))
                .flat_map(|r| &r.declarations)
                .copied()
                .chain(simplecss::DeclarationTokenizer::from(
                    n.attribute("style").unwrap_or(""),
                ))
            {
                if decl.important
                    || !properties
                        .get(decl.name)
                        .is_some_and(|prior| prior.important)
                {
                    properties.insert(decl.name, decl);
                }
            }
            if !properties.is_empty() {
                nodes.insert(n.id(), properties);
            }
        }
        Self(nodes)
    }

    pub(super) fn value(&self, node: Node<'a, 'a>, name: &str) -> Option<String> {
        let inherit = matches!(
            name,
            "fill"
                | "fill-rule"
                | "fill-opacity"
                | "stroke"
                | "stroke-width"
                | "stroke-opacity"
                | "font-size"
                | "font-family"
        );
        for n in node.ancestors().filter(Node::is_element) {
            if let Some(s) = self
                .0
                .get(&n.id())
                .and_then(|props| props.get(name))
                .map(|d| d.value)
                .or_else(|| n.attribute(name))
                && s != "inherit"
            {
                return Some(s.to_owned());
            }
            if !inherit {
                break;
            }
        }
        None
    }
}
