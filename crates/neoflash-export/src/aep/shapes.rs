use super::comp_builder::color_rgb01;
use super::*;

// Vector Path Conversion (`PathCommand` -> `AeBezierContour`)
// ============================================================================

/// Converts NeoFlash [`PathCommand`] contours into exact After Effects Bezier contours
/// (`vertices` + relative `inTangents` and `outTangents`, elevating quadratics to exact cubics).
pub fn path_commands_to_ae_contours(
    commands: &[PathCommand],
    offset: [f64; 2],
) -> Vec<AeBezierContour> {
    let mut contours = Vec::new();
    let mut cur_vertices: Vec<[f64; 2]> = Vec::new();
    let mut cur_in: Vec<[f64; 2]> = Vec::new();
    let mut cur_out: Vec<[f64; 2]> = Vec::new();
    let mut cur_closed = false;

    let flush = |contours: &mut Vec<AeBezierContour>,
                 verts: &mut Vec<[f64; 2]>,
                 ins: &mut Vec<[f64; 2]>,
                 outs: &mut Vec<[f64; 2]>,
                 closed: &mut bool| {
        if verts.len() >= 2 {
            if *closed && verts.len() > 2 {
                let first = verts[0];
                let last = *verts.last().unwrap();
                if (first[0] - last[0]).hypot(first[1] - last[1]) <= 1e-4 {
                    let last_in = ins.pop().unwrap_or([0.0, 0.0]);
                    verts.pop();
                    outs.pop();
                    ins[0] = last_in;
                }
            }
            contours.push(AeBezierContour {
                vertices: std::mem::take(verts).into_iter().map(finite_pair).collect(),
                in_tangents: std::mem::take(ins).into_iter().map(finite_pair).collect(),
                out_tangents: std::mem::take(outs).into_iter().map(finite_pair).collect(),
                closed: *closed,
            });
        } else {
            verts.clear();
            ins.clear();
            outs.clear();
        }
        *closed = false;
    };

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                flush(
                    &mut contours,
                    &mut cur_vertices,
                    &mut cur_in,
                    &mut cur_out,
                    &mut cur_closed,
                );
                cur_vertices.push([x + offset[0], y + offset[1]]);
                cur_in.push([0.0, 0.0]);
                cur_out.push([0.0, 0.0]);
            }
            PathCommand::LineTo { x, y } => {
                cur_vertices.push([x + offset[0], y + offset[1]]);
                cur_in.push([0.0, 0.0]);
                cur_out.push([0.0, 0.0]);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let p1 = [x + offset[0], y + offset[1]];
                let q = [cx + offset[0], cy + offset[1]];
                if let Some(&p0) = cur_vertices.last() {
                    // Exact quadratic-to-cubic elevation:
                    // C1 = P0 + 2/3 * (Q - P0), C2 = P1 + 2/3 * (Q - P1).
                    let out_rel = [(2.0 / 3.0) * (q[0] - p0[0]), (2.0 / 3.0) * (q[1] - p0[1])];
                    let in_rel = [(2.0 / 3.0) * (q[0] - p1[0]), (2.0 / 3.0) * (q[1] - p1[1])];
                    if let Some(last_out) = cur_out.last_mut() {
                        *last_out = out_rel;
                    }
                    cur_vertices.push(p1);
                    cur_in.push(in_rel);
                    cur_out.push([0.0, 0.0]);
                } else {
                    cur_vertices.push(p1);
                    cur_in.push([0.0, 0.0]);
                    cur_out.push([0.0, 0.0]);
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
                let p1 = [x + offset[0], y + offset[1]];
                let c1 = [cx1 + offset[0], cy1 + offset[1]];
                let c2 = [cx2 + offset[0], cy2 + offset[1]];
                if let Some(&p0) = cur_vertices.last() {
                    if let Some(last_out) = cur_out.last_mut() {
                        *last_out = [c1[0] - p0[0], c1[1] - p0[1]];
                    }
                    cur_vertices.push(p1);
                    cur_in.push([c2[0] - p1[0], c2[1] - p1[1]]);
                    cur_out.push([0.0, 0.0]);
                } else {
                    cur_vertices.push(p1);
                    cur_in.push([0.0, 0.0]);
                    cur_out.push([0.0, 0.0]);
                }
            }
            PathCommand::Close => {
                cur_closed = true;
                flush(
                    &mut contours,
                    &mut cur_vertices,
                    &mut cur_in,
                    &mut cur_out,
                    &mut cur_closed,
                );
            }
        }
    }
    flush(
        &mut contours,
        &mut cur_vertices,
        &mut cur_in,
        &mut cur_out,
        &mut cur_closed,
    );
    contours
}

pub(super) type ExtractedVectorShape = (
    Vec<PathCommand>,
    FillRule,
    Color,
    Color,
    Option<GradientPaint>,
    Option<GradientPaint>,
    f64,
);

pub(super) fn element_vector_commands(el: &Element) -> Option<ExtractedVectorShape> {
    match &el.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let cmds = rectangle_path_commands(*width, *height, el.rectangle_options());
            let has_fill = fill[3] > 0;
            let closed_cmds = ensure_filled_closed(cmds, has_fill);
            Some((
                closed_cmds,
                FillRule::NonZero,
                *fill,
                *stroke,
                None,
                None,
                *stroke_width,
            ))
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let cmds = oval_path_commands(*width, *height, el.oval_options());
            let has_fill = fill[3] > 0;
            let closed_cmds = ensure_filled_closed(cmds, has_fill);
            Some((
                closed_cmds,
                FillRule::NonZero,
                *fill,
                *stroke,
                None,
                None,
                *stroke_width,
            ))
        }
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        } => {
            let has_fill = fill[3] > 0;
            Some((
                ensure_filled_closed(commands.clone(), has_fill),
                *fill_rule,
                *fill,
                *stroke,
                None,
                None,
                *stroke_width,
            ))
        }
        ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
        } => {
            let has_fill = fill[3] > 0 || fill_gradient.is_some();
            Some((
                ensure_filled_closed(commands.clone(), has_fill),
                *fill_rule,
                *fill,
                *stroke,
                fill_gradient.clone(),
                stroke_gradient.clone(),
                *stroke_width,
            ))
        }
        _ => None,
    }
}

pub(super) fn ensure_filled_closed(mut cmds: Vec<PathCommand>, has_fill: bool) -> Vec<PathCommand> {
    if has_fill && !cmds.is_empty() && !matches!(cmds.last(), Some(PathCommand::Close)) {
        cmds.push(PathCommand::Close);
    }
    cmds
}

pub(super) fn build_static_shape_group_for_element(
    _project: &Project,
    el: &Element,
    idx: usize,
    force_opaque_matte: bool,
) -> Option<AeShapeGroup> {
    let (cmds, fill_rule, mut fill, stroke, fill_grad, _stroke_grad, stroke_width) =
        element_vector_commands(el)?;
    if force_opaque_matte && fill[3] == 0 && fill_grad.is_none() {
        fill = [255, 255, 255, 255];
    }
    let contours = path_commands_to_ae_contours(&cmds, [0.0, 0.0]);
    if contours.is_empty() {
        return None;
    }
    let contour_tracks: Vec<Vec<AeKey<AeBezierContour>>> = contours
        .into_iter()
        .map(|c| {
            vec![AeKey {
                frame: 0,
                value: c,
                ease: AeEase::default(),
            }]
        })
        .collect();

    let even_odd = fill_rule == FillRule::EvenOdd;
    let mut fills = Vec::new();
    let main_fill = build_ae_fill(fill, fill_grad.as_ref(), even_odd);
    if !matches!(main_fill, AePaintFill::None) {
        fills.push(main_fill);
    }

    let mut strokes = Vec::new();
    if stroke_width > 0.0 && stroke[3] > 0 {
        strokes.push(AePaintStroke {
            color: color_rgb01(stroke),
            opacity: (f64::from(stroke[3]) / 255.0) * 100.0,
            width: stroke_width,
            dashes: None,
        });
    }

    Some(AeShapeGroup {
        name: if el.name.trim().is_empty() {
            format!("Vector Shape {}", idx + 1)
        } else {
            el.name.clone()
        },
        contour_tracks,
        fills,
        strokes,
        modifiers: Vec::new(),
    })
}

pub(super) fn build_ae_fill(
    fill: Color,
    gradient: Option<&GradientPaint>,
    even_odd: bool,
) -> AePaintFill {
    if let Some(grad) = gradient
        && !grad.stops.is_empty()
    {
        let [a, b, c, d, tx, ty] = grad.matrix;
        // Canonical Flash gradient box spans `-819.2..+819.2`.
        let start = [a * -819.2 + tx, b * -819.2 + ty];
        let end = match grad.kind {
            GradientKind::Linear => [a * 819.2 + tx, b * 819.2 + ty],
            GradientKind::Radial => {
                let rx = (a * 819.2).hypot(b * 819.2);
                let ry = (c * 819.2).hypot(d * 819.2);
                let r = rx.max(ry).max(1.0);
                [tx + r, ty]
            }
        };
        let stops: Vec<(f64, [f64; 4])> = grad
            .stops
            .iter()
            .map(|s| {
                let pos = (f64::from(s.ratio) / 255.0).clamp(0.0, 1.0);
                let rgb = color_rgb01(s.color);
                let alpha = (f64::from(s.color[3]) / 255.0).clamp(0.0, 1.0);
                (pos, [rgb[0], rgb[1], rgb[2], alpha])
            })
            .collect();
        return AePaintFill::Gradient {
            radial: grad.kind == GradientKind::Radial,
            start: if grad.kind == GradientKind::Radial {
                [tx, ty]
            } else {
                start
            },
            end,
            highlight_length: (grad.focal_point_ratio * 100.0).clamp(-99.0, 99.0),
            stops,
            opacity: 100.0,
            even_odd,
        };
    }

    if fill[3] == 0 {
        return AePaintFill::None;
    }
    AePaintFill::Solid {
        color: color_rgb01(fill),
        opacity: (f64::from(fill[3]) / 255.0) * 100.0,
        even_odd,
    }
}

pub(super) fn apply_element_appearance_to_group(
    grp: &mut AeShapeGroup,
    app: &neoflash_core::ElementAppearance,
) {
    if !app.entries.is_empty() {
        grp.fills.clear();
        grp.strokes.clear();
        for entry in &app.entries {
            match entry {
                AppearancePaintEntry::Fill {
                    color,
                    gradient,
                    opacity,
                    ..
                } => {
                    let mut f = build_ae_fill(*color, gradient.as_ref(), false);
                    match &mut f {
                        AePaintFill::Solid { opacity: op, .. }
                        | AePaintFill::Gradient { opacity: op, .. } => {
                            *op = (*op * opacity.clamp(0.0, 1.0)).clamp(0.0, 100.0);
                        }
                        AePaintFill::None => {}
                    }
                    if !matches!(f, AePaintFill::None) {
                        grp.fills.push(f);
                    }
                }
                AppearancePaintEntry::Stroke {
                    color,
                    width,
                    pattern_brush,
                    opacity,
                    ..
                } => {
                    if *width > 0.0 && color[3] > 0 {
                        let dashes = match pattern_brush {
                            Some(neoflash_core::PatternBrushKind::DashPattern { dash, gap }) => {
                                Some([dash.max(0.5), gap.max(0.5)])
                            }
                            _ => None,
                        };
                        grp.strokes.push(AePaintStroke {
                            color: color_rgb01(*color),
                            opacity: ((f64::from(color[3]) / 255.0)
                                * opacity.clamp(0.0, 1.0)
                                * 100.0)
                                .clamp(0.0, 100.0),
                            width: *width,
                            dashes,
                        });
                    }
                }
            }
        }
    }

    for modifier in &app.modifiers {
        match *modifier {
            VectorModifier::Repeater {
                copies,
                offset_x,
                offset_y,
                scale_step,
                rotation_step,
            } => {
                grp.modifiers.push(AeVectorModifier::Repeater {
                    copies: copies.max(1),
                    offset_x,
                    offset_y,
                    scale_pct: (scale_step * 100.0).clamp(1.0, 1000.0),
                    rotation: rotation_step,
                });
            }
            VectorModifier::OffsetContour { delta } => {
                grp.modifiers
                    .push(AeVectorModifier::OffsetPaths { amount: delta });
            }
            VectorModifier::LiveCorners { radius } => {
                grp.modifiers.push(AeVectorModifier::RoundCorners {
                    radius: radius.max(0.0),
                });
            }
            VectorModifier::Mirror { .. } => {}
        }
    }
}
