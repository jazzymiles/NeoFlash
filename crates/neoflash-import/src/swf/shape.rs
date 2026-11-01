//! SWF vector shape (`DefineShape` 1/2/3/4) and morph shape (`DefineMorphShape` 1/2)
//! decoders into NeoFlash `ElementKind::Path`, `ElementKind::PaintedPath` and `ElementKind::Bitmap`.

use super::bits::SwfSlice;
use crate::ImportReport;
use neoflash_core::{
    Color, Element, ElementKind, FillRule, GradientKind, GradientPaint, GradientStop, Id,
    MAX_PATH_COMMANDS, PathCommand, Transform, validate_path_commands,
};
use std::collections::{BTreeMap, HashMap};

const MAX_SHAPE_RECORDS: usize = 100_000;

#[derive(Clone, Debug)]
pub(crate) struct SwfBitmapRef {
    pub resource_id: Id,
    pub width: u32,
    pub height: u32,
    pub avg_color: Color,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodedSwfShape {
    pub elements: Vec<Element>,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodedMorphShape {
    pub start_elements: Vec<Element>,
    pub end_elements: Vec<Element>,
}

#[derive(Clone, Debug)]
enum SwfPaint {
    Solid(Color),
    Gradient {
        fallback: Color,
        paint: GradientPaint,
    },
    Bitmap {
        bitmap_id: u16,
        matrix: [f64; 6],
        smoothing: bool,
    },
}

#[derive(Clone, Debug)]
struct SwfStrokeStyle {
    width_px: f64,
    paint: SwfPaint,
}

#[derive(Clone, Copy, Debug)]
struct TwipEdge {
    start: [i32; 2],
    end: [i32; 2],
    control: Option<[i32; 2]>,
}

impl TwipEdge {
    fn is_degenerate(self) -> bool {
        match self.control {
            None => self.start == self.end,
            Some(c) => self.start == self.end && self.start == c,
        }
    }

    fn reversed(self) -> Self {
        Self {
            start: self.end,
            end: self.start,
            control: self.control,
        }
    }

    fn append_px(self, scale: f64, cmds: &mut Vec<PathCommand>) {
        let x = f64::from(self.end[0]) * scale;
        let y = f64::from(self.end[1]) * scale;
        if let Some(c) = self.control {
            cmds.push(PathCommand::QuadraticTo {
                cx: f64::from(c[0]) * scale,
                cy: f64::from(c[1]) * scale,
                x,
                y,
            });
        } else {
            cmds.push(PathCommand::LineTo { x, y });
        }
    }
}

/// Decode `DefineShape` (2), `DefineShape2` (22), `DefineShape3` (32), or `DefineShape4` (83).
pub(crate) fn decode_define_shape(
    tag_code: u16,
    payload: &[u8],
    bitmaps: &HashMap<u16, SwfBitmapRef>,
    location: &str,
    report: &mut ImportReport,
) -> Option<(u16, DecodedSwfShape)> {
    let mut r = SwfSlice::new(payload);
    let shape_id = r.u16()?;
    let _bounds = r.rect_twips()?;
    let mut non_round_stroke = false;
    let mut approx_reason: Option<&'static str> = None;

    if tag_code == 83 {
        let _edge_bounds = r.rect_twips()?;
        let flags = r.u8()?;
        if (flags & 0x02) != 0 {
            non_round_stroke = true;
        }
    }

    let has_rgba = matches!(tag_code, 32 | 83);
    let has_ext_count = tag_code >= 22;
    let is_shape4 = tag_code == 83;

    let mut fills = read_fill_styles(&mut r, has_rgba, has_ext_count, &mut approx_reason)?;
    let mut lines = read_line_styles(
        &mut r,
        has_rgba,
        has_ext_count,
        is_shape4,
        &mut non_round_stroke,
        &mut approx_reason,
    )?;

    r.align();
    let mut num_fill_bits = r.ub(4)? as usize;
    let mut num_line_bits = r.ub(4)? as usize;

    let mut elements = Vec::new();
    let mut elem_seq = 0usize;
    let mut fill_edges: BTreeMap<usize, Vec<TwipEdge>> = BTreeMap::new();
    let mut stroke_edges: BTreeMap<usize, Vec<TwipEdge>> = BTreeMap::new();

    let mut cur_x = 0i32;
    let mut cur_y = 0i32;
    let mut fs0 = 0usize;
    let mut fs1 = 0usize;
    let mut ls = 0usize;
    let mut records = 0usize;

    loop {
        if records >= MAX_SHAPE_RECORDS {
            approx_reason.get_or_insert("Shape exceeded 100,000 edge records and was truncated.");
            break;
        }
        records += 1;
        let Some(is_edge) = r.bit() else {
            approx_reason
                .get_or_insert("Truncated SWF shape edge stream; partial geometry retained.");
            break;
        };
        if !is_edge {
            let Some(flags) = r.ub(5) else {
                break;
            };
            if flags == 0 {
                break;
            }
            let state_new_styles = (flags & 0x10) != 0;
            let state_line_style = (flags & 0x08) != 0;
            let state_fill_style1 = (flags & 0x04) != 0;
            let state_fill_style0 = (flags & 0x02) != 0;
            let state_move_to = (flags & 0x01) != 0;

            if state_move_to {
                let move_bits = r.ub(5)? as usize;
                cur_x = r.sb(move_bits)?;
                cur_y = r.sb(move_bits)?;
            }
            if state_fill_style0 {
                fs0 = r.ub(num_fill_bits)? as usize;
            }
            if state_fill_style1 {
                fs1 = r.ub(num_fill_bits)? as usize;
            }
            if state_line_style {
                ls = r.ub(num_line_bits)? as usize;
            }
            if state_new_styles && has_ext_count {
                flush_style_group(
                    shape_id,
                    &mut elem_seq,
                    &fills,
                    &lines,
                    &mut fill_edges,
                    &mut stroke_edges,
                    bitmaps,
                    &mut approx_reason,
                    &mut elements,
                );
                r.align();
                let Some(new_fills) =
                    read_fill_styles(&mut r, has_rgba, has_ext_count, &mut approx_reason)
                else {
                    break;
                };
                let Some(new_lines) = read_line_styles(
                    &mut r,
                    has_rgba,
                    has_ext_count,
                    is_shape4,
                    &mut non_round_stroke,
                    &mut approx_reason,
                ) else {
                    break;
                };
                fills = new_fills;
                lines = new_lines;
                r.align();
                num_fill_bits = r.ub(4)? as usize;
                num_line_bits = r.ub(4)? as usize;
                fs0 = 0;
                fs1 = 0;
                ls = 0;
            }
        } else {
            let is_straight = r.bit()?;
            let start = [cur_x, cur_y];
            let edge = if is_straight {
                let nbits = (r.ub(4)? as usize) + 2;
                let general = r.bit()?;
                let (dx, dy) = if general {
                    (r.sb(nbits)?, r.sb(nbits)?)
                } else if r.bit()? {
                    (0, r.sb(nbits)?)
                } else {
                    (r.sb(nbits)?, 0)
                };
                cur_x = cur_x.saturating_add(dx);
                cur_y = cur_y.saturating_add(dy);
                TwipEdge {
                    start,
                    end: [cur_x, cur_y],
                    control: None,
                }
            } else {
                let nbits = (r.ub(4)? as usize) + 2;
                let cdx = r.sb(nbits)?;
                let cdy = r.sb(nbits)?;
                let adx = r.sb(nbits)?;
                let ady = r.sb(nbits)?;
                let cx = cur_x.saturating_add(cdx);
                let cy = cur_y.saturating_add(cdy);
                cur_x = cx.saturating_add(adx);
                cur_y = cy.saturating_add(ady);
                TwipEdge {
                    start,
                    end: [cur_x, cur_y],
                    control: Some([cx, cy]),
                }
            };

            if !edge.is_degenerate() {
                if fs0 != fs1 {
                    if fs0 > 0 {
                        fill_edges.entry(fs0).or_default().push(edge.reversed());
                    }
                    if fs1 > 0 {
                        fill_edges.entry(fs1).or_default().push(edge);
                    }
                }
                if ls > 0 {
                    stroke_edges.entry(ls).or_default().push(edge);
                }
            }
        }
    }

    flush_style_group(
        shape_id,
        &mut elem_seq,
        &fills,
        &lines,
        &mut fill_edges,
        &mut stroke_edges,
        bitmaps,
        &mut approx_reason,
        &mut elements,
    );

    if non_round_stroke {
        report.warning(
            "swf_stroke_rendering",
            format!("{location}#shape:{shape_id}"),
            "SWF hairline, non-round cap/join or non-scaling stroke flags are approximated with standard round strokes.",
        );
    }
    if let Some(reason) = approx_reason {
        report.warning(
            "swf_shape_approximated",
            format!("{location}#shape:{shape_id}"),
            reason,
        );
    }

    Some((shape_id, DecodedSwfShape { elements }))
}

/// Decode `DefineMorphShape` (46) or `DefineMorphShape2` (84) into start (`ratio = 0`) and end (`ratio = 65535`) shapes.
pub(crate) fn decode_define_morph_shape(
    tag_code: u16,
    payload: &[u8],
    bitmaps: &HashMap<u16, SwfBitmapRef>,
    location: &str,
    report: &mut ImportReport,
) -> Option<(u16, DecodedMorphShape)> {
    let mut r = SwfSlice::new(payload);
    let morph_id = r.u16()?;
    let _start_bounds = r.rect_twips()?;
    let _end_bounds = r.rect_twips()?;
    if tag_code == 84 {
        let _start_edge = r.rect_twips()?;
        let _end_edge = r.rect_twips()?;
        let _flags = r.u8()?;
    }
    let offset = r.u32()? as usize;
    let after_offset_pos = r.pos();
    let end_edges_pos = after_offset_pos.checked_add(offset)?;

    let mut approx_reason: Option<&'static str> = None;
    let mut non_round = false;

    let fill_count = read_style_count(&mut r, true)?;
    let mut start_fills = Vec::with_capacity(fill_count);
    let mut end_fills = Vec::with_capacity(fill_count);
    for _ in 0..fill_count {
        let (sf, ef) = read_morph_fill_style(&mut r, tag_code == 84, &mut approx_reason)?;
        start_fills.push(sf);
        end_fills.push(ef);
    }

    let line_count = read_style_count(&mut r, true)?;
    let mut start_lines = Vec::with_capacity(line_count);
    let mut end_lines = Vec::with_capacity(line_count);
    for _ in 0..line_count {
        let (sl, el) =
            read_morph_line_style(&mut r, tag_code == 84, &mut non_round, &mut approx_reason)?;
        start_lines.push(sl);
        end_lines.push(el);
    }

    let start_edges_slice = payload.get(r.pos()..end_edges_pos.min(payload.len()))?;
    let end_edges_slice = payload.get(end_edges_pos.min(payload.len())..)?;

    let (start_elements, style_trace) = decode_morph_half_edges(
        morph_id,
        "start",
        start_edges_slice,
        &start_fills,
        &start_lines,
        None,
        bitmaps,
        &mut approx_reason,
    );
    let (end_elements, _) = decode_morph_half_edges(
        morph_id,
        "end",
        end_edges_slice,
        &end_fills,
        &end_lines,
        Some(&style_trace),
        bitmaps,
        &mut approx_reason,
    );

    if non_round {
        report.warning(
            "swf_stroke_rendering",
            format!("{location}#morph:{morph_id}"),
            "Morph shape non-round cap/join or scaling flags are approximated with round strokes.",
        );
    }
    if let Some(reason) = approx_reason {
        report.warning(
            "swf_shape_approximated",
            format!("{location}#morph:{morph_id}"),
            reason,
        );
    }

    let end_elements = if end_elements.is_empty() {
        start_elements.clone()
    } else {
        end_elements
    };

    Some((
        morph_id,
        DecodedMorphShape {
            start_elements,
            end_elements,
        },
    ))
}

/// Read a pure `SHAPE` record (used in `DefineFont` / `DefineFont2` / `DefineFont3` glyph tables)
/// into closed `PathCommand` contours scaled by `scale`.
pub(crate) fn read_pure_shape_commands(
    r: &mut SwfSlice<'_>,
    scale: f64,
) -> Option<Vec<PathCommand>> {
    r.align();
    let num_fill_bits = r.ub(4)? as usize;
    let num_line_bits = r.ub(4)? as usize;
    let mut edges = Vec::new();
    let mut cur_x = 0i32;
    let mut cur_y = 0i32;
    let mut fs0 = 0usize;
    let mut fs1 = 0usize;

    for _ in 0..MAX_SHAPE_RECORDS {
        let is_edge = r.bit()?;
        if !is_edge {
            let flags = r.ub(5)?;
            if flags == 0 {
                break;
            }
            if (flags & 0x01) != 0 {
                let n = r.ub(5)? as usize;
                cur_x = r.sb(n)?;
                cur_y = r.sb(n)?;
            }
            if (flags & 0x02) != 0 {
                fs0 = r.ub(num_fill_bits)? as usize;
            }
            if (flags & 0x04) != 0 {
                fs1 = r.ub(num_fill_bits)? as usize;
            }
            if (flags & 0x08) != 0 {
                let _ = r.ub(num_line_bits)?;
            }
            if (flags & 0x10) != 0 {
                break;
            }
        } else {
            let is_straight = r.bit()?;
            let start = [cur_x, cur_y];
            let edge = if is_straight {
                let n = (r.ub(4)? as usize) + 2;
                let (dx, dy) = if r.bit()? {
                    (r.sb(n)?, r.sb(n)?)
                } else if r.bit()? {
                    (0, r.sb(n)?)
                } else {
                    (r.sb(n)?, 0)
                };
                cur_x = cur_x.saturating_add(dx);
                cur_y = cur_y.saturating_add(dy);
                TwipEdge {
                    start,
                    end: [cur_x, cur_y],
                    control: None,
                }
            } else {
                let n = (r.ub(4)? as usize) + 2;
                let cx = cur_x.saturating_add(r.sb(n)?);
                let cy = cur_y.saturating_add(r.sb(n)?);
                cur_x = cx.saturating_add(r.sb(n)?);
                cur_y = cy.saturating_add(r.sb(n)?);
                TwipEdge {
                    start,
                    end: [cur_x, cur_y],
                    control: Some([cx, cy]),
                }
            };
            if !edge.is_degenerate() {
                if fs0 > 0 && fs0 != fs1 {
                    edges.push(edge.reversed());
                } else if (fs1 > 0 && fs0 != fs1) || (fs0 == 0 && fs1 == 0) {
                    edges.push(edge);
                }
            }
        }
    }
    r.align();
    let (cmds, _) = assemble_fill_contours(&edges, scale);
    Some(cmds)
}

#[derive(Clone, Copy)]
struct MorphEdgeStyle {
    fs0: usize,
    fs1: usize,
    ls: usize,
}

#[allow(clippy::too_many_arguments)]
fn decode_morph_half_edges(
    morph_id: u16,
    tag_suffix: &str,
    slice: &[u8],
    fills: &[SwfPaint],
    lines: &[SwfStrokeStyle],
    inherited_trace: Option<&[MorphEdgeStyle]>,
    bitmaps: &HashMap<u16, SwfBitmapRef>,
    approx_reason: &mut Option<&'static str>,
) -> (Vec<Element>, Vec<MorphEdgeStyle>) {
    let mut r = SwfSlice::new(slice);
    let Some(num_fill_bits) = r.ub(4) else {
        return (Vec::new(), Vec::new());
    };
    let Some(num_line_bits) = r.ub(4) else {
        return (Vec::new(), Vec::new());
    };
    let (num_fill_bits, num_line_bits) = (num_fill_bits as usize, num_line_bits as usize);

    let mut fill_edges: BTreeMap<usize, Vec<TwipEdge>> = BTreeMap::new();
    let mut stroke_edges: BTreeMap<usize, Vec<TwipEdge>> = BTreeMap::new();
    let mut style_trace = Vec::new();
    let mut edge_idx = 0usize;
    let mut cur_x = 0i32;
    let mut cur_y = 0i32;
    let mut fs0 = 0usize;
    let mut fs1 = 0usize;
    let mut ls = 0usize;

    for _ in 0..MAX_SHAPE_RECORDS {
        let Some(is_edge) = r.bit() else { break };
        if !is_edge {
            let Some(flags) = r.ub(5) else { break };
            if flags == 0 {
                break;
            }
            if (flags & 0x01) != 0 {
                let Some(n) = r.ub(5) else { break };
                let (Some(mx), Some(my)) = (r.sb(n as usize), r.sb(n as usize)) else {
                    break;
                };
                cur_x = mx;
                cur_y = my;
            }
            if (flags & 0x02) != 0 {
                let Some(v) = r.ub(num_fill_bits) else { break };
                fs0 = v as usize;
            }
            if (flags & 0x04) != 0 {
                let Some(v) = r.ub(num_fill_bits) else { break };
                fs1 = v as usize;
            }
            if (flags & 0x08) != 0 {
                let Some(v) = r.ub(num_line_bits) else { break };
                ls = v as usize;
            }
        } else {
            let Some(is_straight) = r.bit() else { break };
            let start = [cur_x, cur_y];
            let edge = if is_straight {
                let Some(nb) = r.ub(4) else { break };
                let n = (nb as usize) + 2;
                let Some(general_line) = r.bit() else { break };
                let (dx, dy) = if general_line {
                    let (Some(x), Some(y)) = (r.sb(n), r.sb(n)) else {
                        break;
                    };
                    (x, y)
                } else {
                    let Some(vert) = r.bit() else { break };
                    let Some(d) = r.sb(n) else { break };
                    if vert { (0, d) } else { (d, 0) }
                };
                cur_x = cur_x.saturating_add(dx);
                cur_y = cur_y.saturating_add(dy);
                TwipEdge {
                    start,
                    end: [cur_x, cur_y],
                    control: None,
                }
            } else {
                let Some(nb) = r.ub(4) else { break };
                let n = (nb as usize) + 2;
                let (Some(cdx), Some(cdy), Some(adx), Some(ady)) =
                    (r.sb(n), r.sb(n), r.sb(n), r.sb(n))
                else {
                    break;
                };
                let cx = cur_x.saturating_add(cdx);
                let cy = cur_y.saturating_add(cdy);
                cur_x = cx.saturating_add(adx);
                cur_y = cy.saturating_add(ady);
                TwipEdge {
                    start,
                    end: [cur_x, cur_y],
                    control: Some([cx, cy]),
                }
            };

            // In SWF DefineMorphShape, EndEdges often omits fillStyle/lineStyle changes because
            // each edge corresponds 1-to-1 with StartEdges' active styles!
            let active = if let Some(trace) = inherited_trace
                && fs0 == 0
                && fs1 == 0
                && ls == 0
                && let Some(&traced) = trace.get(edge_idx)
            {
                traced
            } else {
                MorphEdgeStyle { fs0, fs1, ls }
            };
            style_trace.push(active);
            edge_idx += 1;

            if !edge.is_degenerate() {
                if active.fs0 != active.fs1 {
                    if active.fs0 > 0 {
                        fill_edges
                            .entry(active.fs0)
                            .or_default()
                            .push(edge.reversed());
                    }
                    if active.fs1 > 0 {
                        fill_edges.entry(active.fs1).or_default().push(edge);
                    }
                }
                if active.ls > 0 {
                    stroke_edges.entry(active.ls).or_default().push(edge);
                }
            }
        }
    }

    let mut elements = Vec::new();
    let mut seq = 0usize;
    flush_style_group_named(
        &format!("swf-morph-{morph_id}-{tag_suffix}"),
        &mut seq,
        fills,
        lines,
        &mut fill_edges,
        &mut stroke_edges,
        bitmaps,
        approx_reason,
        &mut elements,
    );
    (elements, style_trace)
}

fn read_style_count(r: &mut SwfSlice<'_>, has_ext_count: bool) -> Option<usize> {
    let c = r.u8()?;
    if has_ext_count && c == 0xFF {
        Some(usize::from(r.u16()?))
    } else {
        Some(usize::from(c))
    }
}

fn read_fill_styles(
    r: &mut SwfSlice<'_>,
    has_rgba: bool,
    has_ext_count: bool,
    approx_reason: &mut Option<&'static str>,
) -> Option<Vec<SwfPaint>> {
    let count = read_style_count(r, has_ext_count)?;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        out.push(read_single_fill_style(r, has_rgba, approx_reason)?);
    }
    Some(out)
}

fn read_single_fill_style(
    r: &mut SwfSlice<'_>,
    has_rgba: bool,
    approx_reason: &mut Option<&'static str>,
) -> Option<SwfPaint> {
    let fill_type = r.u8()?;
    match fill_type {
        0x00 => {
            let color = if has_rgba { r.rgba()? } else { r.rgb()? };
            Some(SwfPaint::Solid(color))
        }
        0x10 | 0x12 | 0x13 => {
            let matrix = r.matrix_px()?;
            r.align();
            let spread = r.ub(2)?;
            let interp = r.ub(2)?;
            let num_grads = r.ub(4)? as usize;
            if spread != 0 || interp != 0 {
                approx_reason.get_or_insert(
                    "Non-pad gradient spread or linear-RGB interpolation is approximated with standard pad RGB gradient.",
                );
            }
            let mut stops = Vec::with_capacity(num_grads);
            for _ in 0..num_grads {
                let ratio = r.u8()?;
                let color = if has_rgba { r.rgba()? } else { r.rgb()? };
                stops.push(GradientStop { ratio, color });
            }
            r.align();
            let focal = if fill_type == 0x13 {
                r.fixed8()?.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            stops.sort_by_key(|s| s.ratio);
            let fallback = stops.first().map_or([0, 0, 0, 255], |s| s.color);
            let paint = GradientPaint {
                kind: if fill_type == 0x10 {
                    GradientKind::Linear
                } else {
                    GradientKind::Radial
                },
                matrix,
                stops,
                focal_point_ratio: focal,
            };
            if paint.validate().is_ok() {
                Some(SwfPaint::Gradient { fallback, paint })
            } else {
                approx_reason
                    .get_or_insert("Singular or empty SWF gradient approximated as solid fill.");
                Some(SwfPaint::Solid(fallback))
            }
        }
        0x40..=0x43 => {
            let bitmap_id = r.u16()?;
            let matrix = r.matrix_px()?;
            let smoothing = matches!(fill_type, 0x40 | 0x41);
            Some(SwfPaint::Bitmap {
                bitmap_id,
                matrix,
                smoothing,
            })
        }
        _ => None,
    }
}

fn read_line_styles(
    r: &mut SwfSlice<'_>,
    has_rgba: bool,
    has_ext_count: bool,
    is_shape4: bool,
    non_round_stroke: &mut bool,
    approx_reason: &mut Option<&'static str>,
) -> Option<Vec<SwfStrokeStyle>> {
    let count = read_style_count(r, has_ext_count)?;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        if !is_shape4 {
            let width_twips = r.u16()?;
            if width_twips == 0 {
                *non_round_stroke = true;
            }
            let color = if has_rgba { r.rgba()? } else { r.rgb()? };
            out.push(SwfStrokeStyle {
                width_px: (f64::from(width_twips) / 20.0).max(0.5),
                paint: SwfPaint::Solid(color),
            });
        } else {
            let width_twips = r.u16()?;
            r.align();
            let start_cap = r.ub(2)?;
            let join_style = r.ub(2)?;
            let has_fill = r.bit()?;
            let no_hscale = r.bit()?;
            let no_vscale = r.bit()?;
            let pixel_hinting = r.bit()?;
            let _reserved = r.ub(5)?;
            let _no_close = r.bit()?;
            let end_cap = r.ub(2)?;
            r.align();
            if width_twips == 0
                || start_cap != 0
                || end_cap != 0
                || join_style != 0
                || no_hscale
                || no_vscale
                || pixel_hinting
            {
                *non_round_stroke = true;
            }
            if join_style == 2 {
                let _miter = r.ufixed8()?;
            }
            let paint = if has_fill {
                read_single_fill_style(r, true, approx_reason)?
            } else {
                SwfPaint::Solid(r.rgba()?)
            };
            out.push(SwfStrokeStyle {
                width_px: (f64::from(width_twips) / 20.0).max(0.5),
                paint,
            });
        }
    }
    Some(out)
}

fn read_morph_fill_style(
    r: &mut SwfSlice<'_>,
    is_morph2: bool,
    approx_reason: &mut Option<&'static str>,
) -> Option<(SwfPaint, SwfPaint)> {
    let fill_type = r.u8()?;
    match fill_type {
        0x00 => {
            let sc = r.rgba()?;
            let ec = r.rgba()?;
            Some((SwfPaint::Solid(sc), SwfPaint::Solid(ec)))
        }
        0x10 | 0x12 | 0x13 => {
            let sm = r.matrix_px()?;
            let em = r.matrix_px()?;
            let num_grads = usize::from(r.u8()? & 0x0f);
            let mut s_stops = Vec::with_capacity(num_grads);
            let mut e_stops = Vec::with_capacity(num_grads);
            for _ in 0..num_grads {
                let sr = r.u8()?;
                let sc = r.rgba()?;
                let er = r.u8()?;
                let ec = r.rgba()?;
                s_stops.push(GradientStop {
                    ratio: sr,
                    color: sc,
                });
                e_stops.push(GradientStop {
                    ratio: er,
                    color: ec,
                });
            }
            let (sfocal, efocal) = if fill_type == 0x13 && is_morph2 {
                (r.fixed8()?.clamp(-1.0, 1.0), r.fixed8()?.clamp(-1.0, 1.0))
            } else {
                (0.0, 0.0)
            };
            s_stops.sort_by_key(|s| s.ratio);
            e_stops.sort_by_key(|s| s.ratio);
            let kind = if fill_type == 0x10 {
                GradientKind::Linear
            } else {
                GradientKind::Radial
            };
            let s_fb = s_stops.first().map_or([0, 0, 0, 255], |s| s.color);
            let e_fb = e_stops.first().map_or([0, 0, 0, 255], |s| s.color);
            let sp = GradientPaint {
                kind,
                matrix: sm,
                stops: s_stops,
                focal_point_ratio: sfocal,
            };
            let ep = GradientPaint {
                kind,
                matrix: em,
                stops: e_stops,
                focal_point_ratio: efocal,
            };
            let s_out = if sp.validate().is_ok() {
                SwfPaint::Gradient {
                    fallback: s_fb,
                    paint: sp,
                }
            } else {
                approx_reason.get_or_insert("Singular morph gradient approximated as solid fill.");
                SwfPaint::Solid(s_fb)
            };
            let e_out = if ep.validate().is_ok() {
                SwfPaint::Gradient {
                    fallback: e_fb,
                    paint: ep,
                }
            } else {
                approx_reason.get_or_insert("Singular morph gradient approximated as solid fill.");
                SwfPaint::Solid(e_fb)
            };
            Some((s_out, e_out))
        }
        0x40..=0x43 => {
            let bitmap_id = r.u16()?;
            let sm = r.matrix_px()?;
            let em = r.matrix_px()?;
            let smoothing = matches!(fill_type, 0x40 | 0x41);
            Some((
                SwfPaint::Bitmap {
                    bitmap_id,
                    matrix: sm,
                    smoothing,
                },
                SwfPaint::Bitmap {
                    bitmap_id,
                    matrix: em,
                    smoothing,
                },
            ))
        }
        _ => None,
    }
}

fn read_morph_line_style(
    r: &mut SwfSlice<'_>,
    is_morph2: bool,
    non_round: &mut bool,
    approx_reason: &mut Option<&'static str>,
) -> Option<(SwfStrokeStyle, SwfStrokeStyle)> {
    let sw = r.u16()?;
    let ew = r.u16()?;
    if !is_morph2 {
        let sc = r.rgba()?;
        let ec = r.rgba()?;
        Some((
            SwfStrokeStyle {
                width_px: (f64::from(sw) / 20.0).max(0.5),
                paint: SwfPaint::Solid(sc),
            },
            SwfStrokeStyle {
                width_px: (f64::from(ew) / 20.0).max(0.5),
                paint: SwfPaint::Solid(ec),
            },
        ))
    } else {
        r.align();
        let start_cap = r.ub(2)?;
        let join_style = r.ub(2)?;
        let has_fill = r.bit()?;
        let no_hscale = r.bit()?;
        let no_vscale = r.bit()?;
        let pixel_hinting = r.bit()?;
        let _reserved = r.ub(5)?;
        let _no_close = r.bit()?;
        let end_cap = r.ub(2)?;
        r.align();
        if start_cap != 0
            || end_cap != 0
            || join_style != 0
            || no_hscale
            || no_vscale
            || pixel_hinting
        {
            *non_round = true;
        }
        if join_style == 2 {
            let _miter = r.ufixed8()?;
        }
        let (sp, ep) = if has_fill {
            read_morph_fill_style(r, true, approx_reason)?
        } else {
            (SwfPaint::Solid(r.rgba()?), SwfPaint::Solid(r.rgba()?))
        };
        Some((
            SwfStrokeStyle {
                width_px: (f64::from(sw) / 20.0).max(0.5),
                paint: sp,
            },
            SwfStrokeStyle {
                width_px: (f64::from(ew) / 20.0).max(0.5),
                paint: ep,
            },
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn flush_style_group(
    shape_id: u16,
    elem_seq: &mut usize,
    fills: &[SwfPaint],
    lines: &[SwfStrokeStyle],
    fill_edges: &mut BTreeMap<usize, Vec<TwipEdge>>,
    stroke_edges: &mut BTreeMap<usize, Vec<TwipEdge>>,
    bitmaps: &HashMap<u16, SwfBitmapRef>,
    approx_reason: &mut Option<&'static str>,
    out: &mut Vec<Element>,
) {
    flush_style_group_named(
        &format!("swf-shape-{shape_id}"),
        elem_seq,
        fills,
        lines,
        fill_edges,
        stroke_edges,
        bitmaps,
        approx_reason,
        out,
    );
}

#[allow(clippy::too_many_arguments)]
fn flush_style_group_named(
    prefix: &str,
    elem_seq: &mut usize,
    fills: &[SwfPaint],
    lines: &[SwfStrokeStyle],
    fill_edges: &mut BTreeMap<usize, Vec<TwipEdge>>,
    stroke_edges: &mut BTreeMap<usize, Vec<TwipEdge>>,
    bitmaps: &HashMap<u16, SwfBitmapRef>,
    approx_reason: &mut Option<&'static str>,
    out: &mut Vec<Element>,
) {
    let taken_fills = std::mem::take(fill_edges);
    let taken_strokes = std::mem::take(stroke_edges);

    for (fill_idx, edges) in taken_fills {
        let Some(paint) = fills.get(fill_idx.saturating_sub(1)) else {
            approx_reason.get_or_insert("Dangling SWF fill style index skipped.");
            continue;
        };

        // Fast-path stage bitmap quads compiled by Flash as DefineShape + ClippedBitmap/RepeatingBitmap!
        if let SwfPaint::Bitmap {
            bitmap_id,
            matrix,
            smoothing,
        } = paint
            && let Some(bm) = bitmaps.get(bitmap_id)
            && let Some(bitmap_elem) =
                try_build_bitmap_element(prefix, elem_seq, bm, *matrix, *smoothing, &edges)
        {
            out.push(bitmap_elem);
            continue;
        }

        let (commands, had_open) = assemble_fill_contours(&edges, 1.0 / 20.0);
        if had_open {
            approx_reason
                .get_or_insert("Open SWF fill contour closed automatically to preserve fill.");
        }
        if commands.is_empty() {
            continue;
        }

        let (color, gradient) = match paint {
            SwfPaint::Solid(c) => (*c, None),
            SwfPaint::Gradient { fallback, paint } => (*fallback, Some(paint.clone())),
            SwfPaint::Bitmap { bitmap_id, .. } => {
                approx_reason.get_or_insert(
                    "Non-rectangular or tiled SWF BitmapFill is approximated with the bitmap's average color.",
                );
                let c = bitmaps
                    .get(bitmap_id)
                    .map_or([128, 128, 128, 255], |b| b.avg_color);
                (c, None)
            }
        };

        for chunk in split_valid_command_chunks(commands) {
            let id = format!("{prefix}-el-{elem_seq}");
            *elem_seq += 1;
            let kind = if let Some(grad) = &gradient {
                ElementKind::PaintedPath {
                    commands: chunk,
                    fill_rule: FillRule::NonZero,
                    fill: color,
                    stroke: [0, 0, 0, 0],
                    fill_gradient: Some(grad.clone()),
                    stroke_gradient: None,
                    stroke_width: 0.0,
                }
            } else {
                ElementKind::Path {
                    commands: chunk,
                    fill_rule: FillRule::NonZero,
                    fill: color,
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                }
            };
            out.push(Element {
                id,
                name: String::new(),
                transform: Transform::default(),
                kind,
                scripts: Vec::new(),
                accessibility: None,
            });
        }
    }

    for (line_idx, edges) in taken_strokes {
        let Some(stroke_style) = lines.get(line_idx.saturating_sub(1)) else {
            approx_reason.get_or_insert("Dangling SWF line style index skipped.");
            continue;
        };
        let commands = assemble_stroke_polylines(&edges, 1.0 / 20.0);
        if commands.is_empty() {
            continue;
        }
        let (color, gradient) = match &stroke_style.paint {
            SwfPaint::Solid(c) => (*c, None),
            SwfPaint::Gradient { fallback, paint } => (*fallback, Some(paint.clone())),
            SwfPaint::Bitmap { bitmap_id, .. } => {
                approx_reason
                    .get_or_insert("Bitmap-filled SWF stroke approximated as solid stroke.");
                let c = bitmaps
                    .get(bitmap_id)
                    .map_or([128, 128, 128, 255], |b| b.avg_color);
                (c, None)
            }
        };
        for chunk in split_valid_command_chunks(commands) {
            let id = format!("{prefix}-el-{elem_seq}");
            *elem_seq += 1;
            let kind = if let Some(grad) = &gradient {
                ElementKind::PaintedPath {
                    commands: chunk,
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke: color,
                    fill_gradient: None,
                    stroke_gradient: Some(grad.clone()),
                    stroke_width: stroke_style.width_px,
                }
            } else {
                ElementKind::Path {
                    commands: chunk,
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke: color,
                    stroke_width: stroke_style.width_px,
                }
            };
            out.push(Element {
                id,
                name: String::new(),
                transform: Transform::default(),
                kind,
                scripts: Vec::new(),
                accessibility: None,
            });
        }
    }
}

fn try_build_bitmap_element(
    prefix: &str,
    elem_seq: &mut usize,
    bm: &SwfBitmapRef,
    swf_matrix: [f64; 6],
    smoothing: bool,
    edges: &[TwipEdge],
) -> Option<Element> {
    if edges.len() != 4 || edges.iter().any(|e| e.control.is_some()) {
        return None;
    }
    // SWF bitmap fill matrix maps 1 bitmap pixel -> 20 twips at unit scale (`a = 20, d = 20`),
    // while `swf_matrix[4..6]` is already in pixels (`tx/20, ty/20`).
    let px_matrix = [
        swf_matrix[0] / 20.0,
        swf_matrix[1] / 20.0,
        swf_matrix[2] / 20.0,
        swf_matrix[3] / 20.0,
        swf_matrix[4],
        swf_matrix[5],
    ];
    let det = px_matrix[0] * px_matrix[3] - px_matrix[1] * px_matrix[2];
    if !det.is_finite() || det.abs() < 1e-6 {
        return None;
    }
    let transform = Transform::from_matrix(px_matrix)?;
    let w = f64::from(bm.width);
    let h = f64::from(bm.height);
    let expected_corners = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]].map(|[u, v]| {
        [
            px_matrix[0] * u + px_matrix[2] * v + px_matrix[4],
            px_matrix[1] * u + px_matrix[3] * v + px_matrix[5],
        ]
    });
    // Check that every edge endpoint lies on one of the 4 transformed bitmap corners within 1.5 px.
    let all_corners_match = edges.iter().all(|e| {
        let p = [f64::from(e.start[0]) / 20.0, f64::from(e.start[1]) / 20.0];
        expected_corners
            .iter()
            .any(|c| (c[0] - p[0]).hypot(c[1] - p[1]) <= 1.5)
    });
    if !all_corners_match {
        return None;
    }
    let id = format!("{prefix}-el-{elem_seq}");
    *elem_seq += 1;
    Some(Element {
        id,
        name: String::new(),
        transform,
        kind: ElementKind::Bitmap {
            resource_id: bm.resource_id.clone(),
            width: bm.width,
            height: bm.height,
            smoothing,
        },
        scripts: Vec::new(),
        accessibility: None,
    })
}

fn assemble_fill_contours(edges: &[TwipEdge], scale: f64) -> (Vec<PathCommand>, bool) {
    if edges.is_empty() {
        return (Vec::new(), false);
    }
    let mut outgoing: BTreeMap<[i32; 2], Vec<usize>> = BTreeMap::new();
    for (idx, edge) in edges.iter().enumerate() {
        outgoing.entry(edge.start).or_default().push(idx);
    }
    for list in outgoing.values_mut() {
        list.reverse();
    }
    let mut used = vec![false; edges.len()];
    let mut cmds = Vec::new();
    let mut had_open = false;

    for start_idx in 0..edges.len() {
        if used[start_idx] {
            continue;
        }
        used[start_idx] = true;
        let first = edges[start_idx];
        let origin = first.start;
        cmds.push(PathCommand::MoveTo {
            x: f64::from(origin[0]) * scale,
            y: f64::from(origin[1]) * scale,
        });
        first.append_px(scale, &mut cmds);
        let mut cur = first.end;
        while cur != origin {
            let next_opt = outgoing.get_mut(&cur).and_then(|bucket| {
                while let Some(candidate) = bucket.pop() {
                    if !used[candidate] {
                        return Some(candidate);
                    }
                }
                None
            });
            let Some(next_idx) = next_opt else {
                had_open = true;
                break;
            };
            used[next_idx] = true;
            let next_edge = edges[next_idx];
            next_edge.append_px(scale, &mut cmds);
            cur = next_edge.end;
        }
        cmds.push(PathCommand::Close);
    }
    (cmds, had_open)
}

fn assemble_stroke_polylines(edges: &[TwipEdge], scale: f64) -> Vec<PathCommand> {
    if edges.is_empty() {
        return Vec::new();
    }
    let mut outgoing: BTreeMap<[i32; 2], Vec<usize>> = BTreeMap::new();
    let mut in_deg: BTreeMap<[i32; 2], usize> = BTreeMap::new();
    for (idx, edge) in edges.iter().enumerate() {
        outgoing.entry(edge.start).or_default().push(idx);
        *in_deg.entry(edge.end).or_default() += 1;
        in_deg.entry(edge.start).or_default();
    }
    for list in outgoing.values_mut() {
        list.reverse();
    }
    let mut used = vec![false; edges.len()];
    let mut cmds = Vec::new();

    // First trace chains starting at open endpoints (out_deg > in_deg), then remaining closed loops.
    for pass in 0..2 {
        for start_idx in 0..edges.len() {
            if used[start_idx] {
                continue;
            }
            let s_pt = edges[start_idx].start;
            if pass == 0 {
                let out_d = outgoing.get(&s_pt).map_or(0, Vec::len);
                let in_d = in_deg.get(&s_pt).copied().unwrap_or(0);
                if out_d <= in_d {
                    continue;
                }
            }
            used[start_idx] = true;
            let first = edges[start_idx];
            let origin = first.start;
            cmds.push(PathCommand::MoveTo {
                x: f64::from(origin[0]) * scale,
                y: f64::from(origin[1]) * scale,
            });
            first.append_px(scale, &mut cmds);
            let mut cur = first.end;
            while cur != origin {
                let next_opt = outgoing.get_mut(&cur).and_then(|bucket| {
                    while let Some(candidate) = bucket.pop() {
                        if !used[candidate] {
                            return Some(candidate);
                        }
                    }
                    None
                });
                let Some(next_idx) = next_opt else {
                    break;
                };
                used[next_idx] = true;
                let e = edges[next_idx];
                e.append_px(scale, &mut cmds);
                cur = e.end;
            }
            if cur == origin {
                cmds.push(PathCommand::Close);
            }
        }
    }
    cmds
}

fn split_valid_command_chunks(commands: Vec<PathCommand>) -> Vec<Vec<PathCommand>> {
    if commands.len() <= MAX_PATH_COMMANDS && validate_path_commands(&commands).is_ok() {
        return vec![commands];
    }
    // Split on `MoveTo` contour boundaries so each chunk stays <= MAX_PATH_COMMANDS.
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut contour = Vec::new();
    for cmd in commands {
        if matches!(cmd, PathCommand::MoveTo { .. }) && !contour.is_empty() {
            if current.len() + contour.len() > MAX_PATH_COMMANDS && !current.is_empty() {
                if validate_path_commands(&current).is_ok() {
                    chunks.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
            }
            if contour.len() <= MAX_PATH_COMMANDS {
                current.append(&mut contour);
            } else {
                contour.clear();
            }
        }
        contour.push(cmd);
    }
    if !contour.is_empty() {
        if current.len() + contour.len() > MAX_PATH_COMMANDS && !current.is_empty() {
            if validate_path_commands(&current).is_ok() {
                chunks.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
        if contour.len() <= MAX_PATH_COMMANDS {
            current.extend(contour);
        }
    }
    if !current.is_empty() && validate_path_commands(&current).is_ok() {
        chunks.push(current);
    }
    chunks
}
