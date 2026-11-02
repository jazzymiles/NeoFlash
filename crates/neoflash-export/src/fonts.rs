//! Portable font policies, @font-face embedding, deterministic subsetting, and glyph outline conversion.

use neoflash_core::{ElementKind, FillRule, Project, ResourceUnit, TextFieldState, TextFormat};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontExportPolicy {
    Device,
    #[default]
    Embed,
    Subset,
    Outlines,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontSubsetStats {
    pub family: String,
    pub original_bytes: usize,
    pub subset_bytes: usize,
    pub codepoint_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FontBundle {
    pub policy: FontExportPolicy,
    pub css_rules: String,
    pub metrics: BTreeMap<String, serde_json::Value>,
    pub subset_stats: Vec<FontSubsetStats>,
    pub outlined_elements: usize,
}

pub fn is_font_resource(resource: &ResourceUnit) -> bool {
    let mime = resource.media_type.trim().to_ascii_lowercase();
    if matches!(
        mime.as_str(),
        "font/ttf"
            | "font/otf"
            | "font/woff"
            | "font/woff2"
            | "application/font-sfnt"
            | "application/x-font-ttf"
            | "application/x-font-opentype"
            | "application/vnd.ms-opentype"
            | "application/font-woff"
    ) {
        return true;
    }
    let path_lower = resource.path.to_ascii_lowercase();
    path_lower.ends_with(".ttf")
        || path_lower.ends_with(".otf")
        || path_lower.ends_with(".woff")
        || path_lower.ends_with(".woff2")
}

pub fn font_family_for_resource(resource: &ResourceUnit) -> String {
    let stem = Path::new(&resource.path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&resource.id)
        .trim();
    if stem.is_empty() {
        resource.id.clone()
    } else {
        stem.to_string()
    }
}

pub fn collect_used_codepoints(project: &Project) -> BTreeMap<String, BTreeSet<u32>> {
    let mut used: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for keyframe in &layer.keyframes {
                for element in &keyframe.elements {
                    match &element.kind {
                        ElementKind::Text {
                            text, font_family, ..
                        } => {
                            let entry = used.entry(font_family.clone()).or_default();
                            entry.insert(32);
                            for ch in text.chars() {
                                entry.insert(ch as u32);
                            }
                        }
                        ElementKind::TextBox { text, format, .. } => {
                            let entry = used.entry(format.font.clone()).or_default();
                            entry.insert(32);
                            for ch in text.chars() {
                                entry.insert(ch as u32);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    used
}

pub fn embedded_font_families(project: &Project) -> BTreeSet<String> {
    let mut families = BTreeSet::new();
    for res in &project.resources {
        if is_font_resource(res) {
            let fam = font_family_for_resource(res);
            families.insert(fam.to_ascii_lowercase());
            families.insert(fam);
            families.insert(res.id.to_ascii_lowercase());
            families.insert(res.id.clone());
        }
    }
    families
}

pub fn convert_project_text_to_outlines(project: &mut Project) -> usize {
    let mut font_bytes_by_family: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for res in &project.resources {
        if is_font_resource(res)
            && let Ok(bytes) = res.decoded_bytes()
        {
            font_bytes_by_family.insert(font_family_for_resource(res), bytes.clone());
            font_bytes_by_family.insert(res.id.clone(), bytes);
        }
    }
    let mut converted = 0usize;
    for symbol in &mut project.symbols {
        for layer in &mut symbol.timeline.layers {
            for keyframe in &mut layer.keyframes {
                for element in &mut keyframe.elements {
                    let replacement = match &element.kind {
                        ElementKind::Text {
                            text,
                            font_family,
                            font_size,
                            color,
                        } => {
                            let bytes = font_bytes_by_family.get(font_family).map(Vec::as_slice);
                            let commands = TextFieldState::static_text_to_outline_commands(
                                text,
                                font_family,
                                *font_size,
                                bytes,
                            );
                            Some(ElementKind::Path {
                                commands,
                                fill_rule: FillRule::NonZero,
                                fill: *color,
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            })
                        }
                        ElementKind::TextBox { .. } if element.name.trim().is_empty() => {
                            if let Some(field) = TextFieldState::from_kind(&element.kind) {
                                let bytes = font_bytes_by_family
                                    .get(&field.format.font)
                                    .map(Vec::as_slice);
                                let commands = field.to_outline_commands(bytes);
                                let rgb = field.format.color;
                                let fill = [
                                    ((rgb >> 16) & 0xFF) as u8,
                                    ((rgb >> 8) & 0xFF) as u8,
                                    (rgb & 0xFF) as u8,
                                    field.alpha,
                                ];
                                Some(ElementKind::Path {
                                    commands,
                                    fill_rule: FillRule::NonZero,
                                    fill,
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                })
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some(next_kind) = replacement {
                        element.kind = next_kind;
                        converted += 1;
                    }
                }
            }
        }
    }
    converted
}

pub fn apply_font_policy(project: &mut Project, policy: FontExportPolicy) -> FontBundle {
    let used_by_family = collect_used_codepoints(project);
    let mut bundle = FontBundle {
        policy,
        css_rules: String::new(),
        metrics: BTreeMap::new(),
        subset_stats: Vec::new(),
        outlined_elements: 0,
    };

    for family in used_by_family.keys() {
        if let Ok(json_str) = TextFormat::portable_font_metrics_json(family, None)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str)
        {
            bundle.metrics.insert(family.clone(), val);
        }
    }

    if policy == FontExportPolicy::Outlines {
        bundle.outlined_elements = convert_project_text_to_outlines(project);
        return bundle;
    }

    if policy == FontExportPolicy::Device {
        return bundle;
    }

    for res in &mut project.resources {
        if !is_font_resource(res) {
            continue;
        }
        let family = font_family_for_resource(res);
        let Ok(raw_bytes) = res.decoded_bytes() else {
            continue;
        };
        let original_bytes = raw_bytes.len();
        let active_bytes = if policy == FontExportPolicy::Subset {
            let codepoints = used_by_family
                .get(&family)
                .or_else(|| used_by_family.get(&res.id))
                .cloned()
                .unwrap_or_else(|| (32u32..=126).collect());
            let subset = TextFormat::subset_font_bytes(&raw_bytes, &codepoints)
                .unwrap_or_else(|_| raw_bytes.clone());
            bundle.subset_stats.push(FontSubsetStats {
                family: family.clone(),
                original_bytes,
                subset_bytes: subset.len(),
                codepoint_count: codepoints.len(),
            });
            if let Ok(updated) =
                ResourceUnit::from_bytes(&res.id, &res.path, &res.media_type, &subset)
            {
                *res = updated;
            }
            subset
        } else {
            raw_bytes
        };

        if let Ok(json_str) = TextFormat::portable_font_metrics_json(&family, Some(&active_bytes))
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str)
        {
            bundle.metrics.insert(family.clone(), val);
        }

        let css_family = family.replace('\\', "\\\\").replace('"', "\\\"");
        let mime = if res.media_type.trim().is_empty() {
            "font/ttf"
        } else {
            res.media_type.trim()
        };
        bundle.css_rules.push_str(&format!(
            "@font-face {{ font-family: \"{}\"; src: url('data:{};base64,{}'); font-display: block; }}\n",
            css_family, mime, res.data_base64
        ));
    }

    bundle
}
