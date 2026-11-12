//! Convert directed binary fill edges into compound paths without flattening
//! curves. Archive layout/units are referenced in docs/IMPORTS.md. This graph
//! assembly is independently implemented: all directed edges are consumed once,
//! including separate contours meeting at a vertex; winding is preserved.
use super::*;
use crate::legacy::geometry::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Edge {
    start: [i32; 2],
    end: [i32; 2],
    control: Option<[i32; 2]>,
}
impl Edge {
    fn new(e: &BinaryEdge, reverse: bool) -> Self {
        Self {
            start: if reverse { e.end } else { e.start },
            end: if reverse { e.start } else { e.end },
            control: e.control,
        }
    }
    fn append(&self, commands: &mut Vec<PathCommand>) {
        let [x, y] = point(self.end);
        commands.push(if let Some(c) = self.control {
            let [cx, cy] = point(c);
            PathCommand::QuadraticTo { cx, cy, x, y }
        } else {
            PathCommand::LineTo { x, y }
        });
    }
}
fn point(p: [i32; 2]) -> [f64; 2] {
    [p[0] as f64 / 5120., p[1] as f64 / 5120.]
}
pub(super) fn matrix(w: [i32; 6]) -> [f64; 6] {
    [
        w[0] as f64 / 65536.,
        w[1] as f64 / 65536.,
        w[2] as f64 / 65536.,
        w[3] as f64 / 65536.,
        w[4] as f64 / 20.,
        w[5] as f64 / 20.,
    ]
}
pub(super) fn color(w: u32) -> Color {
    w.to_le_bytes()
}

fn paths(edges: Vec<Edge>, closed: bool) -> Result<Vec<PathCommand>, String> {
    if edges.len() > 100_000 {
        return Err("Shape exceeds edge budget".into());
    }
    let mut outgoing: BTreeMap<[i32; 2], Vec<usize>> = BTreeMap::new();
    let mut balance: BTreeMap<[i32; 2], i32> = BTreeMap::new();
    for (i, e) in edges.iter().enumerate() {
        outgoing.entry(e.start).or_default().push(i);
        *balance.entry(e.start).or_default() += 1;
        *balance.entry(e.end).or_default() -= 1;
    }
    if closed && balance.values().any(|v| *v != 0) {
        return Err(
            "Fill boundary has unmatched endpoints; original edges retained in import source"
                .into(),
        );
    }
    let mut unused: BTreeSet<usize> = (0..edges.len()).collect();
    let mut commands = Vec::new();
    while let Some(&first) = unused.first() {
        let start = edges[first].start;
        let mut stack = vec![(start, None)];
        let mut circuit = Vec::new();
        while let Some((vertex, _)) = stack.last() {
            let next = outgoing.get_mut(vertex).and_then(|ids| {
                while let Some(id) = ids.pop() {
                    if unused.remove(&id) {
                        return Some(id);
                    }
                }
                None
            });
            if let Some(id) = next {
                stack.push((edges[id].end, Some(id)))
            } else {
                if let Some((_, Some(id))) = stack.pop() {
                    circuit.push(id)
                }
            }
        }
        circuit.reverse();
        let mut current = None;
        let mut contour_start = None;
        for id in circuit {
            let e = &edges[id];
            if current != Some(e.start) {
                if closed && current.is_some() && current != contour_start {
                    return Err("Cannot assemble closed fill graph".into());
                }
                let [x, y] = point(e.start);
                commands.push(PathCommand::MoveTo { x, y });
                contour_start = Some(e.start);
            }
            e.append(&mut commands);
            current = Some(e.end);
        }
        if closed {
            if current != contour_start {
                return Err("Unclosed fill boundary".into());
            }
            commands.push(PathCommand::Close);
        }
    }
    Ok(commands)
}

/// A paint and, when it is only approximated, the reason for the report.
type Paint = (
    Color,
    Option<GradientPaint>,
    Option<(u16, [f64; 6], bool, bool)>,
    Option<&'static str>,
);

#[derive(Default)]
pub(super) struct ConvertedShape {
    pub elements: Vec<Element>,
    pub bitmap_fills: Vec<AuthoredBitmapFill>,
    pub appearances: Vec<ElementAppearance>,
    pub vector_effects: Vec<ElementVectorEffects>,
}

fn paint(fill: &BinaryFill, bitmaps: &BTreeMap<u16, super::assets::BitmapAsset>) -> Paint {
    const STANDIN: Color = [128, 128, 128, 255];
    match &fill.encoding {
        BinaryFillEncoding::Legacy { word: 0 } => (color(fill.packed_value), None, None, None),
        BinaryFillEncoding::Extended {
            flags,
            secondary_flags: _,
            payload: BinaryFillPayload::Plain,
        } if flags & 0x70 == 0 => (color(fill.packed_value), None, None, None),
        BinaryFillEncoding::Extended {
            flags,
            payload:
                BinaryFillPayload::Gradient {
                    matrix_words,
                    extension,
                    stops,
                },
            ..
        } => {
            let Some(kind) = (match *flags {
                0x10 => Some(GradientKind::Linear),
                0x12 | 0x13 => Some(GradientKind::Radial),
                _ => None,
            }) else {
                let first = stops.first().map_or(STANDIN, |s| color(s.color_word));
                return (
                    first,
                    None,
                    None,
                    Some("an unknown gradient kind is shown in its first color"),
                );
            };
            let ext_flags = extension.as_ref().map_or(0, |e| e.flags);
            let spread = match (ext_flags >> 2) & 3 {
                1 => GradientSpread::Reflect,
                2 => GradientSpread::Repeat,
                _ => GradientSpread::Extend,
            };
            let linear_rgb = ext_flags & 1 != 0;
            let note = (ext_flags & !13 != 0)
                .then_some("gradient spread/interpolation modes are drawn as pad/RGB");
            let gradient = GradientPaint {
                kind,
                matrix: matrix(*matrix_words),
                stops: stops
                    .iter()
                    .map(|s| GradientStop {
                        ratio: s.ratio,
                        color: color(s.color_word),
                    })
                    .collect(),
                focal_point_ratio: match kind {
                    GradientKind::Radial => extension.as_ref().map_or(0., |e| {
                        ((e.focal_point as i16) as f64 / 255.).clamp(-1., 1.)
                    }),
                    GradientKind::Linear => 0.,
                },
                spread,
                linear_rgb,
            };
            ([0, 0, 0, 255], Some(gradient), None, note)
        }
        BinaryFillEncoding::Extended {
            flags,
            payload:
                BinaryFillPayload::Bitmap {
                    matrix_words,
                    media_id,
                },
            ..
        } if (*flags & 0x70) == 0x40 => {
            let raw = matrix(*matrix_words);
            let fill_matrix = [
                raw[0] / 20.0,
                raw[1] / 20.0,
                raw[2] / 20.0,
                raw[3] / 20.0,
                raw[4],
                raw[5],
            ];
            if bitmaps.contains_key(media_id)
                && fill_matrix.iter().all(|v| v.is_finite() && v.abs() <= 1e9)
            {
                let repeat = (*flags & 1) == 0;
                let smoothing = (*flags & 2) == 0;
                (
                    STANDIN,
                    None,
                    Some((*media_id, fill_matrix, repeat, smoothing)),
                    None,
                )
            } else {
                (
                    STANDIN,
                    None,
                    None,
                    Some("bitmap fills are not rendered yet and are shown as neutral gray"),
                )
            }
        }
        _ => (
            STANDIN,
            None,
            None,
            Some("bitmap fills are not rendered yet and are shown as neutral gray"),
        ),
    }
}

#[cfg(test)]
pub(super) fn convert_shape(
    shape: &BinaryShapeRecord,
    id: &str,
    report: &mut ImportReport,
    location: &str,
) -> Result<Vec<Element>, String> {
    convert_shape_with_effects(shape, id, &BTreeMap::new(), report, location)
        .map(|converted| converted.elements)
}

pub(super) fn convert_shape_with_effects(
    shape: &BinaryShapeRecord,
    id: &str,
    bitmaps: &BTreeMap<u16, super::assets::BitmapAsset>,
    report: &mut ImportReport,
    location: &str,
) -> Result<ConvertedShape, String> {
    let transform =
        Transform::from_matrix(matrix(shape.matrix_words)).ok_or("Invalid shape matrix")?;
    let mut output = ConvertedShape::default();
    // Each paint converts independently; approximations are reported once per shape.
    let mut notes = Vec::new();
    for (i, fill) in shape.fills.iter().enumerate() {
        let index = (i + 1) as u16;
        let edges: Vec<_> = shape
            .edges
            .iter()
            .filter_map(|e| {
                let a = e.styles.fill_0 & 0x7fff;
                let b = e.styles.fill_1 & 0x7fff;
                if a == b {
                    None
                } else if b == index {
                    Some(Edge::new(e, false))
                } else if a == index {
                    Some(Edge::new(e, true))
                } else {
                    None
                }
            })
            .collect();
        if edges.is_empty() {
            continue;
        }
        let commands = match paths(edges, true) {
            Ok(commands) => commands,
            Err(e) => {
                notes.push(format!("fill {} skipped: {e}", i + 1));
                continue;
            }
        };
        let (fill, fill_gradient, bitmap_spec, note) = paint(fill, bitmaps);
        notes.extend(note.map(str::to_owned));
        let elem_id = format!("{id}-fill-{i}");
        if let Some((media_id, fill_matrix, repeat, smoothing)) = bitmap_spec
            && let Some(asset) = bitmaps.get(&media_id)
        {
            output.bitmap_fills.push(AuthoredBitmapFill {
                element_id: elem_id.clone(),
                resource_id: asset.resource_id.clone(),
                matrix: fill_matrix,
                repeat,
                smoothing,
            });
        }
        output.elements.push(Element {
            id: elem_id,
            name: format!("Fill {}", i + 1),
            transform,
            kind: ElementKind::PaintedPath {
                commands,
                fill_rule: FillRule::NonZero,
                fill,
                stroke: [0; 4],
                fill_gradient,
                stroke_gradient: None,
                stroke_width: 0.,
            },
            scripts: vec![],
            accessibility: None,
            ..Element::default()
        });
    }
    for (i, stroke) in shape.strokes.iter().enumerate() {
        let index = (i + 1) as u16;
        let edges: Vec<_> = shape
            .edges
            .iter()
            .filter(|e| e.styles.stroke & 0x7fff == index)
            .map(|e| Edge::new(e, false))
            .collect();
        if edges.is_empty() {
            continue;
        }
        let first_style = stroke.style_words[0];
        let second_style = stroke.style_words[1];
        let stroke_type = if first_style != 0 {
            1
        } else {
            second_style & 7
        };
        if stroke_type >= 6 {
            notes.push("dashed/dotted/hatched strokes are drawn solid".into());
        }
        let (stroke_color, stroke_gradient, stroke_bitmap) = if let Some(e) = &stroke.extension {
            let (c, g, bm, note) = paint(&e.paint, bitmaps);
            notes.extend(note.map(str::to_owned));
            (c, g, bm)
        } else {
            (color(stroke.color_word), None, None)
        };
        if stroke
            .extension
            .as_ref()
            .is_some_and(|e| e.scale_mode != 0 || !matches!((e.cap_style, e.join_style), (1, 1)))
            || stroke.style_words[1] & 0x8000 != 0
        {
            report.warning("fla_stroke_rendering",location,"Stroke cap/join/scale editor settings remain in the embedded source; this renderer uses scaling round strokes.");
        }
        let width = stroke.width_word as f64 / 20.;
        if width == 0. {
            report.warning("fla_hairline_rendering",location,"A device hairline is represented as a one-pixel stroke; the original width remains in source provenance.")
        }
        let stroke_width = if width == 0. { 1. } else { width };
        let commands = match paths(edges, false) {
            Ok(commands) => commands,
            Err(e) => {
                notes.push(format!("stroke {} skipped: {e}", i + 1));
                continue;
            }
        };
        let elem_id = format!("{id}-stroke-{i}");
        if let Some((media_id, fill_matrix, repeat, smoothing)) = stroke_bitmap
            && let Some(asset) = bitmaps.get(&media_id)
        {
            output.bitmap_fills.push(AuthoredBitmapFill {
                element_id: elem_id.clone(),
                resource_id: asset.resource_id.clone(),
                matrix: fill_matrix,
                repeat,
                smoothing,
            });
        }
        match stroke_type {
            1 => {
                let dash = (f64::from(first_style) / 20.0).clamp(0.5, 1000.0);
                let gap = (f64::from(second_style & 0x7fff) / 20.0).clamp(0.5, 1000.0);
                output.appearances.push(ElementAppearance {
                    element_id: elem_id.clone(),
                    entries: vec![AppearancePaintEntry::Stroke {
                        color: stroke_color,
                        gradient: stroke_gradient.clone(),
                        width: stroke_width,
                        swatch_id: None,
                        offset_distance: 0.0,
                        pattern_brush: Some(PatternBrushKind::DashPattern { dash, gap }),
                        opacity: 1.0,
                    }],
                    ..ElementAppearance::default()
                });
            }
            2 => {
                let dash = stroke_width.clamp(0.5, 1000.0);
                let gap = (f64::from(second_style >> 3) / 20.0).clamp(0.5, 1000.0);
                output.appearances.push(ElementAppearance {
                    element_id: elem_id.clone(),
                    entries: vec![AppearancePaintEntry::Stroke {
                        color: stroke_color,
                        gradient: stroke_gradient.clone(),
                        width: stroke_width,
                        swatch_id: None,
                        offset_distance: 0.0,
                        pattern_brush: Some(PatternBrushKind::DashPattern { dash, gap }),
                        opacity: 1.0,
                    }],
                    ..ElementAppearance::default()
                });
            }
            3 => {
                let pattern = (second_style >> 3) & 7;
                let wave_height = (second_style >> 6) & 3;
                let wave_length = (second_style >> 8) & 3;
                output.vector_effects.push(ElementVectorEffects {
                    element_id: elem_id.clone(),
                    procedural_effects: vec![ProceduralVectorEffect::ZigZag {
                        size: match wave_height {
                            0 => 0.5,
                            1 => 2.0,
                            2 => 4.0,
                            _ => 7.0,
                        },
                        ridges_per_segment: match wave_length {
                            0 => 8,
                            1 => 5,
                            2 => 3,
                            _ => 2,
                        },
                        smooth: (pattern & 1) != 0,
                    }],
                    ..ElementVectorEffects::default()
                });
            }
            4 => {
                let dot_size = (second_style >> 3) & 3;
                let variation = (second_style >> 5) & 3;
                let density = (second_style >> 7) & 3;
                output.vector_effects.push(ElementVectorEffects {
                    element_id: elem_id.clone(),
                    procedural_effects: vec![ProceduralVectorEffect::Roughen {
                        size: match dot_size {
                            0 => 0.75,
                            1 => 1.5,
                            2 => 3.0,
                            _ => 5.0,
                        },
                        detail: match density {
                            0 => 8,
                            1 => 6,
                            2 => 4,
                            _ => 2,
                        },
                        seed: u32::from(variation) + 1,
                    }],
                    ..ElementVectorEffects::default()
                });
            }
            5 => {
                let hatch_thickness = (second_style >> 3) & 3;
                let space = (second_style >> 5) & 3;
                let rotate = (second_style >> 7) & 3;
                let jiggle = (second_style >> 9) & 3;
                let curve = (second_style >> 11) & 3;
                let length = (second_style >> 13) & 3;
                output.vector_effects.push(ElementVectorEffects {
                    element_id: elem_id.clone(),
                    procedural_effects: vec![ProceduralVectorEffect::Roughen {
                        size: match length {
                            0 => 1.5,
                            1 => 2.5,
                            2 => 4.0,
                            _ => 6.0,
                        },
                        detail: match space {
                            0 => 8,
                            1 => 6,
                            2 => 4,
                            _ => 2,
                        },
                        seed: u32::from(hatch_thickness)
                            | (u32::from(rotate) << 2)
                            | (u32::from(jiggle) << 4)
                            | (u32::from(curve) << 6),
                    }],
                    ..ElementVectorEffects::default()
                });
            }
            _ => {}
        }
        output.elements.push(Element {
            id: elem_id,
            name: format!("Stroke {}", i + 1),
            transform,
            kind: ElementKind::PaintedPath {
                commands,
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: stroke_color,
                fill_gradient: None,
                stroke_gradient,
                stroke_width,
            },
            scripts: vec![],
            accessibility: None,
            ..Element::default()
        });
    }
    if !notes.is_empty() {
        notes.dedup();
        report.warning(
            "fla_shape_approximated",
            location,
            format!("Shape imported with approximations: {}. The original shape remains in the embedded source.", notes.join("; ")),
        );
    }
    if !shape.auxiliary.is_empty() {
        report.warning("fla_cubic_editing_metadata",location,"Rendered quadratic edges are preserved; auxiliary cubic editing handles remain in source provenance.")
    }
    Ok(output)
}

pub(super) fn try_convert_primitive(
    class: TimelineClass,
    prim: &TimelinePrimitive,
    shape: &BinaryShapeRecord,
) -> Option<(Transform, [f64; 2], ElementKind)> {
    if !shape.auxiliary.is_empty() {
        return None;
    }
    let mut fill_indices = BTreeSet::new();
    let mut stroke_indices = BTreeSet::new();
    for e in &shape.edges {
        let f0 = e.styles.fill_0 & 0x7fff;
        let f1 = e.styles.fill_1 & 0x7fff;
        let st = e.styles.stroke & 0x7fff;
        if f0 != 0 {
            fill_indices.insert(f0);
        }
        if f1 != 0 {
            fill_indices.insert(f1);
        }
        if st != 0 {
            stroke_indices.insert(st);
        }
    }
    if fill_indices.len() > 1 || stroke_indices.len() > 1 {
        return None;
    }
    let fill = if let Some(&idx) = fill_indices.first() {
        let f = shape.fills.get((idx - 1) as usize)?;
        match &f.encoding {
            BinaryFillEncoding::Legacy { word: 0 } => color(f.packed_value),
            BinaryFillEncoding::Extended {
                flags,
                payload: BinaryFillPayload::Plain,
                ..
            } if flags & 0x70 == 0 => color(f.packed_value),
            _ => return None,
        }
    } else {
        [0, 0, 0, 0]
    };
    let (stroke, stroke_width) = if let Some(&idx) = stroke_indices.first() {
        let st = shape.strokes.get((idx - 1) as usize)?;
        if st.style_words[0] != 0 || (st.style_words[1] & 7) != 0 {
            return None;
        }
        let scolor = if let Some(ext) = &st.extension {
            match &ext.paint.encoding {
                BinaryFillEncoding::Legacy { word: 0 } => color(ext.paint.packed_value),
                BinaryFillEncoding::Extended {
                    flags,
                    payload: BinaryFillPayload::Plain,
                    ..
                } if flags & 0x70 == 0 => color(ext.paint.packed_value),
                _ => return None,
            }
        } else {
            color(st.color_word)
        };
        let w = f64::from(st.width_word) / 20.0;
        (scolor, if w == 0.0 { 1.0 } else { w.clamp(0.0, 1e9) })
    } else {
        ([0, 0, 0, 0], 0.0)
    };
    let [xmin, ymin, xmax, ymax] = if class == TimelineClass::Oval {
        let [xmin_tw, xmax_tw, ymin_tw, ymax_tw] = prim.bounds_words?;
        if xmin_tw == i32::MIN {
            return None;
        }
        [
            f64::from(xmin_tw) / 20.0,
            f64::from(ymin_tw) / 20.0,
            f64::from(xmax_tw) / 20.0,
            f64::from(ymax_tw) / 20.0,
        ]
    } else {
        if shape.edges.is_empty() {
            return None;
        }
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for e in &shape.edges {
            for axis in 0..2 {
                let p = f64::from(e.start[axis]) / 5120.0;
                let q = f64::from(e.end[axis]) / 5120.0;
                min[axis] = min[axis].min(p.min(q));
                max[axis] = max[axis].max(p.max(q));
                if let Some(ctrl) = e.control {
                    let c = f64::from(ctrl[axis]) / 5120.0;
                    let denom = p - 2.0 * c + q;
                    if denom.abs() > 1e-12 {
                        let t = (p - c) / denom;
                        if t > 0.0 && t < 1.0 {
                            let u = 1.0 - t;
                            let v = u * u * p + 2.0 * u * t * c + t * t * q;
                            min[axis] = min[axis].min(v);
                            max[axis] = max[axis].max(v);
                        }
                    }
                }
            }
        }
        [min[0], min[1], max[0], max[1]]
    };
    if ![xmin, ymin, xmax, ymax].iter().all(|v| v.is_finite()) || xmax < xmin || ymax < ymin {
        return None;
    }
    let m = matrix(shape.matrix_words);
    let mut transform = Transform::from_matrix(m)?;
    transform.x += m[0] * xmin + m[2] * ymin;
    transform.y += m[1] * xmin + m[3] * ymin;
    if ![transform.x, transform.y]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 1e12)
    {
        return None;
    }
    let width = (xmax - xmin).clamp(0.0, 1e9);
    let height = (ymax - ymin).clamp(0.0, 1e9);
    let kind = if class == TimelineClass::Oval {
        let start_raw = f64::from(prim.start_angle_degrees?);
        let end_raw = f64::from(prim.end_angle_degrees?);
        let inner_raw = f64::from(prim.inner_radius_percent?);
        if ![start_raw, end_raw, inner_raw]
            .iter()
            .all(|v| v.is_finite())
        {
            return None;
        }
        let norm_angle = |a: f64| {
            if (a - 360.0).abs() <= 1e-6 {
                360.0
            } else {
                a.rem_euclid(360.0)
            }
        };
        let opts = OvalOptions {
            start_angle: norm_angle(start_raw),
            end_angle: norm_angle(end_raw),
            inner_radius: inner_raw.clamp(0.0, 99.0),
            close_path: (prim.flags.unwrap_or(1) & 1) != 0,
        };
        if !opts.is_default() {
            transform.primitive = Some(PrimitiveOptions::Oval(opts));
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        }
    } else {
        let radii = prim.corner_radii?;
        let corner_radii = radii.map(|r| (f64::from(r) / 20.0).clamp(0.0, 1e9));
        let opts = RectangleOptions {
            corner_radii,
            corners_locked: (prim.flags.unwrap_or(1) & 1) != 0,
        };
        if !opts.is_default() {
            transform.primitive = Some(PrimitiveOptions::Rectangle(opts));
        }
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        }
    };
    Some((transform, [xmin, ymin], kind))
}
