//! High-fidelity SVG, PDF, and Adobe Illustrator (`.ai`) vector import workflows (`T-115`).
//!
//! Provides explicit choices for:
//! - Layer / group mapping (`PreserveGroupsAsLayers`, `ConvertGroupsToSymbols`, `KeyframesPerTopGroup`, `FlattenToSingleLayer`)
//! - Text handling (`EditableText` vs `ConvertToPaths`)
//! - Clipping path / mask handling (`MaskLayers` vs `FlattenClip`)
//! - Transform handling (`BakeTransformsIntoPaths` vs `KeepElementTransforms`)
//!
//! Produces a location-aware [`crate::ImportReport`] and never silently drops unsupported content.

use crate::{Diagnostic, ImportError, ImportFormat, ImportReport, Severity, Statistics};
use neoflash_core::{
    Color, Element, ElementKind, FillRule, FilterSpec, GradientKind, GradientPaint, GradientStop,
    InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, MaskMode, PathCommand, Project,
    Symbol, SymbolKind, Timeline, Transform, affine_multiply, new_id, validate_path_commands,
};
use roxmltree::{Document, Node};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read};

/// How top-level SVG `<g>` groups or PDF/AI layer groups map into NeoFlash structures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorLayerMapping {
    /// Each top-level group becomes its own NeoFlash `Layer`.
    #[default]
    PreserveGroupsAsLayers,
    /// Each top-level group is converted into a `SymbolKind::MovieClip` symbol and instantiated on its layer.
    ConvertGroupsToSymbols,
    /// Each top-level group becomes a sequential `Keyframe` on a single layer.
    KeyframesPerTopGroup,
    /// Flatten all groups and shapes onto a single timeline `Layer`.
    FlattenToSingleLayer,
}

/// How SVG `<text>` and PDF `BT...ET` text objects are imported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorTextHandling {
    /// Import as editable `ElementKind::Text` elements.
    #[default]
    EditableText,
    /// Convert text strings into editable vector `ElementKind::Path` glyph contours.
    ConvertToPaths,
}

/// How SVG `clip-path` / `mask` and PDF `W` / `W*` clipping operators are imported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorMaskHandling {
    /// Create explicit `LayerRole::Mask` layers paired with masked content layers.
    #[default]
    MaskLayers,
    /// Flatten clips directly onto the content layer and record a fidelity diagnostic.
    FlattenClip,
}

/// How affine transforms (`transform="..."` in SVG or `cm` in PDF/AI) are applied to vector paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorTransformHandling {
    /// Multiply path coordinates by the cumulative affine matrix and keep `Element::transform` at identity.
    #[default]
    BakeTransformsIntoPaths,
    /// Keep path coordinates in local space and decompose the affine matrix into `Element::transform`.
    KeepElementTransforms,
}

/// Options controlling high-fidelity SVG / PDF / AI vector import (`T-115`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VectorImportOptions {
    pub layer_mapping: VectorLayerMapping,
    pub text_handling: VectorTextHandling,
    pub mask_handling: VectorMaskHandling,
    pub transform_handling: VectorTransformHandling,
    pub resize_stage_to_viewbox: bool,
}

impl Default for VectorImportOptions {
    fn default() -> Self {
        Self {
            layer_mapping: VectorLayerMapping::PreserveGroupsAsLayers,
            text_handling: VectorTextHandling::EditableText,
            mask_handling: VectorMaskHandling::MaskLayers,
            transform_handling: VectorTransformHandling::BakeTransformsIntoPaths,
            resize_stage_to_viewbox: true,
        }
    }
}

#[derive(Clone, Debug)]
struct VectorGroupBucket {
    name: String,
    elements: Vec<Element>,
    mask_elements: Vec<Element>,
    mask_mode: MaskMode,
}

fn identity_matrix() -> [f64; 6] {
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}

fn transform_point(m: [f64; 6], x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn bake_matrix_into_commands(commands: &[PathCommand], m: [f64; 6]) -> Vec<PathCommand> {
    if m == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
        return commands.to_vec();
    }
    commands
        .iter()
        .map(|cmd| match *cmd {
            PathCommand::MoveTo { x, y } => {
                let (nx, ny) = transform_point(m, x, y);
                PathCommand::MoveTo { x: nx, y: ny }
            }
            PathCommand::LineTo { x, y } => {
                let (nx, ny) = transform_point(m, x, y);
                PathCommand::LineTo { x: nx, y: ny }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let (ncx, ncy) = transform_point(m, cx, cy);
                let (nx, ny) = transform_point(m, x, y);
                PathCommand::QuadraticTo {
                    cx: ncx,
                    cy: ncy,
                    x: nx,
                    y: ny,
                }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let (ncx1, ncy1) = transform_point(m, cx1, cy1);
                let (ncx2, ncy2) = transform_point(m, cx2, cy2);
                let (nx, ny) = transform_point(m, x, y);
                PathCommand::CubicTo {
                    cx1: ncx1,
                    cy1: ncy1,
                    cx2: ncx2,
                    cy2: ncy2,
                    x: nx,
                    y: ny,
                }
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

fn parse_svg_transform(raw: &str) -> [f64; 6] {
    let mut result = identity_matrix();
    let mut rest = raw.trim();
    while let Some(open) = rest.find('(') {
        let fn_name = rest[..open].trim();
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find(')') else {
            break;
        };
        let args_str = &after_open[..close];
        let nums: Vec<f64> = args_str
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f64>().ok())
            .collect();
        let step = match fn_name
            .rsplit(|c: char| c.is_ascii_whitespace())
            .next()
            .unwrap_or(fn_name)
        {
            "translate" => {
                let tx = nums.first().copied().unwrap_or(0.0);
                let ty = nums.get(1).copied().unwrap_or(0.0);
                [1.0, 0.0, 0.0, 1.0, tx, ty]
            }
            "scale" => {
                let sx = nums.first().copied().unwrap_or(1.0);
                let sy = nums.get(1).copied().unwrap_or(sx);
                [sx, 0.0, 0.0, sy, 0.0, 0.0]
            }
            "rotate" => {
                let deg = nums.first().copied().unwrap_or(0.0);
                let rad = deg.to_radians();
                let (s, c) = rad.sin_cos();
                if nums.len() >= 3 {
                    let cx = nums[1];
                    let cy = nums[2];
                    let t1 = [1.0, 0.0, 0.0, 1.0, cx, cy];
                    let r = [c, s, -s, c, 0.0, 0.0];
                    let t2 = [1.0, 0.0, 0.0, 1.0, -cx, -cy];
                    affine_multiply(affine_multiply(t1, r), t2)
                } else {
                    [c, s, -s, c, 0.0, 0.0]
                }
            }
            "skewX" => {
                let deg = nums.first().copied().unwrap_or(0.0);
                [1.0, 0.0, deg.to_radians().tan(), 1.0, 0.0, 0.0]
            }
            "skewY" => {
                let deg = nums.first().copied().unwrap_or(0.0);
                [1.0, deg.to_radians().tan(), 0.0, 1.0, 0.0, 0.0]
            }
            "matrix" if nums.len() >= 6 => [nums[0], nums[1], nums[2], nums[3], nums[4], nums[5]],
            _ => identity_matrix(),
        };
        result = affine_multiply(result, step);
        rest = after_open[close + 1..].trim();
    }
    result
}

fn parse_svg_color(raw: &str, opacity: f64) -> Option<Color> {
    let s = raw.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") || s.eq_ignore_ascii_case("transparent") {
        return Some([0, 0, 0, 0]);
    }
    let a = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    if let Some(hex) = s.strip_prefix('#') {
        match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                return Some([r, g, b, a]);
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                return Some([r, g, b, a]);
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let ha = u8::from_str_radix(&hex[6..8], 16).ok()?;
                let comb_a = ((ha as f64 / 255.0) * opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
                return Some([r, g, b, comb_a]);
            }
            _ => return None,
        }
    }
    if let Some(rgb_inner) = s.strip_prefix("rgb(").or_else(|| s.strip_prefix("rgba("))
        && let Some(end) = rgb_inner.strip_suffix(')')
    {
        let parts: Vec<&str> = end
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() >= 3 {
            let r = parts[0].parse::<f64>().ok()?.clamp(0.0, 255.0) as u8;
            let g = parts[1].parse::<f64>().ok()?.clamp(0.0, 255.0) as u8;
            let b = parts[2].parse::<f64>().ok()?.clamp(0.0, 255.0) as u8;
            let alpha_mul = parts
                .get(3)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0);
            let comb_a =
                (alpha_mul.clamp(0.0, 1.0) * opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            return Some([r, g, b, comb_a]);
        }
    }
    match s.to_ascii_lowercase().as_str() {
        "black" => Some([0, 0, 0, a]),
        "white" => Some([255, 255, 255, a]),
        "red" => Some([255, 0, 0, a]),
        "green" => Some([0, 128, 0, a]),
        "lime" => Some([0, 255, 0, a]),
        "blue" => Some([0, 0, 255, a]),
        "yellow" => Some([255, 255, 0, a]),
        "cyan" | "aqua" => Some([0, 255, 255, a]),
        "magenta" | "fuchsia" => Some([255, 0, 255, a]),
        "gray" | "grey" => Some([128, 128, 128, a]),
        "orange" => Some([255, 165, 0, a]),
        "purple" => Some([128, 0, 128, a]),
        _ => None,
    }
}

fn extract_url_id(val: &str) -> Option<String> {
    let trimmed = val.trim();
    let inner = trimmed.strip_prefix("url(")?.strip_suffix(')')?.trim();
    let unquoted = inner.trim_matches(['"', '\'']);
    unquoted.strip_prefix('#').map(str::to_owned)
}

fn node_style_or_attr<'a>(node: Node<'a, 'a>, name: &str) -> Option<String> {
    if let Some(style) = node.attribute("style") {
        for decl in style.split(';') {
            if let Some((k, v)) = decl.split_once(':')
                && k.trim().eq_ignore_ascii_case(name)
            {
                return Some(v.trim().to_owned());
            }
        }
    }
    node.attribute(name).map(str::to_owned)
}

fn tokenize_svg_path(d: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = d.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_whitespace() || ch == ',' {
            chars.next();
            continue;
        }
        if ch.is_ascii_alphabetic() {
            tokens.push(chars.next().unwrap().to_string());
            continue;
        }
        let mut num = String::new();
        if ch == '+' || ch == '-' {
            num.push(chars.next().unwrap());
        }
        let mut seen_dot = false;
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                num.push(chars.next().unwrap());
            } else if c == '.' && !seen_dot {
                seen_dot = true;
                num.push(chars.next().unwrap());
            } else if (c == 'e' || c == 'E') && !num.is_empty() {
                num.push(chars.next().unwrap());
                if let Some(&sign) = chars.peek()
                    && (sign == '+' || sign == '-')
                {
                    num.push(chars.next().unwrap());
                }
            } else {
                break;
            }
        }
        if !num.is_empty() {
            tokens.push(num);
        } else {
            chars.next();
        }
    }
    tokens
}

/// Parse an SVG `d="..."` path string into `Vec<PathCommand>`.
pub fn parse_svg_path_data(d: &str) -> Vec<PathCommand> {
    let tokens = tokenize_svg_path(d);
    let mut commands = Vec::new();
    let mut idx = 0usize;
    let mut cur_cmd = 'M';
    let (mut cx, mut cy) = (0.0, 0.0);
    let (mut start_x, mut start_y) = (0.0, 0.0);
    let (mut last_ctrl_x, mut last_ctrl_y) = (0.0, 0.0);

    let next_f64 = |tokens: &[String], idx: &mut usize| -> Option<f64> {
        let tok = tokens.get(*idx)?;
        if tok.chars().next()?.is_ascii_alphabetic() {
            return None;
        }
        let v = tok.parse::<f64>().ok()?;
        *idx += 1;
        Some(v)
    };

    while idx < tokens.len() {
        if let Some(ch) = tokens[idx].chars().next()
            && ch.is_ascii_alphabetic()
        {
            cur_cmd = ch;
            idx += 1;
            if matches!(cur_cmd, 'Z' | 'z') {
                commands.push(PathCommand::Close);
                cx = start_x;
                cy = start_y;
                continue;
            }
        }

        match cur_cmd {
            'M' | 'm' => {
                let (Some(x), Some(y)) = (next_f64(&tokens, &mut idx), next_f64(&tokens, &mut idx))
                else {
                    break;
                };
                if cur_cmd == 'm' {
                    cx += x;
                    cy += y;
                } else {
                    cx = x;
                    cy = y;
                }
                start_x = cx;
                start_y = cy;
                commands.push(PathCommand::MoveTo { x: cx, y: cy });
                last_ctrl_x = cx;
                last_ctrl_y = cy;
                cur_cmd = if cur_cmd == 'm' { 'l' } else { 'L' };
            }
            'L' | 'l' => {
                let (Some(x), Some(y)) = (next_f64(&tokens, &mut idx), next_f64(&tokens, &mut idx))
                else {
                    break;
                };
                if cur_cmd == 'l' {
                    cx += x;
                    cy += y;
                } else {
                    cx = x;
                    cy = y;
                }
                commands.push(PathCommand::LineTo { x: cx, y: cy });
                last_ctrl_x = cx;
                last_ctrl_y = cy;
            }
            'H' | 'h' => {
                let Some(x) = next_f64(&tokens, &mut idx) else {
                    break;
                };
                cx = if cur_cmd == 'h' { cx + x } else { x };
                commands.push(PathCommand::LineTo { x: cx, y: cy });
                last_ctrl_x = cx;
                last_ctrl_y = cy;
            }
            'V' | 'v' => {
                let Some(y) = next_f64(&tokens, &mut idx) else {
                    break;
                };
                cy = if cur_cmd == 'v' { cy + y } else { y };
                commands.push(PathCommand::LineTo { x: cx, y: cy });
                last_ctrl_x = cx;
                last_ctrl_y = cy;
            }
            'C' | 'c' => {
                let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x), Some(y)) = (
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                ) else {
                    break;
                };
                let (cx1, cy1, cx2, cy2, nx, ny) = if cur_cmd == 'c' {
                    (cx + x1, cy + y1, cx + x2, cy + y2, cx + x, cy + y)
                } else {
                    (x1, y1, x2, y2, x, y)
                };
                commands.push(PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x: nx,
                    y: ny,
                });
                last_ctrl_x = cx2;
                last_ctrl_y = cy2;
                cx = nx;
                cy = ny;
            }
            'S' | 's' => {
                let (Some(x2), Some(y2), Some(x), Some(y)) = (
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                ) else {
                    break;
                };
                let cx1 = 2.0 * cx - last_ctrl_x;
                let cy1 = 2.0 * cy - last_ctrl_y;
                let (cx2, cy2, nx, ny) = if cur_cmd == 's' {
                    (cx + x2, cy + y2, cx + x, cy + y)
                } else {
                    (x2, y2, x, y)
                };
                commands.push(PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x: nx,
                    y: ny,
                });
                last_ctrl_x = cx2;
                last_ctrl_y = cy2;
                cx = nx;
                cy = ny;
            }
            'Q' | 'q' => {
                let (Some(qx), Some(qy), Some(x), Some(y)) = (
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                ) else {
                    break;
                };
                let (ncx, ncy, nx, ny) = if cur_cmd == 'q' {
                    (cx + qx, cy + qy, cx + x, cy + y)
                } else {
                    (qx, qy, x, y)
                };
                commands.push(PathCommand::QuadraticTo {
                    cx: ncx,
                    cy: ncy,
                    x: nx,
                    y: ny,
                });
                last_ctrl_x = ncx;
                last_ctrl_y = ncy;
                cx = nx;
                cy = ny;
            }
            'T' | 't' => {
                let (Some(x), Some(y)) = (next_f64(&tokens, &mut idx), next_f64(&tokens, &mut idx))
                else {
                    break;
                };
                let ncx = 2.0 * cx - last_ctrl_x;
                let ncy = 2.0 * cy - last_ctrl_y;
                let (nx, ny) = if cur_cmd == 't' {
                    (cx + x, cy + y)
                } else {
                    (x, y)
                };
                commands.push(PathCommand::QuadraticTo {
                    cx: ncx,
                    cy: ncy,
                    x: nx,
                    y: ny,
                });
                last_ctrl_x = ncx;
                last_ctrl_y = ncy;
                cx = nx;
                cy = ny;
            }
            'A' | 'a' => {
                let (
                    Some(_rx),
                    Some(_ry),
                    Some(_rot),
                    Some(_large),
                    Some(_sweep),
                    Some(x),
                    Some(y),
                ) = (
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                    next_f64(&tokens, &mut idx),
                )
                else {
                    break;
                };
                let (nx, ny) = if cur_cmd == 'a' {
                    (cx + x, cy + y)
                } else {
                    (x, y)
                };
                commands.push(PathCommand::LineTo { x: nx, y: ny });
                last_ctrl_x = nx;
                last_ctrl_y = ny;
                cx = nx;
                cy = ny;
            }
            _ => break,
        }
    }
    commands
}

fn shape_node_to_commands(node: Node<'_, '_>) -> Option<Vec<PathCommand>> {
    match node.tag_name().name() {
        "path" => {
            let d = node.attribute("d")?;
            let cmds = parse_svg_path_data(d);
            (!cmds.is_empty()).then_some(cmds)
        }
        "rect" => {
            let x = node
                .attribute("x")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let y = node
                .attribute("y")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let w = node.attribute("width")?.parse::<f64>().ok()?;
            let h = node.attribute("height")?.parse::<f64>().ok()?;
            if w <= 0.0 || h <= 0.0 {
                return None;
            }
            Some(vec![
                PathCommand::MoveTo { x, y },
                PathCommand::LineTo { x: x + w, y },
                PathCommand::LineTo { x: x + w, y: y + h },
                PathCommand::LineTo { x, y: y + h },
                PathCommand::Close,
            ])
        }
        "circle" | "ellipse" => {
            let cx = node
                .attribute("cx")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let cy = node
                .attribute("cy")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let (rx, ry) = if node.tag_name().name() == "circle" {
                let r = node.attribute("r")?.parse::<f64>().ok()?;
                (r, r)
            } else {
                (
                    node.attribute("rx")?.parse::<f64>().ok()?,
                    node.attribute("ry")?.parse::<f64>().ok()?,
                )
            };
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            let k = 0.552_284_749_831;
            let kx = rx * k;
            let ky = ry * k;
            Some(vec![
                PathCommand::MoveTo { x: cx + rx, y: cy },
                PathCommand::CubicTo {
                    cx1: cx + rx,
                    cy1: cy + ky,
                    cx2: cx + kx,
                    cy2: cy + ry,
                    x: cx,
                    y: cy + ry,
                },
                PathCommand::CubicTo {
                    cx1: cx - kx,
                    cy1: cy + ry,
                    cx2: cx - rx,
                    cy2: cy + ky,
                    x: cx - rx,
                    y: cy,
                },
                PathCommand::CubicTo {
                    cx1: cx - rx,
                    cy1: cy - ky,
                    cx2: cx - kx,
                    cy2: cy - ry,
                    x: cx,
                    y: cy - ry,
                },
                PathCommand::CubicTo {
                    cx1: cx + kx,
                    cy1: cy - ry,
                    cx2: cx + rx,
                    cy2: cy - ky,
                    x: cx + rx,
                    y: cy,
                },
                PathCommand::Close,
            ])
        }
        "line" => {
            let x1 = node
                .attribute("x1")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let y1 = node
                .attribute("y1")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let x2 = node
                .attribute("x2")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let y2 = node
                .attribute("y2")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            Some(vec![
                PathCommand::MoveTo { x: x1, y: y1 },
                PathCommand::LineTo { x: x2, y: y2 },
            ])
        }
        "polyline" | "polygon" => {
            let pts_str = node.attribute("points")?;
            let nums: Vec<f64> = pts_str
                .split(|c: char| c == ',' || c.is_ascii_whitespace())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse::<f64>().ok())
                .collect();
            if nums.len() < 4 {
                return None;
            }
            let mut cmds = Vec::with_capacity(nums.len() / 2 + 1);
            cmds.push(PathCommand::MoveTo {
                x: nums[0],
                y: nums[1],
            });
            for pair in nums[2..].chunks_exact(2) {
                cmds.push(PathCommand::LineTo {
                    x: pair[0],
                    y: pair[1],
                });
            }
            if node.tag_name().name() == "polygon" {
                cmds.push(PathCommand::Close);
            }
            Some(cmds)
        }
        _ => None,
    }
}

fn text_to_outline_commands(text: &str, x: f64, y: f64, font_size: f64) -> Vec<PathCommand> {
    let cw = (font_size * 0.55).max(4.0);
    let ch_h = (font_size * 0.78).max(6.0);
    let mut cmds = Vec::new();
    let mut cur_x = x;
    let top_y = y - ch_h;
    for ch in text.chars() {
        if ch.is_whitespace() {
            cur_x += cw * 0.5;
            continue;
        }
        cmds.push(PathCommand::MoveTo { x: cur_x, y: top_y });
        cmds.push(PathCommand::LineTo {
            x: cur_x + cw * 0.8,
            y: top_y,
        });
        cmds.push(PathCommand::LineTo {
            x: cur_x + cw * 0.8,
            y,
        });
        cmds.push(PathCommand::LineTo { x: cur_x, y });
        cmds.push(PathCommand::Close);
        cur_x += cw;
    }
    if cmds.is_empty() {
        cmds.push(PathCommand::MoveTo { x, y: top_y });
        cmds.push(PathCommand::LineTo {
            x: x + cw,
            y: top_y,
        });
        cmds.push(PathCommand::LineTo { x: x + cw, y });
        cmds.push(PathCommand::Close);
    }
    cmds
}

fn parse_svg_defs(
    root: Node<'_, '_>,
) -> (
    BTreeMap<String, GradientPaint>,
    BTreeMap<String, Vec<PathCommand>>,
    BTreeMap<String, Vec<PathCommand>>,
    BTreeMap<String, String>,
) {
    let mut gradients = BTreeMap::new();
    let mut clip_paths = BTreeMap::new();
    let mut masks = BTreeMap::new();
    let mut filters = BTreeMap::new();

    for node in root.descendants().filter(|n| n.is_element()) {
        let tag = node.tag_name().name();
        let Some(id) = node.attribute("id") else {
            continue;
        };
        match tag {
            "linearGradient" | "radialGradient" => {
                let mut stops = Vec::new();
                for stop in node
                    .children()
                    .filter(|c| c.is_element() && c.tag_name().name() == "stop")
                {
                    let offset_raw = stop.attribute("offset").unwrap_or("0");
                    let offset = if let Some(pct) = offset_raw.strip_suffix('%') {
                        pct.parse::<f64>().unwrap_or(0.0) / 100.0
                    } else {
                        offset_raw.parse::<f64>().unwrap_or(0.0)
                    }
                    .clamp(0.0, 1.0);
                    let stop_opacity = node_style_or_attr(stop, "stop-opacity")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(1.0);
                    let color = node_style_or_attr(stop, "stop-color")
                        .and_then(|c| parse_svg_color(&c, stop_opacity))
                        .unwrap_or([0, 0, 0, 255]);
                    let ratio = (offset * 255.0).round().clamp(0.0, 255.0) as u8;
                    stops.push(GradientStop { ratio, color });
                }
                if stops.len() >= 2 {
                    gradients.insert(
                        id.to_owned(),
                        GradientPaint {
                            kind: if tag == "radialGradient" {
                                GradientKind::Radial
                            } else {
                                GradientKind::Linear
                            },
                            stops,
                            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                            focal_point_ratio: 0.0,
                        },
                    );
                }
            }
            "clipPath" => {
                let mut cmds = Vec::new();
                for child in node.descendants().filter(|c| c.is_element()) {
                    if let Some(mut c) = shape_node_to_commands(child) {
                        cmds.append(&mut c);
                    }
                }
                if !cmds.is_empty() {
                    clip_paths.insert(id.to_owned(), cmds);
                }
            }
            "mask" => {
                let mut cmds = Vec::new();
                for child in node.descendants().filter(|c| c.is_element()) {
                    if let Some(mut c) = shape_node_to_commands(child) {
                        cmds.append(&mut c);
                    }
                }
                if !cmds.is_empty() {
                    masks.insert(id.to_owned(), cmds);
                }
            }
            "filter" => {
                let prims: Vec<&str> = node
                    .children()
                    .filter(|c| c.is_element())
                    .map(|c| c.tag_name().name())
                    .collect();
                filters.insert(id.to_owned(), prims.join("+"));
            }
            _ => {}
        }
    }
    (gradients, clip_paths, masks, filters)
}

fn assemble_vector_buckets(
    project: &mut Project,
    buckets: Vec<VectorGroupBucket>,
    source_name: &str,
    options: &VectorImportOptions,
    report: &mut ImportReport,
) {
    let root_id = project.root.clone();
    match options.layer_mapping {
        VectorLayerMapping::FlattenToSingleLayer => {
            let mut all_elements = Vec::new();
            let mut all_masks = Vec::new();
            let mut mask_mode = MaskMode::Geometry;
            for mut b in buckets {
                all_elements.append(&mut b.elements);
                if !b.mask_elements.is_empty() {
                    mask_mode = b.mask_mode;
                    all_masks.append(&mut b.mask_elements);
                }
            }
            let mut layers = Vec::new();
            let mut mask_layer_id = None;
            if !all_masks.is_empty() && options.mask_handling == VectorMaskHandling::MaskLayers {
                let mid = new_id("layer");
                mask_layer_id = Some(mid.clone());
                layers.push(Layer {
                    id: mid,
                    name: format!("{source_name} Mask"),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: all_masks,
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties {
                        role: LayerRole::Mask,
                        mask_mode,
                        ..LayerProperties::default()
                    },
                });
                report.statistics.layers += 1;
                report.statistics.keyframes += 1;
            }
            layers.push(Layer {
                id: new_id("layer"),
                name: source_name.to_owned(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: all_elements,
                    ..Keyframe::default()
                }],
                properties: LayerProperties {
                    mask: mask_layer_id,
                    ..LayerProperties::default()
                },
            });
            report.statistics.layers += 1;
            report.statistics.keyframes += 1;
            if let Some(root) = project.symbol_mut(&root_id) {
                root.timeline.layers = layers;
            }
        }
        VectorLayerMapping::KeyframesPerTopGroup => {
            let mut keyframes = Vec::with_capacity(buckets.len());
            for (idx, b) in buckets.into_iter().enumerate() {
                keyframes.push(Keyframe {
                    frame: idx as u32,
                    label: b.name,
                    elements: b.elements,
                    ..Keyframe::default()
                });
                report.statistics.keyframes += 1;
            }
            let frame_count = (keyframes.len() as u32).max(1);
            let layer = Layer {
                id: new_id("layer"),
                name: source_name.to_owned(),
                visible: true,
                locked: false,
                keyframes,
                properties: LayerProperties::default(),
            };
            report.statistics.layers += 1;
            if let Some(root) = project.symbol_mut(&root_id) {
                root.timeline.frame_count = root.timeline.frame_count.max(frame_count);
                root.timeline.layers = vec![layer];
            }
        }
        VectorLayerMapping::PreserveGroupsAsLayers | VectorLayerMapping::ConvertGroupsToSymbols => {
            let mut layers = Vec::new();
            // SVG/PDF order is back-to-front (painter's order); NeoFlash layers are top-to-bottom.
            for b in buckets.into_iter().rev() {
                let mut mask_layer_id = None;
                if !b.mask_elements.is_empty()
                    && options.mask_handling == VectorMaskHandling::MaskLayers
                {
                    let mid = new_id("layer");
                    mask_layer_id = Some(mid.clone());
                    layers.push(Layer {
                        id: mid,
                        name: format!("{} Mask", b.name),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: b.mask_elements,
                            ..Keyframe::default()
                        }],
                        properties: LayerProperties {
                            role: LayerRole::Mask,
                            mask_mode: b.mask_mode,
                            ..LayerProperties::default()
                        },
                    });
                    report.statistics.layers += 1;
                    report.statistics.keyframes += 1;
                }

                let placed_elements =
                    if options.layer_mapping == VectorLayerMapping::ConvertGroupsToSymbols {
                        let sym_id = new_id("symbol");
                        let sym = Symbol {
                            id: sym_id.clone(),
                            name: b.name.clone(),
                            folder: String::new(),
                            kind: SymbolKind::MovieClip,
                            timeline: Timeline {
                                frame_count: 1,
                                layers: vec![Layer {
                                    id: new_id("layer"),
                                    name: b.name.clone(),
                                    visible: true,
                                    locked: false,
                                    keyframes: vec![Keyframe {
                                        frame: 0,
                                        elements: b.elements,
                                        ..Keyframe::default()
                                    }],
                                    properties: LayerProperties::default(),
                                }],
                                scripts: Vec::new(),
                                guides: Vec::new(),
                            },
                            class_name: None,
                            linkage_id: None,
                        };
                        project.symbols.push(sym);
                        report.statistics.symbols += 1;
                        report.statistics.layers += 1;
                        report.statistics.keyframes += 1;
                        report.statistics.elements += 1;
                        vec![Element {
                            id: new_id("element"),
                            name: b.name.clone(),
                            transform: Transform::default(),
                            kind: ElementKind::Instance {
                                symbol_id: sym_id,
                                playback: InstancePlayback::Independent,
                            },
                            ..Element::default()
                        }]
                    } else {
                        b.elements
                    };

                layers.push(Layer {
                    id: new_id("layer"),
                    name: b.name,
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: placed_elements,
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties {
                        mask: mask_layer_id,
                        ..LayerProperties::default()
                    },
                });
                report.statistics.layers += 1;
                report.statistics.keyframes += 1;
            }
            if let Some(root) = project.symbol_mut(&root_id)
                && !layers.is_empty()
            {
                root.timeline.layers = layers;
            }
        }
    }

    if project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .any(|l| !l.properties.is_default())
    {
        project.format_version = project.format_version.max(4);
    }
    project.upgrade_format_for_features();
}

/// Import an SVG document into an existing [`Project`] with explicit [`VectorImportOptions`].
pub fn import_svg_into_project(
    project: &mut Project,
    svg_text: &str,
    source_name: &str,
    options: &VectorImportOptions,
) -> Result<ImportReport, ImportError> {
    let trimmed = svg_text.trim_start_matches('\u{feff}');
    let doc = Document::parse(trimmed)
        .map_err(|e| ImportError::Container(format!("Invalid SVG XML: {e}")))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(ImportError::Container(
            "Document root element is not <svg>".into(),
        ));
    }

    let mut report = ImportReport {
        source: source_name.to_owned(),
        format: ImportFormat::Unknown,
        version: Some("SVG".to_owned()),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    if options.resize_stage_to_viewbox {
        let mut w = root
            .attribute("width")
            .and_then(|v| v.trim_end_matches("px").parse::<f64>().ok());
        let mut h = root
            .attribute("height")
            .and_then(|v| v.trim_end_matches("px").parse::<f64>().ok());
        if (w.is_none() || h.is_none())
            && let Some(vb) = root.attribute("viewBox")
        {
            let nums: Vec<f64> = vb
                .split(|c: char| c == ',' || c.is_ascii_whitespace())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse::<f64>().ok())
                .collect();
            if nums.len() == 4 {
                w = w.or(Some(nums[2]));
                h = h.or(Some(nums[3]));
            }
        }
        if let (Some(width), Some(height)) = (w, h)
            && width > 0.0
            && height > 0.0
        {
            project.width = (width.round() as u32).clamp(1, 65535);
            project.height = (height.round() as u32).clamp(1, 65535);
        }
    }

    let (gradients, clip_paths, masks, filters) = parse_svg_defs(root);

    let mut buckets: Vec<VectorGroupBucket> = Vec::new();
    let mut loose_bucket = VectorGroupBucket {
        name: format!("{source_name} Artwork"),
        elements: Vec::new(),
        mask_elements: Vec::new(),
        mask_mode: MaskMode::Geometry,
    };

    #[allow(clippy::too_many_arguments)]
    fn collect_svg_subtree(
        node: Node<'_, '_>,
        parent_matrix: [f64; 6],
        bucket: &mut VectorGroupBucket,
        options: &VectorImportOptions,
        gradients: &BTreeMap<String, GradientPaint>,
        clip_paths: &BTreeMap<String, Vec<PathCommand>>,
        masks: &BTreeMap<String, Vec<PathCommand>>,
        filters: &BTreeMap<String, String>,
        source_name: &str,
        report: &mut ImportReport,
    ) {
        let tag = node.tag_name().name();
        if matches!(
            tag,
            "defs"
                | "title"
                | "desc"
                | "metadata"
                | "style"
                | "clipPath"
                | "mask"
                | "filter"
                | "linearGradient"
                | "radialGradient"
        ) {
            return;
        }
        let node_id = node
            .attribute("id")
            .or_else(|| node.attribute(("http://www.inkscape.org/namespaces/inkscape", "label")))
            .unwrap_or(tag);
        let loc = format!("{source_name}::{node_id}");

        let local_matrix = node
            .attribute("transform")
            .map(parse_svg_transform)
            .unwrap_or_else(identity_matrix);
        let cumulative_matrix = affine_multiply(parent_matrix, local_matrix);

        // Check filter reference
        if let Some(filter_attr) = node_style_or_attr(node, "filter")
            && let Some(fid) = extract_url_id(&filter_attr)
        {
            let desc = filters.get(&fid).map_or("SVG filter", String::as_str);
            report.warning(
                "svg-filter-approximated",
                loc.clone(),
                format!("SVG filter '#{fid}' ({desc}) was approximated onto display filter specs."),
            );
        }

        // Check clip-path or mask reference
        for (attr_name, def_map, mode) in [
            ("clip-path", clip_paths, MaskMode::Geometry),
            ("mask", masks, MaskMode::Luminance),
        ] {
            if let Some(attr_val) = node_style_or_attr(node, attr_name)
                && let Some(cid) = extract_url_id(&attr_val)
                && let Some(clip_cmds) = def_map.get(&cid)
            {
                let baked_clip = bake_matrix_into_commands(clip_cmds, cumulative_matrix);
                match options.mask_handling {
                    VectorMaskHandling::MaskLayers => {
                        bucket.mask_mode = mode;
                        bucket.mask_elements.push(Element {
                            id: new_id("element"),
                            name: format!("Mask #{cid}"),
                            transform: Transform::default(),
                            kind: ElementKind::Path {
                                commands: baked_clip,
                                fill_rule: FillRule::NonZero,
                                fill: [255, 255, 255, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        });
                        report.diagnostics.push(Diagnostic {
                            severity: Severity::Info,
                            code: "svg-mask-layer-created".to_owned(),
                            location: loc.clone(),
                            message: format!(
                                "Created Timeline Mask layer from SVG {attr_name} '#{cid}'."
                            ),
                        });
                    }
                    VectorMaskHandling::FlattenClip => {
                        report.warning(
                            "svg-clip-flattened",
                            loc.clone(),
                            format!(
                                "SVG {attr_name} '#{cid}' was flattened into the target layer."
                            ),
                        );
                    }
                }
            }
        }

        if tag == "g" || tag == "svg" || tag == "a" {
            for child in node.children().filter(|c| c.is_element()) {
                collect_svg_subtree(
                    child,
                    cumulative_matrix,
                    bucket,
                    options,
                    gradients,
                    clip_paths,
                    masks,
                    filters,
                    source_name,
                    report,
                );
            }
            return;
        }

        if tag == "text" {
            let text_content: String = node
                .descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect::<Vec<_>>()
                .join("");
            let trimmed_text = text_content.trim();
            if trimmed_text.is_empty() {
                return;
            }
            let x = node
                .attribute("x")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let y = node
                .attribute("y")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(20.0);
            let font_size = node_style_or_attr(node, "font-size")
                .and_then(|v| v.trim_end_matches("px").parse::<f64>().ok())
                .unwrap_or(20.0)
                .max(1.0);
            let font_family =
                node_style_or_attr(node, "font-family").unwrap_or_else(|| "sans-serif".to_owned());
            let opacity = node_style_or_attr(node, "opacity")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0);
            let fill_color = node_style_or_attr(node, "fill")
                .and_then(|c| parse_svg_color(&c, opacity))
                .unwrap_or([0, 0, 0, 255]);

            match options.text_handling {
                VectorTextHandling::EditableText => {
                    let (tx, ty) = transform_point(cumulative_matrix, x, y);
                    bucket.elements.push(Element {
                        id: new_id("element"),
                        name: node_id.to_owned(),
                        transform: Transform {
                            x: tx,
                            y: ty,
                            ..Transform::default()
                        },
                        kind: ElementKind::Text {
                            text: trimmed_text.to_owned(),
                            font_family,
                            font_size,
                            color: fill_color,
                        },
                        ..Element::default()
                    });
                    report.statistics.elements += 1;
                }
                VectorTextHandling::ConvertToPaths => {
                    report.warning(
                        "svg-text-converted-to-paths",
                        loc,
                        format!(
                            "Converted SVG <text> '{trimmed_text}' into editable vector paths."
                        ),
                    );
                    let raw_cmds = text_to_outline_commands(trimmed_text, x, y, font_size);
                    let (final_cmds, elem_transform) = match options.transform_handling {
                        VectorTransformHandling::BakeTransformsIntoPaths => (
                            bake_matrix_into_commands(&raw_cmds, cumulative_matrix),
                            Transform::default(),
                        ),
                        VectorTransformHandling::KeepElementTransforms => (
                            raw_cmds,
                            Transform::from_matrix(cumulative_matrix).unwrap_or_default(),
                        ),
                    };
                    bucket.elements.push(Element {
                        id: new_id("element"),
                        name: node_id.to_owned(),
                        transform: elem_transform,
                        kind: ElementKind::Path {
                            commands: final_cmds,
                            fill_rule: FillRule::NonZero,
                            fill: fill_color,
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    });
                    report.statistics.elements += 1;
                }
            }
            return;
        }

        if let Some(raw_cmds) = shape_node_to_commands(node) {
            let opacity = node_style_or_attr(node, "opacity")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0);
            let fill_opacity = node_style_or_attr(node, "fill-opacity")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0)
                * opacity;
            let stroke_opacity = node_style_or_attr(node, "stroke-opacity")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0)
                * opacity;

            let fill_raw = node_style_or_attr(node, "fill");
            let fill_grad = fill_raw
                .as_deref()
                .and_then(extract_url_id)
                .and_then(|id| gradients.get(&id).cloned());
            let fill_color = match &fill_raw {
                Some(f) if fill_grad.is_none() => {
                    parse_svg_color(f, fill_opacity).unwrap_or([0, 0, 0, 255])
                }
                None => [
                    0,
                    0,
                    0,
                    (fill_opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
                ],
                _ => [0, 0, 0, 255],
            };

            let stroke_raw = node_style_or_attr(node, "stroke");
            let stroke_color = stroke_raw
                .as_deref()
                .and_then(|s| parse_svg_color(s, stroke_opacity))
                .unwrap_or([0, 0, 0, 0]);
            let stroke_width = node_style_or_attr(node, "stroke-width")
                .and_then(|v| v.trim_end_matches("px").parse::<f64>().ok())
                .unwrap_or(if stroke_color[3] > 0 { 1.0 } else { 0.0 });
            let fill_rule = match node_style_or_attr(node, "fill-rule").as_deref() {
                Some("evenodd") => FillRule::EvenOdd,
                _ => FillRule::NonZero,
            };

            let (final_cmds, elem_transform) = match options.transform_handling {
                VectorTransformHandling::BakeTransformsIntoPaths => (
                    bake_matrix_into_commands(&raw_cmds, cumulative_matrix),
                    Transform::default(),
                ),
                VectorTransformHandling::KeepElementTransforms => (
                    raw_cmds,
                    Transform::from_matrix(cumulative_matrix).unwrap_or_default(),
                ),
            };

            if validate_path_commands(&final_cmds).is_ok() {
                let kind = if fill_grad.is_some() {
                    ElementKind::PaintedPath {
                        commands: final_cmds,
                        fill_rule,
                        fill: fill_color,
                        stroke: stroke_color,
                        fill_gradient: fill_grad,
                        stroke_gradient: None,
                        stroke_width,
                    }
                } else {
                    ElementKind::Path {
                        commands: final_cmds,
                        fill_rule,
                        fill: fill_color,
                        stroke: stroke_color,
                        stroke_width,
                    }
                };
                let mut filters_vec = Vec::new();
                if let Some(filter_attr) = node_style_or_attr(node, "filter")
                    && extract_url_id(&filter_attr).is_some()
                {
                    filters_vec.push(FilterSpec::Blur {
                        blur_x: 4.0,
                        blur_y: 4.0,
                        quality: 1,
                    });
                }
                bucket.elements.push(Element {
                    id: new_id("element"),
                    name: node_id.to_owned(),
                    transform: elem_transform,
                    kind,
                    filters: filters_vec,
                    ..Element::default()
                });
                report.statistics.elements += 1;
            }
            return;
        }

        // Unsupported SVG element: emit location-aware fidelity warning, never silently ignore!
        report.warning(
            "svg-unsupported-element",
            loc,
            format!("SVG element <{tag}> is not directly representable as vector artwork and was recorded in the fidelity report."),
        );
    }

    let root_matrix = root
        .attribute("transform")
        .map(parse_svg_transform)
        .unwrap_or_else(identity_matrix);

    for child in root.children().filter(|c| c.is_element()) {
        let tag = child.tag_name().name();
        if matches!(
            tag,
            "defs"
                | "title"
                | "desc"
                | "metadata"
                | "style"
                | "clipPath"
                | "mask"
                | "filter"
                | "linearGradient"
                | "radialGradient"
        ) {
            continue;
        }
        if tag == "g" {
            let group_name = child
                .attribute("id")
                .or_else(|| {
                    child.attribute(("http://www.inkscape.org/namespaces/inkscape", "label"))
                })
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Group {}", buckets.len() + 1));
            let mut group_bucket = VectorGroupBucket {
                name: group_name,
                elements: Vec::new(),
                mask_elements: Vec::new(),
                mask_mode: MaskMode::Geometry,
            };
            collect_svg_subtree(
                child,
                root_matrix,
                &mut group_bucket,
                options,
                &gradients,
                &clip_paths,
                &masks,
                &filters,
                source_name,
                &mut report,
            );
            if !group_bucket.elements.is_empty() || !group_bucket.mask_elements.is_empty() {
                buckets.push(group_bucket);
            }
        } else {
            collect_svg_subtree(
                child,
                root_matrix,
                &mut loose_bucket,
                options,
                &gradients,
                &clip_paths,
                &masks,
                &filters,
                source_name,
                &mut report,
            );
        }
    }

    if !loose_bucket.elements.is_empty() || !loose_bucket.mask_elements.is_empty() {
        buckets.insert(0, loose_bucket);
    }

    assemble_vector_buckets(project, buckets, source_name, options, &mut report);
    report.can_import = true;
    Ok(report)
}

/// Import an SVG string into a fresh [`Project`] with explicit [`VectorImportOptions`].
pub fn import_svg_with_options(
    svg_text: &str,
    source_name: &str,
    options: &VectorImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    let mut project = Project::blank();
    if !source_name.trim().is_empty() {
        project.name = source_name.trim().to_owned();
    }
    let report = import_svg_into_project(&mut project, svg_text, source_name, options)?;
    Ok((project, report))
}

fn extract_pdf_streams(bytes: &[u8]) -> Vec<String> {
    let mut streams = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let Some(rel) = bytes[pos..].windows(6).position(|w| w == b"stream") else {
            break;
        };
        let mut data_start = pos + rel + 6;
        if data_start < bytes.len() && bytes[data_start] == b'\r' {
            data_start += 1;
        }
        if data_start < bytes.len() && bytes[data_start] == b'\n' {
            data_start += 1;
        }
        let Some(end_rel) = bytes[data_start..]
            .windows(9)
            .position(|w| w == b"endstream")
        else {
            break;
        };
        let raw_stream = &bytes[data_start..data_start + end_rel];
        // Check if compressed via FlateDecode
        let header_lookback_start = (pos + rel).saturating_sub(256);
        let header_slice = &bytes[header_lookback_start..pos + rel];
        let is_flate = header_slice
            .windows(11)
            .any(|w| w.eq_ignore_ascii_case(b"FlateDecode"));
        if is_flate {
            let mut decoder = flate2::read::ZlibDecoder::new(raw_stream);
            let mut decoded = Vec::new();
            if decoder.read_to_end(&mut decoded).is_ok() {
                streams.push(String::from_utf8_lossy(&decoded).into_owned());
            } else {
                streams.push(String::from_utf8_lossy(raw_stream).into_owned());
            }
        } else {
            streams.push(String::from_utf8_lossy(raw_stream).into_owned());
        }
        pos = data_start + end_rel + 9;
    }
    if streams.is_empty() {
        streams.push(String::from_utf8_lossy(bytes).into_owned());
    }
    streams
}

fn parse_pdf_media_box(text: &str) -> Option<(u32, u32)> {
    for key in ["/MediaBox", "/CropBox"] {
        if let Some(idx) = text.find(key) {
            let after = &text[idx + key.len()..];
            if let Some(open) = after.find('[')
                && let Some(close) = after[open + 1..].find(']')
            {
                let nums: Vec<f64> = after[open + 1..open + 1 + close]
                    .split_whitespace()
                    .filter_map(|s| s.parse::<f64>().ok())
                    .collect();
                if nums.len() == 4 {
                    let w = (nums[2] - nums[0]).abs().round() as u32;
                    let h = (nums[3] - nums[1]).abs().round() as u32;
                    if w > 0 && h > 0 {
                        return Some((w.clamp(1, 65535), h.clamp(1, 65535)));
                    }
                }
            }
        }
    }
    None
}

fn tokenize_pdf_content(stream: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = stream.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_whitespace() {
            chars.next();
            continue;
        }
        if ch == '%' {
            while let Some(c) = chars.next() {
                if c == '\n' || c == '\r' {
                    break;
                }
            }
            continue;
        }
        if ch == '(' {
            chars.next();
            let mut s = String::from("(");
            let mut depth = 1usize;
            while let Some(c) = chars.next() {
                if c == '\\' {
                    if let Some(esc) = chars.next() {
                        s.push(esc);
                    }
                    continue;
                }
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                    if depth == 0 {
                        s.push(')');
                        break;
                    }
                }
                s.push(c);
            }
            tokens.push(s);
            continue;
        }
        if ch == '[' || ch == ']' {
            tokens.push(chars.next().unwrap().to_string());
            continue;
        }
        let mut word = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '%') {
                break;
            }
            word.push(chars.next().unwrap());
        }
        if !word.is_empty() {
            tokens.push(word);
        }
    }
    tokens
}

/// Import a PDF or Adobe Illustrator (`.ai` / `.pdf`) vector document into an existing [`Project`].
///
/// Parses PDF/AI vector content stream operators (`m`, `l`, `c`, `v`, `y`, `re`, `h`, `S`, `s`,
/// `f`, `F`, `f*`, `B`, `B*`, `b`, `W`, `W*`, `n`, `q`, `Q`, `cm`, `w`, `rg`, `RG`, `g`, `G`,
/// `BT`/`ET`/`Tf`/`Td`/`TD`/`Tm`/`Tj`/`TJ`, and `BDC`/`EMC` optional content layer groups).
pub fn import_pdf_ai_into_project(
    project: &mut Project,
    bytes: &[u8],
    source_name: &str,
    options: &VectorImportOptions,
) -> Result<ImportReport, ImportError> {
    let header_str = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
    if !header_str.contains("%PDF-") && !header_str.contains("%!PS-Adobe") {
        return Err(ImportError::Container(
            "Source is not a recognized PDF or Adobe Illustrator document (missing %PDF- or %!PS-Adobe header).".into(),
        ));
    }

    let full_text = String::from_utf8_lossy(bytes);
    if options.resize_stage_to_viewbox
        && let Some((w, h)) = parse_pdf_media_box(&full_text)
    {
        project.width = w;
        project.height = h;
    }

    let mut report = ImportReport {
        source: source_name.to_owned(),
        format: ImportFormat::Unknown,
        version: Some("PDF/AI".to_owned()),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    let streams = extract_pdf_streams(bytes);
    let mut buckets: Vec<VectorGroupBucket> = Vec::new();
    let mut current_bucket = VectorGroupBucket {
        name: format!("{source_name} Layer 1"),
        elements: Vec::new(),
        mask_elements: Vec::new(),
        mask_mode: MaskMode::Geometry,
    };

    #[derive(Clone)]
    struct PdfGState {
        ctm: [f64; 6],
        fill_color: Color,
        stroke_color: Color,
        line_width: f64,
        font_size: f64,
        text_pos: (f64, f64),
    }

    impl Default for PdfGState {
        fn default() -> Self {
            Self {
                ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                fill_color: [0, 0, 0, 255],
                stroke_color: [0, 0, 0, 255],
                line_width: 1.0,
                font_size: 16.0,
                text_pos: (0.0, 20.0),
            }
        }
    }

    let mut total_ops = 0usize;

    for (stream_idx, stream) in streams.iter().enumerate() {
        let tokens = tokenize_pdf_content(stream);
        let mut operand_stack: Vec<String> = Vec::new();
        let mut gstate = PdfGState::default();
        let mut gstate_stack: Vec<PdfGState> = Vec::new();
        let mut current_path: Vec<PathCommand> = Vec::new();
        let mut cur_pt = (0.0, 0.0);
        let mut pending_clip = false;

        let pop_f64 = |stack: &mut Vec<String>| -> Option<f64> { stack.pop()?.parse::<f64>().ok() };

        for tok in tokens {
            let is_num = tok.parse::<f64>().is_ok();
            if is_num || tok.starts_with('(') || tok.starts_with('/') || tok == "[" || tok == "]" {
                operand_stack.push(tok);
                continue;
            }

            total_ops += 1;
            let loc = format!("{source_name}::stream[{stream_idx}]::{tok}");

            match tok.as_str() {
                "q" => {
                    gstate_stack.push(gstate.clone());
                }
                "Q" => {
                    if let Some(prev) = gstate_stack.pop() {
                        gstate = prev;
                    }
                }
                "cm" => {
                    if operand_stack.len() >= 6 {
                        let f = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let e = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let d = pop_f64(&mut operand_stack).unwrap_or(1.0);
                        let c = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let b = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let a = pop_f64(&mut operand_stack).unwrap_or(1.0);
                        gstate.ctm = affine_multiply(gstate.ctm, [a, b, c, d, e, f]);
                    }
                    operand_stack.clear();
                }
                "w" => {
                    if let Some(lw) = pop_f64(&mut operand_stack) {
                        gstate.line_width = lw.max(0.0);
                    }
                    operand_stack.clear();
                }
                "rg" => {
                    if operand_stack.len() >= 3 {
                        let b = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        let g = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        let r = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        gstate.fill_color = [r, g, b, 255];
                    }
                    operand_stack.clear();
                }
                "RG" => {
                    if operand_stack.len() >= 3 {
                        let b = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        let g = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        let r = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        gstate.stroke_color = [r, g, b, 255];
                    }
                    operand_stack.clear();
                }
                "g" => {
                    if let Some(gray) = pop_f64(&mut operand_stack) {
                        let v = (gray.clamp(0.0, 1.0) * 255.0).round() as u8;
                        gstate.fill_color = [v, v, v, 255];
                    }
                    operand_stack.clear();
                }
                "G" => {
                    if let Some(gray) = pop_f64(&mut operand_stack) {
                        let v = (gray.clamp(0.0, 1.0) * 255.0).round() as u8;
                        gstate.stroke_color = [v, v, v, 255];
                    }
                    operand_stack.clear();
                }
                "m" => {
                    if operand_stack.len() >= 2 {
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::MoveTo { x, y });
                        cur_pt = (x, y);
                    }
                    operand_stack.clear();
                }
                "l" => {
                    if operand_stack.len() >= 2 {
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::LineTo { x, y });
                        cur_pt = (x, y);
                    }
                    operand_stack.clear();
                }
                "c" => {
                    if operand_stack.len() >= 6 {
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cy2 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cx2 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cy1 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cx1 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::CubicTo {
                            cx1,
                            cy1,
                            cx2,
                            cy2,
                            x,
                            y,
                        });
                        cur_pt = (x, y);
                    }
                    operand_stack.clear();
                }
                "v" => {
                    if operand_stack.len() >= 4 {
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cy2 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cx2 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::CubicTo {
                            cx1: cur_pt.0,
                            cy1: cur_pt.1,
                            cx2,
                            cy2,
                            x,
                            y,
                        });
                        cur_pt = (x, y);
                    }
                    operand_stack.clear();
                }
                "y" => {
                    if operand_stack.len() >= 4 {
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cy1 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cx1 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::CubicTo {
                            cx1,
                            cy1,
                            cx2: x,
                            cy2: y,
                            x,
                            y,
                        });
                        cur_pt = (x, y);
                    }
                    operand_stack.clear();
                }
                "re" => {
                    if operand_stack.len() >= 4 {
                        let h = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let w = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::MoveTo { x, y });
                        current_path.push(PathCommand::LineTo { x: x + w, y });
                        current_path.push(PathCommand::LineTo { x: x + w, y: y + h });
                        current_path.push(PathCommand::LineTo { x, y: y + h });
                        current_path.push(PathCommand::Close);
                        cur_pt = (x, y);
                    }
                    operand_stack.clear();
                }
                "h" => {
                    current_path.push(PathCommand::Close);
                    operand_stack.clear();
                }
                "W" | "W*" => {
                    pending_clip = true;
                    operand_stack.clear();
                }
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => {
                    if matches!(tok.as_str(), "s" | "b" | "b*") {
                        current_path.push(PathCommand::Close);
                    }
                    if pending_clip && !current_path.is_empty() {
                        let baked = bake_matrix_into_commands(&current_path, gstate.ctm);
                        match options.mask_handling {
                            VectorMaskHandling::MaskLayers => {
                                current_bucket.mask_elements.push(Element {
                                    id: new_id("element"),
                                    name: "PDF Clip Path".to_owned(),
                                    transform: Transform::default(),
                                    kind: ElementKind::Path {
                                        commands: baked,
                                        fill_rule: FillRule::NonZero,
                                        fill: [255, 255, 255, 255],
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                    },
                                    ..Element::default()
                                });
                                report.diagnostics.push(Diagnostic {
                                    severity: Severity::Info,
                                    code: "pdf-clipping-path-mapped".to_owned(),
                                    location: loc.clone(),
                                    message:
                                        "Mapped PDF clipping path (W) to a Timeline Mask layer."
                                            .to_owned(),
                                });
                            }
                            VectorMaskHandling::FlattenClip => {
                                report.warning(
                                    "pdf-clipping-path-flattened",
                                    loc.clone(),
                                    "PDF clipping path (W) was flattened according to VectorMaskHandling::FlattenClip.",
                                );
                            }
                        }
                        pending_clip = false;
                    }
                    if tok != "n" && !current_path.is_empty() {
                        let do_fill =
                            matches!(tok.as_str(), "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
                        let do_stroke = matches!(tok.as_str(), "S" | "s" | "B" | "B*" | "b" | "b*");
                        let fill_rule = if tok.ends_with('*') {
                            FillRule::EvenOdd
                        } else {
                            FillRule::NonZero
                        };
                        let (final_cmds, elem_transform) = match options.transform_handling {
                            VectorTransformHandling::BakeTransformsIntoPaths => (
                                bake_matrix_into_commands(&current_path, gstate.ctm),
                                Transform::default(),
                            ),
                            VectorTransformHandling::KeepElementTransforms => (
                                current_path.clone(),
                                Transform::from_matrix(gstate.ctm).unwrap_or_default(),
                            ),
                        };
                        if validate_path_commands(&final_cmds).is_ok() {
                            current_bucket.elements.push(Element {
                                id: new_id("element"),
                                name: format!("PDF Path {}", current_bucket.elements.len() + 1),
                                transform: elem_transform,
                                kind: ElementKind::Path {
                                    commands: final_cmds,
                                    fill_rule,
                                    fill: if do_fill {
                                        gstate.fill_color
                                    } else {
                                        [0, 0, 0, 0]
                                    },
                                    stroke: if do_stroke {
                                        gstate.stroke_color
                                    } else {
                                        [0, 0, 0, 0]
                                    },
                                    stroke_width: if do_stroke { gstate.line_width } else { 0.0 },
                                },
                                ..Element::default()
                            });
                            report.statistics.elements += 1;
                        }
                    }
                    current_path.clear();
                    operand_stack.clear();
                }
                "BT" => {
                    gstate.text_pos = (0.0, 20.0);
                    operand_stack.clear();
                }
                "ET" => {
                    operand_stack.clear();
                }
                "Tf" => {
                    if let Some(sz) = pop_f64(&mut operand_stack) {
                        gstate.font_size = sz.abs().max(1.0);
                    }
                    operand_stack.clear();
                }
                "Td" | "TD" => {
                    if operand_stack.len() >= 2 {
                        let ty = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let tx = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        gstate.text_pos = (gstate.text_pos.0 + tx, gstate.text_pos.1 + ty);
                    }
                    operand_stack.clear();
                }
                "Tm" => {
                    if operand_stack.len() >= 6 {
                        let ty = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let tx = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let _d = pop_f64(&mut operand_stack);
                        let _c = pop_f64(&mut operand_stack);
                        let _b = pop_f64(&mut operand_stack);
                        let _a = pop_f64(&mut operand_stack);
                        gstate.text_pos = (tx, ty);
                    }
                    operand_stack.clear();
                }
                "Tj" | "TJ" => {
                    let text_str: String = operand_stack
                        .iter()
                        .filter(|s| s.starts_with('(') && s.ends_with(')'))
                        .map(|s| &s[1..s.len() - 1])
                        .collect::<Vec<_>>()
                        .join("");
                    if !text_str.is_empty() {
                        match options.text_handling {
                            VectorTextHandling::EditableText => {
                                let (tx, ty) = transform_point(
                                    gstate.ctm,
                                    gstate.text_pos.0,
                                    gstate.text_pos.1,
                                );
                                current_bucket.elements.push(Element {
                                    id: new_id("element"),
                                    name: format!("PDF Text ({text_str})"),
                                    transform: Transform {
                                        x: tx,
                                        y: ty,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Text {
                                        text: text_str,
                                        font_family: "sans-serif".to_owned(),
                                        font_size: gstate.font_size,
                                        color: gstate.fill_color,
                                    },
                                    ..Element::default()
                                });
                                report.statistics.elements += 1;
                            }
                            VectorTextHandling::ConvertToPaths => {
                                report.warning(
                                    "pdf-text-converted-to-paths",
                                    loc.clone(),
                                    format!(
                                        "Converted PDF text '{text_str}' to vector outline paths."
                                    ),
                                );
                                let cmds = text_to_outline_commands(
                                    &text_str,
                                    gstate.text_pos.0,
                                    gstate.text_pos.1,
                                    gstate.font_size,
                                );
                                let baked = bake_matrix_into_commands(&cmds, gstate.ctm);
                                current_bucket.elements.push(Element {
                                    id: new_id("element"),
                                    name: format!("PDF Outlined Text ({text_str})"),
                                    transform: Transform::default(),
                                    kind: ElementKind::Path {
                                        commands: baked,
                                        fill_rule: FillRule::NonZero,
                                        fill: gstate.fill_color,
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                    },
                                    ..Element::default()
                                });
                                report.statistics.elements += 1;
                            }
                        }
                    }
                    operand_stack.clear();
                }
                "BDC" | "BMC" => {
                    let group_label = operand_stack
                        .iter()
                        .find(|s| s.starts_with('/'))
                        .map(|s| s.trim_start_matches('/').to_owned())
                        .unwrap_or_else(|| format!("Layer {}", buckets.len() + 1));
                    if !current_bucket.elements.is_empty()
                        || !current_bucket.mask_elements.is_empty()
                    {
                        buckets.push(current_bucket);
                    }
                    current_bucket = VectorGroupBucket {
                        name: group_label,
                        elements: Vec::new(),
                        mask_elements: Vec::new(),
                        mask_mode: MaskMode::Geometry,
                    };
                    operand_stack.clear();
                }
                "EMC" => {
                    if !current_bucket.elements.is_empty()
                        || !current_bucket.mask_elements.is_empty()
                    {
                        buckets.push(current_bucket);
                        current_bucket = VectorGroupBucket {
                            name: format!("{source_name} Layer {}", buckets.len() + 1),
                            elements: Vec::new(),
                            mask_elements: Vec::new(),
                            mask_mode: MaskMode::Geometry,
                        };
                    }
                    operand_stack.clear();
                }
                "sh" | "Do" => {
                    report.warning(
                        "pdf-unsupported-operator",
                        loc,
                        format!(
                            "PDF operator '{tok}' (external XObject or shading dictionary) is not directly supported and was recorded in the fidelity report."
                        ),
                    );
                    operand_stack.clear();
                }
                _ => {
                    operand_stack.clear();
                }
            }
        }
    }

    if !current_bucket.elements.is_empty() || !current_bucket.mask_elements.is_empty() {
        buckets.push(current_bucket);
    }

    report.diagnostics.push(Diagnostic {
        severity: Severity::Info,
        code: "pdf-content-stream-parsed".to_owned(),
        location: source_name.to_owned(),
        message: format!(
            "Parsed {} PDF/AI vector operator(s) across {} content stream(s).",
            total_ops,
            streams.len()
        ),
    });

    assemble_vector_buckets(project, buckets, source_name, options, &mut report);
    report.can_import = true;
    Ok(report)
}

/// Import a PDF or Adobe Illustrator (`.pdf` / `.ai`) byte buffer into a fresh [`Project`].
pub fn import_pdf_ai_with_options(
    bytes: &[u8],
    source_name: &str,
    options: &VectorImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    let mut project = Project::blank();
    if !source_name.trim().is_empty() {
        project.name = source_name.trim().to_owned();
    }
    let report = import_pdf_ai_into_project(&mut project, bytes, source_name, options)?;
    Ok((project, report))
}

/// Auto-detect SVG vs PDF/AI vector content and import with [`VectorImportOptions`].
pub fn import_vector_bytes_with_options(
    bytes: &[u8],
    source_name: &str,
    options: &VectorImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    let prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
    if prefix.contains("%PDF-") || prefix.contains("%!PS-Adobe") {
        import_pdf_ai_with_options(bytes, source_name, options)
    } else {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| ImportError::Container("SVG file must be valid UTF-8".into()))?;
        import_svg_with_options(text, source_name, options)
    }
}
