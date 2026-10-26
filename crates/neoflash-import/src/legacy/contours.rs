//! Opt-in rendering adapter over preserved records, independent of byte parsing.
//! Unknown semantics are errors, never approximated paint or repaired topology.
mod certificate;

use super::geometry::*;
use neoflash_core::{ElementKind, FillRule, PathCommand, validate_path_commands};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_BINARY_CONTOUR_EDGES: usize = 16_384;
const MAX_WORK: usize = 200_000;
type Point = [i32; 2];

/// The save target is external to a shape record and must not be guessed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BinaryContourContext {
    pub contents_revision: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryContourEdgeRef {
    pub edge_index: usize,
    pub reversed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryContourTrace {
    pub edges: Vec<BinaryContourEdgeRef>,
}

/// Exactly one fill-only core Path. Its affine matrix is baked into all points;
/// place it with an identity Transform. Selection and internal-edge provenance
/// are retained separately; this is not an editable-document conversion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BinarySolidPath {
    pub path: ElementKind,
    pub matrix_words: [i32; 6],
    pub contours: Vec<BinaryContourTrace>,
    pub selected_edges: Vec<usize>,
    pub internal_edges: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("{code}: {message}")]
pub struct BinaryContourError {
    pub code: String,
    pub message: String,
    pub edge_index: Option<usize>,
}
type Result<T> = std::result::Result<T, BinaryContourError>;
fn error(code: &str, message: &str) -> BinaryContourError {
    BinaryContourError {
        code: code.into(),
        message: message.into(),
        edge_index: None,
    }
}
fn at_edge(mut error: BinaryContourError, index: usize) -> BinaryContourError {
    error.edge_index = Some(index);
    error
}

#[derive(Clone, Copy)]
struct Edge {
    start: Point,
    control: Option<Point>,
    end: Point,
    original: usize,
    reversed: bool,
}
struct Work(usize);
impl Work {
    fn charge(&mut self) -> Result<()> {
        self.0 = self.0.checked_sub(1).ok_or_else(|| {
            error(
                "fla_contour_work_limit",
                "Contour certification exceeds 200000 work steps; no approximate result returned.",
            )
        })?;
        Ok(())
    }
}

/// Convert only F3-target shape revision 2/data revision 4, one opaque plain
/// solid fill, no strokes/auxiliary data. Exact directed endpoint graphs must be
/// closed and unbranched. Intersections, uncertain curve separation and winding
/// disagreement are blocked. Nonzero/evenodd must describe the same region.
///
/// This API also validates redundant fields in caller-constructed/deserialized
/// IR. It neither reparses original_bytes nor changes ordinary inspect/import.
pub fn adapt_binary_solid_shape(
    shape: &BinaryShapeRecord,
    context: BinaryContourContext,
) -> Result<BinarySolidPath> {
    if context.contents_revision != 0x14 || shape.shape_revision != 2 || shape.data_revision != 4 {
        return Err(error(
            "fla_contour_revision_unsupported",
            "Only Flash 3 Contents 0x14, shape revision 2/data revision 4 are certified.",
        ));
    }
    if shape.edges.len() > MAX_BINARY_CONTOUR_EDGES || shape.original_bytes.len() > 8 * 1024 * 1024
    {
        return Err(error(
            "fla_contour_size_limit",
            "Adapter limit is 16384 edges and 8 MiB original bytes.",
        ));
    }
    if !shape.strokes.is_empty() || !shape.auxiliary.is_empty() {
        return Err(error(
            "fla_contour_feature_unsupported",
            "Stroke tables and auxiliary curves need separate adapters.",
        ));
    }
    if shape.fills.len() != 1 {
        return Err(error(
            "fla_contour_paint_unsupported",
            "Exactly one fill is supported; empty or multiple paint tables are not discarded.",
        ));
    }
    let fill = &shape.fills[0];
    if !matches!(
        fill.encoding,
        BinaryFillEncoding::Extended {
            flags: 0,
            secondary_flags: 0,
            payload: BinaryFillPayload::Plain
        }
    ) || fill.packed_value >> 24 != 255
    {
        return Err(error(
            "fla_contour_paint_unsupported",
            "Only explicitly opaque plain solid RGBA with zero paint flags is certified.",
        ));
    }
    let [a, b, c, d, _, _] = shape.matrix_words.map(i128::from);
    if a * d - b * c == 0 {
        return Err(error(
            "fla_contour_transform_unsupported",
            "Singular shape transforms are not converted.",
        ));
    }
    let mut edges = Vec::with_capacity(shape.edges.len());
    let mut selected_edges = vec![];
    let mut internal_edges = vec![];
    let mut previous = [0; 2];
    let mut previous_styles = BinaryContourStyles::default();
    for (index, edge) in shape.edges.iter().enumerate() {
        validate_edge(edge, previous, previous_styles).map_err(|e| at_edge(e, index))?;
        previous = edge.end;
        previous_styles = edge.styles;
        if edge.styles.stroke != 0
            || edge.styles.fill_0 & 0x7fff > 1
            || edge.styles.fill_1 & 0x7fff > 1
        {
            return Err(at_edge(
                error(
                    "fla_contour_style_index",
                    "Edge has a stroke or missing fill index.",
                ),
                index,
            ));
        }
        if edge.styles.fill_0 & 0x8000 != 0 || edge.styles.fill_1 & 0x8000 != 0 {
            selected_edges.push(index);
        }
        match (edge.styles.fill_0 & 0x7fff, edge.styles.fill_1 & 0x7fff) {
            (1, 1) => internal_edges.push(index),
            (0, 1) => edges.push(Edge {
                start: edge.start,
                control: edge.control,
                end: edge.end,
                original: index,
                reversed: false,
            }),
            (1, 0) => edges.push(Edge {
                start: edge.end,
                control: edge.control,
                end: edge.start,
                original: index,
                reversed: true,
            }),
            _ => {
                return Err(at_edge(
                    error(
                        "fla_contour_unpainted_edge",
                        "An edge with no painted side is not silently discarded.",
                    ),
                    index,
                ));
            }
        }
    }
    if edges.is_empty() {
        return Err(error(
            "fla_contour_empty_boundary",
            "No supported painted boundary remains.",
        ));
    }
    let cycles = cycles(&edges)?;
    let mut work = Work(MAX_WORK);
    certificate::certify(&edges, &cycles, &mut work)?;
    certify_float_precision(&edges, &cycles, shape.matrix_words)?;
    let mut commands = vec![];
    let mut contours = vec![];
    for cycle in cycles {
        let [x, y] = transform(edges[cycle[0]].start, shape.matrix_words)?;
        commands.push(PathCommand::MoveTo { x, y });
        let mut trace = vec![];
        for index in cycle {
            let edge = edges[index];
            let [x, y] = transform(edge.end, shape.matrix_words)?;
            commands.push(if let Some(control) = edge.control {
                let [cx, cy] = transform(control, shape.matrix_words)?;
                PathCommand::QuadraticTo { cx, cy, x, y }
            } else {
                PathCommand::LineTo { x, y }
            });
            trace.push(BinaryContourEdgeRef {
                edge_index: edge.original,
                reversed: edge.reversed,
            });
        }
        commands.push(PathCommand::Close);
        contours.push(BinaryContourTrace { edges: trace });
    }
    validate_path_commands(&commands).map_err(|_| {
        error(
            "fla_contour_core_limit",
            "Converted path exceeds core command/coordinate limits.",
        )
    })?;
    let color = fill.packed_value.to_le_bytes();
    Ok(BinarySolidPath {
        path: ElementKind::Path {
            commands,
            fill_rule: FillRule::EvenOdd,
            fill: color,
            stroke: [0; 4],
            stroke_width: 0.,
        },
        matrix_words: shape.matrix_words,
        contours,
        selected_edges,
        internal_edges,
    })
}

fn certify_float_precision(edges: &[Edge], cycles: &[Vec<usize>], matrix: [i32; 6]) -> Result<()> {
    let precision = || {
        error(
            "fla_contour_precision_unsupported",
            "Affine baking collapses distinct points or oriented geometry at core f64 precision.",
        )
    };
    let mut mapped = BTreeMap::new();
    for edge in edges {
        for point in [Some(edge.start), edge.control, Some(edge.end)]
            .into_iter()
            .flatten()
        {
            let value = transform(point, matrix)?;
            let key = value.map(|n| if n == 0. { 0 } else { n.to_bits() });
            if let Some(previous) = mapped.insert(key, point)
                && previous != point
            {
                return Err(at_edge(precision(), edge.original));
            }
        }
    }
    let [a, b, c, d, _, _] = matrix.map(i128::from);
    let sign = (a * d - b * c).signum();
    let orientation = |a: Point, b: Point, c: Point| -> Result<()> {
        let [ax, ay] = a.map(i128::from);
        let [bx, by] = b.map(i128::from);
        let [cx, cy] = c.map(i128::from);
        let expected = ((bx - ax) * (cy - ay) - (by - ay) * (cx - ax)).signum() * sign;
        if expected != 0 {
            let [ax, ay] = transform(a, matrix)?;
            let [bx, by] = transform(b, matrix)?;
            let [cx, cy] = transform(c, matrix)?;
            let actual = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
            if actual == 0. || actual.signum() != expected as f64 {
                return Err(precision());
            }
        }
        Ok(())
    };
    for edge in edges {
        if let Some(control) = edge.control {
            orientation(edge.start, control, edge.end).map_err(|e| at_edge(e, edge.original))?;
        }
    }
    for cycle in cycles {
        for (position, &index) in cycle.iter().enumerate() {
            let previous = edges[cycle[(position + cycle.len() - 1) % cycle.len()]];
            orientation(previous.start, edges[index].start, edges[index].end)
                .map_err(|e| at_edge(e, edges[index].original))?;
        }
    }
    Ok(())
}

fn transform(point: Point, words: [i32; 6]) -> Result<[f64; 2]> {
    // One exact i128 numerator per coordinate, followed by one f64 division.
    // Edge words are 1/256 twip, linear matrix words 16.16, translation twips.
    let [a, b, c, d, tx, ty] = words.map(i128::from);
    let [x, y] = point.map(i128::from);
    let result = [
        (a * x + c * y + tx * 256 * 65536) as f64 / 335_544_320.,
        (b * x + d * y + ty * 256 * 65536) as f64 / 335_544_320.,
    ];
    if result.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
        return Err(error(
            "fla_contour_core_limit",
            "Transformed coordinate exceeds core's finite +/-1e9 limit.",
        ));
    }
    Ok(result)
}

fn validate_edge(edge: &BinaryEdge, previous: Point, styles: BinaryContourStyles) -> Result<()> {
    let invalid = || {
        error(
            "fla_contour_inconsistent_ir",
            "Edge encoding, deltas, absolute points or inherited styles disagree.",
        )
    };
    if edge.encoding_flags == 0 || edge.line_flags.is_some() {
        return Err(invalid());
    }
    let modes = [
        edge.encoding_flags & 3,
        (edge.encoding_flags >> 2) & 3,
        (edge.encoding_flags >> 4) & 3,
    ];
    for (mode, delta) in
        modes
            .into_iter()
            .zip([edge.start_delta, edge.control_delta, edge.end_delta])
    {
        if !delta.iter().all(|&v| match mode {
            0 => v == 0,
            1 => i16::try_from(v).is_ok(),
            2 => true,
            3 => v % 128 == 0 && i16::try_from(v / 128).is_ok(),
            _ => unreachable!(),
        }) {
            return Err(invalid());
        }
    }
    if edge.encoding_flags & 0x40 == 0 {
        if edge.styles != styles {
            return Err(invalid());
        }
    } else if edge.encoding_flags & 0x80 != 0
        && [edge.styles.stroke, edge.styles.fill_0, edge.styles.fill_1]
            .iter()
            .any(|&v| v > 255)
    {
        return Err(invalid());
    }
    let sum = |a: Point, b: Point| -> Result<Point> {
        let overflow = || {
            error(
                "fla_contour_wrapped_coordinate",
                "Wrapped archive coordinates are preserved but not converted.",
            )
        };
        Ok([
            a[0].checked_add(b[0]).ok_or_else(overflow)?,
            a[1].checked_add(b[1]).ok_or_else(overflow)?,
        ])
    };
    if sum(previous, edge.start_delta)? != edge.start
        || sum(edge.start, edge.end_delta)? != edge.end
    {
        return Err(invalid());
    }
    let control = if modes[1] != 0 {
        Some(sum(edge.start, edge.control_delta)?)
    } else {
        None
    };
    if control != edge.control {
        return Err(invalid());
    }
    Ok(())
}

fn cycles(edges: &[Edge]) -> Result<Vec<Vec<usize>>> {
    let mut outgoing = BTreeMap::new();
    let mut incoming = BTreeMap::new();
    for (index, edge) in edges.iter().enumerate() {
        if edge.start == edge.end {
            return Err(at_edge(
                error(
                    "fla_contour_degenerate",
                    "Zero-length or retracing one-edge contours are unsupported.",
                ),
                edge.original,
            ));
        }
        if outgoing.insert(edge.start, index).is_some()
            || incoming.insert(edge.end, index).is_some()
        {
            return Err(at_edge(
                error(
                    "fla_contour_branch",
                    "A fill boundary has more than one incoming or outgoing edge at an exact endpoint.",
                ),
                edge.original,
            ));
        }
    }
    if outgoing.keys().ne(incoming.keys()) {
        return Err(error(
            "fla_contour_open",
            "Fill edges do not form exact closed boundaries; no endpoint snapping performed.",
        ));
    }
    let mut seen = vec![false; edges.len()];
    let mut result = vec![];
    // Ordered endpoint map makes contour starts/order independent of edge order.
    for (&start, &first) in &outgoing {
        if seen[first] {
            continue;
        }
        let mut cycle = vec![];
        let mut index = first;
        loop {
            if seen[index] {
                return Err(error(
                    "fla_contour_graph",
                    "Unexpected repeated edge in a contour.",
                ));
            }
            seen[index] = true;
            cycle.push(index);
            let end = edges[index].end;
            if end == start {
                break;
            }
            index = outgoing[&end];
        }
        result.push(cycle);
    }
    Ok(result)
}
