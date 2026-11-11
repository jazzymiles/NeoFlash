//! Zero-WASM `Web Banner · Native` HTML5 ad compiler (`T-216`).
//!
//! Lowers eligible vector artwork, bitmaps, text, nested symbols, classic/motion tweens, masks,
//! filters, and scenes into a single self-contained `.html` file (`inline <svg>` with shared
//! geometry `<defs>` + compact frame scheduler + standard `window.clickTag`) with zero WebAssembly,
//! zero external requests, located eligibility diagnostics, and strict raw/gzip byte budgets.

use crate::{ExportError, atomic_write, gzip_len, html_escape};
use neoflash_core::{
    AuthoredDraw, AuthoredPlayback, BlendMode, Color, ColorTransform, ElementKind, FillRule,
    FilterSpec, GradientKind, GradientPaint, PathCommand, Project, TweenKind,
    flatten_scenes_with_ranges,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

/// Default IAB / Google Ads polite banner gzip ceiling (`150 KiB`).
pub const DEFAULT_BANNER_MAX_GZIP_BYTES: usize = 150 * 1024;

/// Options for [`export_native_banner_html`] and [`estimate_native_banner`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BannerOptions {
    /// Default click-through URL assigned to `window.clickTag` when the ad container does not override it.
    pub click_tag_url: Option<String>,
    /// Start playback automatically on load.
    pub autoplay: bool,
    /// Loop playback (`true` = infinite unless `max_loops` > 0).
    pub loop_timeline: bool,
    /// Optional loop limit (`0` = unlimited when `loop_timeline` is `true`, e.g. `3` for 30s ad caps).
    pub max_loops: u32,
    /// Convert static text to vector `<path>` outlines so no webfont requests are needed.
    pub text_as_outlines: bool,
    /// Coordinate decimal precision (`1..=4`, default `2`).
    pub precision: usize,
    /// Maximum allowed gzip byte size (`None` disables the gzip gate; default `150 KiB`).
    pub max_gzip_bytes: Option<usize>,
    /// Maximum allowed raw HTML byte size (`None` disables the raw gate).
    pub max_raw_bytes: Option<usize>,
    /// Re-encode JPEG/bitmap resources at this quality (`1..=100`, default `80`).
    pub jpeg_quality: u8,
}

impl Default for BannerOptions {
    fn default() -> Self {
        Self {
            click_tag_url: None,
            autoplay: true,
            loop_timeline: true,
            max_loops: 0,
            text_as_outlines: true,
            precision: 2,
            max_gzip_bytes: Some(DEFAULT_BANNER_MAX_GZIP_BYTES),
            max_raw_bytes: None,
            jpeg_quality: 80,
        }
    }
}

/// Located eligibility or budget diagnostic emitted by the native banner compiler.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BannerDiagnostic {
    pub blocking: bool,
    pub code: String,
    pub location: String,
    pub message: String,
}

/// Pre-publish size and eligibility estimate for `Web Banner · Native`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BannerEstimate {
    pub eligible: bool,
    pub raw_bytes: usize,
    pub gzip_bytes: usize,
    pub frame_count: u32,
    pub unique_holds: usize,
    pub shared_defs: usize,
    pub within_budget: bool,
    pub diagnostics: Vec<BannerDiagnostic>,
}

/// Result of writing a `Web Banner · Native` single-file `.html` package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BannerExportReport {
    pub html_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frame_count: u32,
    pub raw_bytes: usize,
    pub gzip_bytes: usize,
    pub shared_defs: usize,
    pub diagnostics: Vec<BannerDiagnostic>,
}

/// Inspect `project` for features that cannot run in the zero-WASM native banner target.
pub fn preflight_native_banner(project: &Project) -> Vec<BannerDiagnostic> {
    let mut diagnostics = Vec::new();

    for source in &project.sources {
        if !source.source.trim().is_empty() {
            diagnostics.push(BannerDiagnostic {
                blocking: true,
                code: "banner-unsupported-script".into(),
                location: format!("Source '{}'", source.path),
                message: "Executable scripts require the standard HTML5 WASM runtime; remove scripts or publish with the Standard Web profile.".into(),
            });
        }
    }

    for symbol in &project.symbols {
        for script in &symbol.timeline.scripts {
            if !script.script.source.trim().is_empty() {
                diagnostics.push(BannerDiagnostic {
                    blocking: true,
                    code: "banner-unsupported-frame-script".into(),
                    location: format!("Symbol '{}', frame {}", symbol.name, script.frame + 1),
                    message: "Timeline frame scripts are not supported in Web Banner · Native (no-WASM) output.".into(),
                });
            }
        }
        for layer in &symbol.timeline.layers {
            for kf in &layer.keyframes {
                let loc = format!(
                    "Symbol '{}', layer '{}', frame {}",
                    symbol.name,
                    layer.name,
                    kf.frame + 1
                );
                if kf.tween.is_some() && kf.tween_kind == TweenKind::Shape {
                    diagnostics.push(BannerDiagnostic {
                        blocking: true,
                        code: "banner-unsupported-shape-tween".into(),
                        location: loc.clone(),
                        message: "Shape morph tweens require convert-to-keyframes or the standard HTML5 runtime before native banner export.".into(),
                    });
                }
                if !kf.ik_armatures.is_empty() {
                    diagnostics.push(BannerDiagnostic {
                        blocking: true,
                        code: "banner-unsupported-ik".into(),
                        location: loc.clone(),
                        message:
                            "Inverse kinematics armatures are not supported in Web Banner · Native."
                                .into(),
                    });
                }
                if !kf.asset_warps.is_empty() {
                    diagnostics.push(BannerDiagnostic {
                        blocking: true,
                        code: "banner-unsupported-asset-warp".into(),
                        location: loc.clone(),
                        message: "Mesh asset warps are not supported in Web Banner · Native."
                            .into(),
                    });
                }
                if kf.sound.is_some() {
                    diagnostics.push(BannerDiagnostic {
                        blocking: true,
                        code: "banner-unsupported-sound".into(),
                        location: loc.clone(),
                        message: "Timeline audio is disallowed in autoplay ad banners; remove frame sounds or use Standard Web.".into(),
                    });
                }
                if kf.video.is_some() {
                    diagnostics.push(BannerDiagnostic {
                        blocking: true,
                        code: "banner-unsupported-video".into(),
                        location: loc.clone(),
                        message: "Timeline video spans are not supported in single-file Web Banner · Native.".into(),
                    });
                }
                for el in &kf.elements {
                    let el_loc = format!(
                        "{loc}, element '{}'",
                        if el.name.is_empty() { &el.id } else { &el.name }
                    );
                    if !el.scripts.is_empty() {
                        diagnostics.push(BannerDiagnostic {
                            blocking: true,
                            code: "banner-unsupported-object-script".into(),
                            location: el_loc.clone(),
                            message: "Instance action handlers (`on(...)` / `onClipEvent(...)`) are not supported in Web Banner · Native.".into(),
                        });
                    }
                    if matches!(el.kind, ElementKind::Video { .. }) {
                        diagnostics.push(BannerDiagnostic {
                            blocking: true,
                            code: "banner-unsupported-video-element".into(),
                            location: el_loc,
                            message: "Video elements are not supported in Web Banner · Native."
                                .into(),
                        });
                    }
                }
            }
        }
    }

    diagnostics
}

fn fmt_num(v: f64, prec: usize) -> String {
    let p = prec.clamp(1, 4);
    let s = format!("{v:.p$}");
    if s.contains('.') {
        let t = s.trim_end_matches('0').trim_end_matches('.');
        if t.is_empty() || t == "-0" {
            "0".to_owned()
        } else {
            t.to_owned()
        }
    } else {
        s
    }
}

fn color_hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn apply_ct(c: Color, ct: ColorTransform) -> Color {
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

fn path_d(commands: &[PathCommand], prec: usize) -> String {
    let mut d = String::new();
    for cmd in commands {
        if !d.is_empty() {
            d.push(' ');
        }
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                let _ = write!(d, "M{} {}", fmt_num(x, prec), fmt_num(y, prec));
            }
            PathCommand::LineTo { x, y } => {
                let _ = write!(d, "L{} {}", fmt_num(x, prec), fmt_num(y, prec));
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let _ = write!(
                    d,
                    "Q{} {} {} {}",
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
                    "C{} {} {} {} {} {}",
                    fmt_num(cx1, prec),
                    fmt_num(cy1, prec),
                    fmt_num(cx2, prec),
                    fmt_num(cy2, prec),
                    fmt_num(x, prec),
                    fmt_num(y, prec)
                );
            }
            PathCommand::Close => d.push('Z'),
        }
    }
    d
}

fn matrix_attr(m: [f64; 6], prec: usize) -> String {
    let is_id = (m[0] - 1.0).abs() < 1e-6
        && m[1].abs() < 1e-6
        && m[2].abs() < 1e-6
        && (m[3] - 1.0).abs() < 1e-6
        && m[4].abs() < 1e-6
        && m[5].abs() < 1e-6;
    if is_id {
        String::new()
    } else if m[1].abs() < 1e-6
        && m[2].abs() < 1e-6
        && (m[0] - 1.0).abs() < 1e-6
        && (m[3] - 1.0).abs() < 1e-6
    {
        format!(
            " transform=\"translate({},{})\"",
            fmt_num(m[4], prec),
            fmt_num(m[5], prec)
        )
    } else {
        format!(
            " transform=\"matrix({},{},{},{},{},{})\"",
            fmt_num(m[0], prec),
            fmt_num(m[1], prec),
            fmt_num(m[2], prec),
            fmt_num(m[3], prec),
            fmt_num(m[4], prec),
            fmt_num(m[5], prec)
        )
    }
}

#[derive(Default)]
struct SharedBannerDefs {
    defs_markup: String,
    paths: BTreeMap<String, String>,
    gradients: BTreeMap<String, String>,
    images: BTreeMap<String, String>,
    clips: BTreeMap<String, String>,
    filters: BTreeMap<String, String>,
    count: usize,
}

impl SharedBannerDefs {
    fn intern_path(&mut self, d: &str) -> String {
        if let Some(id) = self.paths.get(d) {
            return id.clone();
        }
        let id = format!("p{}", self.paths.len());
        let _ = writeln!(self.defs_markup, "<path id=\"{id}\" d=\"{d}\"/>");
        self.paths.insert(d.to_owned(), id.clone());
        self.count += 1;
        id
    }

    fn intern_gradient(
        &mut self,
        paint: &GradientPaint,
        ct: ColorTransform,
        prec: usize,
    ) -> String {
        let mut body = String::new();
        let m = paint.matrix;
        let grad_xf = format!(
            "gradientTransform=\"matrix({},{},{},{},{},{})\"",
            fmt_num(m[0], prec),
            fmt_num(m[1], prec),
            fmt_num(m[2], prec),
            fmt_num(m[3], prec),
            fmt_num(m[4], prec),
            fmt_num(m[5], prec),
        );
        let stops: String = paint
            .stops
            .iter()
            .map(|stop| {
                let c = apply_ct(stop.color, ct);
                let offset = fmt_num(f64::from(stop.ratio) / 255.0 * 100.0, 1);
                let op = if c[3] == 255 {
                    String::new()
                } else {
                    format!(" stop-opacity=\"{}\"", fmt_num(f64::from(c[3]) / 255.0, 2))
                };
                format!(
                    "<stop offset=\"{offset}%\" stop-color=\"{}\"{op}/>",
                    color_hex(c)
                )
            })
            .collect();
        match paint.kind {
            GradientKind::Linear => {
                let _ = write!(body, "L:{grad_xf}:{stops}");
            }
            GradientKind::Radial => {
                let fx = fmt_num(paint.focal_point_ratio.clamp(-0.98, 0.98) * 819.2, prec);
                let _ = write!(body, "R:{fx}:{grad_xf}:{stops}");
            }
        }
        if let Some(id) = self.gradients.get(&body) {
            return id.clone();
        }
        let id = format!("g{}", self.gradients.len());
        match paint.kind {
            GradientKind::Linear => {
                let _ = writeln!(
                    self.defs_markup,
                    "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"-819.2\" y1=\"0\" x2=\"819.2\" y2=\"0\" {grad_xf}>{stops}</linearGradient>"
                );
            }
            GradientKind::Radial => {
                let fx = fmt_num(paint.focal_point_ratio.clamp(-0.98, 0.98) * 819.2, prec);
                let _ = writeln!(
                    self.defs_markup,
                    "<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"0\" cy=\"0\" r=\"819.2\" fx=\"{fx}\" fy=\"0\" {grad_xf}>{stops}</radialGradient>"
                );
            }
        }
        self.gradients.insert(body, id.clone());
        self.count += 1;
        id
    }

    fn intern_image(
        &mut self,
        project: &Project,
        resource_id: &str,
        w: u32,
        h: u32,
    ) -> Option<String> {
        let key = format!("{resource_id}:{w}x{h}");
        if let Some(id) = self.images.get(&key) {
            return Some(id.clone());
        }
        let res = project.resources.iter().find(|r| r.id == resource_id)?;
        let id = format!("b{}", self.images.len());
        let _ = writeln!(
            self.defs_markup,
            "<image id=\"{id}\" width=\"{w}\" height=\"{h}\" href=\"data:{};base64,{}\"/>",
            html_escape(&res.media_type),
            res.data_base64
        );
        self.images.insert(key, id.clone());
        self.count += 1;
        Some(id)
    }

    fn intern_filter(&mut self, specs: &[FilterSpec], prec: usize) -> Option<String> {
        if specs.is_empty() {
            return None;
        }
        let mut inner = String::new();
        for f in specs {
            match *f {
                FilterSpec::Blur { blur_x, blur_y, .. } => {
                    let sx = fmt_num((blur_x * 0.5).max(0.0), prec);
                    let sy = fmt_num((blur_y * 0.5).max(0.0), prec);
                    let _ = write!(inner, "<feGaussianBlur stdDeviation=\"{sx} {sy}\"/>");
                }
                FilterSpec::DropShadow {
                    distance,
                    angle,
                    color,
                    alpha,
                    blur_x,
                    ..
                } => {
                    let rad = angle.to_radians();
                    let dx = fmt_num(distance * rad.cos(), prec);
                    let dy = fmt_num(distance * rad.sin(), prec);
                    let std_dev = fmt_num((blur_x * 0.5).max(0.0), prec);
                    let c: Color = [
                        ((color >> 16) & 0xFF) as u8,
                        ((color >> 8) & 0xFF) as u8,
                        (color & 0xFF) as u8,
                        255,
                    ];
                    let op = fmt_num(alpha.clamp(0.0, 1.0), 2);
                    let _ = write!(
                        inner,
                        "<feDropShadow dx=\"{dx}\" dy=\"{dy}\" stdDeviation=\"{std_dev}\" flood-color=\"{}\" flood-opacity=\"{op}\"/>",
                        color_hex(c)
                    );
                }
                FilterSpec::Glow {
                    color,
                    alpha,
                    blur_x,
                    ..
                } => {
                    let std_dev = fmt_num((blur_x * 0.5).max(0.0), prec);
                    let c: Color = [
                        ((color >> 16) & 0xFF) as u8,
                        ((color >> 8) & 0xFF) as u8,
                        (color & 0xFF) as u8,
                        255,
                    ];
                    let op = fmt_num(alpha.clamp(0.0, 1.0), 2);
                    let _ = write!(
                        inner,
                        "<feDropShadow dx=\"0\" dy=\"0\" stdDeviation=\"{std_dev}\" flood-color=\"{}\" flood-opacity=\"{op}\"/>",
                        color_hex(c)
                    );
                }
                _ => {}
            }
        }
        if inner.is_empty() {
            return None;
        }
        if let Some(id) = self.filters.get(&inner) {
            return Some(id.clone());
        }
        let id = format!("flt{}", self.filters.len());
        let _ = writeln!(
            self.defs_markup,
            "<filter id=\"{id}\" x=\"-25%\" y=\"-25%\" width=\"150%\" height=\"150%\">{inner}</filter>"
        );
        self.filters.insert(inner, id.clone());
        self.count += 1;
        Some(id)
    }
}

#[allow(clippy::too_many_arguments)]
fn render_banner_draw(
    project: &Project,
    draw: &AuthoredDraw,
    options: &BannerOptions,
    element_filters: &BTreeMap<String, Vec<FilterSpec>>,
    frame_clips: &BTreeMap<String, String>,
    defs: &mut SharedBannerDefs,
    out: &mut String,
) {
    let item = &draw.item;
    let prec = options.precision.clamp(1, 4);
    let xf = matrix_attr(item.transform, prec);
    let alpha = item.alpha.clamp(0.0, 1.0);
    let ct = item.color_transform;

    let mut group_attrs = String::new();
    for cid in &draw.clip_ids {
        if let Some(svg_cid) = frame_clips.get(cid) {
            let _ = write!(group_attrs, " clip-path=\"url(#{svg_cid})\"");
        }
    }
    if !draw.blend_mode.is_normal() {
        let css_blend = match draw.blend_mode {
            BlendMode::Hardlight => "hard-light",
            BlendMode::Add => "plus-lighter",
            BlendMode::Normal | BlendMode::Layer => "normal",
            other => other.as_str(),
        };
        if css_blend != "normal" {
            let _ = write!(group_attrs, " style=\"mix-blend-mode:{css_blend}\"");
        }
    }
    if let Some(leaf_id) = item.instance_path.last()
        && let Some(specs) = element_filters.get(leaf_id)
        && let Some(fid) = defs.intern_filter(specs, prec)
    {
        let _ = write!(group_attrs, " filter=\"url(#{fid})\"");
    }
    if (alpha - 1.0).abs() > 1e-4 {
        let _ = write!(group_attrs, " opacity=\"{}\"", fmt_num(alpha, 2));
    }

    let wrapped = !group_attrs.is_empty();
    if wrapped {
        let _ = write!(out, "<g{group_attrs}>");
    }

    let paint_attrs = |fill: Color,
                       stroke: Color,
                       sw: f64,
                       fg: Option<&GradientPaint>,
                       sg: Option<&GradientPaint>,
                       defs: &mut SharedBannerDefs|
     -> String {
        let mut s = String::new();
        if let Some(g) = fg {
            let gid = defs.intern_gradient(g, ct, prec);
            let _ = write!(s, " fill=\"url(#{gid})\"");
        } else {
            let fc = apply_ct(fill, ct);
            if fc[3] == 0 {
                s.push_str(" fill=\"none\"");
            } else {
                let _ = write!(s, " fill=\"{}\"", color_hex(fc));
                if fc[3] < 255 {
                    let _ = write!(
                        s,
                        " fill-opacity=\"{}\"",
                        fmt_num(f64::from(fc[3]) / 255.0, 2)
                    );
                }
            }
        }
        if sw > 0.0 {
            if let Some(g) = sg {
                let gid = defs.intern_gradient(g, ct, prec);
                let _ = write!(
                    s,
                    " stroke=\"url(#{gid})\" stroke-width=\"{}\"",
                    fmt_num(sw, prec)
                );
            } else {
                let sc = apply_ct(stroke, ct);
                if sc[3] > 0 {
                    let _ = write!(
                        s,
                        " stroke=\"{}\" stroke-width=\"{}\"",
                        color_hex(sc),
                        fmt_num(sw, prec)
                    );
                    if sc[3] < 255 {
                        let _ = write!(
                            s,
                            " stroke-opacity=\"{}\"",
                            fmt_num(f64::from(sc[3]) / 255.0, 2)
                        );
                    }
                }
            }
        }
        s
    };

    match &item.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let pa = paint_attrs(*fill, *stroke, *stroke_width, None, None, defs);
            let _ = write!(
                out,
                "<rect width=\"{}\" height=\"{}\"{pa}{xf}/>",
                fmt_num(*width, prec),
                fmt_num(*height, prec)
            );
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let pa = paint_attrs(*fill, *stroke, *stroke_width, None, None, defs);
            let rx = *width * 0.5;
            let ry = *height * 0.5;
            let _ = write!(
                out,
                "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{pa}{xf}/>",
                fmt_num(rx, prec),
                fmt_num(ry, prec),
                fmt_num(rx, prec),
                fmt_num(ry, prec)
            );
        }
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        } => {
            let d = path_d(commands, prec);
            if !d.is_empty() {
                let pid = defs.intern_path(&d);
                let pa = paint_attrs(*fill, *stroke, *stroke_width, None, None, defs);
                let rule = if *fill_rule == FillRule::EvenOdd {
                    " fill-rule=\"evenodd\""
                } else {
                    ""
                };
                let _ = write!(out, "<use href=\"#{pid}\"{rule}{pa}{xf}/>");
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
            let d = path_d(commands, prec);
            if !d.is_empty() {
                let pid = defs.intern_path(&d);
                let pa = paint_attrs(
                    *fill,
                    *stroke,
                    *stroke_width,
                    fill_gradient.as_ref(),
                    stroke_gradient.as_ref(),
                    defs,
                );
                let rule = if *fill_rule == FillRule::EvenOdd {
                    " fill-rule=\"evenodd\""
                } else {
                    ""
                };
                let _ = write!(out, "<use href=\"#{pid}\"{rule}{pa}{xf}/>");
            }
        }
        ElementKind::Bitmap {
            resource_id,
            width,
            height,
            ..
        } => {
            if let Some(iid) = defs.intern_image(project, resource_id, *width, *height) {
                let _ = write!(out, "<use href=\"#{iid}\"{xf}/>");
            }
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            let tc = apply_ct(*color, ct);
            if options.text_as_outlines {
                let outlines = neoflash_core::TextFieldState::static_text_to_outline_commands(
                    text,
                    font_family,
                    *font_size,
                    None,
                );
                let d = path_d(&outlines, prec);
                if !d.is_empty() {
                    let pid = defs.intern_path(&d);
                    let _ = write!(out, "<use href=\"#{pid}\" fill=\"{}\"{xf}/>", color_hex(tc));
                }
            } else {
                let safe_font = html_escape(&font_family.replace('"', "'"));
                let _ = write!(
                    out,
                    "<text x=\"0\" y=\"{}\" font-family=\"{safe_font}\" font-size=\"{}\" fill=\"{}\"{xf}>{}</text>",
                    fmt_num(*font_size, prec),
                    fmt_num(*font_size, prec),
                    color_hex(tc),
                    html_escape(text)
                );
            }
        }
        ElementKind::TextBox { text, format, .. } => {
            let base: Color = [
                ((format.color >> 16) & 0xFF) as u8,
                ((format.color >> 8) & 0xFF) as u8,
                (format.color & 0xFF) as u8,
                255,
            ];
            let tc = apply_ct(base, ct);
            if options.text_as_outlines {
                let outlines = neoflash_core::TextFieldState::static_text_to_outline_commands(
                    text,
                    &format.font,
                    format.size,
                    None,
                );
                let d = path_d(&outlines, prec);
                if !d.is_empty() {
                    let pid = defs.intern_path(&d);
                    let _ = write!(out, "<use href=\"#{pid}\" fill=\"{}\"{xf}/>", color_hex(tc));
                }
            } else {
                let safe_font = html_escape(&format.font.replace('"', "'"));
                let _ = write!(
                    out,
                    "<text x=\"0\" y=\"{}\" font-family=\"{safe_font}\" font-size=\"{}\" fill=\"{}\"{xf}>{}</text>",
                    fmt_num(format.size, prec),
                    fmt_num(format.size, prec),
                    color_hex(tc),
                    html_escape(text)
                );
            }
        }
        ElementKind::Video { .. } | ElementKind::Instance { .. } => {}
    }

    if wrapped {
        out.push_str("</g>");
    }
}

fn compile_native_banner_html_inner(
    project: &Project,
    options: &BannerOptions,
) -> Result<(String, u32, usize, usize, Vec<BannerDiagnostic>), ExportError> {
    project
        .validate()
        .map_err(|e| ExportError::InvalidProject(e.to_string()))?;

    if let Some(raw_click_tag) = options.click_tag_url.as_deref().map(str::trim)
        && !raw_click_tag.is_empty()
    {
        let lower = raw_click_tag.to_ascii_lowercase();
        let valid_scheme = lower.starts_with("https://") || lower.starts_with("http://");
        if !valid_scheme || raw_click_tag.chars().any(char::is_control) {
            return Err(ExportError::Options(
                "Banner clickTag URL must use http:// or https:// without control characters."
                    .to_owned(),
            ));
        }
    }

    let diagnostics = preflight_native_banner(project);
    let blocking: Vec<&BannerDiagnostic> = diagnostics.iter().filter(|d| d.blocking).collect();
    if !blocking.is_empty() {
        let details = blocking
            .iter()
            .map(|d| format!("{} [{}]: {}", d.location, d.code, d.message))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(ExportError::UnsupportedScript(format!(
            "Web Banner · Native preflight blocked export ({} issue(s)):\n{details}",
            blocking.len()
        )));
    }

    let (mut eval_project, _) = flatten_scenes_with_ranges(project);
    let _ = crate::apply_bitmap_export_settings(&mut eval_project, options.jpeg_quality);

    let root_id = eval_project.root.clone();
    let total_frames = eval_project
        .symbol(&root_id)
        .map(|s| s.timeline.frame_count.max(1))
        .unwrap_or(1);

    let mut fallback_filters: BTreeMap<String, Vec<FilterSpec>> = BTreeMap::new();
    for sym in &eval_project.symbols {
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    if !el.filters.is_empty() {
                        fallback_filters
                            .entry(el.id.clone())
                            .or_insert_with(|| el.filters.clone());
                    }
                }
            }
        }
    }

    let mut defs = SharedBannerDefs::default();
    let mut playback = AuthoredPlayback::new(&eval_project, &root_id, 0);
    let prec = options.precision.clamp(1, 4);

    // Compress consecutive identical frame markups into `(start_frame, end_frame, markup)` spans!
    let mut spans: Vec<(u32, u32, String)> = Vec::new();
    for f in 0..total_frames {
        if f > 0 {
            playback.tick(&eval_project);
        }
        let mut frame_element_filters = fallback_filters.clone();
        if let Some(root_sym) = eval_project.symbol(&root_id) {
            for layer in &root_sym.timeline.layers {
                if let Some(kf) = layer.keyframes.iter().rfind(|k| k.frame <= f) {
                    for el in &kf.elements {
                        let mut combined = layer.properties.filters.clone();
                        combined.extend(kf.filters.iter().cloned());
                        combined.extend(el.filters.iter().cloned());
                        if combined.is_empty() {
                            frame_element_filters.remove(&el.id);
                        } else {
                            frame_element_filters.insert(el.id.clone(), combined);
                        }
                    }
                }
            }
        }
        let scene = playback.scene(&eval_project, false, true);
        let mut frame_clips: BTreeMap<String, String> = BTreeMap::new();
        for clip in &scene.clips {
            let mut clip_body = String::new();
            for comp in &clip.components {
                let d = path_d(&comp.commands, prec);
                if d.is_empty() {
                    continue;
                }
                let rule = if comp.fill_rule == FillRule::EvenOdd {
                    " clip-rule=\"evenodd\""
                } else {
                    ""
                };
                let xf = matrix_attr(comp.matrix, prec);
                let _ = write!(clip_body, "<path d=\"{d}\"{rule}{xf}/>");
            }
            if !clip_body.is_empty() {
                let svg_cid = if let Some(existing) = defs.clips.get(&clip_body) {
                    existing.clone()
                } else {
                    let id = format!("c{}", defs.clips.len());
                    let _ = writeln!(
                        defs.defs_markup,
                        "<clipPath id=\"{id}\">{clip_body}</clipPath>"
                    );
                    defs.clips.insert(clip_body, id.clone());
                    defs.count += 1;
                    id
                };
                frame_clips.insert(clip.id.clone(), svg_cid);
            }
        }

        let mut frame_markup = String::new();
        for draw in &scene.draws {
            render_banner_draw(
                &eval_project,
                draw,
                options,
                &frame_element_filters,
                &frame_clips,
                &mut defs,
                &mut frame_markup,
            );
        }

        if let Some(last) = spans.last_mut()
            && last.2 == frame_markup
        {
            last.1 = f;
        } else {
            spans.push((f, f, frame_markup));
        }
    }

    let w = eval_project.width.max(1);
    let h = eval_project.height.max(1);
    let fps = eval_project.fps.max(1);
    let bg = color_hex(eval_project.background);
    let title = html_escape(&eval_project.name);
    let default_click_tag = options
        .click_tag_url
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");

    let mut frames_svg = String::new();
    for (idx, (s, e, content)) in spans.iter().enumerate() {
        let vis = if idx == 0 {
            ""
        } else {
            " style=\"display:none\""
        };
        let _ = writeln!(
            frames_svg,
            "<g class=\"nf-f\" data-s=\"{s}\" data-e=\"{e}\"{vis}>{content}</g>"
        );
    }

    let defs_block = if defs.defs_markup.is_empty() {
        String::new()
    } else {
        format!("<defs>\n{}</defs>\n", defs.defs_markup)
    };

    let runner_js = if total_frames <= 1 {
        String::new()
    } else {
        format!(
            "const F={total_frames},R={fps},L={},M={},A={};let f=0,l=0,p=A,t=performance.now();const G=[...document.querySelectorAll('.nf-f')];function S(n){{for(const g of G)g.style.display=(n>=+g.dataset.s&&n<=+g.dataset.e)?'':'none';}}function K(now){{if(p){{const d=1000/R;while(now-t>=d){{t+=d;if(f+1<F){{f++;S(f);}}else if(L&&(M===0||l+1<M)){{l++;f=0;S(0);}}else{{p=false;break;}}}}}}if(p)requestAnimationFrame(K);}}if(A)requestAnimationFrame(K);",
            if options.loop_timeline {
                "true"
            } else {
                "false"
            },
            options.max_loops,
            if options.autoplay { "true" } else { "false" }
        )
    };

    let html = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<meta name=\"ad.size\" content=\"width={w},height={h}\">\n<title>{title}</title>\n<style>html,body{{margin:0;padding:0;width:100%;height:100%;overflow:hidden;background:{bg}}}#nf-ad{{display:block;width:100%;height:100%;cursor:pointer;text-decoration:none}}svg{{display:block;width:100%;height:100%}}</style>\n<script>window.clickTag=window.clickTag||\"{default_click_tag}\";</script>\n</head>\n<body>\n<a id=\"nf-ad\" href=\"#\" aria-label=\"{title}\">\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n<rect width=\"{w}\" height=\"{h}\" fill=\"{bg}\"/>\n{defs_block}{frames_svg}</svg>\n</a>\n<script>document.getElementById('nf-ad').addEventListener('click',function(e){{e.preventDefault();var u=String(window.clickTag||new URLSearchParams(location.search).get('clickTag')||'').trim();if(/^https?:\\/\\//i.test(u))window.open(u,'_blank','noopener,noreferrer');}});{runner_js}</script>\n</body>\n</html>\n"
    );

    Ok((html, total_frames, spans.len(), defs.count, diagnostics))
}

/// Estimate size and check eligibility of `project` under `Web Banner · Native` without writing files.
pub fn estimate_native_banner(
    project: &Project,
    options: &BannerOptions,
) -> Result<BannerEstimate, ExportError> {
    if let Some(raw_click_tag) = options.click_tag_url.as_deref().map(str::trim)
        && !raw_click_tag.is_empty()
    {
        let lower = raw_click_tag.to_ascii_lowercase();
        let valid_scheme = lower.starts_with("https://") || lower.starts_with("http://");
        if !valid_scheme || raw_click_tag.chars().any(char::is_control) {
            return Err(ExportError::Options(
                "Banner clickTag URL must use http:// or https:// without control characters."
                    .to_owned(),
            ));
        }
    }

    let diagnostics = preflight_native_banner(project);
    if diagnostics.iter().any(|d| d.blocking) {
        return Ok(BannerEstimate {
            eligible: false,
            raw_bytes: 0,
            gzip_bytes: 0,
            frame_count: 0,
            unique_holds: 0,
            shared_defs: 0,
            within_budget: false,
            diagnostics,
        });
    }
    let (html, frame_count, unique_holds, shared_defs, diagnostics) =
        compile_native_banner_html_inner(project, options)?;
    let raw_bytes = html.len();
    let gzip_bytes = gzip_len(html.as_bytes());
    let within_raw = options.max_raw_bytes.is_none_or(|max| raw_bytes <= max);
    let within_gzip = options.max_gzip_bytes.is_none_or(|max| gzip_bytes <= max);
    Ok(BannerEstimate {
        eligible: true,
        raw_bytes,
        gzip_bytes,
        frame_count,
        unique_holds,
        shared_defs,
        within_budget: within_raw && within_gzip,
        diagnostics,
    })
}

/// Compile and write a zero-WASM single-file `.html` banner (`Web Banner · Native`), enforcing
/// eligibility and configured raw/gzip byte budgets.
pub fn export_native_banner_html(
    project: &Project,
    output_path: impl AsRef<Path>,
    options: &BannerOptions,
) -> Result<BannerExportReport, ExportError> {
    let (html, frame_count, _holds, shared_defs, diagnostics) =
        compile_native_banner_html_inner(project, options)?;
    let raw_bytes = html.len();
    let gzip_bytes = gzip_len(html.as_bytes());

    if let Some(max_raw) = options.max_raw_bytes
        && raw_bytes > max_raw
    {
        return Err(ExportError::Options(format!(
            "Web Banner · Native output is {raw_bytes} bytes raw ({gzip_bytes} bytes gzip), exceeding the {max_raw}-byte raw budget."
        )));
    }
    if let Some(max_gzip) = options.max_gzip_bytes
        && gzip_bytes > max_gzip
    {
        return Err(ExportError::Options(format!(
            "Web Banner · Native output is {gzip_bytes} bytes gzip ({raw_bytes} bytes raw), exceeding the {max_gzip}-byte gzip budget."
        )));
    }

    let out = output_path.as_ref();
    let final_path = if out.is_dir() || out.extension().is_none() {
        fs::create_dir_all(out)?;
        out.join("index.html")
    } else {
        if let Some(parent) = out.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        out.to_path_buf()
    };
    atomic_write(&final_path, html.as_bytes())?;

    Ok(BannerExportReport {
        html_path: final_path,
        width: project.width.max(1),
        height: project.height.max(1),
        fps: project.fps.max(1),
        frame_count,
        raw_bytes,
        gzip_bytes,
        shared_defs,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{Easing, Element, Keyframe, Script, ScriptLanguage, Transform, new_id};

    #[test]
    fn native_banner_exports_zero_wasm_single_html_within_budget_and_blocks_scripts() {
        let mut project = Project::blank();
        project.name = "Spring Promo 300x250".into();
        project.width = 300;
        project.height = 250;
        project.fps = 24;
        project.symbols[0].timeline.frame_count = 6;
        let box_id = new_id("cta");
        project.symbols[0].timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: box_id.clone(),
                    name: "CTA".into(),
                    transform: Transform {
                        x: 20.0,
                        y: 100.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Rectangle {
                        width: 120.0,
                        height: 44.0,
                        fill: [240, 80, 40, 255],
                        stroke: [255, 255, 255, 255],
                        stroke_width: 2.0,
                    },
                    ..Element::default()
                }],
                tween: Some(Easing::EaseOut),
                ..Keyframe::default()
            },
            Keyframe {
                frame: 5,
                elements: vec![Element {
                    id: box_id,
                    name: "CTA".into(),
                    transform: Transform {
                        x: 150.0,
                        y: 100.0,
                        ..Transform::default()
                    },
                    kind: ElementKind::Rectangle {
                        width: 120.0,
                        height: 44.0,
                        fill: [240, 80, 40, 255],
                        stroke: [255, 255, 255, 255],
                        stroke_width: 2.0,
                    },
                    ..Element::default()
                }],
                tween: None,
                ..Keyframe::default()
            },
        ];

        let opts = BannerOptions {
            click_tag_url: Some("https://example.com/offer".into()),
            max_loops: 3,
            ..BannerOptions::default()
        };
        let est = estimate_native_banner(&project, &opts).expect("banner estimate");
        assert!(est.eligible);
        assert!(est.within_budget);
        assert!(
            est.gzip_bytes < 4096,
            "expected <4 KiB gzip, got {}",
            est.gzip_bytes
        );

        let dir = std::env::temp_dir().join(format!("nf-banner-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let report = export_native_banner_html(&project, dir.join("banner.html"), &opts)
            .expect("export banner");
        let html = fs::read_to_string(&report.html_path).expect("read banner html");
        assert!(html.contains("ad.size\" content=\"width=300,height=250\""));
        assert!(html.contains("window.clickTag"));
        assert!(!html.contains("WebAssembly"));
        assert!(!html.contains("neoflash_web"));
        let _ = fs::remove_dir_all(&dir);

        // Adding a frame script must block native banner export with a located diagnostic.
        project.symbols[0]
            .timeline
            .scripts
            .push(neoflash_core::FrameScript {
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "stop();".into(),
                },
            });
        let blocked = estimate_native_banner(&project, &opts).expect("blocked estimate");
        assert!(!blocked.eligible);
        assert!(
            blocked
                .diagnostics
                .iter()
                .any(|d| d.code == "banner-unsupported-frame-script"
                    && d.location.contains("frame 1"))
        );
    }
}
