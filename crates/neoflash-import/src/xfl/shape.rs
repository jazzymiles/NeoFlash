//! XFL vector shape conversion: fast-path `ElementKind::Path` for simple solid
//! polygons/strokes, and general 24.8 fixed-point contour & gradient conversion
//! into `ElementKind::PaintedPath` elements. No edge or style is silently dropped.
//!
//! XFL syntax/default references (independent implementation, no copied code):
//! https://github.com/jindrapetrik/xfl-schema/blob/master/xfl.xsd
//! https://github.com/PluieElectrique/xfl2svg/blob/33304eff3c06be6e4a0637b7dbee04b3e37a47b9/xfl2svg/shape/edge.py
//! The corpus frame-labels companion SWF independently confirms its omitted
//! stroke defaults: 20 twips, opaque black, round caps/joins, normal scaling.

use super::{Parser, child, elements};
use crate::Severity;
use neoflash_core::{
    Color, Element, ElementKind, FillRule, GradientKind, GradientPaint, GradientStop, PathCommand,
    validate_path_commands,
};
use roxmltree::Node;
use std::collections::{BTreeMap, BTreeSet};

const MAX_FAST_EDGES: usize = 512;
const MAX_FAST_EDGE_TEXT: usize = 65_536;
const MAX_GENERAL_SEGMENTS: usize = 16_384;
const MAX_SINGLE_EDGE_TEXT: usize = 65_535;
const MAX_SHAPE_EDGE_TEXT: usize = 4 * 1024 * 1024;

type Point = [i32; 2];
type Segment = (Point, Point);
type Point24_8 = [i64; 2];
type Result<T> = std::result::Result<T, ShapeError>;

#[derive(Clone, Copy)]
enum StrokeEdge {
    Line(Point, Point),
    Quad(Point, Point, Point),
}
impl StrokeEdge {
    fn endpoints(self) -> (Point, Point) {
        match self {
            Self::Line(a, b) | Self::Quad(a, _, b) => (a, b),
        }
    }
    fn reversed(self) -> Self {
        match self {
            Self::Line(a, b) => Self::Line(b, a),
            Self::Quad(a, c, b) => Self::Quad(b, c, a),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct GenEdge {
    start: Point24_8,
    end: Point24_8,
    control: Option<Point24_8>,
}

impl GenEdge {
    fn is_zero_length(self) -> bool {
        match self.control {
            None => self.start == self.end,
            Some(ctrl) => self.start == self.end && self.start == ctrl,
        }
    }

    fn reversed(self) -> Self {
        Self {
            start: self.end,
            end: self.start,
            control: self.control,
        }
    }

    fn append(self, commands: &mut Vec<PathCommand>) {
        let [x, y] = point_24_8(self.end);
        match self.control {
            Some(c) => {
                let [cx, cy] = point_24_8(c);
                commands.push(PathCommand::QuadraticTo { cx, cy, x, y });
            }
            None => commands.push(PathCommand::LineTo { x, y }),
        }
    }
}

#[derive(Clone)]
struct ParsedPaint {
    color: Color,
    gradient: Option<GradientPaint>,
}

#[derive(Clone)]
struct ParsedStroke {
    paint: ParsedPaint,
    width: f64,
}

struct ShapeError(&'static str, &'static str);

impl Parser<'_> {
    pub(super) fn shape(
        &mut self,
        node: Node<'_, '_>,
        id: String,
        file: &str,
    ) -> Option<Vec<Element>> {
        let first_diagnostic = self.report.diagnostics.len();
        self.audit(
            node,
            file,
            &[
                "name",
                "selected",
                "locked",
                "referenceID",
                "isDrawingObject",
                "isFloating",
            ],
            &["matrix", "transformationPoint", "fills", "strokes", "edges"],
        );
        let transform = self.transform(node, file);
        if self.boolean(node, "isDrawingObject", false, file)
            || self.boolean(node, "isFloating", false, file)
        {
            self.report.error(
                "xfl_shape_object_mode",
                self.location(node, file),
                "Floating/drawing-object shape authoring semantics are not supported yet.",
            );
            return None;
        }

        // Fast path: preserve byte-identical ElementKind::Path output for simple
        // convex solid polygons and single-chain solid strokes.
        let saved_diagnostics = self.report.diagnostics.len();
        if let Ok(kind) = self.solid_polygon(node, file)
            && !self.report.diagnostics[saved_diagnostics..]
                .iter()
                .any(|d| d.severity == Severity::Error)
        {
            if self.report.diagnostics[first_diagnostic..]
                .iter()
                .any(|d| d.severity == Severity::Error)
            {
                return None;
            }
            return Some(vec![Element {
                id,
                name: node.attribute("name").unwrap_or("DOMShape").into(),
                accessibility: None,
                transform,
                kind,
                scripts: vec![],
            }]);
        }
        self.report.diagnostics.truncate(saved_diagnostics);

        let kinds = match self.general_shape(node, file) {
            Ok(kinds) => kinds,
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
        let base_name: String = node.attribute("name").unwrap_or("DOMShape").into();
        let single = kinds.len() == 1;
        Some(
            kinds
                .into_iter()
                .enumerate()
                .map(|(idx, kind)| Element {
                    id: if single {
                        id.clone()
                    } else {
                        format!("{id}-{idx}")
                    },
                    name: base_name.clone(),
                    accessibility: None,
                    transform,
                    kind,
                    scripts: vec![],
                })
                .collect(),
        )
    }

    fn general_shape(&mut self, node: Node<'_, '_>, file: &str) -> Result<Vec<ElementKind>> {
        let shape_loc = self.location(node, file);
        let mut warn_stroke_rendering = false;
        let mut warn_approximated: Option<&'static str> = None;

        let mut fill_styles: BTreeMap<u32, ParsedPaint> = BTreeMap::new();
        let mut fill_order: Vec<u32> = Vec::new();
        if let Some(fills) = child(node, "fills") {
            self.audit(fills, file, &[], &["FillStyle"]);
            for fill_style in elements(fills).filter(|n| n.has_tag_name("FillStyle")) {
                self.audit(
                    fill_style,
                    file,
                    &["index"],
                    &[
                        "SolidColor",
                        "SolidSwatchItem",
                        "LinearGradient",
                        "LinearGradientSwatchItem",
                        "RadialGradient",
                        "RadialGradientSwatchItem",
                        "BitmapFill",
                    ],
                );
                let index = style_index(fill_style, "index", false)?;
                let paint = self.parse_paint(fill_style, file, &mut warn_approximated)?;
                if fill_styles.insert(index, paint).is_some() {
                    return Err(ShapeError(
                        "xfl_shape_style_index",
                        "Duplicate FillStyle index declaration.",
                    ));
                }
                fill_order.push(index);
            }
        }

        let mut stroke_styles: BTreeMap<u32, ParsedStroke> = BTreeMap::new();
        let mut stroke_order: Vec<u32> = Vec::new();
        if let Some(strokes) = child(node, "strokes") {
            self.audit(strokes, file, &[], &["StrokeStyle"]);
            for stroke_style in elements(strokes).filter(|n| n.has_tag_name("StrokeStyle")) {
                self.audit(
                    stroke_style,
                    file,
                    &["index"],
                    &[
                        "SolidStroke",
                        "DashedStroke",
                        "DottedStroke",
                        "RaggedStroke",
                        "StippleStroke",
                        "HatchedStroke",
                    ],
                );
                let index = style_index(stroke_style, "index", false)?;
                let parsed = self.parse_stroke_style(
                    stroke_style,
                    file,
                    &mut warn_stroke_rendering,
                    &mut warn_approximated,
                )?;
                if stroke_styles.insert(index, parsed).is_some() {
                    return Err(ShapeError(
                        "xfl_shape_style_index",
                        "Duplicate StrokeStyle index declaration.",
                    ));
                }
                stroke_order.push(index);
            }
        }

        let edges_node = child(node, "edges").ok_or(ShapeError(
            "xfl_shape_edges",
            "Shape has no edges container.",
        ))?;
        self.audit(edges_node, file, &[], &["Edge"]);

        if fill_styles.is_empty() && stroke_styles.is_empty() {
            return Err(ShapeError(
                "xfl_shape_fill",
                "Shape declares neither fills nor strokes.",
            ));
        }

        let mut fill_edges: BTreeMap<u32, Vec<GenEdge>> = BTreeMap::new();
        let mut stroke_edges: BTreeMap<u32, Vec<GenEdge>> = BTreeMap::new();
        let mut total_segments = 0usize;
        let mut total_text_bytes = 0usize;
        let mut selected = false;
        let mut has_cubics = false;
        let mut saw_interior_seam = false;
        let mut saw_nonempty_edge_record = false;

        for edge in elements(edges_node).filter(|n| n.has_tag_name("Edge")) {
            self.audit(
                edge,
                file,
                &["fillStyle0", "fillStyle1", "strokeStyle", "edges", "cubics"],
                &[],
            );
            let left = style_index(edge, "fillStyle0", true)?;
            let right = style_index(edge, "fillStyle1", true)?;
            let line = style_index(edge, "strokeStyle", true)?;

            if (left != 0 && !fill_styles.contains_key(&left))
                || (right != 0 && !fill_styles.contains_key(&right))
            {
                return Err(ShapeError(
                    "xfl_shape_style_reference",
                    "Edge references an undeclared fillStyle index.",
                ));
            }
            if line != 0 && !stroke_styles.contains_key(&line) {
                return Err(ShapeError(
                    "xfl_shape_stroke_topology",
                    "Edge references an undeclared strokeStyle index.",
                ));
            }
            if edge.attribute("cubics").is_some() {
                has_cubics = true;
            }
            let raw_text = edge.attribute("edges");
            if raw_text.is_none_or(|t| t.trim().is_empty()) {
                if edge.attribute("cubics").is_some() {
                    continue;
                }
                return Err(ShapeError(
                    "xfl_shape_edges",
                    "Edge has no edge command string.",
                ));
            }
            let text = raw_text.unwrap();
            if text.len() > MAX_SINGLE_EDGE_TEXT {
                return Err(ShapeError(
                    "xfl_shape_limit",
                    "Edge command string exceeds 65535 bytes.",
                ));
            }
            total_text_bytes = total_text_bytes.checked_add(text.len()).ok_or(ShapeError(
                "xfl_shape_limit",
                "Shape edge text exceeds budget.",
            ))?;
            if total_text_bytes > MAX_SHAPE_EDGE_TEXT {
                return Err(ShapeError(
                    "xfl_shape_limit",
                    "Shape edge text exceeds 4 MiB.",
                ));
            }
            if left == 0 && right == 0 && line == 0 {
                return Err(ShapeError(
                    "xfl_shape_style_reference",
                    "Edge references neither a fill style nor a stroke style.",
                ));
            }
            saw_nonempty_edge_record = true;

            let remaining = MAX_GENERAL_SEGMENTS.saturating_sub(total_segments);
            let (parsed, has_sel) = parse_general_edges(text, remaining)?;
            total_segments += parsed.len();
            selected |= has_sel;

            if left != 0 && left == right && line == 0 {
                saw_interior_seam = true;
            }
            for seg in parsed {
                if seg.is_zero_length() {
                    continue;
                }
                if left != right {
                    if right != 0 {
                        fill_edges.entry(right).or_default().push(seg);
                    }
                    if left != 0 {
                        fill_edges.entry(left).or_default().push(seg.reversed());
                    }
                }
                if line != 0 {
                    stroke_edges.entry(line).or_default().push(seg);
                }
            }
        }

        let mut output = Vec::new();
        for idx in fill_order {
            let Some(edges) = fill_edges.remove(&idx) else {
                continue;
            };
            if edges.is_empty() {
                continue;
            }
            let commands = assemble_fill_commands(&edges)?;
            validate_path_commands(&commands).map_err(|_| {
                ShapeError(
                    "xfl_shape_limit",
                    "Converted fill path exceeds path command limits.",
                )
            })?;
            let paint = fill_styles.get(&idx).unwrap();
            output.push(ElementKind::PaintedPath {
                commands,
                fill_rule: FillRule::NonZero,
                fill: paint.color,
                stroke: [0, 0, 0, 0],
                fill_gradient: paint.gradient.clone(),
                stroke_gradient: None,
                stroke_width: 0.0,
            });
        }

        for idx in stroke_order {
            let Some(edges) = stroke_edges.remove(&idx) else {
                continue;
            };
            if edges.is_empty() {
                continue;
            }
            let commands = assemble_stroke_commands(&edges);
            validate_path_commands(&commands).map_err(|_| {
                ShapeError(
                    "xfl_shape_limit",
                    "Converted stroke path exceeds path command limits.",
                )
            })?;
            let stroke_def = stroke_styles.get(&idx).unwrap();
            output.push(ElementKind::PaintedPath {
                commands,
                fill_rule: FillRule::NonZero,
                fill: [0, 0, 0, 0],
                stroke: stroke_def.paint.color,
                fill_gradient: None,
                stroke_gradient: stroke_def.paint.gradient.clone(),
                stroke_width: stroke_def.width,
            });
        }

        if output.is_empty() {
            if saw_interior_seam {
                return Err(ShapeError(
                    "xfl_shape_style_reference",
                    "Shape contains only interior fill seams with no painted boundary or stroke.",
                ));
            }
            if saw_nonempty_edge_record {
                return Err(ShapeError(
                    "xfl_shape_topology",
                    "Shape contains only zero-length degenerate segments.",
                ));
            }
            if !has_cubics {
                return Err(ShapeError(
                    "xfl_shape_edges",
                    "Shape contains no drawable edges.",
                ));
            }
        }

        if selected {
            self.report.warning(
                "xfl_shape_selection",
                &shape_loc,
                "XFL edge selection bits are editor metadata and are not retained; painted geometry and strokes are retained.",
            );
        }
        if has_cubics {
            self.report.warning(
                "xfl_cubic_editing_metadata",
                &shape_loc,
                "Rendered quadratic edges are preserved; auxiliary cubic editing handles are ignored.",
            );
        }
        if warn_stroke_rendering {
            self.report.warning(
                "xfl_stroke_rendering",
                &shape_loc,
                "Flash stroke caps/joins, hairline, scale-mode or hinting options are rendered with NeoFlash round-join scaling strokes.",
            );
        }
        if let Some(reason) = warn_approximated {
            self.report
                .warning("xfl_shape_approximated", &shape_loc, reason);
        }

        Ok(output)
    }

    fn parse_paint(
        &mut self,
        wrapper: Node<'_, '_>,
        file: &str,
        warn_approximated: &mut Option<&'static str>,
    ) -> Result<ParsedPaint> {
        let children: Vec<_> = elements(wrapper).collect();
        if children.len() != 1 {
            return Err(ShapeError(
                "xfl_shape_paint",
                "Style definition must contain exactly one paint element.",
            ));
        }
        let paint_node = children[0];
        let tag = paint_node.tag_name().name();
        match tag {
            "SolidColor" | "SolidSwatchItem" => {
                self.audit(paint_node, file, &["color", "alpha"], &[]);
                let alpha = self.number(paint_node, "alpha", 1.0, file);
                let color = self.color(
                    paint_node.attribute("color").unwrap_or("#000000"),
                    alpha,
                    &self.location(paint_node, file),
                );
                Ok(ParsedPaint {
                    color,
                    gradient: None,
                })
            }
            "LinearGradient"
            | "LinearGradientSwatchItem"
            | "RadialGradient"
            | "RadialGradientSwatchItem" => {
                let is_radial = tag.starts_with("RadialGradient");
                if is_radial {
                    self.audit(
                        paint_node,
                        file,
                        &["spreadMethod", "interpolationMethod", "focalPointRatio"],
                        &["matrix", "GradientEntry"],
                    );
                } else {
                    self.audit(
                        paint_node,
                        file,
                        &["spreadMethod", "interpolationMethod"],
                        &["matrix", "GradientEntry"],
                    );
                }
                let spread = paint_node.attribute("spreadMethod").unwrap_or("pad");
                if !matches!(spread, "pad" | "reflect" | "repeat") {
                    return Err(ShapeError(
                        "xfl_shape_paint",
                        "Unsupported gradient spreadMethod.",
                    ));
                }
                let interp = paint_node.attribute("interpolationMethod").unwrap_or("rgb");
                if !matches!(interp, "rgb" | "linearRGB") {
                    return Err(ShapeError(
                        "xfl_shape_paint",
                        "Unsupported gradient interpolationMethod.",
                    ));
                }
                if spread != "pad" || interp != "rgb" {
                    warn_approximated.get_or_insert(
                        "Gradient spreadMethod/interpolationMethod is approximated with pad/RGB.",
                    );
                }
                let focal = if is_radial {
                    let f = self.number(paint_node, "focalPointRatio", 0.0, file);
                    if !(-1.0..=1.0).contains(&f) {
                        return Err(ShapeError(
                            "xfl_shape_paint",
                            "RadialGradient focalPointRatio must be within [-1, 1].",
                        ));
                    }
                    f
                } else {
                    0.0
                };
                let matrix = self.parse_gradient_matrix(paint_node, file);
                let entry_nodes: Vec<_> = elements(paint_node)
                    .filter(|n| n.has_tag_name("GradientEntry"))
                    .collect();
                if !(1..=15).contains(&entry_nodes.len()) {
                    return Err(ShapeError(
                        "xfl_shape_paint",
                        "Gradients require between 1 and 15 GradientEntry stops.",
                    ));
                }
                let mut stops = Vec::with_capacity(entry_nodes.len());
                for entry in entry_nodes {
                    self.audit(entry, file, &["color", "alpha", "ratio"], &[]);
                    let alpha = self.number(entry, "alpha", 1.0, file);
                    let color = self.color(
                        entry.attribute("color").unwrap_or("#000000"),
                        alpha,
                        &self.location(entry, file),
                    );
                    let ratio_f = self.number(entry, "ratio", 0.0, file);
                    if !(0.0..=1.0).contains(&ratio_f) {
                        return Err(ShapeError(
                            "xfl_shape_paint",
                            "GradientEntry ratio must be in [0, 1].",
                        ));
                    }
                    stops.push(GradientStop {
                        ratio: (ratio_f * 255.0).round() as u8,
                        color,
                    });
                }
                stops.sort_by_key(|s| s.ratio);
                let fallback = stops[0].color;
                let paint = GradientPaint {
                    kind: if is_radial {
                        GradientKind::Radial
                    } else {
                        GradientKind::Linear
                    },
                    matrix,
                    stops,
                    focal_point_ratio: focal,
                };
                paint.validate().map_err(|_| {
                    ShapeError(
                        "xfl_shape_paint",
                        "Gradient matrix is singular or gradient stops are invalid.",
                    )
                })?;
                Ok(ParsedPaint {
                    color: fallback,
                    gradient: Some(paint),
                })
            }
            "BitmapFill" => {
                self.audit(
                    paint_node,
                    file,
                    &["bitmapPath", "bitmapIsClipped"],
                    &["matrix"],
                );
                self.boolean(paint_node, "bitmapIsClipped", false, file);
                let _ = self.parse_gradient_matrix(paint_node, file);
                warn_approximated.get_or_insert(
                    "BitmapFill is approximated as neutral gray; use DOMBitmapInstance for full bitmap rendering.",
                );
                Ok(ParsedPaint {
                    color: [128, 128, 128, 255],
                    gradient: None,
                })
            }
            _ => Err(ShapeError(
                "xfl_shape_paint",
                "Unsupported fill or stroke paint element.",
            )),
        }
    }

    fn parse_gradient_matrix(&mut self, parent: Node<'_, '_>, file: &str) -> [f64; 6] {
        let Some(wrapper) = child(parent, "matrix") else {
            return [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        };
        self.audit(wrapper, file, &[], &["Matrix"]);
        let Some(matrix) = child(wrapper, "Matrix") else {
            return [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        };
        self.audit(matrix, file, &["a", "b", "c", "d", "tx", "ty"], &[]);
        [
            self.number(matrix, "a", 1.0, file),
            self.number(matrix, "b", 0.0, file),
            self.number(matrix, "c", 0.0, file),
            self.number(matrix, "d", 1.0, file),
            self.number(matrix, "tx", 0.0, file),
            self.number(matrix, "ty", 0.0, file),
        ]
    }

    fn parse_stroke_style(
        &mut self,
        style: Node<'_, '_>,
        file: &str,
        warn_stroke_rendering: &mut bool,
        warn_approximated: &mut Option<&'static str>,
    ) -> Result<ParsedStroke> {
        let children: Vec<_> = elements(style).collect();
        if children.len() != 1 {
            return Err(ShapeError(
                "xfl_shape_stroke",
                "StrokeStyle must contain exactly one stroke definition.",
            ));
        }
        let stroke_node = children[0];
        let tag = stroke_node.tag_name().name();
        match tag {
            "SolidStroke" => {
                self.audit(
                    stroke_node,
                    file,
                    &[
                        "scaleMode",
                        "weight",
                        "caps",
                        "joints",
                        "miterLimit",
                        "pixelHinting",
                        "sharpCorners",
                        "solidStyle",
                    ],
                    &["fill"],
                );
            }
            "DashedStroke" => {
                self.audit(
                    stroke_node,
                    file,
                    &[
                        "scaleMode",
                        "weight",
                        "caps",
                        "joints",
                        "miterLimit",
                        "pixelHinting",
                        "sharpCorners",
                        "solidStyle",
                        "dash1",
                        "dash2",
                    ],
                    &["fill"],
                );
                let _ = self.number(stroke_node, "dash1", 2.0, file);
                let _ = self.number(stroke_node, "dash2", 2.0, file);
                warn_approximated
                    .get_or_insert("Patterned XFL stroke style is approximated as a solid stroke.");
            }
            "DottedStroke" => {
                self.audit(
                    stroke_node,
                    file,
                    &[
                        "scaleMode",
                        "weight",
                        "caps",
                        "joints",
                        "miterLimit",
                        "pixelHinting",
                        "sharpCorners",
                        "solidStyle",
                        "dotSpace",
                        "dotSize",
                    ],
                    &["fill"],
                );
                let _ = self.number(stroke_node, "dotSpace", 2.0, file);
                warn_approximated
                    .get_or_insert("Patterned XFL stroke style is approximated as a solid stroke.");
            }
            "RaggedStroke" => {
                self.audit(
                    stroke_node,
                    file,
                    &[
                        "scaleMode",
                        "weight",
                        "caps",
                        "joints",
                        "miterLimit",
                        "pixelHinting",
                        "sharpCorners",
                        "solidStyle",
                        "pattern",
                        "waveHeight",
                        "waveLength",
                    ],
                    &["fill"],
                );
                warn_approximated
                    .get_or_insert("Patterned XFL stroke style is approximated as a solid stroke.");
            }
            "StippleStroke" => {
                self.audit(
                    stroke_node,
                    file,
                    &[
                        "scaleMode",
                        "weight",
                        "caps",
                        "joints",
                        "miterLimit",
                        "pixelHinting",
                        "sharpCorners",
                        "solidStyle",
                        "dotSize",
                        "variation",
                        "density",
                    ],
                    &["fill"],
                );
                warn_approximated
                    .get_or_insert("Patterned XFL stroke style is approximated as a solid stroke.");
            }
            "HatchedStroke" => {
                self.audit(
                    stroke_node,
                    file,
                    &[
                        "scaleMode",
                        "weight",
                        "caps",
                        "joints",
                        "miterLimit",
                        "pixelHinting",
                        "sharpCorners",
                        "solidStyle",
                        "thickness",
                        "space",
                        "jiggle",
                        "rotate",
                        "curve",
                        "length",
                    ],
                    &["fill"],
                );
                warn_approximated
                    .get_or_insert("Patterned XFL stroke style is approximated as a solid stroke.");
            }
            _ => {
                return Err(ShapeError(
                    "xfl_shape_stroke",
                    "Unsupported StrokeStyle definition.",
                ));
            }
        }

        let scale_mode = stroke_node.attribute("scaleMode").unwrap_or("none");
        if !matches!(scale_mode, "normal" | "horizontal" | "vertical" | "none") {
            return Err(ShapeError(
                "xfl_shape_stroke",
                "Unsupported stroke scaleMode.",
            ));
        }
        let caps = stroke_node.attribute("caps").unwrap_or("round");
        if !matches!(caps, "round" | "none" | "square") {
            return Err(ShapeError("xfl_shape_stroke", "Unsupported stroke caps."));
        }
        let joints = stroke_node.attribute("joints").unwrap_or("round");
        if !matches!(joints, "round" | "miter" | "bevel") {
            return Err(ShapeError("xfl_shape_stroke", "Unsupported stroke joints."));
        }
        if let Some(solid_style) = stroke_node.attribute("solidStyle")
            && !matches!(solid_style, "hairline" | "normal")
        {
            return Err(ShapeError(
                "xfl_shape_stroke",
                "Unsupported stroke solidStyle.",
            ));
        }
        let miter = self.number(stroke_node, "miterLimit", 3.0, file);
        if miter <= 0.0 || miter > 100_000.0 {
            return Err(ShapeError(
                "xfl_shape_stroke",
                "Stroke miterLimit must be positive and <=100000.",
            ));
        }
        let pixel_hinting = self.boolean(stroke_node, "pixelHinting", false, file);
        let sharp_corners = self.boolean(stroke_node, "sharpCorners", false, file);
        let raw_weight = self.number(stroke_node, "weight", 1.0, file);
        if !(0.0..=100_000.0).contains(&raw_weight) {
            return Err(ShapeError(
                "xfl_shape_stroke",
                "Stroke weight must be nonnegative and <=100000 pixels.",
            ));
        }
        let is_hairline =
            raw_weight == 0.0 || stroke_node.attribute("solidStyle") == Some("hairline");
        let width = if raw_weight == 0.0 { 1.0 } else { raw_weight };
        if is_hairline
            || scale_mode != "normal"
            || caps != "round"
            || joints != "round"
            || pixel_hinting
            || sharp_corners
        {
            *warn_stroke_rendering = true;
        }

        let fill_wrapper = child(stroke_node, "fill").ok_or(ShapeError(
            "xfl_shape_stroke",
            "Stroke style is missing its fill paint.",
        ))?;
        self.audit(
            fill_wrapper,
            file,
            &[],
            &[
                "SolidColor",
                "SolidSwatchItem",
                "LinearGradient",
                "LinearGradientSwatchItem",
                "RadialGradient",
                "RadialGradientSwatchItem",
                "BitmapFill",
            ],
        );
        let paint = self.parse_paint(fill_wrapper, file, warn_approximated)?;
        Ok(ParsedStroke { paint, width })
    }

    fn solid_polygon(&mut self, node: Node<'_, '_>, file: &str) -> Result<ElementKind> {
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

        let fill_info = if let Some(fills) = child(node, "fills") {
            self.audit(fills, file, &[], &["FillStyle"]);
            let fill_styles: Vec<_> = elements(fills)
                .filter(|n| n.has_tag_name("FillStyle"))
                .collect();
            if fill_styles.is_empty() && stroke_index > 0 {
                None
            } else if fill_styles.len() != 1 {
                return Err(ShapeError(
                    "xfl_shape_fill",
                    "Exactly one fill style is supported; multiple/empty fill tables are not discarded.",
                ));
            } else {
                let fill_style = fill_styles[0];
                self.audit(fill_style, file, &["index"], &["SolidColor"]);
                let fill_index = style_index(fill_style, "index", false)?;
                let fill = self.shape_color(fill_style, file)?;
                Some((fill_index, fill))
            }
        } else if stroke_index > 0 {
            None
        } else {
            return Err(ShapeError(
                "xfl_shape_fill",
                "A polygon requires exactly one solid fill; unfilled shapes require a declared solid stroke.",
            ));
        };

        let edges_node = child(node, "edges").ok_or(ShapeError(
            "xfl_shape_edges",
            "Shape has no edges container.",
        ))?;
        self.audit(edges_node, file, &[], &["Edge"]);

        let Some((fill_index, fill)) = fill_info else {
            return self.stroke_only_path(
                node,
                edges_node,
                stroke_index,
                stroke,
                stroke_width,
                file,
            );
        };
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
            if text_bytes > MAX_FAST_EDGE_TEXT {
                return Err(ShapeError(
                    "xfl_shape_limit",
                    "Shape edge text exceeds 65536 bytes.",
                ));
            }
            let (parsed, has_selection) = parse_edges(text, MAX_FAST_EDGES - segments.len())?;
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

    fn stroke_only_path(
        &mut self,
        node: Node<'_, '_>,
        edges_node: Node<'_, '_>,
        stroke_index: u32,
        stroke: Color,
        stroke_width: f64,
        file: &str,
    ) -> Result<ElementKind> {
        let mut edges = vec![];
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
            if left != 0 || right != 0 {
                return Err(ShapeError(
                    "xfl_shape_style_reference",
                    "Unfilled stroke-only shapes cannot reference fill styles.",
                ));
            }
            if line != stroke_index {
                return Err(ShapeError(
                    "xfl_shape_stroke_topology",
                    "Every edge of a stroke-only shape must reference the declared stroke style.",
                ));
            }
            let text = edge.attribute("edges").ok_or(ShapeError(
                "xfl_shape_edges",
                "Edge has no edge command string.",
            ))?;
            text_bytes = text_bytes.checked_add(text.len()).ok_or(ShapeError(
                "xfl_shape_limit",
                "Shape edge text exceeds 65536 bytes.",
            ))?;
            if text_bytes > MAX_FAST_EDGE_TEXT {
                return Err(ShapeError(
                    "xfl_shape_limit",
                    "Shape edge text exceeds 65536 bytes.",
                ));
            }
            let (parsed, has_selection) = parse_stroke_edges(text, MAX_FAST_EDGES - edges.len())?;
            selected |= has_selection;
            edges.extend(parsed);
        }
        let (ordered, closed) = order_stroke_chain(&edges)?;
        let (start, _) = ordered[0].endpoints();
        let mut commands = vec![PathCommand::MoveTo {
            x: pixel(start[0]),
            y: pixel(start[1]),
        }];
        for seg in &ordered {
            match *seg {
                StrokeEdge::Line(_, b) => commands.push(PathCommand::LineTo {
                    x: pixel(b[0]),
                    y: pixel(b[1]),
                }),
                StrokeEdge::Quad(_, c, b) => commands.push(PathCommand::QuadraticTo {
                    cx: pixel(c[0]),
                    cy: pixel(c[1]),
                    x: pixel(b[0]),
                    y: pixel(b[1]),
                }),
            }
        }
        if closed {
            commands.push(PathCommand::Close);
        }
        if selected {
            self.report.warning(
                "xfl_shape_selection",
                self.location(node, file),
                "XFL edge selection bits are editor metadata and are not retained; stroke geometry is retained.",
            );
        }
        Ok(ElementKind::Path {
            commands,
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke,
            stroke_width,
        })
    }
}

fn point_24_8(p: Point24_8) -> [f64; 2] {
    [p[0] as f64 / 5120.0, p[1] as f64 / 5120.0]
}

fn parse_general_edges(text: &str, limit: usize) -> Result<(Vec<GenEdge>, bool)> {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut current: Option<Point24_8> = None;
    let mut pending_move = false;
    let mut selected = false;
    let mut result = Vec::new();
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
                    coordinate_24_8(bytes, &mut cursor)?,
                    coordinate_24_8(bytes, &mut cursor)?,
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
                    coordinate_24_8(bytes, &mut cursor)?,
                    coordinate_24_8(bytes, &mut cursor)?,
                ];
                if result.len() == limit {
                    return Err(ShapeError(
                        "xfl_shape_limit",
                        "Shape exceeds 16384 segments.",
                    ));
                }
                result.push(GenEdge {
                    start,
                    end,
                    control: None,
                });
                current = Some(end);
                pending_move = false;
            }
            b'[' | b']' => {
                let start = current.ok_or(ShapeError(
                    "xfl_shape_edges",
                    "Quadratic curve command must follow an explicit move.",
                ))?;
                let control = [
                    coordinate_24_8(bytes, &mut cursor)?,
                    coordinate_24_8(bytes, &mut cursor)?,
                ];
                let end = [
                    coordinate_24_8(bytes, &mut cursor)?,
                    coordinate_24_8(bytes, &mut cursor)?,
                ];
                if result.len() == limit {
                    return Err(ShapeError(
                        "xfl_shape_limit",
                        "Shape exceeds 16384 segments.",
                    ));
                }
                result.push(GenEdge {
                    start,
                    end,
                    control: Some(control),
                });
                current = Some(end);
                pending_move = false;
            }
            _ => {
                return Err(ShapeError(
                    "xfl_shape_edge_syntax",
                    "Unknown edge token in XFL shape command string.",
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

fn coordinate_24_8(bytes: &[u8], cursor: &mut usize) -> Result<i64> {
    whitespace(bytes, cursor);
    if bytes.get(*cursor) == Some(&b'#') {
        *cursor += 1;
        let whole_start = *cursor;
        while bytes.get(*cursor).is_some_and(u8::is_ascii_hexdigit) {
            *cursor += 1;
        }
        let whole_len = *cursor - whole_start;
        if !(1..=6).contains(&whole_len) {
            return Err(ShapeError(
                "xfl_shape_coordinate",
                "Hexadecimal XFL coordinate must have 1 to 6 whole hex digits.",
            ));
        }
        let whole_str = std::str::from_utf8(&bytes[whole_start..*cursor]).unwrap();
        let raw_whole = u32::from_str_radix(whole_str, 16).map_err(|_| {
            ShapeError(
                "xfl_shape_coordinate",
                "Invalid hexadecimal XFL coordinate.",
            )
        })?;
        let signed_whole = if raw_whole > 0x7f_ffff {
            i64::from(raw_whole) - 0x100_0000
        } else {
            i64::from(raw_whole)
        };
        let frac = if bytes.get(*cursor) == Some(&b'.') {
            *cursor += 1;
            let frac_start = *cursor;
            while bytes.get(*cursor).is_some_and(u8::is_ascii_hexdigit) {
                *cursor += 1;
            }
            let frac_len = *cursor - frac_start;
            if !(1..=2).contains(&frac_len) {
                return Err(ShapeError(
                    "xfl_shape_coordinate",
                    "Hexadecimal XFL coordinate fraction must have 1 or 2 hex digits.",
                ));
            }
            let frac_str = std::str::from_utf8(&bytes[frac_start..*cursor]).unwrap();
            let raw_frac = u8::from_str_radix(frac_str, 16)
                .map_err(|_| ShapeError("xfl_shape_coordinate", "Invalid hexadecimal fraction."))?;
            if frac_len == 1 {
                raw_frac << 4
            } else {
                raw_frac
            }
        } else {
            0
        };
        if bytes.get(*cursor).is_some_and(|c| matches!(c, b'.' | b'#')) {
            return Err(ShapeError(
                "xfl_shape_coordinate",
                "Malformed trailing characters in hexadecimal coordinate.",
            ));
        }
        return Ok(signed_whole * 256 + i64::from(frac));
    }

    let negative = if bytes.get(*cursor) == Some(&b'-') {
        *cursor += 1;
        true
    } else {
        false
    };
    let int_start = *cursor;
    while bytes.get(*cursor).is_some_and(u8::is_ascii_digit) {
        *cursor += 1;
    }
    if *cursor == int_start {
        return Err(ShapeError(
            "xfl_shape_coordinate",
            "Missing numeric XFL coordinate.",
        ));
    }
    let int_str = std::str::from_utf8(&bytes[int_start..*cursor]).unwrap();
    let int_val = int_str.parse::<u64>().map_err(|_| {
        ShapeError(
            "xfl_shape_coordinate",
            "Coordinate exceeds the signed 32-bit twip range.",
        )
    })?;
    let max_mag = if negative {
        (i32::MAX as u64) + 1
    } else {
        i32::MAX as u64
    };
    if int_val > max_mag {
        return Err(ShapeError(
            "xfl_shape_coordinate",
            "Coordinate exceeds the signed 32-bit twip range.",
        ));
    }

    let frac_256 = if bytes.get(*cursor) == Some(&b'.') {
        *cursor += 1;
        let frac_start = *cursor;
        while bytes.get(*cursor).is_some_and(u8::is_ascii_digit) {
            *cursor += 1;
        }
        if *cursor == frac_start {
            return Err(ShapeError(
                "xfl_shape_coordinate",
                "Decimal coordinate is missing digits after decimal point.",
            ));
        }
        let frac_str = std::str::from_utf8(&bytes[frac_start..*cursor]).unwrap();
        let slice = &frac_str[..frac_str.len().min(12)];
        let num = slice.parse::<u128>().unwrap_or(0);
        let scale = 10_u128.pow(slice.len() as u32);
        ((num * 256 + scale / 2) / scale) as i64
    } else {
        0
    };

    if bytes
        .get(*cursor)
        .is_some_and(|c| matches!(c, b'.' | b'#' | b'e' | b'E'))
    {
        return Err(ShapeError(
            "xfl_shape_coordinate",
            "Exponent or malformed coordinate suffix is unsupported.",
        ));
    }

    let mag = (int_val as i64) * 256 + frac_256;
    let val = if negative { -mag } else { mag };
    let min_24_8 = i64::from(i32::MIN) * 256;
    let max_24_8 = i64::from(i32::MAX) * 256 + 255;
    if !(min_24_8..=max_24_8).contains(&val) {
        return Err(ShapeError(
            "xfl_shape_coordinate",
            "Coordinate exceeds the signed 32-bit twip range.",
        ));
    }
    Ok(val)
}

fn assemble_fill_commands(edges: &[GenEdge]) -> Result<Vec<PathCommand>> {
    let mut outgoing: BTreeMap<Point24_8, Vec<usize>> = BTreeMap::new();
    let mut balance: BTreeMap<Point24_8, i64> = BTreeMap::new();
    for (i, e) in edges.iter().enumerate() {
        outgoing.entry(e.start).or_default().push(i);
        *balance.entry(e.start).or_default() += 1;
        *balance.entry(e.end).or_default() -= 1;
    }
    if balance.values().any(|&v| v != 0) {
        return Err(ShapeError(
            "xfl_shape_topology",
            "Filled contour is not closed.",
        ));
    }
    // Reverse per-vertex adjacency lists so `.pop()` consumes edges in authored order.
    for list in outgoing.values_mut() {
        list.reverse();
    }

    let mut unused: BTreeSet<usize> = (0..edges.len()).collect();
    let mut commands = Vec::new();
    while let Some(&first) = unused.first() {
        let start = edges[first].start;
        let mut stack: Vec<(Point24_8, Option<usize>)> = vec![(start, None)];
        let mut circuit = Vec::new();
        while let Some(&(vertex, _)) = stack.last() {
            let next = outgoing.get_mut(&vertex).and_then(|ids| {
                while let Some(id) = ids.pop() {
                    if unused.remove(&id) {
                        return Some(id);
                    }
                }
                None
            });
            if let Some(id) = next {
                stack.push((edges[id].end, Some(id)));
            } else if let Some((_, Some(id))) = stack.pop() {
                circuit.push(id);
            }
        }
        circuit.reverse();
        if circuit.is_empty() {
            return Err(ShapeError(
                "xfl_shape_topology",
                "Filled contour is not closed.",
            ));
        }
        let contour_start = edges[circuit[0]].start;
        let [sx, sy] = point_24_8(contour_start);
        commands.push(PathCommand::MoveTo { x: sx, y: sy });
        let mut current = contour_start;
        for id in circuit {
            let e = edges[id];
            if e.start != current {
                return Err(ShapeError(
                    "xfl_shape_topology",
                    "Filled contour is not closed.",
                ));
            }
            e.append(&mut commands);
            current = e.end;
        }
        if current != contour_start {
            return Err(ShapeError(
                "xfl_shape_topology",
                "Filled contour is not closed.",
            ));
        }
        commands.push(PathCommand::Close);
    }
    Ok(commands)
}

fn assemble_stroke_commands(edges: &[GenEdge]) -> Vec<PathCommand> {
    let mut adj: BTreeMap<Point24_8, Vec<usize>> = BTreeMap::new();
    let mut degree: BTreeMap<Point24_8, usize> = BTreeMap::new();
    for (i, e) in edges.iter().enumerate() {
        adj.entry(e.start).or_default().push(i);
        *degree.entry(e.start).or_default() += 1;
        if e.end != e.start {
            adj.entry(e.end).or_default().push(i);
        }
        *degree.entry(e.end).or_default() += 1;
    }
    for list in adj.values_mut() {
        list.reverse();
    }

    let mut unused: BTreeSet<usize> = (0..edges.len()).collect();
    let mut commands = Vec::new();
    while !unused.is_empty() {
        // Prefer an odd-degree start vertex in authored edge order so open polylines
        // traverse from end to end without splitting mid-chain.
        let mut start = None;
        for &idx in &unused {
            let e = edges[idx];
            if degree.get(&e.start).copied().unwrap_or(0) % 2 == 1 {
                start = Some(e.start);
                break;
            }
            if degree.get(&e.end).copied().unwrap_or(0) % 2 == 1 {
                start = Some(e.end);
                break;
            }
        }
        let start = start.unwrap_or_else(|| edges[*unused.first().unwrap()].start);

        let mut stack: Vec<(Point24_8, Option<GenEdge>)> = vec![(start, None)];
        let mut trail: Vec<GenEdge> = Vec::new();
        while let Some(&(vertex, _)) = stack.last() {
            let next = adj.get_mut(&vertex).and_then(|ids| {
                while let Some(id) = ids.pop() {
                    if unused.remove(&id) {
                        return Some(id);
                    }
                }
                None
            });
            if let Some(id) = next {
                let raw = edges[id];
                let oriented = if raw.start == vertex {
                    raw
                } else {
                    raw.reversed()
                };
                *degree.get_mut(&oriented.start).unwrap() -= 1;
                *degree.get_mut(&oriented.end).unwrap() -= 1;
                stack.push((oriented.end, Some(oriented)));
            } else if let Some((_, maybe_seg)) = stack.pop()
                && let Some(seg) = maybe_seg
            {
                trail.push(seg);
            }
        }
        trail.reverse();

        let mut current: Option<Point24_8> = None;
        let mut subpath_start: Option<Point24_8> = None;
        let mut subpath_breaks = 0usize;
        for seg in &trail {
            if current != Some(seg.start) {
                let [x, y] = point_24_8(seg.start);
                commands.push(PathCommand::MoveTo { x, y });
                subpath_start = Some(seg.start);
                subpath_breaks += 1;
            }
            seg.append(&mut commands);
            current = Some(seg.end);
        }
        if subpath_breaks == 1 && current.is_some() && current == subpath_start {
            commands.push(PathCommand::Close);
        }
    }
    commands
}

fn parse_stroke_edges(text: &str, limit: usize) -> Result<(Vec<StrokeEdge>, bool)> {
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
                if start == end {
                    return Err(ShapeError(
                        "xfl_shape_topology",
                        "Zero-length stroke segments are unsupported.",
                    ));
                }
                if result.len() == limit {
                    return Err(ShapeError(
                        "xfl_shape_limit",
                        "Shape exceeds 512 edges; simplify the source contour.",
                    ));
                }
                result.push(StrokeEdge::Line(start, end));
                current = Some(end);
                pending_move = false;
            }
            b'[' | b']' => {
                let start = current.ok_or(ShapeError(
                    "xfl_shape_edges",
                    "Quadratic curve command must follow an explicit move.",
                ))?;
                let control = [
                    coordinate(bytes, &mut cursor)?,
                    coordinate(bytes, &mut cursor)?,
                ];
                let end = [
                    coordinate(bytes, &mut cursor)?,
                    coordinate(bytes, &mut cursor)?,
                ];
                if start == end && start == control {
                    return Err(ShapeError(
                        "xfl_shape_topology",
                        "Zero-length stroke segments are unsupported.",
                    ));
                }
                if result.len() == limit {
                    return Err(ShapeError(
                        "xfl_shape_limit",
                        "Shape exceeds 512 edges; simplify the source contour.",
                    ));
                }
                result.push(StrokeEdge::Quad(start, control, end));
                current = Some(end);
                pending_move = false;
            }
            b'(' | b')' => {
                return Err(ShapeError(
                    "xfl_shape_curve_unsupported",
                    "Cubic XFL edit-hint edges are not converted by the stroke adapter.",
                ));
            }
            _ => {
                return Err(ShapeError(
                    "xfl_shape_edge_syntax",
                    "Unknown edge token in stroke-only shape.",
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

fn order_stroke_chain(edges: &[StrokeEdge]) -> Result<(Vec<StrokeEdge>, bool)> {
    if edges.is_empty() {
        return Err(ShapeError(
            "xfl_shape_edges",
            "Stroke shape has no segments.",
        ));
    }
    // Check if the authored sequence is already contiguous in order.
    let mut contiguous = true;
    for pair in edges.windows(2) {
        if pair[0].endpoints().1 != pair[1].endpoints().0 {
            contiguous = false;
            break;
        }
    }
    let candidate = if contiguous {
        edges.to_vec()
    } else {
        // Reassemble a single undirected non-branching chain starting from the first
        // endpoint of degree 1 (or edges[0] for a closed loop).
        let mut adj: BTreeMap<Point, Vec<usize>> = BTreeMap::new();
        for (i, e) in edges.iter().enumerate() {
            let (a, b) = e.endpoints();
            if a == b {
                return Err(ShapeError(
                    "xfl_shape_topology",
                    "Self-loop stroke segments are unsupported.",
                ));
            }
            adj.entry(a).or_default().push(i);
            adj.entry(b).or_default().push(i);
        }
        if adj.values().any(|list| list.len() > 2) {
            return Err(ShapeError(
                "xfl_shape_topology",
                "Branching stroke graphs are not supported by the single-path stroke adapter.",
            ));
        }
        let degree_one: Vec<Point> = adj
            .iter()
            .filter_map(|(&pt, list)| (list.len() == 1).then_some(pt))
            .collect();
        let start = match degree_one.len() {
            0 => edges[0].endpoints().0,
            2 => {
                let first_start = edges[0].endpoints().0;
                if degree_one.contains(&first_start) {
                    first_start
                } else {
                    degree_one[0]
                }
            }
            _ => {
                return Err(ShapeError(
                    "xfl_shape_topology",
                    "Disconnected multi-stroke shapes are not supported by the single-path stroke adapter.",
                ));
            }
        };
        let mut used = vec![false; edges.len()];
        let mut chain = Vec::with_capacity(edges.len());
        let mut cursor = start;
        for _ in 0..edges.len() {
            let next_idx = adj
                .get(&cursor)
                .and_then(|list| list.iter().copied().find(|&idx| !used[idx]))
                .ok_or(ShapeError(
                    "xfl_shape_topology",
                    "Disconnected stroke components are not supported.",
                ))?;
            used[next_idx] = true;
            let seg = edges[next_idx];
            let oriented = if seg.endpoints().0 == cursor {
                seg
            } else {
                seg.reversed()
            };
            cursor = oriented.endpoints().1;
            chain.push(oriented);
        }
        chain
    };
    // Verify no branching/self-touching interior vertices on the assembled chain.
    let first_pt = candidate[0].endpoints().0;
    let last_pt = candidate.last().unwrap().endpoints().1;
    let closed = candidate.len() >= 2 && first_pt == last_pt;
    let mut visited = BTreeSet::from([first_pt]);
    for (idx, seg) in candidate.iter().enumerate() {
        let end = seg.endpoints().1;
        if closed && idx + 1 == candidate.len() {
            continue;
        }
        if !visited.insert(end) {
            return Err(ShapeError(
                "xfl_shape_topology",
                "Self-intersecting or multi-loop stroke topologies are not supported.",
            ));
        }
    }
    Ok((candidate, closed))
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
