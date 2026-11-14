#![forbid(unsafe_code)]
//! Figma REST API / plugin `.figma.json` scene-graph parser (`T-249`, `T-312`).

use std::collections::{BTreeMap, HashMap};

use base64::Engine as _;
use neoflash_core::{Color, FillRule, GradientKind, GradientStop, PathCommand, TextAlign};
use serde_json::Value;

use super::{
    BASE64, FigmaArcData, FigmaDoc, FigmaEffect, FigmaNode, FigmaPaint, FigmaTextData,
    FigmaTextStyleOverride, MAX_FIGMA_DECODE_BYTES, MAX_FIGMA_NODES,
    gradient_handles_from_paint_matrix,
};
use crate::{ImportError, vector_fidelity::parse_svg_path_data};

pub(super) fn looks_like_figma_json(bytes: &[u8]) -> bool {
    let trimmed = bytes.trim_ascii_start();
    if !trimmed.starts_with(b"{") {
        return false;
    }
    let head_len = trimmed.len().min(8192);
    let Ok(head) = std::str::from_utf8(&trimmed[..head_len]) else {
        return false;
    };
    if head.contains("\"format_version\"") && head.contains("\"symbols\"") {
        return false;
    }
    head.contains("\"schemaVersion\"")
        || (head.contains("\"document\"")
            && (head.contains("\"DOCUMENT\"") || head.contains("\"CANVAS\"")))
        || (head.contains("\"nodes\"")
            && (head.contains("\"DOCUMENT\"") || head.contains("\"CANVAS\"")))
        || (head.contains("\"type\"")
            && (head.contains("\"DOCUMENT\"") || head.contains("\"CANVAS\""))
            && head.contains("\"children\""))
}

fn json_color_to_rgba(c: &Value, extra_opacity: f64) -> Color {
    let r = (c
        .get("r")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let g = (c
        .get("g")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let b = (c
        .get("b")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let a = ((c.get("a").and_then(Value::as_f64).unwrap_or(1.0) * extra_opacity).clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    [r, g, b, a]
}

fn parse_json_matrix2x3(val: &Value) -> Option<[f64; 6]> {
    let rows = val.as_array()?;
    if rows.len() < 2 {
        return None;
    }
    let r0 = rows[0].as_array()?;
    let r1 = rows[1].as_array()?;
    if r0.len() < 3 || r1.len() < 3 {
        return None;
    }
    Some([
        r0[0].as_f64().unwrap_or(1.0),
        r0[1].as_f64().unwrap_or(0.0),
        r0[2].as_f64().unwrap_or(0.0),
        r1[0].as_f64().unwrap_or(0.0),
        r1[1].as_f64().unwrap_or(1.0),
        r1[2].as_f64().unwrap_or(0.0),
    ])
}

fn parse_json_paints(arr: &[Value], w: f64, h: f64) -> Vec<FigmaPaint> {
    let mut out = Vec::new();
    for p in arr {
        if !p.get("visible").and_then(Value::as_bool).unwrap_or(true) {
            continue;
        }
        let opacity = p
            .get("opacity")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        let ptype = p.get("type").and_then(Value::as_str).unwrap_or("SOLID");
        match ptype {
            "SOLID" => {
                if let Some(c) = p.get("color") {
                    out.push(FigmaPaint::Solid {
                        color: json_color_to_rgba(c, opacity),
                    });
                }
            }
            "GRADIENT_LINEAR" | "GRADIENT_RADIAL" | "GRADIENT_ANGULAR" | "GRADIENT_DIAMOND" => {
                let mut stops = Vec::new();
                let stop_arr = p
                    .get("gradientStops")
                    .or_else(|| p.get("stops"))
                    .and_then(Value::as_array);
                if let Some(sarr) = stop_arr {
                    for s in sarr {
                        let pos = s
                            .get("position")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0)
                            .clamp(0.0, 1.0);
                        if let Some(c) = s.get("color") {
                            stops.push(GradientStop {
                                ratio: (pos * 255.0).round() as u8,
                                color: json_color_to_rgba(c, opacity),
                            });
                        }
                    }
                }
                if stops.len() < 2 {
                    continue;
                }
                let handles = p.get("gradientHandlePositions").and_then(Value::as_array);
                let pt = |idx: usize, def_x: f64, def_y: f64| -> [f64; 2] {
                    handles
                        .and_then(|hs| hs.get(idx))
                        .map(|hp| {
                            [
                                hp.get("x").and_then(Value::as_f64).unwrap_or(def_x) * w,
                                hp.get("y").and_then(Value::as_f64).unwrap_or(def_y) * h,
                            ]
                        })
                        .unwrap_or([def_x * w, def_y * h])
                };
                let radial = ptype != "GRADIENT_LINEAR";
                let (start, end) = if handles.is_some_and(|hs| hs.len() >= 2) {
                    if radial {
                        (pt(0, 0.5, 0.5), pt(1, 1.0, 0.5))
                    } else {
                        (pt(0, 0.0, 0.5), pt(1, 1.0, 0.5))
                    }
                } else if let Some(mat) = p
                    .get("gradientTransform")
                    .or_else(|| p.get("transform"))
                    .and_then(parse_json_matrix2x3)
                {
                    gradient_handles_from_paint_matrix(mat, w, h, radial)
                } else if radial {
                    ([0.5 * w, 0.5 * h], [w, 0.5 * h])
                } else {
                    ([0.0, 0.5 * h], [w, 0.5 * h])
                };
                let kind = if radial {
                    GradientKind::Radial
                } else {
                    GradientKind::Linear
                };
                let approximated_from = match ptype {
                    "GRADIENT_ANGULAR" => Some("GRADIENT_ANGULAR"),
                    "GRADIENT_DIAMOND" => Some("GRADIENT_DIAMOND"),
                    _ => None,
                };
                out.push(FigmaPaint::Gradient {
                    kind,
                    stops,
                    start,
                    end,
                    approximated_from,
                });
            }
            "IMAGE" => {
                if let Some(iref) = p
                    .get("imageRef")
                    .or_else(|| p.get("imageHash"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    let scale_mode = p
                        .get("scaleMode")
                        .and_then(Value::as_str)
                        .unwrap_or("FILL")
                        .to_owned();
                    out.push(FigmaPaint::Image {
                        image_ref: iref.to_owned(),
                        scale_mode,
                        alpha: opacity,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

type DecodedVectorNetwork = (Vec<(Vec<PathCommand>, FillRule)>, Vec<Vec<PathCommand>>);

fn parse_json_vector_network(vn: &Value) -> DecodedVectorNetwork {
    let Some(verts_arr) = vn.get("vertices").and_then(Value::as_array) else {
        return (Vec::new(), Vec::new());
    };
    let Some(segs_arr) = vn.get("segments").and_then(Value::as_array) else {
        return (Vec::new(), Vec::new());
    };
    if verts_arr.is_empty() || segs_arr.is_empty() || verts_arr.len() > 100_000 {
        return (Vec::new(), Vec::new());
    }
    let verts: Vec<[f64; 2]> = verts_arr
        .iter()
        .map(|v| {
            [
                v.get("x").and_then(Value::as_f64).unwrap_or(0.0),
                v.get("y").and_then(Value::as_f64).unwrap_or(0.0),
            ]
        })
        .collect();
    #[derive(Clone, Copy)]
    struct JSeg {
        s: usize,
        dx1: f64,
        dy1: f64,
        e: usize,
        dx2: f64,
        dy2: f64,
    }
    let mut segs = Vec::with_capacity(segs_arr.len());
    for sv in segs_arr {
        let s = sv.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
        let e = sv.get("end").and_then(Value::as_u64).unwrap_or(0) as usize;
        if s >= verts.len() || e >= verts.len() {
            return (Vec::new(), Vec::new());
        }
        let (dx1, dy1) = sv
            .get("tangentStart")
            .map(|t| {
                (
                    t.get("x").and_then(Value::as_f64).unwrap_or(0.0),
                    t.get("y").and_then(Value::as_f64).unwrap_or(0.0),
                )
            })
            .unwrap_or((0.0, 0.0));
        let (dx2, dy2) = sv
            .get("tangentEnd")
            .map(|t| {
                (
                    t.get("x").and_then(Value::as_f64).unwrap_or(0.0),
                    t.get("y").and_then(Value::as_f64).unwrap_or(0.0),
                )
            })
            .unwrap_or((0.0, 0.0));
        segs.push(JSeg {
            s,
            dx1,
            dy1,
            e,
            dx2,
            dy2,
        });
    }

    let emit = |cmds: &mut Vec<PathCommand>, seg: JSeg, fwd: bool| {
        let (v0, tx0, ty0, v1, tx1, ty1) = if fwd {
            (
                verts[seg.s],
                seg.dx1,
                seg.dy1,
                verts[seg.e],
                seg.dx2,
                seg.dy2,
            )
        } else {
            (
                verts[seg.e],
                seg.dx2,
                seg.dy2,
                verts[seg.s],
                seg.dx1,
                seg.dy1,
            )
        };
        if tx0.abs() <= 1e-7 && ty0.abs() <= 1e-7 && tx1.abs() <= 1e-7 && ty1.abs() <= 1e-7 {
            cmds.push(PathCommand::LineTo { x: v1[0], y: v1[1] });
        } else {
            cmds.push(PathCommand::CubicTo {
                cx1: v0[0] + tx0,
                cy1: v0[1] + ty0,
                cx2: v1[0] + tx1,
                cy2: v1[1] + ty1,
                x: v1[0],
                y: v1[1],
            });
        }
    };

    let mut fill_paths = Vec::new();
    if let Some(regs) = vn.get("regions").and_then(Value::as_array) {
        for r in regs {
            let Some(loops) = r.get("loops").and_then(Value::as_array) else {
                continue;
            };
            let rule = match r.get("windingRule").and_then(Value::as_str) {
                Some("EVENODD" | "ODD") => FillRule::EvenOdd,
                _ => FillRule::NonZero,
            };
            let mut rcmds = Vec::new();
            for lp in loops {
                let Some(id_arr) = lp.as_array() else {
                    continue;
                };
                let lsegs: Vec<JSeg> = id_arr
                    .iter()
                    .filter_map(|iv| iv.as_u64().and_then(|u| segs.get(u as usize).copied()))
                    .collect();
                if lsegs.is_empty() {
                    continue;
                }
                let first_fwd = if lsegs.len() == 1 {
                    true
                } else {
                    lsegs[0].e == lsegs[1].s || lsegs[0].e == lsegs[1].e
                };
                let mut cur = if first_fwd { lsegs[0].s } else { lsegs[0].e };
                rcmds.push(PathCommand::MoveTo {
                    x: verts[cur][0],
                    y: verts[cur][1],
                });
                for (k, seg) in lsegs.into_iter().enumerate() {
                    let fwd = if k == 0 {
                        first_fwd
                    } else if seg.s == cur {
                        true
                    } else if seg.e == cur {
                        false
                    } else {
                        rcmds.push(PathCommand::MoveTo {
                            x: verts[seg.s][0],
                            y: verts[seg.s][1],
                        });
                        true
                    };
                    emit(&mut rcmds, seg, fwd);
                    cur = if fwd { seg.e } else { seg.s };
                }
                rcmds.push(PathCommand::Close);
            }
            if rcmds.len() >= 2 {
                fill_paths.push((rcmds, rule));
            }
        }
    }

    let mut scmds = Vec::new();
    let mut cur: Option<usize> = None;
    let mut start: Option<usize> = None;
    for seg in segs {
        let fwd = if cur == Some(seg.s) {
            true
        } else if cur == Some(seg.e) {
            false
        } else {
            scmds.push(PathCommand::MoveTo {
                x: verts[seg.s][0],
                y: verts[seg.s][1],
            });
            start = Some(seg.s);
            true
        };
        emit(&mut scmds, seg, fwd);
        let nxt = if fwd { seg.e } else { seg.s };
        cur = Some(nxt);
        if start == Some(nxt) {
            scmds.push(PathCommand::Close);
            cur = None;
            start = None;
        }
    }
    let stroke_paths = if scmds.len() >= 2 {
        vec![scmds]
    } else {
        Vec::new()
    };
    (fill_paths, stroke_paths)
}

fn parse_json_text_style_override(st: &Value, base_fs: f64) -> FigmaTextStyleOverride {
    let mut out = FigmaTextStyleOverride::default();
    if let Some(fs) = st
        .get("fontSize")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v > 0.0)
    {
        out.font_size = Some(fs);
    }
    let eff_fs = out.font_size.unwrap_or(base_fs);
    if let Some(fam) = st
        .get("fontFamily")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        out.font_family = Some(fam.to_owned());
    }
    if st.get("fontWeight").is_some() || st.get("fontStyle").is_some() {
        let fw = st
            .get("fontWeight")
            .and_then(Value::as_f64)
            .unwrap_or(400.0);
        let fst = st
            .get("fontStyle")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        if st.get("fontWeight").is_some() || fst.contains("bold") {
            out.bold = Some(fw >= 600.0 || fst.contains("bold"));
        }
        if st.get("italic").is_some() || st.get("fontStyle").is_some() {
            out.italic = Some(
                st.get("italic").and_then(Value::as_bool).unwrap_or(false)
                    || fst.contains("italic")
                    || fst.contains("oblique"),
            );
        }
    } else if let Some(it) = st.get("italic").and_then(Value::as_bool) {
        out.italic = Some(it);
    }
    if let Some(dec) = st.get("textDecoration").and_then(Value::as_str) {
        out.underline = Some(dec == "UNDERLINE");
        out.strikethrough = Some(dec == "STRIKETHROUGH");
    }
    if let Some(fills) = st.get("fills").and_then(Value::as_array) {
        for p in parse_json_paints(fills, 100.0, 20.0) {
            if let FigmaPaint::Solid { color } = p {
                out.fill_color = Some(color);
                break;
            }
        }
    }
    if let Some(ls) = st.get("letterSpacing").and_then(Value::as_f64) {
        out.letter_spacing = Some(ls);
    }
    if let Some(lh) = st
        .get("lineHeightPx")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v > 0.0)
    {
        out.line_height = Some(lh);
    } else if let Some(pct) = st
        .get("lineHeightPercentFontSize")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v > 0.0)
    {
        out.line_height = Some(eff_fs * pct / 100.0);
    }
    out
}

fn parse_json_node(
    v: &Value,
    parent_bbox: Option<(f64, f64)>,
    node_budget: &mut usize,
    depth: usize,
) -> Result<FigmaNode, String> {
    if depth > 128 {
        return Err("Figma JSON node hierarchy exceeds maximum depth of 128".into());
    }
    *node_budget = node_budget
        .checked_sub(1)
        .ok_or_else(|| format!("Figma JSON exceeds maximum node count ({MAX_FIGMA_NODES})"))?;

    let id = v
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("0:0")
        .to_owned();
    let node_type = v
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("FRAME")
        .to_owned();
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&node_type)
        .to_owned();
    let visible = v.get("visible").and_then(Value::as_bool).unwrap_or(true);
    let locked = v.get("locked").and_then(Value::as_bool).unwrap_or(false);
    let internal_only = v
        .get("internalOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || node_type == "INTERNAL_ONLY_CANVAS"
        || (node_type == "CANVAS" && name == "Internal Only Canvas");
    let is_state_group = node_type == "COMPONENT_SET"
        || v.get("isStateGroup")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    let opacity = v
        .get("opacity")
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let blend_mode = v
        .get("blendMode")
        .and_then(Value::as_str)
        .unwrap_or("PASS_THROUGH")
        .to_owned();

    let bbox = v.get("absoluteBoundingBox").and_then(Value::as_object);
    let size_obj = v.get("size").and_then(Value::as_object);

    let width = size_obj
        .and_then(|s| s.get("x"))
        .and_then(Value::as_f64)
        .or_else(|| bbox.and_then(|b| b.get("width")).and_then(Value::as_f64))
        .unwrap_or(100.0)
        .max(0.0);
    let height = size_obj
        .and_then(|s| s.get("y"))
        .and_then(Value::as_f64)
        .or_else(|| bbox.and_then(|b| b.get("height")).and_then(Value::as_f64))
        .unwrap_or(100.0)
        .max(0.0);

    let rotation_deg = v
        .get("rotation")
        .and_then(Value::as_f64)
        .map(|r| -r.to_degrees())
        .unwrap_or(0.0);

    let mut x = 0.0;
    let mut y = 0.0;
    let mut relative_transform = None;

    if let Some(rt) = v.get("relativeTransform").and_then(Value::as_array)
        && rt.len() >= 2
        && let (Some(r0), Some(r1)) = (rt[0].as_array(), rt[1].as_array())
        && r0.len() >= 3
        && r1.len() >= 3
    {
        let m00 = r0[0].as_f64().unwrap_or(1.0);
        let m01 = r0[1].as_f64().unwrap_or(0.0);
        let m02 = r0[2].as_f64().unwrap_or(0.0);
        let m10 = r1[0].as_f64().unwrap_or(0.0);
        let m11 = r1[1].as_f64().unwrap_or(1.0);
        let m12 = r1[2].as_f64().unwrap_or(0.0);
        x = m02;
        y = m12;
        relative_transform = Some([m00, m10, m01, m11, m02, m12]);
    } else if let Some(b) = bbox {
        let abs_x = b.get("x").and_then(Value::as_f64).unwrap_or(0.0);
        let abs_y = b.get("y").and_then(Value::as_f64).unwrap_or(0.0);
        if let Some((px, py)) = parent_bbox {
            x = abs_x - px;
            y = abs_y - py;
        } else {
            x = abs_x;
            y = abs_y;
        }
    }

    let uniform_r = v
        .get("cornerRadius")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .max(0.0);
    let corner_radii = if let Some(arr) = v.get("rectangleCornerRadii").and_then(Value::as_array)
        && arr.len() >= 4
    {
        [
            arr[0].as_f64().unwrap_or(0.0).max(0.0),
            arr[1].as_f64().unwrap_or(0.0).max(0.0),
            arr[2].as_f64().unwrap_or(0.0).max(0.0),
            arr[3].as_f64().unwrap_or(0.0).max(0.0),
        ]
    } else {
        [uniform_r; 4]
    };

    let arc_data = v
        .get("arcData")
        .and_then(Value::as_object)
        .map(|ao| FigmaArcData {
            start_angle_rad: ao
                .get("startingAngle")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            end_angle_rad: ao
                .get("endingAngle")
                .and_then(Value::as_f64)
                .unwrap_or(std::f64::consts::TAU),
            inner_radius_ratio: ao.get("innerRadius").and_then(Value::as_f64).unwrap_or(0.0),
        });

    let point_count = v
        .get("count")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());
    let star_inner_radius = v.get("starInnerRadius").and_then(Value::as_f64);

    let mut fills = v
        .get("fills")
        .and_then(Value::as_array)
        .map(|a| parse_json_paints(a, width, height))
        .unwrap_or_default();
    if fills.is_empty()
        && let Some(bg_arr) = v.get("background").and_then(Value::as_array)
    {
        fills = parse_json_paints(bg_arr, width, height);
    }

    let background_color = v
        .get("backgroundColor")
        .map(|bgc| json_color_to_rgba(bgc, 1.0));

    let strokes = v
        .get("strokes")
        .and_then(Value::as_array)
        .map(|a| parse_json_paints(a, width, height))
        .unwrap_or_default();
    let stroke_weight = v
        .get("strokeWeight")
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
        .max(0.0);
    let border_weights = v
        .get("individualStrokeWeights")
        .and_then(Value::as_object)
        .map(|iw| {
            [
                iw.get("top")
                    .and_then(Value::as_f64)
                    .unwrap_or(stroke_weight)
                    .max(0.0),
                iw.get("right")
                    .and_then(Value::as_f64)
                    .unwrap_or(stroke_weight)
                    .max(0.0),
                iw.get("bottom")
                    .and_then(Value::as_f64)
                    .unwrap_or(stroke_weight)
                    .max(0.0),
                iw.get("left")
                    .and_then(Value::as_f64)
                    .unwrap_or(stroke_weight)
                    .max(0.0),
            ]
        });

    let mut fill_paths = Vec::new();
    if let Some(arr) = v.get("fillGeometry").and_then(Value::as_array) {
        for g in arr {
            let rule = match g.get("windingRule").and_then(Value::as_str) {
                Some("EVENODD" | "ODD") => FillRule::EvenOdd,
                _ => FillRule::NonZero,
            };
            if let Some(path_str) = g.get("path").and_then(Value::as_str) {
                let cmds = parse_svg_path_data(path_str);
                if !cmds.is_empty() {
                    fill_paths.push((cmds, rule));
                }
            }
        }
    }
    let mut stroke_paths = Vec::new();
    if let Some(arr) = v.get("strokeGeometry").and_then(Value::as_array) {
        for g in arr {
            if let Some(path_str) = g.get("path").and_then(Value::as_str) {
                let cmds = parse_svg_path_data(path_str);
                if !cmds.is_empty() {
                    stroke_paths.push(cmds);
                }
            }
        }
    }

    if (fill_paths.is_empty() || (stroke_paths.is_empty() && !strokes.is_empty()))
        && let Some(vn) = v.get("vectorNetwork")
    {
        let (vn_fills, vn_strokes) = parse_json_vector_network(vn);
        if fill_paths.is_empty() {
            if !vn_fills.is_empty() {
                fill_paths = vn_fills;
            } else if !fills.is_empty() && !vn_strokes.is_empty() {
                fill_paths = vn_strokes
                    .iter()
                    .cloned()
                    .map(|c| (c, FillRule::NonZero))
                    .collect();
            }
        }
        if stroke_paths.is_empty() && !vn_strokes.is_empty() {
            stroke_paths = vn_strokes;
        }
    }

    let mut effects = Vec::new();
    if let Some(earr) = v.get("effects").and_then(Value::as_array) {
        for e in earr {
            if !e.get("visible").and_then(Value::as_bool).unwrap_or(true) {
                continue;
            }
            let etype = e.get("type").and_then(Value::as_str).unwrap_or("");
            let radius = e
                .get("radius")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .max(0.0);
            match etype {
                "DROP_SHADOW" | "INNER_SHADOW" => {
                    let color = e
                        .get("color")
                        .map(|c| json_color_to_rgba(c, 1.0))
                        .unwrap_or([0, 0, 0, 64]);
                    let ox = e
                        .get("offset")
                        .and_then(|o| o.get("x"))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    let oy = e
                        .get("offset")
                        .and_then(|o| o.get("y"))
                        .and_then(Value::as_f64)
                        .unwrap_or(2.0);
                    effects.push(FigmaEffect::DropShadow {
                        color,
                        offset_x: ox,
                        offset_y: oy,
                        radius,
                        inner: etype == "INNER_SHADOW",
                    });
                }
                "LAYER_BLUR" => effects.push(FigmaEffect::LayerBlur { radius }),
                "BACKGROUND_BLUR" => effects.push(FigmaEffect::BackgroundBlur { radius }),
                _ => {}
            }
        }
    }

    let text = if node_type == "TEXT" {
        let characters = v
            .get("characters")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let st = v.get("style");
        let font_family = st
            .and_then(|s| s.get("fontFamily"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("Inter")
            .to_owned();
        let font_size = st
            .and_then(|s| s.get("fontSize"))
            .and_then(Value::as_f64)
            .unwrap_or(16.0)
            .max(1.0);
        let font_weight = st
            .and_then(|s| s.get("fontWeight"))
            .and_then(Value::as_f64)
            .unwrap_or(400.0);
        let font_style_str = st
            .and_then(|s| s.get("fontStyle"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let bold = font_weight >= 600.0 || font_style_str.contains("bold");
        let italic = st
            .and_then(|s| s.get("italic"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || font_style_str.contains("italic")
            || font_style_str.contains("oblique");
        let text_dec = st
            .and_then(|s| s.get("textDecoration"))
            .and_then(Value::as_str)
            .unwrap_or("NONE");
        let underline = text_dec == "UNDERLINE";
        let strikethrough = text_dec == "STRIKETHROUGH";
        let (align, justify) = match st
            .and_then(|s| s.get("textAlignHorizontal"))
            .and_then(Value::as_str)
        {
            Some("CENTER") => (TextAlign::Center, false),
            Some("RIGHT") => (TextAlign::Right, false),
            Some("JUSTIFIED") => (TextAlign::Left, true),
            _ => (TextAlign::Left, false),
        };
        let align_v = match st
            .and_then(|s| s.get("textAlignVertical"))
            .and_then(Value::as_str)
        {
            Some("CENTER") => "middle",
            Some("BOTTOM") => "bottom",
            _ => "top",
        }
        .to_owned();
        let letter_spacing = st
            .and_then(|s| s.get("letterSpacing"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let line_height = st
            .and_then(|s| s.get("lineHeightPx"))
            .and_then(Value::as_f64)
            .filter(|lh| lh.is_finite() && *lh > 0.0);
        let auto_resize = st
            .and_then(|s| s.get("textAutoResize"))
            .and_then(Value::as_str)
            .unwrap_or("NONE")
            .to_owned();

        let char_ids: Vec<u64> = v
            .get("characterStyleOverrides")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().map(|x| x.as_u64().unwrap_or(0)).collect())
            .unwrap_or_default();
        let mut override_map = HashMap::new();
        if !char_ids.is_empty()
            && let Some(sot_obj) = v.get("styleOverrideTable").and_then(Value::as_object)
        {
            for (k, sv) in sot_obj {
                if let Ok(sid) = k.parse::<u64>()
                    && sid != 0
                {
                    override_map.insert(sid, parse_json_text_style_override(sv, font_size));
                }
            }
        }
        let style_runs = super::kiwi::build_text_style_runs(&characters, &char_ids, &override_map);

        Some(FigmaTextData {
            characters,
            font_family,
            font_size,
            bold,
            italic,
            underline,
            strikethrough,
            align,
            justify,
            align_v,
            letter_spacing,
            line_height,
            auto_resize,
            style_runs,
        })
    } else {
        None
    };

    let clips_content = v
        .get("clipsContent")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_mask = v.get("isMask").and_then(Value::as_bool).unwrap_or(false);
    let mask_type = v
        .get("maskType")
        .and_then(Value::as_str)
        .unwrap_or("ALPHA")
        .to_owned();
    let component_id = v
        .get("componentId")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let this_abs_origin = if matches!(node_type.as_str(), "DOCUMENT" | "CANVAS") {
        None
    } else {
        bbox.map(|b| {
            (
                b.get("x").and_then(Value::as_f64).unwrap_or(0.0),
                b.get("y").and_then(Value::as_f64).unwrap_or(0.0),
            )
        })
        .or(parent_bbox)
    };

    let mut children = Vec::new();
    if let Some(carr) = v.get("children").and_then(Value::as_array) {
        children.reserve(carr.len());
        for c in carr {
            children.push(parse_json_node(c, this_abs_origin, node_budget, depth + 1)?);
        }
    }

    Ok(FigmaNode {
        id,
        name,
        node_type,
        visible,
        locked,
        internal_only,
        is_state_group,
        opacity,
        blend_mode,
        is_mask,
        mask_type,
        clips_content,
        x,
        y,
        width,
        height,
        rotation_deg,
        relative_transform,
        corner_radii,
        arc_data,
        point_count,
        star_inner_radius,
        fills,
        strokes,
        stroke_weight,
        border_weights,
        fill_paths,
        stroke_paths,
        text,
        effects,
        component_id,
        symbol_overrides: Vec::new(),
        background_color,
        children,
    })
}

pub(super) fn parse_figma_json_doc(
    json_str: &str,
    default_name: &str,
) -> Result<FigmaDoc, ImportError> {
    if json_str.len() > MAX_FIGMA_DECODE_BYTES {
        return Err(ImportError::Container(
            "Figma JSON payload exceeds 256 MiB limit".into(),
        ));
    }
    let root_val: Value = serde_json::from_str(json_str)
        .map_err(|e| ImportError::Container(format!("Invalid JSON in Figma file: {e}")))?;

    let doc_name = root_val
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(default_name)
        .to_owned();

    let mut inline_images = BTreeMap::new();
    if let Some(imgs_obj) = root_val.get("images").and_then(Value::as_object) {
        for (k, v) in imgs_obj {
            if let Some(s) = v.as_str() {
                let payload = s.split_once(";base64,").map(|(_, b)| b).unwrap_or(s).trim();
                if let Ok(decoded) = BASE64.decode(payload)
                    && !decoded.is_empty()
                {
                    inline_images.insert(k.clone(), decoded);
                }
            }
        }
    }

    let mut budget = MAX_FIGMA_NODES;
    let mut pages = Vec::new();

    if let Some(doc_val) = root_val.get("document") {
        let doc_node =
            parse_json_node(doc_val, None, &mut budget, 0).map_err(ImportError::Container)?;
        if doc_node.node_type == "DOCUMENT" {
            pages = doc_node.children;
        } else {
            pages.push(doc_node);
        }
    } else if let Some(nodes_obj) = root_val.get("nodes").and_then(Value::as_object) {
        for (_id, entry) in nodes_obj {
            if let Some(d) = entry.get("document") {
                let parsed =
                    parse_json_node(d, None, &mut budget, 0).map_err(ImportError::Container)?;
                if parsed.node_type == "CANVAS" {
                    pages.push(parsed);
                } else {
                    pages.push(FigmaNode {
                        id: "canvas-0".into(),
                        name: "Page 1".into(),
                        node_type: "CANVAS".into(),
                        width: parsed.width,
                        height: parsed.height,
                        children: vec![parsed],
                        ..FigmaNode::default()
                    });
                }
            }
        }
    } else if matches!(
        root_val.get("type").and_then(Value::as_str),
        Some("DOCUMENT" | "CANVAS")
    ) {
        let root_node =
            parse_json_node(&root_val, None, &mut budget, 0).map_err(ImportError::Container)?;
        if root_node.node_type == "DOCUMENT" {
            pages = root_node.children;
        } else {
            pages.push(root_node);
        }
    } else {
        return Err(ImportError::Container(
            "JSON file does not match Figma REST API (`document` / `nodes`) schema".into(),
        ));
    }

    if pages.is_empty() {
        return Err(ImportError::Blocked(
            "Figma document contains no importable pages or frames.".into(),
        ));
    }

    Ok(FigmaDoc {
        name: doc_name,
        version_label: "Figma REST JSON".into(),
        pages,
        inline_images,
    })
}
