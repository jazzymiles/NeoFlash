//! Standalone SVG 1.1 / 2.0 vector publishing (`T-110`).
//!
//! Exports any frame, symbol, or timeline frame sequence of a `Project` to deterministic,
//! self-contained SVG markup with `<defs>` gradient/clip-path/filter definitions, vector paths,
//! affine transforms, embedded bitmaps, and text/outline rendering.

use crate::{ExportError, atomic_write};
use neoflash_core::{
    AuthoredDraw, AuthoredScene, Color, ColorTransform, ElementKind, FillRule, FilterSpec,
    GradientKind, GradientPaint, PathCommand, Project, evaluate_scene, flatten_scenes_with_ranges,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

/// Options for standalone SVG export (`T-110`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SvgOptions {
    /// Target symbol ID; `None` exports the flattened main timeline (or `project.root`).
    pub symbol_id: Option<String>,
    /// Zero-based frame index to export.
    pub frame: u32,
    /// Optional inclusive end frame when exporting a numbered SVG sequence (`frame..=end_frame`).
    pub end_frame: Option<u32>,
    /// Output width in pixels (`None` uses `project.width`).
    pub width: Option<u32>,
    /// Output height in pixels (`None` uses `project.height`).
    pub height: Option<u32>,
    /// Omit the opaque stage background rectangle when `true`.
    pub transparent_background: bool,
    /// Include authored guide layers in the exported scene. Hidden guides must be made visible
    /// explicitly by the higher-level export workflow; ordinary hidden artwork stays hidden.
    #[serde(default)]
    pub include_guides: bool,
    /// Convert static text elements into `<path>` outlines using `static_text_to_outline_commands`.
    pub text_as_outlines: bool,
    /// Coordinate decimal precision (`1..=6`, default `2`).
    pub precision: usize,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            symbol_id: None,
            frame: 0,
            end_frame: None,
            width: None,
            height: None,
            transparent_background: false,
            include_guides: false,
            text_as_outlines: false,
            precision: 2,
        }
    }
}

/// Summary of an SVG export operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SvgExportReport {
    pub files: Vec<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    pub element_count: usize,
    pub defs_count: usize,
}

fn fmt_num(v: f64, prec: usize) -> String {
    let p = prec.clamp(1, 6);
    let s = format!("{v:.p$}");
    if s.contains('.') {
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        if trimmed == "-0" || trimmed.is_empty() {
            "0".to_owned()
        } else {
            trimmed.to_owned()
        }
    } else {
        s
    }
}

fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn apply_color_transform(c: Color, ct: ColorTransform) -> Color {
    if ct.is_identity() {
        return c;
    }
    [
        (f64::from(c[0]) * ct.multipliers[0] + ct.offsets[0])
            .round()
            .clamp(0.0, 255.0) as u8,
        (f64::from(c[1]) * ct.multipliers[1] + ct.offsets[1])
            .round()
            .clamp(0.0, 255.0) as u8,
        (f64::from(c[2]) * ct.multipliers[2] + ct.offsets[2])
            .round()
            .clamp(0.0, 255.0) as u8,
        (f64::from(c[3]) * ct.multipliers[3] + ct.offsets[3])
            .round()
            .clamp(0.0, 255.0) as u8,
    ]
}

fn svg_color_hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn svg_path_d(commands: &[PathCommand], prec: usize) -> String {
    let mut d = String::new();
    for cmd in commands {
        if !d.is_empty() {
            d.push(' ');
        }
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                let _ = write!(d, "M {} {}", fmt_num(x, prec), fmt_num(y, prec));
            }
            PathCommand::LineTo { x, y } => {
                let _ = write!(d, "L {} {}", fmt_num(x, prec), fmt_num(y, prec));
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let _ = write!(
                    d,
                    "Q {} {} {} {}",
                    fmt_num(cx, prec),
                    fmt_num(cy, prec),
                    fmt_num(x, prec),
                    fmt_num(y, prec)
                );
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let _ = write!(
                    d,
                    "C {} {} {} {} {} {}",
                    fmt_num(cx1, prec),
                    fmt_num(cy1, prec),
                    fmt_num(cx2, prec),
                    fmt_num(cy2, prec),
                    fmt_num(x, prec),
                    fmt_num(y, prec)
                );
            }
            PathCommand::Close => {
                d.push('Z');
            }
        }
    }
    d
}

fn is_identity_matrix(m: [f64; 6]) -> bool {
    (m[0] - 1.0).abs() < 1e-6
        && m[1].abs() < 1e-6
        && m[2].abs() < 1e-6
        && (m[3] - 1.0).abs() < 1e-6
        && m[4].abs() < 1e-6
        && m[5].abs() < 1e-6
}

fn svg_matrix_attr(m: [f64; 6], prec: usize) -> String {
    if is_identity_matrix(m) {
        String::new()
    } else {
        format!(
            " transform=\"matrix({} {} {} {} {} {})\"",
            fmt_num(m[0], prec),
            fmt_num(m[1], prec),
            fmt_num(m[2], prec),
            fmt_num(m[3], prec),
            fmt_num(m[4], prec),
            fmt_num(m[5], prec),
        )
    }
}

fn emit_gradient_def(
    defs: &mut String,
    id: &str,
    grad: &GradientPaint,
    ct: ColorTransform,
    prec: usize,
) {
    let [a, b, c, d, tx, ty] = grad.matrix;
    let xform = format!(
        "matrix({} {} {} {} {} {})",
        fmt_num(a, prec),
        fmt_num(b, prec),
        fmt_num(c, prec),
        fmt_num(d, prec),
        fmt_num(tx, prec),
        fmt_num(ty, prec),
    );
    match grad.kind {
        GradientKind::Linear => {
            let _ = writeln!(
                defs,
                "    <linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"-819.2\" y1=\"0\" x2=\"819.2\" y2=\"0\" gradientTransform=\"{xform}\">"
            );
            for stop in &grad.stops {
                let col = apply_color_transform(stop.color, ct);
                let offset = fmt_num(f64::from(stop.ratio) / 255.0 * 100.0, prec);
                let alpha = fmt_num(f64::from(col[3]) / 255.0, prec);
                let _ = writeln!(
                    defs,
                    "      <stop offset=\"{offset}%\" stop-color=\"{}\" stop-opacity=\"{alpha}\"/>",
                    svg_color_hex(col)
                );
            }
            let _ = writeln!(defs, "    </linearGradient>");
        }
        GradientKind::Radial => {
            let fx = fmt_num(grad.focal_point_ratio * 819.2, prec);
            let _ = writeln!(
                defs,
                "    <radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"0\" cy=\"0\" r=\"819.2\" fx=\"{fx}\" fy=\"0\" gradientTransform=\"{xform}\">"
            );
            for stop in &grad.stops {
                let col = apply_color_transform(stop.color, ct);
                let offset = fmt_num(f64::from(stop.ratio) / 255.0 * 100.0, prec);
                let alpha = fmt_num(f64::from(col[3]) / 255.0, prec);
                let _ = writeln!(
                    defs,
                    "      <stop offset=\"{offset}%\" stop-color=\"{}\" stop-opacity=\"{alpha}\"/>",
                    svg_color_hex(col)
                );
            }
            let _ = writeln!(defs, "    </radialGradient>");
        }
    }
}

fn emit_filter_def(defs: &mut String, id: &str, filters: &[FilterSpec], prec: usize) {
    let _ = writeln!(
        defs,
        "    <filter id=\"{id}\" x=\"-25%\" y=\"-25%\" width=\"150%\" height=\"150%\">"
    );
    for spec in filters {
        match spec {
            FilterSpec::Blur { blur_x, blur_y, .. } => {
                let sx = fmt_num((blur_x * 0.5).max(0.0), prec);
                let sy = fmt_num((blur_y * 0.5).max(0.0), prec);
                let _ = writeln!(defs, "      <feGaussianBlur stdDeviation=\"{sx} {sy}\"/>");
            }
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                ..
            } => {
                let rad = angle.to_radians();
                let dx = fmt_num(distance * rad.cos(), prec);
                let dy = fmt_num(distance * rad.sin(), prec);
                let std_dev = fmt_num(((blur_x + blur_y) * 0.25).max(0.0), prec);
                let hex = format!(
                    "#{:02x}{:02x}{:02x}",
                    (color >> 16) & 0xff,
                    (color >> 8) & 0xff,
                    color & 0xff
                );
                let op = fmt_num(alpha.clamp(0.0, 1.0), prec);
                let _ = writeln!(
                    defs,
                    "      <feDropShadow dx=\"{dx}\" dy=\"{dy}\" stdDeviation=\"{std_dev}\" flood-color=\"{hex}\" flood-opacity=\"{op}\"/>"
                );
            }
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                ..
            } => {
                let std_dev = fmt_num(((blur_x + blur_y) * 0.25).max(0.0), prec);
                let hex = format!(
                    "#{:02x}{:02x}{:02x}",
                    (color >> 16) & 0xff,
                    (color >> 8) & 0xff,
                    color & 0xff
                );
                let op = fmt_num(alpha.clamp(0.0, 1.0), prec);
                let _ = writeln!(
                    defs,
                    "      <feDropShadow dx=\"0\" dy=\"0\" stdDeviation=\"{std_dev}\" flood-color=\"{hex}\" flood-opacity=\"{op}\"/>"
                );
            }
            FilterSpec::ColorMatrix { matrix } => {
                let vals: Vec<String> = matrix
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let scaled = if i % 5 == 4 { *v / 255.0 } else { *v };
                        fmt_num(scaled, prec)
                    })
                    .collect();
                let _ = writeln!(
                    defs,
                    "      <feColorMatrix type=\"matrix\" values=\"{}\"/>",
                    vals.join(" ")
                );
            }
            _ => {}
        }
    }
    let _ = writeln!(defs, "    </filter>");
}

/// Render a single frame of `project` (or a specific symbol) to a standalone SVG string.
pub fn render_frame_svg(
    project: &Project,
    options: &SvgOptions,
) -> Result<(String, usize, usize), ExportError> {
    project
        .validate()
        .map_err(|e| ExportError::InvalidProject(e.to_string()))?;
    let eval_project = if options.symbol_id.is_none() && !project.scenes.is_empty() {
        flatten_scenes_with_ranges(project).0
    } else {
        project.clone()
    };
    let target_symbol = options
        .symbol_id
        .as_deref()
        .unwrap_or(eval_project.root.as_str());
    let scene: AuthoredScene = evaluate_scene(
        &eval_project,
        target_symbol,
        options.frame,
        options.include_guides,
        true,
    );

    let stage_w = eval_project.width.max(1);
    let stage_h = eval_project.height.max(1);
    let out_w = options.width.unwrap_or(stage_w).max(1);
    let out_h = options.height.unwrap_or(stage_h).max(1);
    let prec = options.precision.clamp(1, 6);

    let mut defs = String::new();
    let mut defs_count = 0usize;
    let mut clip_id_map: BTreeMap<String, String> = BTreeMap::new();

    for (ci, clip) in scene.clips.iter().enumerate() {
        let svg_clip_id = format!("nf-clip-{ci}");
        clip_id_map.insert(clip.id.clone(), svg_clip_id.clone());
        defs_count += 1;
        let _ = writeln!(defs, "    <clipPath id=\"{svg_clip_id}\">");
        for comp in &clip.components {
            let d = svg_path_d(&comp.commands, prec);
            if d.is_empty() {
                continue;
            }
            let rule = match comp.fill_rule {
                FillRule::NonZero => "nonzero",
                FillRule::EvenOdd => "evenodd",
            };
            let xf = svg_matrix_attr(comp.matrix, prec);
            let _ = writeln!(defs, "      <path d=\"{d}\" clip-rule=\"{rule}\"{xf}/>");
        }
        let _ = writeln!(defs, "    </clipPath>");
    }

    // Collect element filters by element ID for optional `<filter>` emission.
    let mut element_filters: BTreeMap<String, Vec<FilterSpec>> = BTreeMap::new();
    for sym in &eval_project.symbols {
        for layer in &sym.timeline.layers {
            for key in &layer.keyframes {
                for el in &key.elements {
                    if !el.filters.is_empty() {
                        element_filters.insert(el.id.clone(), el.filters.clone());
                    }
                }
            }
        }
    }

    let mut body = String::new();
    if !options.transparent_background && eval_project.background[3] > 0 {
        let bg = eval_project.background;
        let op = if bg[3] == 255 {
            String::new()
        } else {
            format!(
                " fill-opacity=\"{}\"",
                fmt_num(f64::from(bg[3]) / 255.0, prec)
            )
        };
        let _ = writeln!(
            body,
            "  <rect width=\"{stage_w}\" height=\"{stage_h}\" fill=\"{}\"{op}/>",
            svg_color_hex(bg)
        );
    }

    let mut grad_counter = 0usize;
    let mut filter_counter = 0usize;
    let mut element_count = 0usize;

    for draw in &scene.draws {
        element_count += 1;
        render_draw_svg(
            &eval_project,
            draw,
            options,
            prec,
            &clip_id_map,
            &element_filters,
            &mut grad_counter,
            &mut filter_counter,
            &mut defs_count,
            &mut defs,
            &mut body,
        );
    }

    let mut svg = String::with_capacity(defs.len() + body.len() + 256);
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{out_w}\" height=\"{out_h}\" viewBox=\"0 0 {stage_w} {stage_h}\">"
    );
    if !defs.is_empty() {
        let _ = writeln!(svg, "  <defs>\n{defs}  </defs>");
    }
    svg.push_str(&body);
    svg.push_str("</svg>\n");
    Ok((svg, element_count, defs_count))
}

#[allow(clippy::too_many_arguments)]
fn render_draw_svg(
    project: &Project,
    draw: &AuthoredDraw,
    options: &SvgOptions,
    prec: usize,
    clip_id_map: &BTreeMap<String, String>,
    element_filters: &BTreeMap<String, Vec<FilterSpec>>,
    grad_counter: &mut usize,
    filter_counter: &mut usize,
    defs_count: &mut usize,
    defs: &mut String,
    body: &mut String,
) {
    let item = &draw.item;
    let xf = svg_matrix_attr(item.transform, prec);
    let mut wrapper_opens = Vec::new();
    for cid in &draw.clip_ids {
        if let Some(svg_cid) = clip_id_map.get(cid) {
            wrapper_opens.push(format!("clip-path=\"url(#{svg_cid})\""));
        }
    }
    if !draw.blend_mode.is_normal() {
        wrapper_opens.push(format!(
            "style=\"mix-blend-mode:{}\"",
            draw.blend_mode.as_str()
        ));
    }
    if let Some(filters) = element_filters.get(&item.element_id)
        && !filters.is_empty()
    {
        let fid = format!("nf-filter-{}", *filter_counter);
        *filter_counter += 1;
        *defs_count += 1;
        emit_filter_def(defs, &fid, filters, prec);
        wrapper_opens.push(format!("filter=\"url(#{fid})\""));
    }

    let indent = if wrapper_opens.is_empty() {
        "  "
    } else {
        let _ = writeln!(body, "  <g {}>", wrapper_opens.join(" "));
        "    "
    };

    let ct = item.color_transform;
    let paint_attrs = |fill: Color,
                       stroke: Color,
                       stroke_width: f64,
                       fill_grad: Option<&GradientPaint>,
                       stroke_grad: Option<&GradientPaint>,
                       defs: &mut String,
                       grad_counter: &mut usize,
                       defs_count: &mut usize|
     -> String {
        let mut attrs = String::new();
        if let Some(fg) = fill_grad {
            let gid = format!("nf-grad-{}", *grad_counter);
            *grad_counter += 1;
            *defs_count += 1;
            emit_gradient_def(defs, &gid, fg, ct, prec);
            let _ = write!(attrs, " fill=\"url(#{gid})\"");
        } else {
            let fc = apply_color_transform(fill, ct);
            if fc[3] == 0 {
                attrs.push_str(" fill=\"none\"");
            } else {
                let _ = write!(attrs, " fill=\"{}\"", svg_color_hex(fc));
                if fc[3] < 255 {
                    let _ = write!(
                        attrs,
                        " fill-opacity=\"{}\"",
                        fmt_num(f64::from(fc[3]) / 255.0, prec)
                    );
                }
            }
        }
        if stroke_width > 0.0 {
            if let Some(sg) = stroke_grad {
                let gid = format!("nf-grad-{}", *grad_counter);
                *grad_counter += 1;
                *defs_count += 1;
                emit_gradient_def(defs, &gid, sg, ct, prec);
                let _ = write!(
                    attrs,
                    " stroke=\"url(#{gid})\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
                    fmt_num(stroke_width, prec)
                );
            } else {
                let sc = apply_color_transform(stroke, ct);
                if sc[3] > 0 {
                    let _ = write!(
                        attrs,
                        " stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
                        svg_color_hex(sc),
                        fmt_num(stroke_width, prec)
                    );
                    if sc[3] < 255 {
                        let _ = write!(
                            attrs,
                            " stroke-opacity=\"{}\"",
                            fmt_num(f64::from(sc[3]) / 255.0, prec)
                        );
                    }
                }
            }
        }
        attrs
    };

    match &item.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let attrs = paint_attrs(
                *fill,
                *stroke,
                *stroke_width,
                None,
                None,
                defs,
                grad_counter,
                defs_count,
            );
            let _ = writeln!(
                body,
                "{indent}<rect width=\"{}\" height=\"{}\"{attrs}{xf}/>",
                fmt_num(*width, prec),
                fmt_num(*height, prec),
            );
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let rx = *width * 0.5;
            let ry = *height * 0.5;
            let attrs = paint_attrs(
                *fill,
                *stroke,
                *stroke_width,
                None,
                None,
                defs,
                grad_counter,
                defs_count,
            );
            let _ = writeln!(
                body,
                "{indent}<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{attrs}{xf}/>",
                fmt_num(rx, prec),
                fmt_num(ry, prec),
                fmt_num(rx, prec),
                fmt_num(ry, prec),
            );
        }
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        } => {
            let d = svg_path_d(commands, prec);
            if !d.is_empty() {
                let rule = match fill_rule {
                    FillRule::NonZero => "",
                    FillRule::EvenOdd => " fill-rule=\"evenodd\"",
                };
                let attrs = paint_attrs(
                    *fill,
                    *stroke,
                    *stroke_width,
                    None,
                    None,
                    defs,
                    grad_counter,
                    defs_count,
                );
                let _ = writeln!(body, "{indent}<path d=\"{d}\"{rule}{attrs}{xf}/>");
            }
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
            let d = svg_path_d(commands, prec);
            if !d.is_empty() {
                let rule = match fill_rule {
                    FillRule::NonZero => "",
                    FillRule::EvenOdd => " fill-rule=\"evenodd\"",
                };
                let attrs = paint_attrs(
                    *fill,
                    *stroke,
                    *stroke_width,
                    fill_gradient.as_ref(),
                    stroke_gradient.as_ref(),
                    defs,
                    grad_counter,
                    defs_count,
                );
                let _ = writeln!(body, "{indent}<path d=\"{d}\"{rule}{attrs}{xf}/>");
            }
        }
        ElementKind::Bitmap {
            resource_id,
            width,
            height,
            ..
        } => {
            if let Some(res) = project.resources.iter().find(|r| r.id == *resource_id) {
                let href = format!("data:{};base64,{}", res.media_type, res.data_base64);
                let op = if item.alpha < 1.0 {
                    format!(" opacity=\"{}\"", fmt_num(item.alpha, prec))
                } else {
                    String::new()
                };
                let _ = writeln!(
                    body,
                    "{indent}<image width=\"{width}\" height=\"{height}\" href=\"{href}\"{op}{xf}/>"
                );
            }
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            let col = apply_color_transform(*color, ct);
            if options.text_as_outlines {
                let cmds = neoflash_core::TextFieldState::static_text_to_outline_commands(
                    text,
                    font_family,
                    *font_size,
                    None,
                );
                let d = svg_path_d(&cmds, prec);
                if !d.is_empty() {
                    let _ = writeln!(
                        body,
                        "{indent}<path d=\"{d}\" fill=\"{}\"{xf}/>",
                        svg_color_hex(col)
                    );
                }
            } else {
                let _ = writeln!(
                    body,
                    "{indent}<text x=\"0\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\"{xf}>{}</text>",
                    fmt_num(*font_size, prec),
                    xml_escape(font_family),
                    fmt_num(*font_size, prec),
                    svg_color_hex(col),
                    xml_escape(text)
                );
            }
        }
        ElementKind::TextBox { text, format, .. } => {
            let raw_color = [
                ((format.color >> 16) & 0xff) as u8,
                ((format.color >> 8) & 0xff) as u8,
                (format.color & 0xff) as u8,
                255,
            ];
            let col = apply_color_transform(raw_color, ct);
            let _ = writeln!(
                body,
                "{indent}<text x=\"0\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\"{xf}>{}</text>",
                fmt_num(format.size, prec),
                xml_escape(&format.font),
                fmt_num(format.size, prec),
                svg_color_hex(col),
                xml_escape(text)
            );
        }
        ElementKind::Video { width, height } => {
            let _ = writeln!(
                body,
                "{indent}<rect width=\"{}\" height=\"{}\" fill=\"#000000\" fill-opacity=\"0.25\"{xf}/>",
                fmt_num(*width, prec),
                fmt_num(*height, prec)
            );
        }
        ElementKind::Instance { .. } => {}
    }

    if !wrapper_opens.is_empty() {
        let _ = writeln!(body, "  </g>");
    }
}

/// Export a frame or frame range of `project` to standalone `.svg` file(s) on disk.
pub fn export_project_svg(
    project: &Project,
    output_path: impl AsRef<Path>,
    options: &SvgOptions,
) -> Result<SvgExportReport, ExportError> {
    let output_path = output_path.as_ref();
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let out_w = options.width.unwrap_or(project.width).max(1);
    let out_h = options.height.unwrap_or(project.height).max(1);
    let end_frame = options
        .end_frame
        .unwrap_or(options.frame)
        .max(options.frame);
    let mut files = Vec::new();
    let mut total_elements = 0usize;
    let mut total_defs = 0usize;

    if end_frame == options.frame {
        let (svg, elems, defs) = render_frame_svg(project, options)?;
        atomic_write(output_path, svg.as_bytes())?;
        files.push(output_path.to_path_buf());
        total_elements += elems;
        total_defs += defs;
    } else {
        let stem = output_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("frame");
        let dir = output_path.parent().unwrap_or_else(|| Path::new("."));
        for f in options.frame..=end_frame {
            let mut frame_opts = options.clone();
            frame_opts.frame = f;
            frame_opts.end_frame = None;
            let (svg, elems, defs) = render_frame_svg(project, &frame_opts)?;
            let file_path = dir.join(format!("{stem}_{:04}.svg", f + 1));
            atomic_write(&file_path, svg.as_bytes())?;
            files.push(file_path);
            total_elements += elems;
            total_defs += defs;
        }
    }

    Ok(SvgExportReport {
        frame_count: files.len() as u32,
        files,
        width: out_w,
        height: out_h,
        element_count: total_elements,
        defs_count: total_defs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{
        Element, GradientStop, Keyframe, Layer, Symbol, SymbolKind, Timeline, Transform, new_id,
    };

    #[test]
    fn svg_export_renders_gradients_filters_paths_and_text_deterministically() {
        let mut project = Project::demo();
        let grad = GradientPaint {
            kind: GradientKind::Linear,
            matrix: [1.0, 0.0, 0.0, 1.0, 100.0, 100.0],
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    ratio: 255,
                    color: [0, 0, 255, 255],
                },
            ],
            focal_point_ratio: 0.0,
            ..Default::default()
        };
        let mut elem = Element {
            id: new_id("grad-shape"),
            name: "GradBox".into(),
            transform: Transform::default(),
            kind: ElementKind::PaintedPath {
                commands: vec![
                    PathCommand::MoveTo { x: 10.0, y: 10.0 },
                    PathCommand::LineTo { x: 110.0, y: 10.0 },
                    PathCommand::LineTo { x: 110.0, y: 90.0 },
                    PathCommand::Close,
                ],
                fill_rule: FillRule::EvenOdd,
                fill: [255, 0, 0, 255],
                stroke: [0, 0, 0, 255],
                fill_gradient: Some(grad),
                stroke_gradient: None,
                stroke_width: 2.0,
            },
            ..Element::default()
        };
        elem.filters.push(FilterSpec::Blur {
            blur_x: 4.0,
            blur_y: 4.0,
            quality: 1,
        });
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(elem);
        project.upgrade_format_for_features();

        let opts = SvgOptions::default();
        let (svg1, elems, defs) = render_frame_svg(&project, &opts).expect("svg 1");
        let (svg2, _, _) = render_frame_svg(&project, &opts).expect("svg 2");
        assert_eq!(svg1, svg2, "SVG output must be deterministic");
        assert!(elems >= 2);
        assert!(defs >= 2);
        assert!(svg1.contains("<linearGradient id=\"nf-grad-0\""));
        assert!(svg1.contains("<feGaussianBlur stdDeviation=\"2 2\"/>"));
        assert!(svg1.contains("fill-rule=\"evenodd\""));
    }

    #[test]
    fn svg_export_supports_symbol_scope_and_text_outlines() {
        let mut project = Project::blank();
        let sym_id = new_id("badge-sym");
        project.symbols.push(Symbol {
            id: sym_id.clone(),
            name: "Badge".into(),
            folder: String::new(),
            kind: SymbolKind::Graphic,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: new_id("layer"),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: new_id("txt"),
                            name: "Label".into(),
                            transform: Transform::default(),
                            kind: ElementKind::Text {
                                text: "HI".into(),
                                font_family: "Inter".into(),
                                font_size: 24.0,
                                color: [20, 40, 60, 255],
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                    properties: Default::default(),
                }],
                scripts: vec![],
                guides: vec![],
            },
            class_name: None,
            linkage_id: None,
        });
        let opts = SvgOptions {
            symbol_id: Some(sym_id),
            text_as_outlines: true,
            transparent_background: true,
            ..SvgOptions::default()
        };
        let (svg, elems, _) = render_frame_svg(&project, &opts).expect("symbol svg");
        assert_eq!(elems, 1);
        assert!(svg.contains("<path d=\"M "));
        assert!(!svg.contains("<text "));
    }
}
