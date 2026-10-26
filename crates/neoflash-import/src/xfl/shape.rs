//! Bounded XFL straight-contour conversion. No edge/style is silently dropped.
//!
//! XFL syntax/default references (independent implementation, no copied code):
//! https://github.com/jindrapetrik/xfl-schema/blob/master/xfl.xsd
//! https://github.com/PluieElectrique/xfl2svg/blob/33304eff3c06be6e4a0637b7dbee04b3e37a47b9/xfl2svg/shape/edge.py
//! The corpus frame-labels companion SWF independently confirms its omitted
//! stroke defaults: 20 twips, opaque black, round caps/joins, normal scaling.

use super::{Parser, child, elements};
use crate::Severity;
use neoflash_core::{Color, Element, ElementKind, FillRule, PathCommand};
use roxmltree::Node;
use std::collections::{BTreeMap, BTreeSet};

const MAX_EDGES: usize = 512;
const MAX_EDGE_TEXT: usize = 65_536;
type Point = [i32; 2];
type Segment = (Point, Point);
type Result<T> = std::result::Result<T, ShapeError>;

struct ShapeError(&'static str, &'static str);

impl Parser<'_> {
    pub(super) fn shape(&mut self, node: Node<'_, '_>, id: String, file: &str) -> Option<Element> {
        let first_diagnostic = self.report.diagnostics.len();
        self.audit(
            node,
            file,
            &[
                "name",
                "selected",
                "referenceID",
                "isDrawingObject",
                "isFloating",
            ],
            &["matrix", "transformationPoint", "fills", "strokes", "edges"],
        );
        let transform = self.transform(node, file);
        let result = self.solid_polygon(node, file);
        let kind = match result {
            Ok(kind) => kind,
            Err(ShapeError(code, message)) => {
                self.report.error(code, self.location(node, file), message);
                return None;
            }
        };
        if self.report.diagnostics[first_diagnostic..]
            .iter()
            .any(|d| d.severity == Severity::Error)
        {
            return None;
        }
        Some(Element {
            id,
            name: node.attribute("name").unwrap_or("DOMShape").into(),
            accessibility: None,
            transform,
            kind,
            scripts: vec![],
        })
    }

    fn solid_polygon(&mut self, node: Node<'_, '_>, file: &str) -> Result<ElementKind> {
        if self.boolean(node, "isDrawingObject", false, file)
            || self.boolean(node, "isFloating", false, file)
        {
            return Err(ShapeError(
                "xfl_shape_object_mode",
                "Floating/drawing-object shape authoring semantics are not supported yet.",
            ));
        }
        let fills = child(node, "fills").ok_or(ShapeError(
            "xfl_shape_fill",
            "A polygon requires exactly one solid fill; unfilled shapes need a stroke-only adapter.",
        ))?;
        self.audit(fills, file, &[], &["FillStyle"]);
        let fill_styles: Vec<_> = elements(fills)
            .filter(|n| n.has_tag_name("FillStyle"))
            .collect();
        if fill_styles.len() != 1 {
            return Err(ShapeError(
                "xfl_shape_fill",
                "Exactly one fill style is supported; multiple/empty fill tables are not discarded.",
            ));
        }
        let fill_style = fill_styles[0];
        self.audit(fill_style, file, &["index"], &["SolidColor"]);
        let fill_index = style_index(fill_style, "index", false)?;
        let fill = self.shape_color(fill_style, file)?;

        let mut stroke_index = 0;
        let mut stroke = [0; 4];
        let mut stroke_width = 0.;
        if let Some(strokes) = child(node, "strokes") {
            self.audit(strokes, file, &[], &["StrokeStyle"]);
            let styles: Vec<_> = elements(strokes)
                .filter(|n| n.has_tag_name("StrokeStyle"))
                .collect();
            if styles.len() > 1 {
                return Err(ShapeError(
                    "xfl_shape_stroke",
                    "At most one solid stroke is supported; multiple stroke styles are not discarded.",
                ));
            }
            if let Some(style) = styles.first().copied() {
                self.audit(style, file, &["index"], &["SolidStroke"]);
                stroke_index = style_index(style, "index", false)?;
                let solid = child(style, "SolidStroke").ok_or(ShapeError(
                    "xfl_shape_stroke",
                    "Only a plain solid round stroke is supported.",
                ))?;
                self.audit(
                    solid,
                    file,
                    &[
                        "scaleMode",
                        "weight",
                        "caps",
                        "joints",
                        "pixelHinting",
                        "sharpCorners",
                    ],
                    &["fill"],
                );
                if solid.attribute("scaleMode") != Some("normal")
                    || solid.attribute("caps").unwrap_or("round") != "round"
                    || solid.attribute("joints").unwrap_or("round") != "round"
                    || self.boolean(solid, "pixelHinting", false, file)
                    || self.boolean(solid, "sharpCorners", false, file)
                {
                    return Err(ShapeError(
                        "xfl_shape_stroke",
                        "Only scaleMode=normal, round caps/joins and no pixel hinting/sharp corners are supported.",
                    ));
                }
                stroke_width = self.number(solid, "weight", 1., file);
                if stroke_width <= 0. || stroke_width > 100_000. {
                    return Err(ShapeError(
                        "xfl_shape_stroke",
                        "Stroke weight must be positive and <=100000 pixels; hairline/non-scaling strokes need a separate adapter.",
                    ));
                }
                let paint = child(solid, "fill").ok_or(ShapeError(
                    "xfl_shape_stroke",
                    "SolidStroke is missing its fill paint.",
                ))?;
                self.audit(paint, file, &[], &["SolidColor"]);
                stroke = self.shape_color(paint, file)?;
            }
        }

        let edges_node = child(node, "edges").ok_or(ShapeError(
            "xfl_shape_edges",
            "Shape has no edges container.",
        ))?;
        self.audit(edges_node, file, &[], &["Edge"]);
        let mut segments = vec![];
        let mut text_bytes = 0usize;
        let mut selected = false;
        for edge in elements(edges_node).filter(|n| n.has_tag_name("Edge")) {
            self.audit(
                edge,
                file,
                &["fillStyle0", "fillStyle1", "strokeStyle", "edges"],
                &[],
            );
            let left = style_index(edge, "fillStyle0", true)?;
            let right = style_index(edge, "fillStyle1", true)?;
            let line = style_index(edge, "strokeStyle", true)?;
            if line != stroke_index {
                return Err(ShapeError(
                    "xfl_shape_stroke_topology",
                    "Every boundary edge must use the declared stroke, or all must be unstroked. Partial/missing stroke references are unsupported.",
                ));
            }
            let reverse = match (left, right) {
                (0, value) if value == fill_index => false,
                (value, 0) if value == fill_index => true,
                _ => {
                    return Err(ShapeError(
                        "xfl_shape_style_reference",
                        "Each edge must have exactly one painted side referring to the single fill. Missing fills, unpainted edges and interior seams are unsupported.",
                    ));
                }
            };
            let text = edge.attribute("edges").ok_or(ShapeError(
                "xfl_shape_edges",
                "Edge has no edge command string.",
            ))?;
            text_bytes = text_bytes.checked_add(text.len()).ok_or(ShapeError(
                "xfl_shape_limit",
                "Shape edge text exceeds 65536 bytes.",
            ))?;
            if text_bytes > MAX_EDGE_TEXT {
                return Err(ShapeError(
                    "xfl_shape_limit",
                    "Shape edge text exceeds 65536 bytes.",
                ));
            }
            let (parsed, has_selection) = parse_edges(text, MAX_EDGES - segments.len())?;
            selected |= has_selection;
            segments.extend(
                parsed
                    .into_iter()
                    .map(|(a, b)| if reverse { (b, a) } else { (a, b) }),
            );
        }
        let polygon = convex_cycle(&segments)?;
        let mut commands = vec![PathCommand::MoveTo {
            x: pixel(polygon[0][0]),
            y: pixel(polygon[0][1]),
        }];
        commands.extend(polygon[1..].iter().map(|p| PathCommand::LineTo {
            x: pixel(p[0]),
            y: pixel(p[1]),
        }));
        commands.push(PathCommand::Close);
        if selected {
            self.report.warning("xfl_shape_selection", self.location(node, file), "XFL edge selection bits are editor metadata and are not retained; painted geometry and stroke are retained.");
        }
        Ok(ElementKind::Path {
            commands,
            fill_rule: FillRule::NonZero,
            fill,
            stroke,
            stroke_width,
        })
    }

    fn shape_color(&mut self, wrapper: Node<'_, '_>, file: &str) -> Result<Color> {
        let colors: Vec<_> = elements(wrapper)
            .filter(|n| n.has_tag_name("SolidColor"))
            .collect();
        if colors.len() != 1 {
            return Err(ShapeError(
                "xfl_shape_paint",
                "Exactly one SolidColor is required; gradients, bitmaps and missing paints are unsupported.",
            ));
        }
        let solid = colors[0];
        self.audit(solid, file, &["color", "alpha"], &[]);
        let alpha = self.number(solid, "alpha", 1., file);
        Ok(self.color(
            solid.attribute("color").unwrap_or("#000000"),
            alpha,
            &self.location(solid, file),
        ))
    }
}

fn pixel(twips: i32) -> f64 {
    f64::from(twips) / 20.
}

fn style_index(node: Node<'_, '_>, name: &str, allow_absent: bool) -> Result<u32> {
    let value = match node.attribute(name) {
        None if allow_absent => return Ok(0),
        Some(value) => value,
        None => {
            return Err(ShapeError(
                "xfl_shape_style_index",
                "Style declarations require a positive integer index.",
            ));
        }
    };
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
        return Err(ShapeError(
            "xfl_shape_style_index",
            "Style indices must be unsigned decimal integers.",
        ));
    }
    let index = value.parse::<u32>().map_err(|_| {
        ShapeError(
            "xfl_shape_style_index",
            "Style index exceeds its unsigned 32-bit range.",
        )
    })?;
    if index == 0 && !allow_absent {
        return Err(ShapeError(
            "xfl_shape_style_index",
            "Style declaration index zero means no style and is invalid.",
        ));
    }
    Ok(index)
}

fn parse_edges(text: &str, limit: usize) -> Result<(Vec<Segment>, bool)> {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut current = None;
    let mut pending_move = false;
    let mut selected = false;
    let mut result = vec![];
    while {
        whitespace(bytes, &mut cursor);
        cursor < bytes.len()
    } {
        let command = bytes[cursor];
        cursor += 1;
        match command {
            b'!' => {
                if pending_move {
                    return Err(ShapeError(
                        "xfl_shape_edges",
                        "A move command has no following edge.",
                    ));
                }
                current = Some([
                    coordinate(bytes, &mut cursor)?,
                    coordinate(bytes, &mut cursor)?,
                ]);
                pending_move = true;
                whitespace(bytes, &mut cursor);
                if bytes.get(cursor) == Some(&b'S') {
                    cursor += 1;
                    match bytes.get(cursor) {
                        Some(b'1'..=b'7') => {
                            cursor += 1;
                            selected = true;
                        }
                        _ => {
                            return Err(ShapeError(
                                "xfl_shape_selection",
                                "Selection metadata must be S1 through S7 immediately after a move.",
                            ));
                        }
                    }
                    if bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                        return Err(ShapeError(
                            "xfl_shape_selection",
                            "Selection metadata is a three-bit mask, not a style index.",
                        ));
                    }
                }
            }
            b'|' | b'/' => {
                let start = current.ok_or(ShapeError(
                    "xfl_shape_edges",
                    "Line command must follow an explicit move.",
                ))?;
                let end = [
                    coordinate(bytes, &mut cursor)?,
                    coordinate(bytes, &mut cursor)?,
                ];
                if result.len() == limit {
                    return Err(ShapeError(
                        "xfl_shape_limit",
                        "Shape exceeds 512 straight edges; simplify the source contour.",
                    ));
                }
                result.push((start, end));
                current = Some(end);
                pending_move = false;
            }
            b'[' | b']' | b'(' | b')' => {
                return Err(ShapeError(
                    "xfl_shape_curve_unsupported",
                    "Quadratic/cubic XFL edges are not converted by the straight convex contour adapter.",
                ));
            }
            _ => {
                return Err(ShapeError(
                    "xfl_shape_edge_syntax",
                    "Unknown edge token; only ! moves, | or / lines and move-selection metadata are supported.",
                ));
            }
        }
    }
    if pending_move || result.is_empty() {
        return Err(ShapeError(
            "xfl_shape_edges",
            "An edge command string must contain drawing segments and cannot end with an unused move.",
        ));
    }
    Ok((result, selected))
}

fn whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes.get(*cursor).is_some_and(u8::is_ascii_whitespace) {
        *cursor += 1;
    }
}

fn coordinate(bytes: &[u8], cursor: &mut usize) -> Result<i32> {
    whitespace(bytes, cursor);
    let begin = *cursor;
    if bytes.get(*cursor) == Some(&b'-') {
        *cursor += 1;
    }
    let digits = *cursor;
    while bytes.get(*cursor).is_some_and(u8::is_ascii_digit) {
        *cursor += 1;
    }
    if *cursor == digits
        || bytes
            .get(*cursor)
            .is_some_and(|c| matches!(c, b'.' | b'#' | b'e' | b'E'))
    {
        return Err(ShapeError(
            "xfl_shape_coordinate",
            "Only signed 32-bit integer twip coordinates are supported; fractional, hexadecimal and exponent forms require another adapter.",
        ));
    }
    std::str::from_utf8(&bytes[begin..*cursor])
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or(ShapeError(
            "xfl_shape_coordinate",
            "Coordinate exceeds the signed 32-bit twip range.",
        ))
}

fn convex_cycle(segments: &[Segment]) -> Result<Vec<Point>> {
    if segments.len() < 3 {
        return Err(ShapeError(
            "xfl_shape_topology",
            "A filled polygon requires at least three nondegenerate edges.",
        ));
    }
    let mut outgoing = BTreeMap::new();
    let mut incoming = BTreeSet::new();
    for &(a, b) in segments {
        if a == b || outgoing.insert(a, b).is_some() || !incoming.insert(b) {
            return Err(ShapeError(
                "xfl_shape_topology",
                "Zero-length, duplicate or branching edges cannot form the supported simple contour.",
            ));
        }
    }
    let first = segments[0].0;
    let mut current = first;
    let mut visited = BTreeSet::new();
    let mut points = vec![];
    for _ in segments {
        if !visited.insert(current) {
            return Err(ShapeError(
                "xfl_shape_topology",
                "Multiple contours/holes are not supported; no contour is discarded.",
            ));
        }
        points.push(current);
        current = *outgoing.get(&current).ok_or(ShapeError(
            "xfl_shape_topology",
            "Contour is open or has mismatched endpoint coordinates.",
        ))?;
    }
    if current != first {
        return Err(ShapeError(
            "xfl_shape_topology",
            "Contour is not exactly closed; endpoints are never snapped.",
        ));
    }
    // Every other vertex must lie strictly on the same side of every edge.
    // Unlike a same-sign-turn check, this also excludes self-intersecting stars.
    let cross = |a: Point, b: Point, p: Point| {
        let [ax, ay] = a.map(i128::from);
        let [bx, by] = b.map(i128::from);
        let [px, py] = p.map(i128::from);
        (bx - ax) * (py - ay) - (by - ay) * (px - ax)
    };
    let sign = cross(points[0], points[1], points[2]).signum();
    if sign == 0 {
        return Err(ShapeError(
            "xfl_shape_convexity",
            "Degenerate/collinear contours are unsupported.",
        ));
    }
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        for &p in &points {
            if p != a && p != b && cross(a, b, p).signum() != sign {
                return Err(ShapeError(
                    "xfl_shape_convexity",
                    "Only strictly convex simple polygons are supported; concave, intersecting or collinear boundaries need a general contour adapter.",
                ));
            }
        }
    }
    Ok(points)
}
