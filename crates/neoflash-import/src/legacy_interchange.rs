//! Legacy authoring interchange, colour palette import/export (`.act`, `.clr`, `.ase`),
//! Adobe FXG 2.0 vector import, and migration workflows for Fireworks PNG, FreeHand, WMF/EMF,
//! DXF, EPS, Flash MX 2004 Screens/Forms XML, Adobe AIR descriptors, Flex MXML, and Flash
//! Projector executables (`T-122`).

use crate::vector_fidelity::parse_svg_path_data;
use crate::{Diagnostic, ImportError, ImportFormat, ImportReport, Severity, Statistics};
use image::GenericImageView;
use neoflash_core::{
    BlendMode, Color, Element, ElementKind, FillRule, FilterSpec, FrameScript, GlobalSwatch,
    GradientKind, GradientPaint, GradientStop, InstancePlayback, Keyframe, Layer, LayerProperties,
    PathCommand, Project, RectangleOptions, ResourceUnit, Script, ScriptLanguage, Symbol,
    SymbolKind, Timeline, Transform, new_id, validate_path_commands,
};
use roxmltree::{Document, Node};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ============================================================================
// 1. Legacy Palette Import & Export (.act, .clr, .ase)
// ============================================================================

/// Parse an Adobe Color Table (`.act`) palette buffer into [`GlobalSwatch`] entries.
///
/// Supports both standard 768-byte `.act` files (256 RGB triplets) and extended 772-byte `.act`
/// files (768 RGB bytes + 16-bit big-endian colour count `1..=256` + 16-bit big-endian
/// transparency index `0..=255` or `0xFFFF` for none).
pub fn import_act_palette(bytes: &[u8], prefix: &str) -> Result<Vec<GlobalSwatch>, ImportError> {
    if bytes.len() != 768 && bytes.len() != 772 {
        return Err(ImportError::Container(format!(
            "Adobe Color Table (.act) must be 768 or 772 bytes, got {} bytes",
            bytes.len()
        )));
    }
    let name_prefix = if prefix.trim().is_empty() {
        "ACT"
    } else {
        prefix.trim()
    };
    let (count, transparent_index) = if bytes.len() == 772 {
        let raw_count = u16::from_be_bytes([bytes[768], bytes[769]]);
        if raw_count == 0 || raw_count > 256 {
            return Err(ImportError::Container(format!(
                "Invalid .act swatch count {raw_count}; expected 1..=256"
            )));
        }
        let raw_trans = u16::from_be_bytes([bytes[770], bytes[771]]);
        let trans = (raw_trans < 256).then_some(raw_trans as usize);
        (raw_count as usize, trans)
    } else {
        (256usize, None)
    };

    let mut swatches = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * 3;
        let r = bytes[base];
        let g = bytes[base + 1];
        let b = bytes[base + 2];
        let a = if transparent_index == Some(i) { 0 } else { 255 };
        swatches.push(GlobalSwatch {
            id: format!("{name_prefix}-{}", i + 1),
            name: format!("{name_prefix} {}", i + 1),
            color: [r, g, b, a],
        });
    }
    Ok(swatches)
}

/// Serialize up to 256 [`GlobalSwatch`] entries into an extended 772-byte Adobe Color Table (`.act`).
///
/// Writes up to 256 RGB triplets (zero-padded to 768 bytes), followed by the 16-bit big-endian
/// swatch count at offset 768 and transparency index `0xFFFF` at offset 770.
pub fn export_act_palette(swatches: &[GlobalSwatch]) -> Vec<u8> {
    let mut buf = vec![0u8; 772];
    let count = swatches.len().min(256);
    for (i, swatch) in swatches.iter().take(count).enumerate() {
        let base = i * 3;
        buf[base] = swatch.color[0];
        buf[base + 1] = swatch.color[1];
        buf[base + 2] = swatch.color[2];
    }
    let count_be = (count as u16).to_be_bytes();
    buf[768] = count_be[0];
    buf[769] = count_be[1];
    buf[770] = 0xFF;
    buf[771] = 0xFF;
    buf
}

/// Parse a Flash Color Set (`.clr`) palette buffer into [`GlobalSwatch`] entries.
///
/// Supports:
/// - Binary `FCLR` header + 16-bit big-endian count + `[r, g, b, a, name_len, name_bytes...]`
/// - Text `NEOFLASH-CLR-1` / `#RRGGBBAA name` or `#RRGGBB name` lines
/// - JSON array of `GlobalSwatch` objects
pub fn import_clr_palette(bytes: &[u8], prefix: &str) -> Result<Vec<GlobalSwatch>, ImportError> {
    if bytes.is_empty() {
        return Err(ImportError::Container(
            "Empty Flash Color Set (.clr) buffer".into(),
        ));
    }
    let name_prefix = if prefix.trim().is_empty() {
        "CLR"
    } else {
        prefix.trim()
    };

    if bytes.starts_with(b"FCLR") {
        if bytes.len() < 6 {
            return Err(ImportError::Container(
                "Truncated binary FCLR header in .clr file".into(),
            ));
        }
        let count = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        let mut pos = 6usize;
        let mut swatches = Vec::with_capacity(count.min(1024));
        for i in 0..count {
            if pos + 5 > bytes.len() {
                return Err(ImportError::Container(format!(
                    "Truncated FCLR entry {} at byte {}",
                    i, pos
                )));
            }
            let r = bytes[pos];
            let g = bytes[pos + 1];
            let b = bytes[pos + 2];
            let a = bytes[pos + 3];
            let name_len = bytes[pos + 4] as usize;
            pos += 5;
            if pos + name_len > bytes.len() {
                return Err(ImportError::Container(format!(
                    "Truncated FCLR swatch name at entry {}",
                    i
                )));
            }
            let raw_name = String::from_utf8_lossy(&bytes[pos..pos + name_len])
                .trim()
                .to_owned();
            pos += name_len;
            let name = if raw_name.is_empty() {
                format!("{name_prefix} {}", i + 1)
            } else {
                raw_name
            };
            swatches.push(GlobalSwatch {
                id: format!("{name_prefix}-{}", i + 1),
                name,
                color: [r, g, b, a],
            });
        }
        return Ok(swatches);
    }

    let text = std::str::from_utf8(bytes)
        .map_err(|e| ImportError::Container(format!("Invalid UTF-8 in .clr palette: {e}")))?
        .trim();

    if text.starts_with('[') {
        let parsed: Vec<GlobalSwatch> = serde_json::from_str(text)
            .map_err(|e| ImportError::Container(format!("Invalid JSON .clr palette: {e}")))?;
        return Ok(parsed);
    }

    let mut swatches = Vec::new();
    for (line_idx, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with("//")
            || line.starts_with(';')
            || line.eq_ignore_ascii_case("NEOFLASH-CLR-1")
        {
            continue;
        }
        let Some(hex_rest) = line.strip_prefix('#') else {
            return Err(ImportError::Container(format!(
                "Invalid .clr line {}: expected '#RRGGBBAA name'",
                line_idx + 1
            )));
        };
        let (hex_part, name_part) = match hex_rest.find(char::is_whitespace) {
            Some(split_idx) => (&hex_rest[..split_idx], hex_rest[split_idx..].trim()),
            None => (hex_rest, ""),
        };
        let color = match hex_part.len() {
            6 => {
                let r = u8::from_str_radix(&hex_part[0..2], 16).map_err(|_| {
                    ImportError::Container(format!("Invalid hex colour on line {}", line_idx + 1))
                })?;
                let g = u8::from_str_radix(&hex_part[2..4], 16).map_err(|_| {
                    ImportError::Container(format!("Invalid hex colour on line {}", line_idx + 1))
                })?;
                let b = u8::from_str_radix(&hex_part[4..6], 16).map_err(|_| {
                    ImportError::Container(format!("Invalid hex colour on line {}", line_idx + 1))
                })?;
                [r, g, b, 255]
            }
            8 => {
                let r = u8::from_str_radix(&hex_part[0..2], 16).map_err(|_| {
                    ImportError::Container(format!("Invalid hex colour on line {}", line_idx + 1))
                })?;
                let g = u8::from_str_radix(&hex_part[2..4], 16).map_err(|_| {
                    ImportError::Container(format!("Invalid hex colour on line {}", line_idx + 1))
                })?;
                let b = u8::from_str_radix(&hex_part[4..6], 16).map_err(|_| {
                    ImportError::Container(format!("Invalid hex colour on line {}", line_idx + 1))
                })?;
                let a = u8::from_str_radix(&hex_part[6..8], 16).map_err(|_| {
                    ImportError::Container(format!("Invalid hex colour on line {}", line_idx + 1))
                })?;
                [r, g, b, a]
            }
            _ => {
                return Err(ImportError::Container(format!(
                    "Expected 6 or 8 hex digits on line {}",
                    line_idx + 1
                )));
            }
        };
        let idx = swatches.len() + 1;
        let name = if name_part.is_empty() {
            format!("{name_prefix} {idx}")
        } else {
            name_part.to_owned()
        };
        swatches.push(GlobalSwatch {
            id: format!("{name_prefix}-{idx}"),
            name,
            color,
        });
    }

    if swatches.is_empty() {
        return Err(ImportError::Container(
            "No valid colour swatches found in .clr file".into(),
        ));
    }
    Ok(swatches)
}

/// Serialize [`GlobalSwatch`] entries into a Flash Color Set (`.clr`) binary buffer (`FCLR`).
pub fn export_clr_palette(swatches: &[GlobalSwatch]) -> Vec<u8> {
    let count = swatches.len().min(u16::MAX as usize);
    let mut out = Vec::with_capacity(6 + count * 16);
    out.extend_from_slice(b"FCLR");
    out.extend_from_slice(&(count as u16).to_be_bytes());
    for swatch in swatches.iter().take(count) {
        out.extend_from_slice(&swatch.color);
        let name_bytes = swatch.name.as_bytes();
        let name_len = name_bytes.len().min(255);
        out.push(name_len as u8);
        out.extend_from_slice(&name_bytes[..name_len]);
    }
    out
}

/// Parse an Adobe Swatch Exchange (`.ase`) binary file into [`GlobalSwatch`] entries.
///
/// Supports `ASEF` version `1.0` headers, group blocks (`0xC001`/`0xC002`), and colour entry
/// blocks (`0x0001`) with UTF-16BE swatch names and `RGB `, `GRAY`, or `CMYK` colour models.
pub fn import_ase_palette(bytes: &[u8]) -> Result<Vec<GlobalSwatch>, ImportError> {
    if bytes.len() < 12 || &bytes[0..4] != b"ASEF" {
        return Err(ImportError::Container(
            "Invalid Adobe Swatch Exchange (.ase) header: expected 'ASEF' magic".into(),
        ));
    }
    let major = u16::from_be_bytes([bytes[4], bytes[5]]);
    let minor = u16::from_be_bytes([bytes[6], bytes[7]]);
    if major != 1 || minor != 0 {
        return Err(ImportError::Container(format!(
            "Unsupported .ase version {major}.{minor}; expected 1.0"
        )));
    }
    let block_count = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if block_count > 65535 {
        return Err(ImportError::Container(format!(
            "ASE block count {block_count} exceeds safety limit"
        )));
    }

    let mut pos = 12usize;
    let mut swatches = Vec::new();
    for block_idx in 0..block_count {
        if pos + 6 > bytes.len() {
            return Err(ImportError::Container(format!(
                "Truncated ASE block header at block {block_idx}"
            )));
        }
        let block_type = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]);
        let block_len = u32::from_be_bytes([
            bytes[pos + 2],
            bytes[pos + 3],
            bytes[pos + 4],
            bytes[pos + 5],
        ]) as usize;
        pos += 6;
        if pos + block_len > bytes.len() {
            return Err(ImportError::Container(format!(
                "Truncated ASE block payload at block {block_idx}"
            )));
        }
        let payload = &bytes[pos..pos + block_len];
        pos += block_len;

        if block_type != 0x0001 {
            // Skip group start (0xC001) and group end (0xC002) blocks.
            continue;
        }
        if payload.len() < 6 {
            return Err(ImportError::Container(format!(
                "ASE colour block {block_idx} payload too short"
            )));
        }
        let char_count = u16::from_be_bytes([payload[0], payload[1]]) as usize;
        let name_byte_len = char_count.saturating_mul(2);
        if 2 + name_byte_len + 4 > payload.len() {
            return Err(ImportError::Container(format!(
                "ASE colour block {block_idx} name exceeds block payload"
            )));
        }
        let mut utf16_units = Vec::with_capacity(char_count);
        for chunk in payload[2..2 + name_byte_len].chunks_exact(2) {
            let unit = u16::from_be_bytes([chunk[0], chunk[1]]);
            if unit != 0 {
                utf16_units.push(unit);
            }
        }
        let decoded_name = String::from_utf16_lossy(&utf16_units).trim().to_owned();
        let cpos = 2 + name_byte_len;
        let model = &payload[cpos..cpos + 4];
        let values = &payload[cpos + 4..];

        let read_f32 = |slice: &[u8], offset: usize| -> Result<f32, ImportError> {
            let b = slice.get(offset..offset + 4).ok_or_else(|| {
                ImportError::Container(format!("Truncated float in ASE colour block {block_idx}"))
            })?;
            Ok(f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        };

        let color = match model {
            b"RGB " => {
                let r = (read_f32(values, 0)?.clamp(0.0, 1.0) * 255.0).round() as u8;
                let g = (read_f32(values, 4)?.clamp(0.0, 1.0) * 255.0).round() as u8;
                let b = (read_f32(values, 8)?.clamp(0.0, 1.0) * 255.0).round() as u8;
                [r, g, b, 255]
            }
            b"GRAY" => {
                let v = (read_f32(values, 0)?.clamp(0.0, 1.0) * 255.0).round() as u8;
                [v, v, v, 255]
            }
            b"CMYK" => {
                let c = read_f32(values, 0)?.clamp(0.0, 1.0);
                let m = read_f32(values, 4)?.clamp(0.0, 1.0);
                let y = read_f32(values, 8)?.clamp(0.0, 1.0);
                let k = read_f32(values, 12)?.clamp(0.0, 1.0);
                let r = ((1.0 - c) * (1.0 - k) * 255.0).round() as u8;
                let g = ((1.0 - m) * (1.0 - k) * 255.0).round() as u8;
                let b = ((1.0 - y) * (1.0 - k) * 255.0).round() as u8;
                [r, g, b, 255]
            }
            other => {
                return Err(ImportError::Container(format!(
                    "Unsupported ASE colour model '{}' in block {block_idx}",
                    String::from_utf8_lossy(other)
                )));
            }
        };

        let idx = swatches.len() + 1;
        let name = if decoded_name.is_empty() {
            format!("ASE Swatch {idx}")
        } else {
            decoded_name
        };
        swatches.push(GlobalSwatch {
            id: format!("ase-{idx}"),
            name,
            color,
        });
    }

    Ok(swatches)
}

// ============================================================================
// 2. Adobe FXG 2.0 Vector Interchange
// ============================================================================

/// Parse an Adobe FXG 2.0 XML document into a standalone [`Project`] and [`ImportReport`].
pub fn import_fxg_str(
    xml_text: &str,
    source_name: &str,
) -> Result<(Project, ImportReport), ImportError> {
    let root_id = new_id("symbol");
    let layer_id = new_id("layer");
    let mut project = Project {
        format_version: 1,
        name: source_name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(source_name)
            .trim_end_matches(".fxg")
            .trim_end_matches(".FXG")
            .to_owned(),
        width: 800,
        height: 600,
        fps: 24,
        background: [255, 255, 255, 255],
        root: root_id.clone(),
        symbols: vec![Symbol {
            id: root_id,
            name: "Main".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: layer_id,
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        ..Default::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        }],
        library_folders: Vec::new(),
        sources: Vec::new(),
        resources: Vec::new(),
        document_class: None,
        scenes: Vec::new(),
    };
    if project.name.is_empty() {
        project.name = "FXG Document".into();
    }
    let report = import_fxg_into_project(&mut project, xml_text, source_name)?;
    Ok((project, report))
}

/// Parse an Adobe FXG 2.0 XML document and merge its `<Library>` definitions, `<Group>` layers,
/// and vector/text elements into `project`.
///
/// Emits location-aware [`ImportReport`] warnings (`fxg-unsupported-element`,
/// `fxg-approximated-filter`) for unsupported FXG tags rather than silently dropping them.
pub fn import_fxg_into_project(
    project: &mut Project,
    xml_text: &str,
    source_name: &str,
) -> Result<ImportReport, ImportError> {
    let doc = Document::parse(xml_text).map_err(|e| {
        ImportError::Container(format!("Invalid FXG 2.0 XML in '{source_name}': {e}"))
    })?;
    let root = doc.root_element();
    if root.tag_name().name() != "Graphic" {
        return Err(ImportError::Container(format!(
            "Expected root <Graphic> element in FXG 2.0 document '{source_name}', found <{}>",
            root.tag_name().name()
        )));
    }

    let version = root
        .attribute("version")
        .map(str::to_owned)
        .or_else(|| Some("2.0".into()));
    let mut report = ImportReport {
        source: source_name.to_owned(),
        format: ImportFormat::Unknown,
        version,
        can_import: true,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    if let Some(vw) = root
        .attribute("viewWidth")
        .or_else(|| root.attribute("width"))
        .and_then(|v| v.parse::<f64>().ok())
        && vw >= 1.0
    {
        project.width = (vw.round() as u32).clamp(1, 65535);
    }
    if let Some(vh) = root
        .attribute("viewHeight")
        .or_else(|| root.attribute("height"))
        .and_then(|v| v.parse::<f64>().ok())
        && vh >= 1.0
    {
        project.height = (vh.round() as u32).clamp(1, 65535);
    }

    let mut definition_map: BTreeMap<String, String> = BTreeMap::new();

    // First pass: parse <Library> -> <Definition name="..."> into Symbols.
    for child in root.children().filter(|n| n.is_element()) {
        if child.tag_name().name() == "Library" {
            for def_node in child.children().filter(|n| n.is_element()) {
                if def_node.tag_name().name() == "Definition" {
                    let def_name = def_node
                        .attribute("name")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("FXGDefinition")
                        .to_owned();
                    let mut def_elements = Vec::new();
                    for item in def_node.children().filter(|n| n.is_element()) {
                        parse_fxg_node_into_elements(
                            item,
                            &definition_map,
                            source_name,
                            &doc,
                            &mut def_elements,
                            &mut report,
                        );
                    }
                    let sym_id = new_id("symbol");
                    let layer_id = new_id("layer");
                    report.statistics.elements += def_elements.len();
                    report.statistics.layers += 1;
                    report.statistics.keyframes += 1;
                    report.statistics.symbols += 1;
                    project.symbols.push(Symbol {
                        id: sym_id.clone(),
                        name: def_name.clone(),
                        folder: String::new(),
                        kind: SymbolKind::MovieClip,
                        timeline: Timeline {
                            frame_count: 1,
                            layers: vec![Layer {
                                id: layer_id,
                                name: def_name.clone(),
                                visible: true,
                                locked: false,
                                keyframes: vec![Keyframe {
                                    frame: 0,
                                    elements: def_elements,
                                    ..Default::default()
                                }],
                                properties: LayerProperties::default(),
                            }],
                            scripts: Vec::new(),
                            guides: Vec::new(),
                        },
                        class_name: None,
                        linkage_id: Some(def_name.clone()),
                    });
                    definition_map.insert(def_name, sym_id);
                } else {
                    let loc = fxg_node_location(&doc, def_node, source_name);
                    report.warning(
                        "fxg-unsupported-element",
                        loc,
                        format!(
                            "Unsupported FXG element <{}> inside <Library>",
                            def_node.tag_name().name()
                        ),
                    );
                }
            }
        }
    }

    // Second pass: parse top-level <Group> as layers and direct shapes onto a main layer.
    let mut created_layers: Vec<Layer> = Vec::new();
    let mut loose_elements: Vec<Element> = Vec::new();

    for child in root.children().filter(|n| n.is_element()) {
        let tag = child.tag_name().name();
        if tag == "Library" || tag == "Private" {
            continue;
        }
        if tag == "Group" {
            let layer_name = child
                .attribute("id")
                .or_else(|| child.attribute("name"))
                .or_else(|| {
                    child
                        .attributes()
                        .find(|a| a.name() == "userLabel")
                        .map(|a| a.value())
                })
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Layer {}", created_layers.len() + 1));
            let visible = child
                .attribute("visible")
                .map(|v| !v.eq_ignore_ascii_case("false"))
                .unwrap_or(true);
            let mut group_elements = Vec::new();
            parse_fxg_node_into_elements(
                child,
                &definition_map,
                source_name,
                &doc,
                &mut group_elements,
                &mut report,
            );
            report.statistics.elements += group_elements.len();
            report.statistics.layers += 1;
            report.statistics.keyframes += 1;
            created_layers.push(Layer {
                id: new_id("layer"),
                name: layer_name,
                visible,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: group_elements,
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            });
        } else {
            parse_fxg_node_into_elements(
                child,
                &definition_map,
                source_name,
                &doc,
                &mut loose_elements,
                &mut report,
            );
        }
    }

    if !loose_elements.is_empty() {
        report.statistics.elements += loose_elements.len();
        report.statistics.layers += 1;
        report.statistics.keyframes += 1;
        created_layers.push(Layer {
            id: new_id("layer"),
            name: "FXG Artwork".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: loose_elements,
                ..Default::default()
            }],
            properties: LayerProperties::default(),
        });
    }

    let root_id = project.root.clone();
    if let Some(root_symbol) = project.symbol_mut(&root_id) {
        if !created_layers.is_empty() {
            let root_is_empty_default = root_symbol.timeline.layers.len() == 1
                && root_symbol.timeline.layers[0]
                    .keyframes
                    .iter()
                    .all(|k| k.elements.is_empty());
            if root_is_empty_default {
                root_symbol.timeline.layers = created_layers;
            } else {
                root_symbol.timeline.layers.extend(created_layers);
            }
        }
        root_symbol.timeline.frame_count = root_symbol.timeline.frame_count.max(1);
    }

    project.upgrade_format_for_features();
    Ok(report)
}

fn fxg_node_location(doc: &Document<'_>, node: Node<'_, '_>, source_name: &str) -> String {
    let pos = doc.text_pos_at(node.range().start);
    format!("{source_name}:{}:{}", pos.row, pos.col)
}

fn parse_fxg_color(raw: Option<&str>, alpha: Option<&str>, default_rgb: [u8; 3]) -> Color {
    let a_f64 = alpha
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let a = (a_f64 * 255.0).round() as u8;
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return [default_rgb[0], default_rgb[1], default_rgb[2], a];
    };
    let hex = s
        .strip_prefix('#')
        .or_else(|| s.strip_prefix("0x"))
        .unwrap_or(s);
    if hex.len() == 6
        && let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        )
    {
        return [r, g, b, a];
    }
    [default_rgb[0], default_rgb[1], default_rgb[2], a]
}

fn parse_fxg_gradient(node: Node<'_, '_>, kind: GradientKind) -> Option<GradientPaint> {
    let mut stops = Vec::new();
    for entry in node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "GradientEntry")
    {
        let ratio_f = entry
            .attribute("ratio")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(if stops.is_empty() { 0.0 } else { 1.0 })
            .clamp(0.0, 1.0);
        let ratio = (ratio_f * 255.0).round() as u8;
        let color = parse_fxg_color(
            entry.attribute("color"),
            entry.attribute("alpha"),
            [0, 0, 0],
        );
        stops.push(GradientStop { ratio, color });
    }
    if stops.is_empty() {
        return None;
    }
    stops.sort_by_key(|s| s.ratio);
    stops.truncate(15);

    let x = node
        .attribute("x")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    let y = node
        .attribute("y")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    let scale_x = node
        .attribute("scaleX")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(100.0)
        .max(1.0);
    let scale_y = node
        .attribute("scaleY")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(scale_x)
        .max(1.0);
    let rot_rad = node
        .attribute("rotation")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0)
        .to_radians();
    let (sin_r, cos_r) = rot_rad.sin_cos();
    let sx = scale_x / 1638.4;
    let sy = scale_y / 1638.4;
    let matrix = [
        sx * cos_r,
        sx * sin_r,
        -sy * sin_r,
        sy * cos_r,
        x + scale_x * 0.5,
        y + scale_y * 0.5,
    ];
    let focal_point_ratio = node
        .attribute("focalPointRatio")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0)
        .clamp(-1.0, 1.0);

    let paint = GradientPaint {
        kind,
        matrix,
        stops,
        focal_point_ratio,
    };
    paint.validate().ok().map(|()| paint)
}

fn extract_fxg_fill_and_stroke(
    node: Node<'_, '_>,
    doc: &Document<'_>,
    source_name: &str,
    report: &mut ImportReport,
) -> (
    Color,
    Option<GradientPaint>,
    Color,
    Option<GradientPaint>,
    f64,
) {
    let mut fill_color = [0, 0, 0, 0];
    let mut fill_gradient = None;
    let mut stroke_color = [0, 0, 0, 0];
    let mut stroke_gradient = None;
    let mut stroke_width = 0.0;

    for child in node.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "fill" => {
                for paint_node in child.children().filter(|n| n.is_element()) {
                    match paint_node.tag_name().name() {
                        "SolidColor" => {
                            fill_color = parse_fxg_color(
                                paint_node.attribute("color"),
                                paint_node.attribute("alpha"),
                                [0, 0, 0],
                            );
                        }
                        "LinearGradient" => {
                            if let Some(grad) = parse_fxg_gradient(paint_node, GradientKind::Linear)
                            {
                                fill_color = grad.stops[0].color;
                                fill_gradient = Some(grad);
                            }
                        }
                        "RadialGradient" => {
                            if let Some(grad) = parse_fxg_gradient(paint_node, GradientKind::Radial)
                            {
                                fill_color = grad.stops[0].color;
                                fill_gradient = Some(grad);
                            }
                        }
                        other => {
                            let loc = fxg_node_location(doc, paint_node, source_name);
                            report.warning(
                                "fxg-unsupported-element",
                                loc,
                                format!("Unsupported FXG fill paint <{other}>"),
                            );
                        }
                    }
                }
            }
            "stroke" => {
                for stroke_node in child.children().filter(|n| n.is_element()) {
                    match stroke_node.tag_name().name() {
                        "SolidColorStroke" => {
                            stroke_color = parse_fxg_color(
                                stroke_node.attribute("color"),
                                stroke_node.attribute("alpha"),
                                [0, 0, 0],
                            );
                            stroke_width = stroke_node
                                .attribute("weight")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(1.0)
                                .max(0.0);
                        }
                        "LinearGradientStroke" => {
                            stroke_width = stroke_node
                                .attribute("weight")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(1.0)
                                .max(0.0);
                            if let Some(grad) =
                                parse_fxg_gradient(stroke_node, GradientKind::Linear)
                            {
                                stroke_color = grad.stops[0].color;
                                stroke_gradient = Some(grad);
                            }
                        }
                        "RadialGradientStroke" => {
                            stroke_width = stroke_node
                                .attribute("weight")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(1.0)
                                .max(0.0);
                            if let Some(grad) =
                                parse_fxg_gradient(stroke_node, GradientKind::Radial)
                            {
                                stroke_color = grad.stops[0].color;
                                stroke_gradient = Some(grad);
                            }
                        }
                        other => {
                            let loc = fxg_node_location(doc, stroke_node, source_name);
                            report.warning(
                                "fxg-unsupported-element",
                                loc,
                                format!("Unsupported FXG stroke paint <{other}>"),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    (
        fill_color,
        fill_gradient,
        stroke_color,
        stroke_gradient,
        stroke_width,
    )
}

fn extract_fxg_transform_and_filters(
    node: Node<'_, '_>,
    doc: &Document<'_>,
    source_name: &str,
    report: &mut ImportReport,
) -> (Transform, BlendMode, bool, Vec<FilterSpec>) {
    let mut transform = Transform {
        x: node
            .attribute("x")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0),
        y: node
            .attribute("y")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0),
        scale_x: node
            .attribute("scaleX")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0),
        scale_y: node
            .attribute("scaleY")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0),
        rotation: node
            .attribute("rotation")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0),
        alpha: node
            .attribute("alpha")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0)
            .clamp(0.0, 1.0),
        ..Default::default()
    };

    let visible = node
        .attribute("visible")
        .map(|v| !v.eq_ignore_ascii_case("false"))
        .unwrap_or(true);
    let blend_mode = node
        .attribute("blendMode")
        .and_then(BlendMode::from_name)
        .unwrap_or(BlendMode::Normal);

    let mut filters = Vec::new();
    for child in node.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "transform" => {
                for tr in child.descendants().filter(|n| n.is_element()) {
                    if tr.tag_name().name() == "Matrix" {
                        let a = tr
                            .attribute("a")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(1.0);
                        let b = tr
                            .attribute("b")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        let c = tr
                            .attribute("c")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        let d = tr
                            .attribute("d")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(1.0);
                        let tx = tr
                            .attribute("tx")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        let ty = tr
                            .attribute("ty")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        if let Some(mut mat_t) = Transform::from_matrix([a, b, c, d, tx, ty]) {
                            mat_t.alpha = transform.alpha;
                            transform = mat_t;
                        }
                    }
                }
            }
            "filters" => {
                for fnode in child.children().filter(|n| n.is_element()) {
                    let fname = fnode.tag_name().name();
                    let loc = fxg_node_location(doc, fnode, source_name);
                    match fname {
                        "BlurFilter" => {
                            let blur_x = fnode
                                .attribute("blurX")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(4.0)
                                .clamp(0.0, 255.0);
                            let blur_y = fnode
                                .attribute("blurY")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(4.0)
                                .clamp(0.0, 255.0);
                            let quality = fnode
                                .attribute("quality")
                                .and_then(|v| v.parse::<u8>().ok())
                                .unwrap_or(1)
                                .clamp(1, 3);
                            filters.push(FilterSpec::Blur {
                                blur_x,
                                blur_y,
                                quality,
                            });
                            report.warning(
                                "fxg-approximated-filter",
                                loc,
                                "FXG <BlurFilter> imported with NeoFlash filter parameters",
                            );
                        }
                        "GlowFilter" => {
                            let blur_x = fnode
                                .attribute("blurX")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(6.0)
                                .clamp(0.0, 255.0);
                            let blur_y = fnode
                                .attribute("blurY")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(6.0)
                                .clamp(0.0, 255.0);
                            let color = parse_fxg_color(
                                fnode.attribute("color"),
                                fnode.attribute("alpha"),
                                [255, 0, 0],
                            );
                            let color_u32 = (u32::from(color[0]) << 16)
                                | (u32::from(color[1]) << 8)
                                | u32::from(color[2]);
                            filters.push(FilterSpec::Glow {
                                color: color_u32,
                                alpha: f64::from(color[3]) / 255.0,
                                blur_x,
                                blur_y,
                                strength: 1.0,
                                quality: 1,
                                inner: false,
                                knockout: false,
                            });
                            report.warning(
                                "fxg-approximated-filter",
                                loc,
                                "FXG <GlowFilter> approximated onto NeoFlash Glow filter",
                            );
                        }
                        "DropShadowFilter" => {
                            let blur_x = fnode
                                .attribute("blurX")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(4.0)
                                .clamp(0.0, 255.0);
                            let blur_y = fnode
                                .attribute("blurY")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(4.0)
                                .clamp(0.0, 255.0);
                            let distance = fnode
                                .attribute("distance")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(4.0)
                                .clamp(-255.0, 255.0);
                            let angle = fnode
                                .attribute("angle")
                                .and_then(|v| v.parse::<f64>().ok())
                                .unwrap_or(45.0);
                            let color = parse_fxg_color(
                                fnode.attribute("color"),
                                fnode.attribute("alpha"),
                                [0, 0, 0],
                            );
                            let color_u32 = (u32::from(color[0]) << 16)
                                | (u32::from(color[1]) << 8)
                                | u32::from(color[2]);
                            filters.push(FilterSpec::DropShadow {
                                distance,
                                angle,
                                color: color_u32,
                                alpha: f64::from(color[3]) / 255.0,
                                blur_x,
                                blur_y,
                                strength: 1.0,
                                quality: 1,
                                inner: false,
                                knockout: false,
                                hide_object: false,
                            });
                            report.warning(
                                "fxg-approximated-filter",
                                loc,
                                "FXG <DropShadowFilter> approximated onto NeoFlash DropShadow filter",
                            );
                        }
                        other => {
                            report.warning(
                                "fxg-approximated-filter",
                                loc,
                                format!("FXG filter <{other}> is approximated or unsupported"),
                            );
                        }
                    }
                }
            }
            "fill" | "stroke" | "content" => {}
            other => {
                let loc = fxg_node_location(doc, child, source_name);
                report.warning(
                    "fxg-unsupported-element",
                    loc,
                    format!(
                        "Unsupported child element <{other}> inside <{}>",
                        node.tag_name().name()
                    ),
                );
            }
        }
    }

    (transform, blend_mode, visible, filters)
}

fn collect_fxg_text(node: Node<'_, '_>) -> String {
    let mut out = String::new();
    for desc in node.descendants() {
        if desc.is_text()
            && let Some(t) = desc.text()
        {
            out.push_str(t);
        } else if desc.is_element()
            && matches!(desc.tag_name().name(), "br" | "p")
            && !out.is_empty()
        {
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out.trim().to_owned()
}

fn parse_fxg_node_into_elements(
    node: Node<'_, '_>,
    definition_map: &BTreeMap<String, String>,
    source_name: &str,
    doc: &Document<'_>,
    out: &mut Vec<Element>,
    report: &mut ImportReport,
) {
    let tag = node.tag_name().name();
    let name = node
        .attribute("id")
        .or_else(|| node.attribute("name"))
        .unwrap_or(tag)
        .to_owned();

    if let Some(symbol_id) = definition_map.get(tag) {
        let (transform, blend_mode, visible, filters) =
            extract_fxg_transform_and_filters(node, doc, source_name, report);
        out.push(Element {
            id: new_id("element"),
            name,
            transform,
            kind: ElementKind::Instance {
                symbol_id: symbol_id.clone(),
                playback: InstancePlayback::Independent,
            },
            scripts: Vec::new(),
            accessibility: None,
            visible,
            cache_as_bitmap: false,
            blend_mode,
            filters,
            merge: false,
        });
        return;
    }

    match tag {
        "Group" => {
            let (group_transform, group_blend, group_visible, group_filters) =
                extract_fxg_transform_and_filters(node, doc, source_name, report);
            let mut children = Vec::new();
            for child in node.children().filter(|n| n.is_element()) {
                if matches!(child.tag_name().name(), "transform" | "filters") {
                    continue;
                }
                parse_fxg_node_into_elements(
                    child,
                    definition_map,
                    source_name,
                    doc,
                    &mut children,
                    report,
                );
            }
            for mut el in children {
                el.transform.x += group_transform.x;
                el.transform.y += group_transform.y;
                el.transform.scale_x *= group_transform.scale_x;
                el.transform.scale_y *= group_transform.scale_y;
                el.transform.rotation += group_transform.rotation;
                el.transform.alpha = (el.transform.alpha * group_transform.alpha).clamp(0.0, 1.0);
                if !group_visible {
                    el.visible = false;
                }
                if !group_blend.is_normal() && el.blend_mode.is_normal() {
                    el.blend_mode = group_blend;
                }
                for f in &group_filters {
                    if el.filters.len() < neoflash_core::MAX_OBJECT_FILTERS {
                        el.filters.push(f.clone());
                    }
                }
                out.push(el);
            }
        }
        "Rect" => {
            let width = node
                .attribute("width")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(100.0)
                .max(1.0);
            let height = node
                .attribute("height")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(100.0)
                .max(1.0);
            let (mut transform, blend_mode, visible, filters) =
                extract_fxg_transform_and_filters(node, doc, source_name, report);
            let rect_opts = node
                .attribute("radiusX")
                .or_else(|| node.attribute("topLeftRadiusX"))
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|&rx| rx > 0.0)
                .map(|rx| RectangleOptions {
                    corner_radii: [rx; 4],
                    corners_locked: true,
                });
            let (fill, fill_grad, stroke, stroke_grad, stroke_width) =
                extract_fxg_fill_and_stroke(node, doc, source_name, report);
            let kind = if fill_grad.is_some() || stroke_grad.is_some() {
                let commands = neoflash_core::rectangle_path_commands(
                    width,
                    height,
                    rect_opts.unwrap_or_default(),
                );
                ElementKind::PaintedPath {
                    commands,
                    fill_rule: FillRule::NonZero,
                    fill,
                    stroke,
                    fill_gradient: fill_grad,
                    stroke_gradient: stroke_grad,
                    stroke_width,
                }
            } else {
                if let Some(opts) = rect_opts {
                    transform.primitive = Some(neoflash_core::PrimitiveOptions::Rectangle(opts));
                }
                ElementKind::Rectangle {
                    width,
                    height,
                    fill,
                    stroke,
                    stroke_width,
                }
            };
            out.push(Element {
                id: new_id("element"),
                name,
                transform,
                kind,
                scripts: Vec::new(),
                accessibility: None,
                visible,
                cache_as_bitmap: false,
                blend_mode,
                filters,
                merge: false,
            });
        }
        "Ellipse" => {
            let width = node
                .attribute("width")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(100.0)
                .max(1.0);
            let height = node
                .attribute("height")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(100.0)
                .max(1.0);
            let (transform, blend_mode, visible, filters) =
                extract_fxg_transform_and_filters(node, doc, source_name, report);
            let (fill, fill_grad, stroke, stroke_grad, stroke_width) =
                extract_fxg_fill_and_stroke(node, doc, source_name, report);
            let kind = if fill_grad.is_some() || stroke_grad.is_some() {
                let commands = neoflash_core::oval_path_commands(width, height, Default::default());
                ElementKind::PaintedPath {
                    commands,
                    fill_rule: FillRule::NonZero,
                    fill,
                    stroke,
                    fill_gradient: fill_grad,
                    stroke_gradient: stroke_grad,
                    stroke_width,
                }
            } else {
                ElementKind::Ellipse {
                    width,
                    height,
                    fill,
                    stroke,
                    stroke_width,
                }
            };
            out.push(Element {
                id: new_id("element"),
                name,
                transform,
                kind,
                scripts: Vec::new(),
                accessibility: None,
                visible,
                cache_as_bitmap: false,
                blend_mode,
                filters,
                merge: false,
            });
        }
        "Line" => {
            let x_from = node
                .attribute("xFrom")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let y_from = node
                .attribute("yFrom")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let x_to = node
                .attribute("xTo")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(100.0);
            let y_to = node
                .attribute("yTo")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let (transform, blend_mode, visible, filters) =
                extract_fxg_transform_and_filters(node, doc, source_name, report);
            let (_, _, mut stroke, stroke_grad, mut stroke_width) =
                extract_fxg_fill_and_stroke(node, doc, source_name, report);
            if stroke_width <= 0.0 {
                stroke_width = 1.0;
                if stroke == [0, 0, 0, 0] {
                    stroke = [0, 0, 0, 255];
                }
            }
            let commands = vec![
                PathCommand::MoveTo {
                    x: x_from,
                    y: y_from,
                },
                PathCommand::LineTo { x: x_to, y: y_to },
            ];
            let kind = if stroke_grad.is_some() {
                ElementKind::PaintedPath {
                    commands,
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke,
                    fill_gradient: None,
                    stroke_gradient: stroke_grad,
                    stroke_width,
                }
            } else {
                ElementKind::Path {
                    commands,
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke,
                    stroke_width,
                }
            };
            out.push(Element {
                id: new_id("element"),
                name,
                transform,
                kind,
                scripts: Vec::new(),
                accessibility: None,
                visible,
                cache_as_bitmap: false,
                blend_mode,
                filters,
                merge: false,
            });
        }
        "Path" => {
            let data = node.attribute("data").unwrap_or("");
            let commands = parse_svg_path_data(data);
            if commands.is_empty() || validate_path_commands(&commands).is_err() {
                let loc = fxg_node_location(doc, node, source_name);
                report.warning(
                    "fxg-unsupported-element",
                    loc,
                    "FXG <Path> has empty or invalid path command data",
                );
                return;
            }
            let fill_rule = match node.attribute("winding") {
                Some(w) if w.eq_ignore_ascii_case("evenOdd") => FillRule::EvenOdd,
                _ => FillRule::NonZero,
            };
            let (transform, blend_mode, visible, filters) =
                extract_fxg_transform_and_filters(node, doc, source_name, report);
            let (fill, fill_grad, stroke, stroke_grad, stroke_width) =
                extract_fxg_fill_and_stroke(node, doc, source_name, report);
            let kind = if fill_grad.is_some() || stroke_grad.is_some() {
                ElementKind::PaintedPath {
                    commands,
                    fill_rule,
                    fill,
                    stroke,
                    fill_gradient: fill_grad,
                    stroke_gradient: stroke_grad,
                    stroke_width,
                }
            } else {
                ElementKind::Path {
                    commands,
                    fill_rule,
                    fill,
                    stroke,
                    stroke_width,
                }
            };
            out.push(Element {
                id: new_id("element"),
                name,
                transform,
                kind,
                scripts: Vec::new(),
                accessibility: None,
                visible,
                cache_as_bitmap: false,
                blend_mode,
                filters,
                merge: false,
            });
        }
        "RichText" | "SimpleText" | "BitmapGraphic" if tag != "BitmapGraphic" => {
            let text = collect_fxg_text(node);
            let (transform, blend_mode, visible, filters) =
                extract_fxg_transform_and_filters(node, doc, source_name, report);
            let font_family = node
                .attribute("fontFamily")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("sans-serif")
                .to_owned();
            let font_size = node
                .attribute("fontSize")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(16.0)
                .clamp(1.0, 1024.0);
            let color =
                parse_fxg_color(node.attribute("color"), node.attribute("alpha"), [0, 0, 0]);
            out.push(Element {
                id: new_id("element"),
                name,
                transform,
                kind: ElementKind::Text {
                    text: if text.is_empty() { "Text".into() } else { text },
                    font_family,
                    font_size,
                    color,
                },
                scripts: Vec::new(),
                accessibility: None,
                visible,
                cache_as_bitmap: false,
                blend_mode,
                filters,
                merge: false,
            });
        }
        other => {
            let loc = fxg_node_location(doc, node, source_name);
            report.warning(
                "fxg-unsupported-element",
                loc,
                format!("Unsupported FXG element <{other}>"),
            );
        }
    }
}

// ============================================================================
// 3. Fireworks PNG, FreeHand, WMF/DXF/EPS & Screens/Projector/AIR Migration
// ============================================================================

/// Classification of specialized legacy authoring, vector, or packaging formats (`T-122`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyInterchangeKind {
    FireworksPng,
    FreeHand,
    WmfEmf,
    Dxf,
    Eps,
    FlashScreensXml,
    AirApplicationXml,
    FlexMxml,
    ProjectorExecutable,
}

/// Structured migration assessment for a legacy authoring or deployment file (`T-122`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMigrationAssessment {
    pub kind: LegacyInterchangeKind,
    pub can_convert: bool,
    pub target_recommendation: String,
    pub notes: Vec<String>,
}

fn detect_fireworks_png_chunks(bytes: &[u8]) -> Vec<String> {
    const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 8 || &bytes[0..8] != PNG_MAGIC {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut pos = 8usize;
    while pos + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let tag = &bytes[pos + 4..pos + 8];
        if matches!(tag, b"mkBF" | b"mkBS" | b"mkBT" | b"mkTS" | b"prVW") {
            let s = String::from_utf8_lossy(tag).to_string();
            if !found.contains(&s) {
                found.push(s);
            }
        }
        let next = pos.saturating_add(12).saturating_add(len);
        if next <= pos || next > bytes.len() {
            break;
        }
        pos = next;
    }
    if found.is_empty() {
        for needle in [b"mkBF", b"mkBS", b"mkBT", b"prVW"] {
            if bytes.windows(4).any(|w| w == needle) {
                found.push(String::from_utf8_lossy(needle).to_string());
            }
        }
    }
    found
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack.windows(needle.len()).any(|w| w == needle)
}

/// Inspect a legacy interchange file (`.png` Fireworks, FreeHand, WMF/EMF, DXF, EPS,
/// Flash `<screens>` XML, Adobe AIR descriptor, Flex MXML, or Flash Projector `.exe`/`.app`)
/// and return a [`LegacyMigrationAssessment`] and location-aware [`ImportReport`].
pub fn inspect_legacy_interchange(
    bytes: &[u8],
    source_name: &str,
) -> Result<(LegacyMigrationAssessment, ImportReport), ImportError> {
    if bytes.is_empty() {
        return Err(ImportError::Container(format!(
            "Legacy interchange source '{source_name}' is empty"
        )));
    }

    let mut report = ImportReport {
        source: source_name.to_owned(),
        format: ImportFormat::Unknown,
        version: None,
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    // 1. Fireworks layered PNG
    let fw_chunks = detect_fireworks_png_chunks(bytes);
    if !fw_chunks.is_empty() {
        report.can_import = true;
        report.warning(
            "fireworks-png-ancillary-chunks",
            source_name,
            format!(
                "Fireworks PNG ancillary chunks ({}) detected; composite PNG imported while proprietary MNG/tree chunks are noted.",
                fw_chunks.join(", ")
            ),
        );
        return Ok((
            LegacyMigrationAssessment {
                kind: LegacyInterchangeKind::FireworksPng,
                can_convert: true,
                target_recommendation:
                    "Import composite PNG bitmap or export Fireworks layers via PSD/SVG interchange."
                        .into(),
                notes: vec![format!(
                    "Detected Macromedia/Adobe Fireworks ancillary chunks: {}",
                    fw_chunks.join(", ")
                )],
            },
            report,
        ));
    }

    // 2. FreeHand (AGD1..AGD4 or FreeHand header)
    if bytes.starts_with(b"AGD1")
        || bytes.starts_with(b"AGD2")
        || bytes.starts_with(b"AGD3")
        || bytes.starts_with(b"AGD4")
        || contains_subslice(&bytes[..bytes.len().min(128)], b"FreeHand")
    {
        report.warning(
            "freehand-legacy-format",
            source_name,
            "Macromedia FreeHand document detected; convert via SVG, PDF, or Illustrator (.ai) for editable vector import.",
        );
        return Ok((
            LegacyMigrationAssessment {
                kind: LegacyInterchangeKind::FreeHand,
                can_convert: false,
                target_recommendation:
                    "Convert FreeHand artwork to SVG, PDF, or Adobe Illustrator (.ai) and import via Vector Fidelity."
                        .into(),
                notes: vec!["Detected FreeHand AGD header signature.".into()],
            },
            report,
        ));
    }

    // 3. WMF / EMF
    if bytes.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A])
        || bytes.starts_with(&[0x01, 0x00, 0x09, 0x00])
        || (bytes.len() >= 44 && &bytes[40..44] == b" EMF")
    {
        report.warning(
            "wmf-emf-legacy-metafile",
            source_name,
            "Windows Metafile (WMF/EMF) detected; convert to SVG or PDF for cross-platform vector import.",
        );
        return Ok((
            LegacyMigrationAssessment {
                kind: LegacyInterchangeKind::WmfEmf,
                can_convert: false,
                target_recommendation:
                    "Convert WMF/EMF metafile to SVG or PDF for cross-platform vector fidelity."
                        .into(),
                notes: vec!["Detected Aldus Placeable WMF or EMF header.".into()],
            },
            report,
        ));
    }

    // 4. EPS (%!PS-Adobe ... EPSF)
    if bytes.starts_with(b"%!PS-Adobe")
        && contains_subslice(&bytes[..bytes.len().min(256)], b"EPSF")
        || bytes.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6])
    {
        report.warning(
            "eps-postscript-interchange",
            source_name,
            "Encapsulated PostScript (EPS) requires a PostScript interpreter; convert to PDF or SVG for editable vector paths.",
        );
        return Ok((
            LegacyMigrationAssessment {
                kind: LegacyInterchangeKind::Eps,
                can_convert: false,
                target_recommendation:
                    "Convert Encapsulated PostScript (.eps) to PDF or SVG for editable vector import."
                        .into(),
                notes: vec!["Detected %!PS-Adobe EPSF header.".into()],
            },
            report,
        ));
    }

    // 5. Flash Projector executable (.exe / .app with FA123456 footer or embedded SWF)
    let has_projector_footer = contains_subslice(bytes, b"FA123456")
        || contains_subslice(bytes, &[0x56, 0x34, 0x12, 0xFA])
        || contains_subslice(bytes, &[0xFA, 0x12, 0x34, 0x56]);
    let has_swf_sig = contains_subslice(bytes, b"FWS")
        || contains_subslice(bytes, b"CWS")
        || contains_subslice(bytes, b"ZWS");
    let looks_like_projector_name = source_name.to_ascii_lowercase().ends_with(".exe")
        || source_name.to_ascii_lowercase().ends_with(".app");
    if has_projector_footer
        || ((bytes.starts_with(b"MZ") || looks_like_projector_name) && has_swf_sig)
    {
        report.can_import = true;
        report.warning(
            "projector-executable-detected",
            source_name,
            "Legacy Flash Projector executable detected; extract embedded SWF or publish as an offline HTML5 PWA.",
        );
        return Ok((
            LegacyMigrationAssessment {
                kind: LegacyInterchangeKind::ProjectorExecutable,
                can_convert: true,
                target_recommendation:
                    "Extract embedded SWF payload and publish as an offline HTML5 Web App (manifest.webmanifest + sw.js)."
                        .into(),
                notes: vec![
                    "Detected Flash Projector signature and embedded SWF container.".into(),
                ],
            },
            report,
        ));
    }

    // Text/XML-based formats: DXF, Flash Screens XML, AIR descriptor, Flex MXML
    if let Ok(text) = std::str::from_utf8(bytes) {
        let trimmed = text.trim_start();
        if trimmed.starts_with("0\nSECTION") || trimmed.starts_with("0\r\nSECTION") {
            report.warning(
                "dxf-cad-interchange",
                source_name,
                "AutoCAD DXF drawing detected; export to SVG or PDF for 2D stage import.",
            );
            return Ok((
                LegacyMigrationAssessment {
                    kind: LegacyInterchangeKind::Dxf,
                    can_convert: false,
                    target_recommendation:
                        "Convert AutoCAD DXF to SVG or PDF for 2D vector stage import.".into(),
                    notes: vec!["Detected AutoCAD DXF 0 SECTION header.".into()],
                },
                report,
            ));
        }

        if trimmed.contains("http://ns.adobe.com/air/application/")
            || (trimmed.contains("<application") && trimmed.contains("<initialWindow"))
        {
            report.can_import = true;
            report.warning(
                "air-descriptor-pwa-migration",
                source_name,
                "Adobe AIR application descriptor detected; use generate_pwa_offline_manifest() for offline HTML5 deployment.",
            );
            return Ok((
                LegacyMigrationAssessment {
                    kind: LegacyInterchangeKind::AirApplicationXml,
                    can_convert: true,
                    target_recommendation:
                        "Migrate Adobe AIR desktop packaging to HTML5 PWA (manifest.webmanifest + offline Service Worker)."
                            .into(),
                    notes: vec!["Detected Adobe AIR application descriptor XML.".into()],
                },
                report,
            ));
        }

        if trimmed.contains("<mx:Application")
            || trimmed.contains("<s:Application")
            || trimmed.contains("<s:WindowedApplication")
            || trimmed.contains("http://www.adobe.com/2006/mxml")
        {
            report.warning(
                "flex-mxml-application",
                source_name,
                "Apache/Adobe Flex MXML layout detected; migrate MXML view states to NeoFlash scenes or HTML5 UI.",
            );
            return Ok((
                LegacyMigrationAssessment {
                    kind: LegacyInterchangeKind::FlexMxml,
                    can_convert: false,
                    target_recommendation:
                        "Migrate Flex MXML layout states into NeoFlash scenes/symbols or standard HTML5 components."
                            .into(),
                    notes: vec!["Detected Flex MXML Application root tag.".into()],
                },
                report,
            ));
        }

        if trimmed.contains("<screens")
            || trimmed.starts_with("<Slide")
            || trimmed.starts_with("<Form")
        {
            report.can_import = true;
            report.warning(
                "screens-migrated-to-scenes",
                source_name,
                "Flash MX 2004 / Flash 8 Screens/Forms XML can be migrated into NeoFlash scenes.",
            );
            return Ok((
                LegacyMigrationAssessment {
                    kind: LegacyInterchangeKind::FlashScreensXml,
                    can_convert: true,
                    target_recommendation:
                        "Import <Slide> and <Form> screen hierarchy into NeoFlash project.scenes MovieClip timelines."
                            .into(),
                    notes: vec!["Detected Flash Screens/Forms XML hierarchy.".into()],
                },
                report,
            ));
        }
    }

    Err(ImportError::Container(format!(
        "Unrecognized legacy interchange format in '{source_name}'"
    )))
}

/// Inspect a legacy interchange payload and, when convertible (such as Flash `<screens>` XML or
/// Fireworks PNG), migrate its content directly into `project`.
pub fn inspect_and_migrate_legacy_interchange(
    project: &mut Project,
    bytes: &[u8],
    source_name: &str,
) -> Result<(LegacyMigrationAssessment, ImportReport), ImportError> {
    let (assessment, mut report) = inspect_legacy_interchange(bytes, source_name)?;
    match assessment.kind {
        LegacyInterchangeKind::FlashScreensXml => {
            let xml_text = std::str::from_utf8(bytes).map_err(|e| {
                ImportError::Container(format!("Invalid UTF-8 in Flash Screens XML: {e}"))
            })?;
            report = import_flash_screens_xml_into_project(project, xml_text, source_name)?;
        }
        LegacyInterchangeKind::FireworksPng => {
            if let Ok(img) = image::load_from_memory(bytes) {
                let (w, h) = img.dimensions();
                let res_id = new_id("resource");
                let res_path = format!(
                    "fireworks/{}",
                    source_name
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or("fireworks.png")
                );
                if let Ok(unit) =
                    ResourceUnit::from_bytes(res_id.clone(), &res_path, "image/png", bytes)
                {
                    project.resources.push(unit);
                    project.format_version = project.format_version.max(3);
                    let root_id = project.root.clone();
                    if let Some(root_sym) = project.symbol_mut(&root_id)
                        && let Some(first_layer) = root_sym.timeline.layers.first_mut()
                        && let Some(first_key) = first_layer.keyframes.first_mut()
                    {
                        first_key.elements.push(Element {
                            id: new_id("element"),
                            name: "Fireworks Composite".into(),
                            transform: Transform::default(),
                            kind: ElementKind::Bitmap {
                                resource_id: res_id,
                                width: w.max(1),
                                height: h.max(1),
                                smoothing: true,
                            },
                            scripts: Vec::new(),
                            accessibility: None,
                            visible: true,
                            cache_as_bitmap: false,
                            blend_mode: BlendMode::Normal,
                            filters: Vec::new(),
                            merge: false,
                        });
                        report.statistics.elements += 1;
                    }
                }
            }
        }
        _ => {}
    }
    Ok((assessment, report))
}

#[derive(Clone, Debug)]
struct ParsedFlashScreen {
    name: String,
    kind_label: String,
    width: Option<u32>,
    height: Option<u32>,
    script: Option<String>,
    texts: Vec<String>,
}

fn collect_flash_screens_recursive(node: Node<'_, '_>, out: &mut Vec<ParsedFlashScreen>) {
    let tag = node.tag_name().name();
    if matches!(tag, "Slide" | "Form" | "screen" | "Screen") {
        let name = node
            .attribute("name")
            .or_else(|| node.attribute("id"))
            .or_else(|| node.attribute("label"))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(tag)
            .to_owned();
        let width = node
            .attribute("width")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|&w| (1..=65535).contains(&w));
        let height = node
            .attribute("height")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|&h| (1..=65535).contains(&h));
        let mut script = node.attribute("script").map(str::to_owned);
        let mut texts = Vec::new();
        for child in node.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "script" | "Script" | "actions" => {
                    if let Some(code) = child.text().map(str::trim).filter(|s| !s.is_empty()) {
                        script = Some(code.to_owned());
                    }
                }
                "text" | "Label" | "title" => {
                    if let Some(t) = child.text().map(str::trim).filter(|s| !s.is_empty()) {
                        texts.push(t.to_owned());
                    } else if let Some(t) = child
                        .attribute("text")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        texts.push(t.to_owned());
                    }
                }
                _ => {}
            }
        }
        out.push(ParsedFlashScreen {
            name,
            kind_label: tag.to_owned(),
            width,
            height,
            script,
            texts,
        });
    }
    for child in node.children().filter(|n| n.is_element()) {
        collect_flash_screens_recursive(child, out);
    }
}

/// Convert a Flash MX 2004 / Flash 8 `<screens>` (`<Slide>` / `<Form>`) XML document into
/// NeoFlash `SymbolKind::MovieClip` scene symbols registered in `project.scenes`.
///
/// Preserves screen names, dimensions, and frame scripts/elements, and records
/// `screens-migrated-to-scenes` in the returned [`ImportReport`].
pub fn import_flash_screens_xml_into_project(
    project: &mut Project,
    xml_text: &str,
    source_name: &str,
) -> Result<ImportReport, ImportError> {
    let doc = Document::parse(xml_text).map_err(|e| {
        ImportError::Container(format!("Invalid Flash Screens XML in '{source_name}': {e}"))
    })?;
    let root = doc.root_element();
    let mut screens = Vec::new();
    collect_flash_screens_recursive(root, &mut screens);
    if screens.is_empty() {
        return Err(ImportError::Container(format!(
            "No <Slide> or <Form> screens found in '{source_name}'"
        )));
    }

    if let Some(w) = root
        .attribute("width")
        .and_then(|v| v.parse::<u32>().ok())
        .or_else(|| screens.iter().find_map(|s| s.width))
        && (1..=65535).contains(&w)
    {
        project.width = w;
    }
    if let Some(h) = root
        .attribute("height")
        .and_then(|v| v.parse::<u32>().ok())
        .or_else(|| screens.iter().find_map(|s| s.height))
        && (1..=65535).contains(&h)
    {
        project.height = h;
    }

    let mut report = ImportReport {
        source: source_name.to_owned(),
        format: ImportFormat::Unknown,
        version: Some("Flash MX 2004 Screens".into()),
        can_import: true,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    if project.scenes.is_empty() {
        project.scenes.push(project.root.clone());
    }

    let root_id = project.root.clone();
    let reuse_root_for_first = project.scenes.len() == 1
        && project.symbol(&root_id).is_some_and(|sym| {
            sym.timeline.layers.len() <= 1
                && sym
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .all(|k| k.elements.is_empty())
        });

    for (idx, screen) in screens.iter().enumerate() {
        let mut elements = Vec::new();
        for (t_idx, text_str) in screen.texts.iter().enumerate() {
            elements.push(Element {
                id: new_id("element"),
                name: format!("{} Text {}", screen.name, t_idx + 1),
                transform: Transform {
                    x: 24.0,
                    y: 32.0 + (t_idx as f64) * 28.0,
                    ..Default::default()
                },
                kind: ElementKind::Text {
                    text: text_str.clone(),
                    font_family: "sans-serif".into(),
                    font_size: 18.0,
                    color: [0, 0, 0, 255],
                },
                scripts: Vec::new(),
                accessibility: None,
                visible: true,
                cache_as_bitmap: false,
                blend_mode: BlendMode::Normal,
                filters: Vec::new(),
                merge: false,
            });
        }
        let mut scripts = Vec::new();
        if let Some(code) = &screen.script {
            scripts.push(FrameScript {
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: code.clone(),
                },
            });
            report.statistics.scripts += 1;
        }

        report.statistics.elements += elements.len();
        report.statistics.layers += 1;
        report.statistics.keyframes += 1;
        report.statistics.symbols += 1;

        let layer = Layer {
            id: new_id("layer"),
            name: format!("{} ({})", screen.name, screen.kind_label),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements,
                label: screen.name.clone(),
                ..Default::default()
            }],
            properties: LayerProperties::default(),
        };

        if idx == 0 && reuse_root_for_first {
            if let Some(root_sym) = project.symbol_mut(&root_id) {
                root_sym.name = screen.name.clone();
                root_sym.kind = SymbolKind::MovieClip;
                root_sym.timeline.frame_count = 1;
                root_sym.timeline.layers = vec![layer];
                root_sym.timeline.scripts = scripts;
            }
        } else {
            let sym_id = new_id("symbol");
            project.symbols.push(Symbol {
                id: sym_id.clone(),
                name: screen.name.clone(),
                folder: String::new(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    frame_count: 1,
                    layers: vec![layer],
                    scripts,
                    guides: Vec::new(),
                },
                class_name: None,
                linkage_id: None,
            });
            project.scenes.push(sym_id);
        }
    }

    report.diagnostics.push(Diagnostic {
        severity: Severity::Info,
        code: "screens-migrated-to-scenes".into(),
        location: source_name.to_owned(),
        message: format!(
            "Migrated {} Flash <Slide>/<Form> screen(s) into NeoFlash scene timelines.",
            screens.len()
        ),
    });

    project.upgrade_format_for_features();
    Ok(report)
}

/// Generate a standards-compliant Web App Manifest (`manifest.webmanifest`) and offline Service
/// Worker (`sw.js`) for `project` as the HTML5 replacement for legacy Flash Projector and Adobe
/// AIR desktop packaging (`T-122`).
pub fn generate_pwa_offline_manifest(project: &Project, start_url: &str) -> (String, String) {
    let resolved_start = if start_url.trim().is_empty() {
        "./index.html"
    } else {
        start_url.trim()
    };
    let bg_hex = format!(
        "#{:02x}{:02x}{:02x}",
        project.background[0], project.background[1], project.background[2]
    );
    let orientation = if project.width >= project.height {
        "landscape"
    } else {
        "portrait"
    };
    let app_name = if project.name.trim().is_empty() {
        "NeoFlash Application"
    } else {
        project.name.trim()
    };

    let manifest_obj = serde_json::json!({
        "name": app_name,
        "short_name": app_name,
        "start_url": resolved_start,
        "scope": "./",
        "display": "standalone",
        "orientation": orientation,
        "background_color": bg_hex,
        "theme_color": bg_hex,
        "description": format!("Published by NeoFlash ({}x{} @ {} fps)", project.width, project.height, project.fps),
        "icons": [
            {
                "src": "icon-192.png",
                "sizes": "192x192",
                "type": "image/png"
            },
            {
                "src": "icon-512.png",
                "sizes": "512x512",
                "type": "image/png"
            }
        ]
    });
    let manifest_json =
        serde_json::to_string_pretty(&manifest_obj).unwrap_or_else(|_| "{}".to_owned());

    let slug: String = app_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let cache_name = format!(
        "neoflash-pwa-{}-v{}",
        slug.trim_matches('-'),
        project.format_version
    );

    let sw_js = format!(
        r#"const CACHE_NAME = "{cache_name}";
const PRECACHE_URLS = [
  "{resolved_start}",
  "./index.html",
  "./player.js",
  "./audio.js",
  "./neoflash_web_bg.wasm",
  "./project.json",
  "./manifest.webmanifest"
];

self.addEventListener("install", (event) => {{
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(PRECACHE_URLS))
  );
  self.skipWaiting();
}});

self.addEventListener("activate", (event) => {{
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(
        keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key))
      )
    )
  );
  self.clients.claim();
}});

self.addEventListener("fetch", (event) => {{
  if (event.request.method !== "GET") return;
  event.respondWith(
    caches.match(event.request).then((cached) => cached || fetch(event.request))
  );
}});
"#
    );

    (manifest_json, sw_js)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn act_palette_768_and_772_roundtrip() {
        let mut raw_768 = vec![0u8; 768];
        raw_768[0..3].copy_from_slice(&[255, 64, 32]);
        raw_768[3..6].copy_from_slice(&[16, 128, 240]);
        let parsed_768 = import_act_palette(&raw_768, "act").expect("parse 768-byte ACT");
        assert_eq!(parsed_768.len(), 256);
        assert_eq!(parsed_768[0].color, [255, 64, 32, 255]);
        assert_eq!(parsed_768[1].color, [16, 128, 240, 255]);

        let small_swatches = vec![
            GlobalSwatch {
                id: "s-1".into(),
                name: "Primary".into(),
                color: [220, 40, 80, 255],
            },
            GlobalSwatch {
                id: "s-2".into(),
                name: "Secondary".into(),
                color: [30, 180, 90, 255],
            },
            GlobalSwatch {
                id: "s-3".into(),
                name: "Accent".into(),
                color: [10, 20, 250, 255],
            },
        ];
        let exported_772 = export_act_palette(&small_swatches);
        assert_eq!(exported_772.len(), 772);
        assert_eq!(
            u16::from_be_bytes([exported_772[768], exported_772[769]]),
            3
        );
        assert_eq!(
            u16::from_be_bytes([exported_772[770], exported_772[771]]),
            0xFFFF
        );

        let reimported =
            import_act_palette(&exported_772, "Swatch").expect("reimport 772-byte ACT");
        assert_eq!(reimported.len(), 3);
        assert_eq!(reimported[0].color, [220, 40, 80, 255]);
        assert_eq!(reimported[1].color, [30, 180, 90, 255]);
        assert_eq!(reimported[2].color, [10, 20, 250, 255]);

        // Transparency index check in 772-byte ACT
        let mut with_trans = exported_772.clone();
        with_trans[770] = 0x00;
        with_trans[771] = 0x01; // index 1 is transparent
        let trans_parsed =
            import_act_palette(&with_trans, "Swatch").expect("parse transparent ACT");
        assert_eq!(trans_parsed[0].color[3], 255);
        assert_eq!(trans_parsed[1].color[3], 0);
    }

    #[test]
    fn clr_palette_binary_and_text_roundtrip() {
        let swatches = vec![
            GlobalSwatch {
                id: "clr-1".into(),
                name: "Hero Red".into(),
                color: [255, 32, 64, 255],
            },
            GlobalSwatch {
                id: "clr-2".into(),
                name: "Glass Cyan".into(),
                color: [0, 210, 255, 128],
            },
        ];
        let bytes = export_clr_palette(&swatches);
        assert!(bytes.starts_with(b"FCLR"));
        let roundtripped = import_clr_palette(&bytes, "clr").expect("roundtrip binary CLR");
        assert_eq!(roundtripped.len(), 2);
        assert_eq!(roundtripped[0].name, "Hero Red");
        assert_eq!(roundtripped[0].color, [255, 32, 64, 255]);
        assert_eq!(roundtripped[1].name, "Glass Cyan");
        assert_eq!(roundtripped[1].color, [0, 210, 255, 128]);

        let text_clr = b"NEOFLASH-CLR-1\n#FF2040FF Hero Red\n#00D2FF80 Glass Cyan\n";
        let from_text = import_clr_palette(text_clr, "clr").expect("parse text CLR");
        assert_eq!(from_text.len(), 2);
        assert_eq!(from_text[0].name, "Hero Red");
        assert_eq!(from_text[0].color, [255, 32, 64, 255]);
        assert_eq!(from_text[1].name, "Glass Cyan");
        assert_eq!(from_text[1].color, [0, 210, 255, 128]);
    }

    #[test]
    fn ase_palette_binary_chunk_parsing() {
        let mut ase = Vec::new();
        ase.extend_from_slice(b"ASEF");
        ase.extend_from_slice(&1u16.to_be_bytes());
        ase.extend_from_slice(&0u16.to_be_bytes());
        ase.extend_from_slice(&2u32.to_be_bytes()); // 2 blocks

        // Block 1: RGB swatch "Sun"
        let mut b1 = Vec::new();
        let name_utf16: [u16; 4] = [b'S' as u16, b'u' as u16, b'n' as u16, 0];
        b1.extend_from_slice(&(name_utf16.len() as u16).to_be_bytes());
        for u in name_utf16 {
            b1.extend_from_slice(&u.to_be_bytes());
        }
        b1.extend_from_slice(b"RGB ");
        b1.extend_from_slice(&1.0f32.to_be_bytes());
        b1.extend_from_slice(&0.5f32.to_be_bytes());
        b1.extend_from_slice(&0.0f32.to_be_bytes());
        b1.extend_from_slice(&0u16.to_be_bytes()); // Global color type

        ase.extend_from_slice(&0x0001u16.to_be_bytes());
        ase.extend_from_slice(&(b1.len() as u32).to_be_bytes());
        ase.extend_from_slice(&b1);

        // Block 2: GRAY swatch "Slate"
        let mut b2 = Vec::new();
        let gray_name: [u16; 6] = [
            b'S' as u16,
            b'l' as u16,
            b'a' as u16,
            b't' as u16,
            b'e' as u16,
            0,
        ];
        b2.extend_from_slice(&(gray_name.len() as u16).to_be_bytes());
        for u in gray_name {
            b2.extend_from_slice(&u.to_be_bytes());
        }
        b2.extend_from_slice(b"GRAY");
        b2.extend_from_slice(&0.25f32.to_be_bytes());
        b2.extend_from_slice(&2u16.to_be_bytes());

        ase.extend_from_slice(&0x0001u16.to_be_bytes());
        ase.extend_from_slice(&(b2.len() as u32).to_be_bytes());
        ase.extend_from_slice(&b2);

        let parsed = import_ase_palette(&ase).expect("parse ASE binary");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "Sun");
        assert_eq!(parsed[0].color, [255, 128, 0, 255]);
        assert_eq!(parsed[1].name, "Slate");
        assert_eq!(parsed[1].color, [64, 64, 64, 255]);
    }

    #[test]
    fn fxg_2_0_import_creates_valid_project_and_reports_diagnostics() {
        let fxg_xml = r##"<?xml version="1.0" encoding="utf-8"?>
<Graphic version="2.0" viewWidth="1024" viewHeight="768" xmlns="http://ns.adobe.com/fxg/2008">
  <Library>
    <Definition name="BadgeSymbol">
      <Ellipse width="60" height="60">
        <fill><SolidColor color="#FF8800" alpha="1.0"/></fill>
      </Ellipse>
    </Definition>
  </Library>
  <Group id="HeroLayer">
    <Rect x="20" y="30" width="200" height="120" radiusX="8">
      <fill>
        <LinearGradient x="0" y="0" scaleX="200">
          <GradientEntry ratio="0.0" color="#112233" alpha="1.0"/>
          <GradientEntry ratio="1.0" color="#4488CC" alpha="1.0"/>
        </LinearGradient>
      </fill>
      <stroke><SolidColorStroke color="#FFFFFF" weight="2"/></stroke>
      <filters>
        <DropShadowFilter distance="4" angle="45" blurX="6" blurY="6" color="#000000" alpha="0.5"/>
      </filters>
    </Rect>
    <Path data="M 10 10 L 90 10 L 50 70 Z" winding="evenOdd">
      <fill><SolidColor color="#00CC66"/></fill>
    </Path>
    <Line xFrom="0" yFrom="0" xTo="120" yTo="40">
      <stroke><SolidColorStroke color="#333333" weight="3"/></stroke>
    </Line>
    <RichText x="40" y="180" fontFamily="Inter" fontSize="22" color="#222222">
      <content><p><span>Hello FXG 2.0</span></p></content>
    </RichText>
    <BadgeSymbol x="300" y="100"/>
    <CustomUnsupportedTag foo="bar"/>
  </Group>
</Graphic>"##;

        let (project, report) = import_fxg_str(fxg_xml, "sample.fxg").expect("import FXG 2.0");
        project
            .validate()
            .expect("imported FXG project must validate");
        assert_eq!(project.width, 1024);
        assert_eq!(project.height, 768);
        assert!(
            project.symbols.iter().any(|s| s.name == "BadgeSymbol"),
            "Definition must become a Symbol"
        );
        let root_sym = project.symbol(&project.root).expect("root symbol");
        assert_eq!(root_sym.timeline.layers[0].name, "HeroLayer");
        let elements = &root_sym.timeline.layers[0].keyframes[0].elements;
        assert_eq!(elements.len(), 5);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "fxg-unsupported-element"),
            "must warn on unsupported FXG element"
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "fxg-approximated-filter"),
            "must warn on FXG filter approximation"
        );
    }

    #[test]
    fn legacy_interchange_detection_screens_migration_and_pwa_generation() {
        // 1. Fireworks PNG chunk detection
        let mut fw_png = Vec::new();
        fw_png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        fw_png.extend_from_slice(&4u32.to_be_bytes());
        fw_png.extend_from_slice(b"mkBF");
        fw_png.extend_from_slice(&[1, 2, 3, 4]);
        fw_png.extend_from_slice(&0u32.to_be_bytes()); // CRC
        let (fw_assess, fw_report) =
            inspect_legacy_interchange(&fw_png, "banner.fw.png").expect("inspect Fireworks PNG");
        assert_eq!(fw_assess.kind, LegacyInterchangeKind::FireworksPng);
        assert!(fw_assess.can_convert);
        assert!(!fw_report.diagnostics.is_empty());

        // 2. AIR descriptor, Flex MXML, and Projector inspection
        let air_xml = br#"<?xml version="1.0" encoding="utf-8"?>
<application xmlns="http://ns.adobe.com/air/application/32.0">
  <id>com.example.demo</id>
  <initialWindow><content>demo.swf</content></initialWindow>
</application>"#;
        let (air_assess, _) =
            inspect_legacy_interchange(air_xml, "app.xml").expect("inspect AIR xml");
        assert_eq!(air_assess.kind, LegacyInterchangeKind::AirApplicationXml);
        assert!(air_assess.can_convert);

        let mxml = br#"<?xml version="1.0"?>
<s:Application xmlns:fx="http://ns.adobe.com/mxml/2009" xmlns:s="library://ns.adobe.com/flex/spark">
</s:Application>"#;
        let (mxml_assess, _) =
            inspect_legacy_interchange(mxml, "Main.mxml").expect("inspect Flex MXML");
        assert_eq!(mxml_assess.kind, LegacyInterchangeKind::FlexMxml);
        assert!(!mxml_assess.can_convert);

        let mut projector = b"MZ\x90\x00projector_stub_FWS\x08".to_vec();
        projector.extend_from_slice(&[0x56, 0x34, 0x12, 0xFA]);
        let (proj_assess, _) =
            inspect_legacy_interchange(&projector, "standalone.exe").expect("inspect Projector");
        assert_eq!(proj_assess.kind, LegacyInterchangeKind::ProjectorExecutable);
        assert!(proj_assess.can_convert);

        // 3. Flash <screens> XML migration into project.scenes
        let screens_xml = r#"<screens width="960" height="540">
  <Slide name="IntroSlide" script="stop();">
    <text>Welcome to Slide 1</text>
    <Slide name="DetailSlide">
      <text>Nested Slide 2</text>
    </Slide>
  </Slide>
  <Form name="CheckoutForm">
    <Label text="Order Summary"/>
  </Form>
</screens>"#;
        let (mut project, _) = import_fxg_str(
            r#"<Graphic version="2.0" xmlns="http://ns.adobe.com/fxg/2008"/>"#,
            "empty.fxg",
        )
        .expect("base project");
        let screens_report =
            import_flash_screens_xml_into_project(&mut project, screens_xml, "presentation.xml")
                .expect("migrate screens XML");
        project
            .validate()
            .expect("migrated screens project must validate");
        assert_eq!(project.width, 960);
        assert_eq!(project.height, 540);
        assert_eq!(project.scenes.len(), 3);
        assert!(
            screens_report
                .diagnostics
                .iter()
                .any(|d| d.code == "screens-migrated-to-scenes")
        );

        // 4. PWA manifest & offline Service Worker generation
        let (manifest_json, sw_js) = generate_pwa_offline_manifest(&project, "./index.html");
        let parsed_manifest: serde_json::Value =
            serde_json::from_str(&manifest_json).expect("valid Web App Manifest JSON");
        assert_eq!(parsed_manifest["display"], "standalone");
        assert_eq!(parsed_manifest["start_url"], "./index.html");
        assert!(sw_js.contains("self.addEventListener(\"install\""));
        assert!(sw_js.contains("self.addEventListener(\"fetch\""));
    }
}
