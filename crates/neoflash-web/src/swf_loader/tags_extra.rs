//! Embedded font glyph shapes (`DefineFont`/`2`/`3`), `DefineButton`/`DefineButton2`,
//! `DefineMorphShape`/`DefineMorphShape2`, and `PlaceObject2`/`PlaceObject3` `CLIPACTIONS`
//! decoding for the bounded WASM SWF loader.

use super::timeline::{ActivePlacement, decode_swf_blend_mode, decode_swf_filter_list};
use super::vm::compile_avm1_handler_fn_expr;
use super::{
    BitReader, DirectedSeg, MAX_SWF_KEYFRAMES, SwfDecodeCtx, SwfFontRecord,
    chain_segments_to_commands, make_default_element,
};
use neoflash_core::{
    BlendMode, Color, ColorTransform, Element, ElementKind, FillRule, FilterSpec, InstancePlayback,
    Keyframe, Layer, LayerProperties, ObjectEvent, ObjectScript, PathCommand, Script,
    ScriptLanguage, Symbol, SymbolKind, Timeline, Transform,
};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(super) struct SwfMorphFill {
    start_color: Color,
    end_color: Color,
}

#[derive(Clone)]
pub(super) struct SwfMorphLine {
    start_width: f64,
    end_width: f64,
    start_color: Color,
    end_color: Color,
}

#[derive(Clone)]
pub(crate) struct SwfMorphRecord {
    fills: Vec<SwfMorphFill>,
    lines: Vec<SwfMorphLine>,
    start_fill_groups: BTreeMap<usize, Vec<(usize, DirectedSeg, bool)>>,
    start_line_groups: BTreeMap<usize, Vec<(usize, DirectedSeg)>>,
    end_edges: Vec<DirectedSeg>,
}

pub(super) fn parse_glyph_shape_records(r: &mut BitReader<'_>) -> Vec<DirectedSeg> {
    r.align();
    let Ok(num_fill_bits_raw) = r.read_ubits(4) else {
        return Vec::new();
    };
    let Ok(num_line_bits_raw) = r.read_ubits(4) else {
        return Vec::new();
    };
    let num_fill_bits = num_fill_bits_raw as usize;
    let num_line_bits = num_line_bits_raw as usize;
    let mut cur_x = 0i32;
    let mut cur_y = 0i32;
    let mut fs0 = 0usize;
    let mut fs1 = 0usize;
    let mut segs = Vec::new();

    for _ in 0..16_384 {
        let Ok(type_flag) = r.read_ubits(1) else {
            break;
        };
        if type_flag == 0 {
            let Ok(flags) = r.read_ubits(5) else {
                break;
            };
            if flags == 0 {
                break;
            }
            if (flags & 0x01) != 0 {
                let Ok(mb) = r.read_ubits(5) else { break };
                let Ok(mx) = r.read_sbits(mb as usize) else {
                    break;
                };
                let Ok(my) = r.read_sbits(mb as usize) else {
                    break;
                };
                cur_x = mx;
                cur_y = my;
            }
            if (flags & 0x02) != 0 {
                let Ok(v) = r.read_ubits(num_fill_bits) else {
                    break;
                };
                fs0 = v as usize;
            }
            if (flags & 0x04) != 0 {
                let Ok(v) = r.read_ubits(num_fill_bits) else {
                    break;
                };
                fs1 = v as usize;
            }
            if (flags & 0x08) != 0 && r.read_ubits(num_line_bits).is_err() {
                break;
            }
        } else {
            let Ok(straight) = r.read_ubits(1) else {
                break;
            };
            let Ok(nb_raw) = r.read_ubits(4) else {
                break;
            };
            let nbits = (nb_raw as usize) + 2;
            let seg = if straight == 1 {
                let Ok(general) = r.read_ubits(1) else {
                    break;
                };
                let (dx, dy) = if general == 1 {
                    let (Ok(dx), Ok(dy)) = (r.read_sbits(nbits), r.read_sbits(nbits)) else {
                        break;
                    };
                    (dx, dy)
                } else {
                    let Ok(vert) = r.read_ubits(1) else {
                        break;
                    };
                    if vert == 1 {
                        let Ok(dy) = r.read_sbits(nbits) else {
                            break;
                        };
                        (0, dy)
                    } else {
                        let Ok(dx) = r.read_sbits(nbits) else {
                            break;
                        };
                        (dx, 0)
                    }
                };
                let nx = cur_x + dx;
                let ny = cur_y + dy;
                let s = DirectedSeg::Line {
                    x0: cur_x,
                    y0: cur_y,
                    x1: nx,
                    y1: ny,
                };
                cur_x = nx;
                cur_y = ny;
                s
            } else {
                let (Ok(cdx), Ok(cdy), Ok(adx), Ok(ady)) = (
                    r.read_sbits(nbits),
                    r.read_sbits(nbits),
                    r.read_sbits(nbits),
                    r.read_sbits(nbits),
                ) else {
                    break;
                };
                let cx = cur_x + cdx;
                let cy = cur_y + cdy;
                let nx = cx + adx;
                let ny = cy + ady;
                let s = DirectedSeg::Quad {
                    x0: cur_x,
                    y0: cur_y,
                    cx,
                    cy,
                    x1: nx,
                    y1: ny,
                };
                cur_x = nx;
                cur_y = ny;
                s
            };
            if fs1 > 0 || (fs0 == 0 && fs1 == 0) {
                segs.push(seg);
            }
            if fs0 > 0 {
                segs.push(seg.reversed());
            }
        }
    }
    segs
}

pub(super) fn build_glyph_path_commands(
    font: &SwfFontRecord,
    glyph_idx: usize,
    x_px: f64,
    y_px: f64,
    height_px: f64,
) -> Option<Vec<PathCommand>> {
    let segs = font.glyph_shapes.get(glyph_idx)?;
    if segs.is_empty() {
        return None;
    }
    let raw_cmds = chain_segments_to_commands(segs, true);
    if raw_cmds.is_empty() {
        return None;
    }
    // `chain_segments_to_commands` divides twip coords by 20.0.
    // A glyph coordinate `u` in EM space (`1024` for DefineFont/2, `20480` for DefineFont3)
    // maps to `u * (height_px / em_units)` pixels; since `raw_cmd` already divided `u` by `20.0`,
    // multiply by `(20.0 * height_px) / font.em_units`!
    let scale = (20.0 * height_px) / font.em_units.max(1.0);
    let mut out = Vec::with_capacity(raw_cmds.len());
    for cmd in raw_cmds {
        match cmd {
            PathCommand::MoveTo { x, y } => out.push(PathCommand::MoveTo {
                x: x_px + x * scale,
                y: y_px + y * scale,
            }),
            PathCommand::LineTo { x, y } => out.push(PathCommand::LineTo {
                x: x_px + x * scale,
                y: y_px + y * scale,
            }),
            PathCommand::QuadraticTo { cx, cy, x, y } => out.push(PathCommand::QuadraticTo {
                cx: x_px + cx * scale,
                cy: y_px + cy * scale,
                x: x_px + x * scale,
                y: y_px + y * scale,
            }),
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => out.push(PathCommand::CubicTo {
                cx1: x_px + cx1 * scale,
                cy1: y_px + cy1 * scale,
                cx2: x_px + cx2 * scale,
                cy2: y_px + cy2 * scale,
                x: x_px + x * scale,
                y: y_px + y * scale,
            }),
            PathCommand::Close => out.push(PathCommand::Close),
        }
    }
    Some(out)
}

fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    (f64::from(a) + (f64::from(b) - f64::from(a)) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn lerp_color(a: Color, b: Color, t: f64) -> Color {
    [
        lerp_u8(a[0], b[0], t),
        lerp_u8(a[1], b[1], t),
        lerp_u8(a[2], b[2], t),
        lerp_u8(a[3], b[3], t),
    ]
}

fn lerp_i32(a: i32, b: i32, t: f64) -> i32 {
    (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as i32
}

pub(super) fn decode_morph_shape_tag(
    ctx: &mut SwfDecodeCtx,
    data: &[u8],
    is_v2: bool,
) -> Result<(), String> {
    let mut r = BitReader::new(data, 0);
    let char_id = r.read_u16()?;
    let _start_bounds = r.read_rect_twips()?;
    let _end_bounds = r.read_rect_twips()?;
    if is_v2 {
        let _start_edge_bounds = r.read_rect_twips()?;
        let _end_edge_bounds = r.read_rect_twips()?;
        let _flags = r.read_u8()?;
    }
    let offset = r.read_u32()? as usize;
    let end_edges_byte_pos = r.byte_pos().saturating_add(offset);

    let mut fill_count = usize::from(r.read_u8()?);
    if fill_count == 0xFF {
        fill_count = usize::from(r.read_u16()?);
    }
    let mut fills = Vec::with_capacity(fill_count.min(256));
    for _ in 0..fill_count.min(1024) {
        let kind = r.read_u8()?;
        match kind {
            0x00 => {
                let sc = r.read_rgba()?;
                let ec = r.read_rgba()?;
                fills.push(SwfMorphFill {
                    start_color: sc,
                    end_color: ec,
                });
            }
            0x10 | 0x12 | 0x13 => {
                let _sm = r.read_matrix()?;
                let _em = r.read_matrix()?;
                let num_grads = usize::from(r.read_u8()? & 0x0F);
                let mut first_sc = [0, 0, 0, 255];
                let mut first_ec = [0, 0, 0, 255];
                for gi in 0..num_grads {
                    let _sr = r.read_u8()?;
                    let sc = r.read_rgba()?;
                    let _er = r.read_u8()?;
                    let ec = r.read_rgba()?;
                    if gi == 0 {
                        first_sc = sc;
                        first_ec = ec;
                    }
                }
                if kind == 0x13 {
                    let _sf = r.read_i16()?;
                    let _ef = r.read_i16()?;
                }
                fills.push(SwfMorphFill {
                    start_color: first_sc,
                    end_color: first_ec,
                });
            }
            0x40..=0x43 => {
                let _bmp_id = r.read_u16()?;
                let _sm = r.read_matrix()?;
                let _em = r.read_matrix()?;
                fills.push(SwfMorphFill {
                    start_color: [128, 128, 128, 255],
                    end_color: [128, 128, 128, 255],
                });
            }
            _ => {
                fills.push(SwfMorphFill {
                    start_color: [0, 0, 0, 0],
                    end_color: [0, 0, 0, 0],
                });
            }
        }
    }

    let mut line_count = usize::from(r.read_u8()?);
    if line_count == 0xFF {
        line_count = usize::from(r.read_u16()?);
    }
    let mut lines = Vec::with_capacity(line_count.min(256));
    for _ in 0..line_count.min(1024) {
        let sw = f64::from(r.read_u16()?) / 20.0;
        let ew = f64::from(r.read_u16()?) / 20.0;
        if !is_v2 {
            let sc = r.read_rgba()?;
            let ec = r.read_rgba()?;
            lines.push(SwfMorphLine {
                start_width: sw,
                end_width: ew,
                start_color: sc,
                end_color: ec,
            });
        } else {
            let _start_cap = r.read_ubits(2)?;
            let join_style = r.read_ubits(2)?;
            let has_fill = r.read_ubits(1)? == 1;
            let _rest = r.read_ubits(11)?;
            if join_style == 2 {
                let _miter = r.read_u16()?;
            }
            if has_fill {
                let kind = r.read_u8()?;
                let (sc, ec) = if kind == 0x00 {
                    (r.read_rgba()?, r.read_rgba()?)
                } else {
                    ([0, 0, 0, 255], [0, 0, 0, 255])
                };
                lines.push(SwfMorphLine {
                    start_width: sw,
                    end_width: ew,
                    start_color: sc,
                    end_color: ec,
                });
            } else {
                let sc = r.read_rgba()?;
                let ec = r.read_rgba()?;
                lines.push(SwfMorphLine {
                    start_width: sw,
                    end_width: ew,
                    start_color: sc,
                    end_color: ec,
                });
            }
        }
    }

    let (start_fill_groups, start_line_groups) = parse_morph_start_edges(&mut r);
    let end_edges = if end_edges_byte_pos < data.len() {
        let mut er = BitReader::new(data, end_edges_byte_pos);
        parse_morph_end_edges(&mut er)
    } else {
        Vec::new()
    };

    let rec = SwfMorphRecord {
        fills,
        lines,
        start_fill_groups,
        start_line_groups,
        end_edges,
    };
    ctx.char_morphs.insert(char_id, rec);
    let sym_id = ensure_morph_ratio_symbol(ctx, char_id, 0);
    ctx.char_symbols.insert(char_id, sym_id);
    Ok(())
}

type MorphFillEdgeMap = BTreeMap<usize, Vec<(usize, DirectedSeg, bool)>>;
type MorphLineEdgeMap = BTreeMap<usize, Vec<(usize, DirectedSeg)>>;

fn parse_morph_start_edges(r: &mut BitReader<'_>) -> (MorphFillEdgeMap, MorphLineEdgeMap) {
    r.align();
    let num_fill_bits = r.read_ubits(4).unwrap_or(0) as usize;
    let num_line_bits = r.read_ubits(4).unwrap_or(0) as usize;
    let mut cur_x = 0i32;
    let mut cur_y = 0i32;
    let mut fs0 = 0usize;
    let mut fs1 = 0usize;
    let mut ls = 0usize;
    let mut edge_ord = 0usize;
    let mut fill_groups: BTreeMap<usize, Vec<(usize, DirectedSeg, bool)>> = BTreeMap::new();
    let mut line_groups: BTreeMap<usize, Vec<(usize, DirectedSeg)>> = BTreeMap::new();

    for _ in 0..32_768 {
        let Ok(tf) = r.read_ubits(1) else { break };
        if tf == 0 {
            let Ok(flags) = r.read_ubits(5) else { break };
            if flags == 0 {
                break;
            }
            if (flags & 0x01) != 0 {
                let Ok(mb) = r.read_ubits(5) else { break };
                let (Ok(mx), Ok(my)) = (r.read_sbits(mb as usize), r.read_sbits(mb as usize))
                else {
                    break;
                };
                cur_x = mx;
                cur_y = my;
            }
            if (flags & 0x02) != 0 {
                let Ok(v) = r.read_ubits(num_fill_bits) else {
                    break;
                };
                fs0 = v as usize;
            }
            if (flags & 0x04) != 0 {
                let Ok(v) = r.read_ubits(num_fill_bits) else {
                    break;
                };
                fs1 = v as usize;
            }
            if (flags & 0x08) != 0 {
                let Ok(v) = r.read_ubits(num_line_bits) else {
                    break;
                };
                ls = v as usize;
            }
        } else {
            let Some(seg) = read_single_edge_seg(r, &mut cur_x, &mut cur_y) else {
                break;
            };
            let idx = edge_ord;
            edge_ord += 1;
            if fs1 > 0 {
                fill_groups.entry(fs1).or_default().push((idx, seg, false));
            }
            if fs0 > 0 {
                fill_groups.entry(fs0).or_default().push((idx, seg, true));
            }
            if ls > 0 {
                line_groups.entry(ls).or_default().push((idx, seg));
            }
        }
    }
    (fill_groups, line_groups)
}

fn parse_morph_end_edges(r: &mut BitReader<'_>) -> Vec<DirectedSeg> {
    r.align();
    let num_fill_bits = r.read_ubits(4).unwrap_or(0) as usize;
    let num_line_bits = r.read_ubits(4).unwrap_or(0) as usize;
    let mut cur_x = 0i32;
    let mut cur_y = 0i32;
    let mut out = Vec::new();
    for _ in 0..32_768 {
        let Ok(tf) = r.read_ubits(1) else { break };
        if tf == 0 {
            let Ok(flags) = r.read_ubits(5) else { break };
            if flags == 0 {
                break;
            }
            if (flags & 0x01) != 0 {
                let Ok(mb) = r.read_ubits(5) else { break };
                let (Ok(mx), Ok(my)) = (r.read_sbits(mb as usize), r.read_sbits(mb as usize))
                else {
                    break;
                };
                cur_x = mx;
                cur_y = my;
            }
            if (flags & 0x02) != 0 && r.read_ubits(num_fill_bits).is_err() {
                break;
            }
            if (flags & 0x04) != 0 && r.read_ubits(num_fill_bits).is_err() {
                break;
            }
            if (flags & 0x08) != 0 && r.read_ubits(num_line_bits).is_err() {
                break;
            }
        } else {
            let Some(seg) = read_single_edge_seg(r, &mut cur_x, &mut cur_y) else {
                break;
            };
            out.push(seg);
        }
    }
    out
}

fn read_single_edge_seg(
    r: &mut BitReader<'_>,
    cur_x: &mut i32,
    cur_y: &mut i32,
) -> Option<DirectedSeg> {
    let straight = r.read_ubits(1).ok()? == 1;
    let nbits = (r.read_ubits(4).ok()? as usize) + 2;
    if straight {
        let general = r.read_ubits(1).ok()? == 1;
        let (dx, dy) = if general {
            (r.read_sbits(nbits).ok()?, r.read_sbits(nbits).ok()?)
        } else if r.read_ubits(1).ok()? == 1 {
            (0, r.read_sbits(nbits).ok()?)
        } else {
            (r.read_sbits(nbits).ok()?, 0)
        };
        let nx = *cur_x + dx;
        let ny = *cur_y + dy;
        let s = DirectedSeg::Line {
            x0: *cur_x,
            y0: *cur_y,
            x1: nx,
            y1: ny,
        };
        *cur_x = nx;
        *cur_y = ny;
        Some(s)
    } else {
        let cdx = r.read_sbits(nbits).ok()?;
        let cdy = r.read_sbits(nbits).ok()?;
        let adx = r.read_sbits(nbits).ok()?;
        let ady = r.read_sbits(nbits).ok()?;
        let cx = *cur_x + cdx;
        let cy = *cur_y + cdy;
        let nx = cx + adx;
        let ny = cy + ady;
        let s = DirectedSeg::Quad {
            x0: *cur_x,
            y0: *cur_y,
            cx,
            cy,
            x1: nx,
            y1: ny,
        };
        *cur_x = nx;
        *cur_y = ny;
        Some(s)
    }
}

fn interpolate_seg(a: DirectedSeg, b_opt: Option<DirectedSeg>, t: f64) -> DirectedSeg {
    let Some(b) = b_opt else { return a };
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    let to_quad = |s: DirectedSeg| match s {
        DirectedSeg::Line { x0, y0, x1, y1 } => (x0, y0, (x0 + x1) / 2, (y0 + y1) / 2, x1, y1),
        DirectedSeg::Quad {
            x0,
            y0,
            cx,
            cy,
            x1,
            y1,
        } => (x0, y0, cx, cy, x1, y1),
    };
    match (a, b) {
        (
            DirectedSeg::Line {
                x0: ax0,
                y0: ay0,
                x1: ax1,
                y1: ay1,
            },
            DirectedSeg::Line {
                x0: bx0,
                y0: by0,
                x1: bx1,
                y1: by1,
            },
        ) => DirectedSeg::Line {
            x0: lerp_i32(ax0, bx0, t),
            y0: lerp_i32(ay0, by0, t),
            x1: lerp_i32(ax1, bx1, t),
            y1: lerp_i32(ay1, by1, t),
        },
        _ => {
            let (ax0, ay0, acx, acy, ax1, ay1) = to_quad(a);
            let (bx0, by0, bcx, bcy, bx1, by1) = to_quad(b);
            DirectedSeg::Quad {
                x0: lerp_i32(ax0, bx0, t),
                y0: lerp_i32(ay0, by0, t),
                cx: lerp_i32(acx, bcx, t),
                cy: lerp_i32(acy, bcy, t),
                x1: lerp_i32(ax1, bx1, t),
                y1: lerp_i32(ay1, by1, t),
            }
        }
    }
}

pub(super) fn ensure_morph_ratio_symbol(
    ctx: &mut SwfDecodeCtx,
    char_id: u16,
    ratio: u16,
) -> String {
    if let Some(existing) = ctx.morph_ratio_symbols.get(&(char_id, ratio)) {
        return existing.clone();
    }
    if ctx.morph_ratio_symbols.len() >= 1024
        && let Some(base) = ctx.char_symbols.get(&char_id)
    {
        return base.clone();
    }
    let Some(rec) = ctx.char_morphs.get(&char_id).cloned() else {
        return format!("swf-{}-char-{char_id}", ctx.mount_seq);
    };
    let t = (f64::from(ratio) / 65535.0).clamp(0.0, 1.0);
    let mut elements = Vec::new();
    let mut elem_idx = 0usize;

    for (&f_idx, indexed_segs) in &rec.start_fill_groups {
        let Some(style) = rec.fills.get(f_idx.saturating_sub(1)) else {
            continue;
        };
        let segs: Vec<DirectedSeg> = indexed_segs
            .iter()
            .map(|&(ord, s, rev)| {
                let interp = interpolate_seg(s, rec.end_edges.get(ord).copied(), t);
                if rev { interp.reversed() } else { interp }
            })
            .collect();
        let cmds = chain_segments_to_commands(&segs, true);
        if cmds.is_empty() {
            continue;
        }
        elem_idx += 1;
        elements.push(make_default_element(
            format!("swf-{}-mrph{char_id}-r{ratio}-f{elem_idx}", ctx.mount_seq),
            ElementKind::Path {
                commands: cmds,
                fill_rule: FillRule::EvenOdd,
                fill: lerp_color(style.start_color, style.end_color, t),
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            0.0,
            0.0,
        ));
    }

    for (&l_idx, indexed_segs) in &rec.start_line_groups {
        let Some(style) = rec.lines.get(l_idx.saturating_sub(1)) else {
            continue;
        };
        let w = style.start_width + (style.end_width - style.start_width) * t;
        if w <= 0.0 {
            continue;
        }
        let segs: Vec<DirectedSeg> = indexed_segs
            .iter()
            .map(|&(ord, s)| interpolate_seg(s, rec.end_edges.get(ord).copied(), t))
            .collect();
        let cmds = chain_segments_to_commands(&segs, false);
        if cmds.is_empty() {
            continue;
        }
        elem_idx += 1;
        elements.push(make_default_element(
            format!("swf-{}-mrph{char_id}-r{ratio}-s{elem_idx}", ctx.mount_seq),
            ElementKind::Path {
                commands: cmds,
                fill_rule: FillRule::NonZero,
                fill: [0, 0, 0, 0],
                stroke: lerp_color(style.start_color, style.end_color, t),
                stroke_width: w,
            },
            0.0,
            0.0,
        ));
    }

    let sym_id = if ratio == 0 {
        format!("swf-{}-char-{char_id}", ctx.mount_seq)
    } else {
        format!("swf-{}-char-{char_id}-r{ratio}", ctx.mount_seq)
    };
    ctx.symbols.push(Symbol {
        id: sym_id.clone(),
        name: format!("Morph_{char_id}_{ratio}"),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: format!("swf-{}-mrphlyr-{char_id}-{ratio}", ctx.mount_seq),
                name: "Morph".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    elements,
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    ctx.morph_ratio_symbols
        .insert((char_id, ratio), sym_id.clone());
    sym_id
}

fn swf_key_code_to_spec(code: u8) -> Option<String> {
    match code {
        1 => Some("<Left>".into()),
        2 => Some("<Right>".into()),
        3 => Some("<Home>".into()),
        4 => Some("<End>".into()),
        5 => Some("<Insert>".into()),
        6 => Some("<Delete>".into()),
        8 => Some("<Backspace>".into()),
        13 => Some("<Enter>".into()),
        14 => Some("<Up>".into()),
        15 => Some("<Down>".into()),
        16 => Some("<PageUp>".into()),
        17 => Some("<PageDown>".into()),
        18 => Some("<Tab>".into()),
        19 => Some("<Escape>".into()),
        32 => Some("<Space>".into()),
        33..=126 => Some(char::from(code).to_string()),
        _ => None,
    }
}

pub(super) fn decode_button_tag(
    ctx: &mut SwfDecodeCtx,
    data: &[u8],
    is_v2: bool,
) -> Result<(), String> {
    let mut r = BitReader::new(data, 0);
    let button_id = r.read_u16()?;
    let action_offset = if is_v2 {
        let _flags = r.read_u8()?;
        usize::from(r.read_u16()?)
    } else {
        0usize
    };
    let cond_actions_start = if is_v2 && action_offset > 0 {
        Some(3usize.saturating_add(action_offset))
    } else {
        None
    };

    // Frame 0 = Up, Frame 1 = Over, Frame 2 = Down, Frame 3 = HitTest
    let mut state_depths: [BTreeMap<u16, ActivePlacement>; 4] = Default::default();
    let mut all_depths = std::collections::BTreeSet::new();
    let mut button_records = Vec::new();

    while r.remaining_bytes() > 0 {
        let flags = r.read_u8()?;
        if flags == 0 {
            break;
        }
        let state_hit = (flags & 0x08) != 0;
        let state_down = (flags & 0x04) != 0;
        let state_over = (flags & 0x02) != 0;
        let state_up = (flags & 0x01) != 0;
        let has_blend = is_v2 && (flags & 0x20) != 0;
        let has_filters = is_v2 && (flags & 0x10) != 0;

        let rec_cid = r.read_u16()?;
        let rec_depth = r.read_u16()?;
        if button_records.len() < 256 {
            button_records.push((flags & 0x0F, rec_cid));
        }
        let matrix = r.read_matrix()?;
        let cxform = if is_v2 {
            r.read_cxform(true)?
        } else {
            ColorTransform::default()
        };
        let filters: Vec<FilterSpec> = if has_filters {
            decode_swf_filter_list(ctx, &mut r)?
        } else {
            Vec::new()
        };
        let blend_mode = if has_blend {
            let mb = r.read_u8()?;
            decode_swf_blend_mode(ctx, mb)
        } else {
            BlendMode::Normal
        };

        let p = ActivePlacement {
            char_id: rec_cid,
            matrix,
            cxform,
            name: String::new(),
            clip_depth: 0,
            visible: true,
            cache_as_bitmap: false,
            blend_mode,
            filters,
            ratio: 0,
            clip_script_js: None,
        };
        all_depths.insert(rec_depth);
        for (idx, present) in [state_up, state_over, state_down, state_hit]
            .into_iter()
            .enumerate()
        {
            if present {
                state_depths[idx].insert(rec_depth, p.clone());
            }
        }
    }

    let mut layers = Vec::new();
    for depth in all_depths {
        if ctx.total_keyframes >= MAX_SWF_KEYFRAMES {
            break;
        }
        let mut keyframes = Vec::with_capacity(4);
        for (f_idx, map) in state_depths.iter().enumerate() {
            let mut kf = Keyframe {
                frame: f_idx as u32,
                ..Default::default()
            };
            if let Some(p) = map.get(&depth)
                && let Some(sym_id) = ctx.char_symbols.get(&p.char_id).cloned()
            {
                let mut tf = Transform::from_matrix(p.matrix).unwrap_or_default();
                tf.alpha = p.cxform.multipliers[3].clamp(0.0, 1.0);
                tf.color_transform = p.cxform;
                let mut el = make_default_element(
                    format!(
                        "swf-{}-btn{button_id}-d{depth}-f{f_idx}-c{}",
                        ctx.mount_seq, p.char_id
                    ),
                    ElementKind::Instance {
                        symbol_id: sym_id,
                        playback: InstancePlayback::Independent,
                    },
                    tf.x,
                    tf.y,
                );
                el.transform = tf;
                el.blend_mode = p.blend_mode;
                el.filters.clone_from(&p.filters);
                kf.elements.push(el);
            }
            keyframes.push(kf);
            ctx.total_keyframes += 1;
        }
        layers.push(Layer {
            id: format!("swf-{}-btn{button_id}-lyr{depth}", ctx.mount_seq),
            name: format!("Depth {depth}"),
            visible: true,
            locked: false,
            keyframes,
            properties: LayerProperties::default(),
        });
    }
    if layers.is_empty() {
        layers.push(Layer {
            id: format!("swf-{}-btn{button_id}-empty", ctx.mount_seq),
            name: "State".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe::default()],
            properties: LayerProperties::default(),
        });
    }

    let sym_id = format!("swf-{}-char-{button_id}", ctx.mount_seq);
    ctx.symbols.push(Symbol {
        id: sym_id.clone(),
        name: format!("Button_{button_id}"),
        folder: String::new(),
        kind: SymbolKind::Button,
        timeline: Timeline {
            frame_count: 4,
            layers,
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    ctx.char_symbols.insert(button_id, sym_id);
    ctx.char_button_records.insert(button_id, button_records);

    // Decode button AVM1 actions
    let mut js_registrations = String::new();
    if !is_v2 {
        let pos = r.byte_pos();
        if pos < data.len() {
            ctx.saw_do_action = true;
            if let Some(fn_expr) = compile_avm1_handler_fn_expr(
                ctx,
                &data[pos..],
                &format!("DefineButton {button_id}"),
            ) {
                js_registrations.push_str(&format!(
                    "if (__nfDisplay && typeof __nfDisplay.addLegacyEvent === 'function') __nfDisplay.addLegacyEvent(this, 'button', 'release', null, {fn_expr});\n"
                ));
            }
        }
    } else if let Some(mut cpos) = cond_actions_start {
        while cpos + 4 <= data.len() {
            let cond_size = usize::from(u16::from_le_bytes([data[cpos], data[cpos + 1]]));
            let cond_flags = u16::from_le_bytes([data[cpos + 2], data[cpos + 3]]);
            let action_end = if cond_size == 0 {
                data.len()
            } else {
                cpos.saturating_add(cond_size).min(data.len())
            };
            let actions = &data[cpos + 4..action_end];
            if !actions.is_empty() {
                ctx.saw_do_action = true;
                if let Some(fn_expr) = compile_avm1_handler_fn_expr(
                    ctx,
                    actions,
                    &format!("DefineButton2 {button_id}"),
                ) {
                    let fn_var = format!("__btnFn_{cpos}");
                    js_registrations.push_str(&format!("{{ const {fn_var} = {fn_expr};\n"));
                    for (mask, ev_name) in [
                        (0x0001u16, "rollOver"),
                        (0x0002, "rollOut"),
                        (0x0004, "press"),
                        (0x0008, "release"),
                        (0x0010, "dragOut"),
                        (0x0020, "dragOver"),
                        (0x0040, "releaseOutside"),
                        (0x0080, "dragOver"),
                        (0x0100, "releaseOutside"),
                    ] {
                        if (cond_flags & mask) != 0 {
                            js_registrations.push_str(&format!(
                                "if (__nfDisplay && typeof __nfDisplay.addLegacyEvent === 'function') __nfDisplay.addLegacyEvent(this, 'button', '{ev_name}', null, {fn_var});\n"
                            ));
                        }
                    }
                    let key_code = ((cond_flags >> 9) & 0x7F) as u8;
                    if key_code != 0
                        && let Some(spec) = swf_key_code_to_spec(key_code)
                    {
                        let spec_lit = super::vm::js_string_literal(&spec);
                        js_registrations.push_str(&format!(
                            "if (__nfDisplay && typeof __nfDisplay.addLegacyEvent === 'function') __nfDisplay.addLegacyEvent(this, 'button', 'keyPress', {spec_lit}, {fn_var});\n"
                        ));
                    }
                    js_registrations.push_str("}\n");
                }
            }
            if cond_size == 0 {
                break;
            }
            cpos = action_end;
        }
    }

    if !js_registrations.is_empty() {
        ctx.char_button_scripts.insert(button_id, js_registrations);
    }
    Ok(())
}

pub(super) fn parse_clip_actions_to_js(
    ctx: &mut SwfDecodeCtx,
    r: &mut BitReader<'_>,
    depth: u16,
) -> Result<Option<String>, String> {
    if r.remaining_bytes() < 4 {
        return Ok(None);
    }
    let _reserved = r.read_u16()?;
    let wide_flags = ctx.swf_version >= 6;
    let _all_flags = if wide_flags {
        r.read_u32()?
    } else {
        u32::from(r.read_u16()?)
    };

    let mut decl_blocks = Vec::new();
    let mut init_vars = Vec::new();
    let mut construct_vars = Vec::new();
    let mut load_vars = Vec::new();
    let mut enter_vars = Vec::new();
    let mut unload_vars = Vec::new();
    let mut reg_blocks = Vec::new();
    let mut rec_idx = 0usize;

    loop {
        if r.remaining_bytes() < if wide_flags { 4 } else { 2 } {
            break;
        }
        let ev_flags = if wide_flags {
            r.read_u32()?
        } else {
            u32::from(r.read_u16()?)
        };
        if ev_flags == 0 {
            break;
        }
        if r.remaining_bytes() < 4 {
            break;
        }
        let rec_size = r.read_u32()? as usize;
        let has_key = wide_flags && (ev_flags & 0x0002_0000) != 0;
        let key_code = if has_key && r.remaining_bytes() >= 1 {
            r.read_u8()?
        } else {
            0
        };
        let action_len = rec_size.saturating_sub(usize::from(has_key));
        let start = r.byte_pos();
        let end = start.saturating_add(action_len).min(r.data.len());
        r.bit_pos = end * 8;
        let actions = &r.data[start..end];
        if actions.is_empty() {
            continue;
        }
        ctx.saw_do_action = true;
        rec_idx += 1;
        let Some(fn_expr) = compile_avm1_handler_fn_expr(
            ctx,
            actions,
            &format!("PlaceObject2 depth {depth} ClipAction {rec_idx}"),
        ) else {
            continue;
        };
        let fn_var = format!("__clipFn_{rec_idx}");
        decl_blocks.push(format!("const {fn_var} = {fn_expr};\n"));
        if (ev_flags & 0x0000_0200) != 0 {
            init_vars.push(fn_var.clone());
        }
        if (ev_flags & 0x0004_0000) != 0 {
            construct_vars.push(fn_var.clone());
        }
        if (ev_flags & 0x0000_0001) != 0 {
            load_vars.push(fn_var.clone());
        }
        if (ev_flags & 0x0000_0002) != 0 {
            enter_vars.push(fn_var.clone());
        }
        if (ev_flags & 0x0000_0004) != 0 {
            unload_vars.push(fn_var.clone());
        }

        let mut single_reg = String::new();
        for (mask, ev_name) in [
            (0x0000_0008u32, "mouseMove"),
            (0x0000_0010, "mouseDown"),
            (0x0000_0020, "mouseUp"),
            (0x0000_0040, "keyDown"),
            (0x0000_0080, "keyUp"),
            (0x0000_0100, "data"),
        ] {
            if (ev_flags & mask) != 0 {
                single_reg.push_str(&format!(
                    "if (__nfDisplay && typeof __nfDisplay.addLegacyEvent === 'function') __nfDisplay.addLegacyEvent(this, 'clip', '{ev_name}', null, {fn_var});\n"
                ));
            }
        }
        for (mask, btn_ev) in [
            (0x0000_0400u32, "press"),
            (0x0000_0800, "release"),
            (0x0000_1000, "releaseOutside"),
            (0x0000_2000, "rollOver"),
            (0x0000_4000, "rollOut"),
            (0x0000_8000, "dragOver"),
            (0x0001_0000, "dragOut"),
        ] {
            if (ev_flags & mask) != 0 {
                single_reg.push_str(&format!(
                    "if (__nfDisplay && typeof __nfDisplay.addLegacyEvent === 'function') __nfDisplay.addLegacyEvent(this, 'button', '{btn_ev}', null, {fn_var});\n"
                ));
            }
        }
        if has_key && let Some(spec) = swf_key_code_to_spec(key_code) {
            let spec_lit = super::vm::js_string_literal(&spec);
            single_reg.push_str(&format!(
                "if (__nfDisplay && typeof __nfDisplay.addLegacyEvent === 'function') __nfDisplay.addLegacyEvent(this, 'button', 'keyPress', {spec_lit}, {fn_var});\n"
            ));
        }
        if !single_reg.is_empty() {
            reg_blocks.push(single_reg);
        }
    }

    if decl_blocks.is_empty() {
        return Ok(None);
    }

    let mut combined = format!("{{\n{}", super::vm::AVM1_RUNTIME_INIT_JS);
    for d in decl_blocks {
        combined.push_str(&d);
    }
    if !init_vars.is_empty()
        || !construct_vars.is_empty()
        || !load_vars.is_empty()
        || !enter_vars.is_empty()
        || !unload_vars.is_empty()
    {
        combined.push_str(&format!(
            "if (__avm1) __avm1.registerClip(this, [{}], [{}], [{}], [{}], [{}]);\n",
            init_vars.join(", "),
            construct_vars.join(", "),
            load_vars.join(", "),
            enter_vars.join(", "),
            unload_vars.join(", "),
        ));
    }
    for b in reg_blocks {
        combined.push_str(&b);
    }
    combined.push_str("}\n");
    Ok(Some(combined))
}

pub(super) fn attach_element_object_script(
    ctx: &mut SwfDecodeCtx,
    el: &mut Element,
    js_code: &str,
    label: &str,
) {
    if js_code.trim().is_empty() {
        return;
    }
    let idx = ctx.scripts.len() + 1;
    let source_id = format!("/*swf-{}-{label}-{idx}*/", ctx.mount_seq);
    ctx.scripts.push(super::CompiledSwfScriptUnit {
        language: ScriptLanguage::ActionScript2,
        source: source_id.clone(),
        code: js_code.to_owned(),
        location: format!("{label}:load"),
    });
    el.scripts.push(ObjectScript {
        event: ObjectEvent::Load,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: source_id,
        },
    });
}
