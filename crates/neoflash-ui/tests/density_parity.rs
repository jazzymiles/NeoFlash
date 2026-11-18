//! The Figma variable collection `NF3 · Density` and `neoflash_ui::metrics` describe the same numbers.
//! The Figma side is built from `interface/figma-plugin/v3/src/05-density.js` (object `NF3_DENSITY`,
//! entries `'name': [normal, compact, classic, 'S']`); this test reads that file and compares every
//! shared variable with the Rust metric in all three densities.
use neoflash_ui::metrics::{self, Density};
use std::collections::BTreeMap;

const SOURCE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../interface/figma-plugin/v3/src/05-density.js"
);

/// Figma variable → Rust metric (a `DENSITY_TABLE` name).
const PAIRS: &[(&str, &str)] = &[
    ("size/control", "control"),
    ("size/row", "row"),
    ("size/tab", "panel_tab"),
    ("size/section", "section_header"),
    ("size/top-bar", "top_bar"),
    ("size/status", "status_bar"),
    ("size/dialog-header", "dialog_header"),
    ("size/tool", "tool_button"),
    ("size/icon-tool", "icon_tool"),
    ("size/toggle", "toggle_row"),
    ("size/check", "check_box"),
    ("size/switch-w", "switch_width"),
    ("size/switch-h", "switch_height"),
    ("size/slider-thumb", "slider_thumb"),
    ("size/badge", "badge"),
    ("size/swatch", "swatch"),
    ("size/timeline-controls", "timeline_controls"),
    ("size/timeline-ruler", "timeline_ruler"),
    ("size/stage-ruler", "stage_ruler"),
    ("size/stage-bar", "zoom_control"),
    ("size/menu-separator", "menu_separator"),
    ("pad/field", "space_3"),
    ("pad/panel", "space_4"),
    ("pad/dialog", "space_5"),
    ("gap/control", "space_2"),
    ("gap/row", "row_gap"),
    ("radius/control", "radius_md"),
    ("radius/panel", "radius_lg"),
    ("width/inspector", "inspector"),
    ("width/label", "label_width"),
    ("width/dialog-label", "dialog_label"),
    ("width/num", "field_num"),
    ("width/tools-1", "tools_rail"),
    ("width/tools-2", "tools_two_columns"),
    ("width/menu", "menu_width"),
    ("width/sidebar", "sidebar"),
    ("width/dialog-s", "dialog_s"),
    ("width/dialog-m", "dialog_m"),
    ("width/dialog-l", "dialog_l"),
    ("width/dialog-xl", "dialog_xl"),
    ("height/dialog-l", "dialog_l_height"),
    ("height/dialog-xl", "dialog_xl_height"),
];

/// Parse the `NF3_DENSITY` object literal: `'name': [a, b, c, 'S']` entries up to the closing `};`.
fn figma_density() -> BTreeMap<String, [f32; 3]> {
    let text = std::fs::read_to_string(SOURCE).unwrap_or_else(|e| panic!("{SOURCE}: {e}"));
    let start = text
        .find("const NF3_DENSITY = {")
        .expect("NF3_DENSITY object");
    let body = &text[start..];
    let body = &body[..body.find("};").expect("end of NF3_DENSITY")];
    let mut out = BTreeMap::new();
    let mut rest = body;
    while let Some(open) = rest.find('\'') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('\'') else { break };
        let name = &after[..close];
        let tail = &after[close + 1..];
        let Some(values) = tail.strip_prefix(": [") else {
            // The scope letter (`'S'`) inside a value list, not a variable name.
            rest = tail;
            continue;
        };
        let end = values.find(']').expect("closing bracket");
        let numbers: Vec<f32> = values[..end]
            .split(',')
            .take(3)
            .map(|n| {
                n.trim()
                    .parse()
                    .unwrap_or_else(|_| panic!("{name}: not a number: {n:?}"))
            })
            .collect();
        let triple: [f32; 3] = numbers
            .try_into()
            .unwrap_or_else(|_| panic!("{name}: expected three density values"));
        assert!(
            out.insert(name.to_owned(), triple).is_none(),
            "{name} is defined twice"
        );
        // Continue after `]`, past the quoted scope letter inside the brackets.
        rest = &values[end..];
    }
    out
}

fn rust_metric(name: &str) -> [f32; 3] {
    metrics::DENSITY_TABLE
        .iter()
        .find(|(metric, _)| *metric == name)
        .unwrap_or_else(|| panic!("metrics::{name} is not in DENSITY_TABLE"))
        .1
}

#[test]
fn figma_density_variables_equal_the_rust_metrics_in_every_density() {
    let figma = figma_density();
    assert!(
        figma.len() >= PAIRS.len(),
        "parsed {} variables",
        figma.len()
    );
    let mut wrong = Vec::new();
    for (variable, metric) in PAIRS {
        let Some(values) = figma.get(*variable) else {
            wrong.push(format!("{variable}: missing in 05-density.js"));
            continue;
        };
        let rust = rust_metric(metric);
        if *values != rust {
            wrong.push(format!(
                "{variable} = {values:?}, metrics::{metric} = {rust:?}"
            ));
        }
    }
    // Layer rows are the three row presets, not a density function.
    let rows = [
        metrics::LAYER_ROW,
        metrics::LAYER_ROW_SHORT,
        metrics::LAYER_ROW_COMPACT,
    ];
    if figma.get("size/layer-row") != Some(&rows) {
        wrong.push(format!(
            "size/layer-row = {:?}, layer row presets = {rows:?}",
            figma.get("size/layer-row")
        ));
    }
    assert!(
        wrong.is_empty(),
        "Figma and Rust disagree:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn the_table_is_what_the_metric_functions_return() {
    for (index, density) in Density::ALL.into_iter().enumerate() {
        metrics::set_density(density);
        let column = |name: &str| rust_metric(name)[index];
        assert_eq!(metrics::control(), column("control"));
        assert_eq!(metrics::row(), column("row"));
        assert_eq!(metrics::panel_tab(), column("panel_tab"));
        assert_eq!(metrics::toggle_row(), column("toggle_row"));
        assert_eq!(metrics::field_m(), column("field_m"));
        assert_eq!(neoflash_ui::tokens::space::s3(), column("space_3"));
        assert_eq!(neoflash_ui::tokens::radius::md(), column("radius_md"));
        assert_eq!(
            metrics::default_layer_row(),
            [
                metrics::LAYER_ROW,
                metrics::LAYER_ROW_SHORT,
                metrics::LAYER_ROW_COMPACT
            ][index]
        );
    }
    metrics::set_density(Density::Comfortable);
}
