//! Publish-time layer visibility is independent of the authoring eye toggle.
//! Adobe documents Include Hidden Layers as enabled by default, including nested
//! movie clips. CS6 XFL stores this option as PublishFlashProperties/InvisibleLayer:
//! https://helpx.adobe.com/animate/desktop/exporting-and-publishing/publish-settings.html
//! The unchanged public Ruffle avm1/goto_label FLA independently contains this field.

use super::{elements, publish_language, publish_value};
use crate::ImportReport;
use neoflash_core::ScriptLanguage;
use roxmltree::{Document, Node};

pub(super) struct Settings {
    pub language: Option<ScriptLanguage>,
    pub include_hidden_layers: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: None,
            include_hidden_layers: true,
        }
    }
}

pub(super) fn read(doc: &Document<'_>, report: &mut ImportReport) -> Settings {
    let Some(properties) = active_properties(doc, report) else {
        return Settings::default();
    };
    let language = publish_language(properties, report);
    let mut visibility = vec![];
    for node in properties.document().descendants().filter(Node::is_element) {
        if !matches!(
            node.tag_name().name(),
            "InvisibleLayer" | "IncludeHiddenLayers"
        ) {
            continue;
        }
        // Values in another explicitly inactive profile do not describe this movie.
        if node.ancestors().any(|parent| {
            parent.has_tag_name("flash_profile")
                && parent.parent() == Some(doc.root_element())
                && properties.parent().is_some_and(|active| parent != active)
        }) {
            continue;
        }
        if node.parent() != Some(properties) || node.tag_name().name() != "InvisibleLayer" {
            report.error("publish_layer_policy", location(node), "Hidden-layer publishing must be specified by one direct PublishFlashProperties/InvisibleLayer field in the selected profile; other placements/names are unsupported.");
            continue;
        }
        visibility.push(node);
    }
    let include_hidden_layers = match visibility.as_slice() {
        [] => true,
        [node] => match publish_value(*node, report).as_deref() {
            Some("1") => true,
            Some("0") => false,
            _ => {
                report.error("publish_layer_policy", location(*node), "InvisibleLayer must be the plain numeric boolean 0 or 1 used by Flash publish profiles; other encodings are unsupported.");
                true
            }
        },
        _ => {
            report.error(
                "publish_layer_policy",
                "PublishSettings.xml",
                "Repeated InvisibleLayer fields are ambiguous, even when their values agree.",
            );
            true
        }
    };
    Settings {
        language,
        include_hidden_layers,
    }
}

fn active_properties<'a, 'input>(
    doc: &'a Document<'input>,
    report: &mut ImportReport,
) -> Option<Node<'a, 'input>> {
    let root = doc.root_element();
    if root.tag_name().name() != "flash_profiles" || !plain_wrapper(root, &[], report) {
        report.error(
            "publish_profile",
            "PublishSettings.xml",
            "Expected an unnamespaced flash_profiles container.",
        );
        return None;
    }
    // Only direct profile entries participate in current-profile selection.
    // A nested same-named node must never hide a policy as "inactive" content.
    if let Some(nested) = root.descendants().find(|node| {
        node.is_element() && node.has_tag_name("flash_profile") && node.parent() != Some(root)
    }) {
        report.error(
            "publish_profile",
            location(nested),
            "Nested flash_profile elements are unsupported; profiles must be direct children of flash_profiles.",
        );
        return None;
    }
    let profiles: Vec<_> = elements(root).collect();
    let mut active = vec![];
    for profile in &profiles {
        if profile.tag_name().name() != "flash_profile"
            || !plain_wrapper(*profile, &["name", "version", "current"], report)
        {
            report.error(
                "publish_profile",
                location(*profile),
                "Only plain flash_profile entries are supported in flash_profiles.",
            );
            return None;
        }
        match profile.attribute("current") {
            Some("true" | "1") => active.push(*profile),
            Some("false" | "0") => {}
            None if profiles.len() == 1 => active.push(*profile),
            None => {}
            _ => {
                report.error("publish_profile", location(*profile), "Profile current must be a boolean; an active publishing profile cannot be guessed.");
                return None;
            }
        }
    }
    if active.len() != 1 {
        report.error(
            "publish_profile",
            "PublishSettings.xml",
            "Select exactly one current publishing profile (a single profile may omit current).",
        );
        return None;
    }
    if profiles.len() > 1 {
        report.warning("inactive_publish_profile", "PublishSettings.xml", "Only the explicit current profile determines ActionScript and hidden-layer publishing; inactive profiles are not imported.");
    }
    let profile = active[0];
    let properties: Vec<_> = elements(profile)
        .filter(|n| n.has_tag_name("PublishFlashProperties"))
        .collect();
    let [properties] = properties.as_slice() else {
        report.error("publish_profile", location(profile), "The selected profile must contain exactly one direct PublishFlashProperties container.");
        return None;
    };
    if !plain_wrapper(*properties, &["enabled"], report) {
        return None;
    }
    if properties
        .attribute("enabled")
        .is_some_and(|v| !matches!(v, "true" | "false" | "0" | "1"))
    {
        report.error(
            "publish_profile",
            location(*properties),
            "PublishFlashProperties enabled must be a boolean.",
        );
        return None;
    }
    Some(*properties)
}

fn plain_wrapper(node: Node<'_, '_>, allowed: &[&str], report: &mut ImportReport) -> bool {
    let valid = node.tag_name().namespace().is_none()
        && node
            .attributes()
            .all(|a| a.namespace().is_none() && allowed.contains(&a.name()))
        && !node
            .children()
            .any(|n| n.is_text() && n.text().is_some_and(|s| !s.trim().is_empty()));
    if !valid {
        report.error("publish_profile", location(node), "Publish profile containers must be unnamespaced and contain only supported attributes and structural children.");
    }
    valid
}

fn location(node: Node<'_, '_>) -> String {
    format!(
        "PublishSettings.xml:{} <{}>",
        node.document().text_pos_at(node.range().start).row,
        node.tag_name().name()
    )
}
