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
type Paint = (Color, Option<GradientPaint>, Option<&'static str>);

fn paint(fill: &BinaryFill) -> Paint {
    const STANDIN: Color = [128, 128, 128, 255];
    match &fill.encoding {
        BinaryFillEncoding::Legacy { word: 0 } => (color(fill.packed_value), None, None),
        BinaryFillEncoding::Extended {
            flags,
            secondary_flags: _,
            payload: BinaryFillPayload::Plain,
        } if flags & 0x70 == 0 => (color(fill.packed_value), None, None),
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
                    Some("an unknown gradient kind is shown in its first color"),
                );
            };
            let note = extension
                .as_ref()
                .is_some_and(|e| e.flags & 13 != 0)
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
                focal_point_ratio: extension
                    .as_ref()
                    .map_or(0., |e| (e.focal_point as i16) as f64 / 255.),
            };
            ([0, 0, 0, 255], Some(gradient), note)
        }
        _ => (
            STANDIN,
            None,
            Some("bitmap fills are not rendered yet and are shown as neutral gray"),
        ),
    }
}

pub(super) fn convert_shape(
    shape: &BinaryShapeRecord,
    id: &str,
    report: &mut ImportReport,
    location: &str,
) -> Result<Vec<Element>, String> {
    let transform =
        Transform::from_matrix(matrix(shape.matrix_words)).ok_or("Invalid shape matrix")?;
    let mut output = Vec::new();
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
        let (fill, fill_gradient, note) = paint(fill);
        notes.extend(note.map(str::to_owned));
        output.push(Element {
            id: format!("{id}-fill-{i}"),
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
        if stroke.style_words[0] != 0 || stroke.style_words[1] & 7 != 0 {
            notes.push("dashed/dotted/hatched strokes are drawn solid".into());
        }
        let (stroke_color, stroke_gradient) = if let Some(e) = &stroke.extension {
            let (c, g, note) = paint(&e.paint);
            notes.extend(note.map(str::to_owned));
            (c, g)
        } else {
            (color(stroke.color_word), None)
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
        output.push(Element {
            id: format!("{id}-stroke-{i}"),
            name: format!("Stroke {}", i + 1),
            transform,
            kind: ElementKind::PaintedPath {
                commands: match paths(edges, false) {
                    Ok(commands) => commands,
                    Err(e) => {
                        notes.push(format!("stroke {} skipped: {e}", i + 1));
                        continue;
                    }
                },
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: stroke_color,
                fill_gradient: None,
                stroke_gradient,
                stroke_width: if width == 0. { 1. } else { width },
            },
            scripts: vec![],
            accessibility: None,
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
