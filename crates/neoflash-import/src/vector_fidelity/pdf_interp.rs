//! Recursive PDF 1.4–1.7 / Adobe Illustrator content stream interpreter (`T-415`)
//! and Stage clipboard entrypoints (`import_pdf_clipboard`, `import_eps_clipboard`).

use super::pdf::{
    MAX_PDF_FORM_DEPTH, Page, PdfFormRef, PdfResourceScope, resolve_color_in_cs,
    tokenize_pdf_content,
};
use super::pdf_color_fn::cmyk_to_rgba;
use super::pdf_text_font::{PdfTextBlockBuilder, PdfTextStyleState};
use super::*;
use lopdf::ObjectId;
use std::collections::HashSet;

pub(super) fn invert_affine_2x2_with_origin(m: [f64; 6]) -> Option<[f64; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if !det.is_finite() || det.abs() <= 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let a = m[3] * inv_det;
    let b = -m[1] * inv_det;
    let c = -m[2] * inv_det;
    let d = m[0] * inv_det;
    let e = -(a * m[4] + c * m[5]);
    let f = -(b * m[4] + d * m[5]);
    Some([a, b, c, d, e, f])
}

pub(super) fn ctm_stroke_scale(m: [f64; 6]) -> f64 {
    let sx = m[0].hypot(m[1]);
    let sy = m[2].hypot(m[3]);
    let avg = (sx + sy) * 0.5;
    if avg.is_finite() && avg > 1e-6 {
        avg
    } else {
        1.0
    }
}

pub(super) fn dash_subdivide_path(
    commands: &[PathCommand],
    raw_pattern: &[f64],
    phase: f64,
) -> Vec<PathCommand> {
    let clean: Vec<f64> = raw_pattern
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect();
    let total_cycle: f64 = clean.iter().sum();
    if clean.is_empty() || total_cycle <= 1e-6 {
        return commands.to_vec();
    }
    // Odd-length dash arrays repeat twice per cycle in PDF/PostScript (`[4] -> [4, 4]`)
    let cycle: Vec<f64> = if clean.len().is_multiple_of(2) {
        clean
    } else {
        let mut doubled = Vec::with_capacity(clean.len() * 2);
        doubled.extend_from_slice(&clean);
        doubled.extend_from_slice(&clean);
        doubled
    };
    let cycle_sum: f64 = cycle.iter().sum();
    if cycle_sum <= 1e-6 {
        return commands.to_vec();
    }

    // Flatten each subpath into dense polyline vertices, then slice on/off intervals
    let mut subpaths: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur_pts: Vec<(f64, f64)> = Vec::new();
    let mut cur = (0.0, 0.0);
    let mut start = (0.0, 0.0);

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                if cur_pts.len() >= 2 {
                    subpaths.push(std::mem::take(&mut cur_pts));
                } else {
                    cur_pts.clear();
                }
                cur_pts.push((x, y));
                cur = (x, y);
                start = (x, y);
            }
            PathCommand::LineTo { x, y } => {
                if cur_pts.is_empty() {
                    cur_pts.push(cur);
                }
                cur_pts.push((x, y));
                cur = (x, y);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if cur_pts.is_empty() {
                    cur_pts.push(cur);
                }
                let steps = 12usize;
                for s in 1..=steps {
                    let t = (s as f64) / (steps as f64);
                    let mt = 1.0 - t;
                    let px = mt * mt * cur.0 + 2.0 * mt * t * cx + t * t * x;
                    let py = mt * mt * cur.1 + 2.0 * mt * t * cy + t * t * y;
                    cur_pts.push((px, py));
                }
                cur = (x, y);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if cur_pts.is_empty() {
                    cur_pts.push(cur);
                }
                let steps = 16usize;
                for s in 1..=steps {
                    let t = (s as f64) / (steps as f64);
                    let mt = 1.0 - t;
                    let px = mt * mt * mt * cur.0
                        + 3.0 * mt * mt * t * cx1
                        + 3.0 * mt * t * t * cx2
                        + t * t * t * x;
                    let py = mt * mt * mt * cur.1
                        + 3.0 * mt * mt * t * cy1
                        + 3.0 * mt * t * t * cy2
                        + t * t * t * y;
                    cur_pts.push((px, py));
                }
                cur = (x, y);
            }
            PathCommand::Close => {
                if !cur_pts.is_empty() && (cur.0 - start.0).hypot(cur.1 - start.1) > 1e-6 {
                    cur_pts.push(start);
                }
                cur = start;
                if cur_pts.len() >= 2 {
                    subpaths.push(std::mem::take(&mut cur_pts));
                } else {
                    cur_pts.clear();
                }
            }
        }
    }
    if cur_pts.len() >= 2 {
        subpaths.push(cur_pts);
    }

    let mut out = Vec::new();
    for pts in subpaths {
        let mut pat_idx = 0usize;
        let mut rem_in_pat = cycle[0];
        let mut rem_phase = phase.rem_euclid(cycle_sum);
        while rem_phase > 1e-9 {
            if rem_phase < rem_in_pat {
                rem_in_pat -= rem_phase;
                rem_phase = 0.0;
            } else {
                rem_phase -= rem_in_pat;
                pat_idx = (pat_idx + 1) % cycle.len();
                rem_in_pat = cycle[pat_idx];
            }
        }
        let mut pen_down = false;
        for seg in pts.windows(2) {
            let (mut x0, mut y0) = seg[0];
            let (x1, y1) = seg[1];
            let mut seg_rem = (x1 - x0).hypot(y1 - y0);
            if seg_rem <= 1e-9 {
                continue;
            }
            while seg_rem > 1e-9 && out.len() + 2 <= MAX_SVG_PATH_COMMANDS {
                if rem_in_pat <= 1e-9 {
                    pat_idx = (pat_idx + 1) % cycle.len();
                    rem_in_pat = cycle[pat_idx];
                    if rem_in_pat <= 1e-9 && cycle.iter().all(|&v| v <= 1e-9) {
                        break;
                    }
                    continue;
                }
                let is_on = pat_idx.is_multiple_of(2);
                let step = seg_rem.min(rem_in_pat);
                let frac = step / seg_rem;
                let nx = x0 + (x1 - x0) * frac;
                let ny = y0 + (y1 - y0) * frac;
                if is_on {
                    if !pen_down {
                        out.push(PathCommand::MoveTo { x: x0, y: y0 });
                        pen_down = true;
                    }
                    out.push(PathCommand::LineTo { x: nx, y: ny });
                } else {
                    pen_down = false;
                }
                x0 = nx;
                y0 = ny;
                seg_rem -= step;
                rem_in_pat -= step;
                if rem_in_pat <= 1e-9 {
                    pat_idx = (pat_idx + 1) % cycle.len();
                    rem_in_pat = cycle[pat_idx];
                    if !pat_idx.is_multiple_of(2) {
                        pen_down = false;
                    }
                }
            }
            if out.len() + 2 > MAX_SVG_PATH_COMMANDS {
                return out;
            }
        }
    }

    if out.is_empty() {
        commands.to_vec()
    } else {
        out
    }
}

#[derive(Clone)]
struct PdfGState {
    ctm: [f64; 6],
    fill_color: Color,
    stroke_color: Color,
    fill_alpha: f64,
    stroke_alpha: f64,
    blend_mode: BlendMode,
    fill_cs_name: Option<String>,
    stroke_cs_name: Option<String>,
    fill_pattern: Option<GradientPaint>,
    stroke_pattern: Option<GradientPaint>,
    line_width: f64,
    line_cap: i64,
    line_join: i64,
    miter_limit: f64,
    dash_pattern: Vec<f64>,
    dash_phase: f64,
    text_style: PdfTextStyleState,
    clip: Option<std::sync::Arc<Element>>,
    clip_geom: Option<(Vec<PathCommand>, FillRule)>,
    mask_mode: MaskMode,
}

impl Default for PdfGState {
    fn default() -> Self {
        Self {
            ctm: identity_matrix(),
            fill_color: [0, 0, 0, 255],
            stroke_color: [0, 0, 0, 255],
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
            blend_mode: BlendMode::Normal,
            fill_cs_name: None,
            stroke_cs_name: None,
            fill_pattern: None,
            stroke_pattern: None,
            line_width: 1.0,
            line_cap: 0,
            line_join: 0,
            miter_limit: 10.0,
            dash_pattern: Vec::new(),
            dash_phase: 0.0,
            text_style: PdfTextStyleState::default(),
            clip: None,
            clip_geom: None,
            mask_mode: MaskMode::Geometry,
        }
    }
}

fn with_alpha(mut c: Color, alpha_mul: f64) -> Color {
    c[3] = ((f64::from(c[3]) * alpha_mul.clamp(0.0, 1.0)).round()).clamp(0.0, 255.0) as u8;
    c
}

struct PdfSharedBuild<'a> {
    project: &'a mut Project,
    options: &'a VectorImportOptions,
    report: &'a mut ImportReport,
    form_symbol_cache: HashMap<ObjectId, (String, [f64; 4], [f64; 6])>,
    active_form_ids: HashSet<ObjectId>,
    png_resource_cache: HashMap<Vec<u8>, String>,
    total_ops: usize,
}

impl<'a> PdfSharedBuild<'a> {
    fn intern_png_resource(&mut self, png_bytes: &[u8]) -> Result<String, ImportError> {
        if let Some(existing) = self.png_resource_cache.get(png_bytes) {
            return Ok(existing.clone());
        }
        for res in &self.project.resources {
            if res.media_type.eq_ignore_ascii_case("image/png")
                && let Ok(decoded) = res.decoded_bytes()
                && decoded == png_bytes
            {
                self.png_resource_cache
                    .insert(png_bytes.to_vec(), res.id.clone());
                return Ok(res.id.clone());
            }
        }
        if self.project.resources.len() >= 256 {
            return Err(ImportError::Container(
                "PDF exceeds maximum 256 embedded bitmap resources.".into(),
            ));
        }
        let res_id = new_id("resource");
        let unit = ResourceUnit::from_bytes(
            res_id.clone(),
            format!("pdf/{res_id}.png"),
            "image/png",
            png_bytes,
        )
        .map_err(|e| ImportError::Container(format!("PDF bitmap resource error: {e}")))?;
        self.project.resources.push(unit);
        self.project.format_version = self.project.format_version.max(3);
        self.png_resource_cache
            .insert(png_bytes.to_vec(), res_id.clone());
        Ok(res_id)
    }

    fn build_or_reuse_form_symbol(
        &mut self,
        scope: &PdfResourceScope,
        form_ref: &PdfFormRef,
        source_label: &str,
        form_depth: usize,
    ) -> Result<(String, [f64; 4], [f64; 6]), ImportError> {
        if form_depth >= MAX_PDF_FORM_DEPTH {
            return Err(ImportError::Container(format!(
                "PDF Form XObject recursion depth exceeds {MAX_PDF_FORM_DEPTH}."
            )));
        }
        if let Some(oid) = form_ref.id
            && self.active_form_ids.contains(&oid)
        {
            return Err(ImportError::Container(
                "Cyclic PDF Form XObject reference detected.".into(),
            ));
        }
        if let Some(oid) = form_ref.id
            && let Some(cached) = self.form_symbol_cache.get(&oid)
        {
            return Ok(cached.clone());
        }
        if let Some(oid) = form_ref.id {
            self.active_form_ids.insert(oid);
        }

        let res = (|| {
            let form = scope.resolve_form(form_ref)?;
            let [bx0, by0, bx1, by1] = form.bbox;
            let bw = (bx1 - bx0).abs().max(1.0);
            let bh = (by1 - by0).abs().max(1.0);
            let internal_flip = [1.0, 0.0, 0.0, -1.0, -bx0, by1];

            let sub_buckets = self.interpret_stream(
                &form.content,
                &form.scope,
                internal_flip,
                (bw, bh),
                &format!("{source_label}::Form[{}]", form.name),
                form_depth + 1,
            )?;

            let mut layers = Vec::new();
            for b in sub_buckets {
                let mut mask_layer_id = None;
                if !b.mask_elements.is_empty()
                    && self.options.mask_handling == VectorMaskHandling::MaskLayers
                {
                    let mid = new_id("layer");
                    mask_layer_id = Some(mid.clone());
                    layers.push(Layer {
                        id: mid,
                        name: format!("{} Mask", b.name),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: b.mask_elements,
                            ..Keyframe::default()
                        }],
                        properties: LayerProperties {
                            role: LayerRole::Mask,
                            mask_mode: b.mask_mode,
                            ..LayerProperties::default()
                        },
                    });
                    self.report.statistics.layers += 1;
                    self.report.statistics.keyframes += 1;
                }
                layers.push(Layer {
                    id: new_id("layer"),
                    name: b.name,
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: b.elements,
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties {
                        mask: mask_layer_id,
                        ..LayerProperties::default()
                    },
                });
                self.report.statistics.layers += 1;
                self.report.statistics.keyframes += 1;
            }
            if layers.is_empty() {
                layers.push(Layer {
                    id: new_id("layer"),
                    name: form.name.clone(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe::default()],
                    properties: LayerProperties::default(),
                });
            }

            let sym_id = new_id("symbol");
            self.project.symbols.push(Symbol {
                id: sym_id.clone(),
                name: format!("PDF Form {}", form.name),
                folder: String::new(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    frame_count: 1,
                    layers,
                    scripts: Vec::new(),
                    guides: Vec::new(),
                },
                class_name: None,
                linkage_id: None,
            });
            self.report.statistics.symbols += 1;
            let triple = (sym_id, form.bbox, form.matrix);
            if let Some(oid) = form.id {
                self.form_symbol_cache.insert(oid, triple.clone());
            }
            Ok(triple)
        })();

        if let Some(oid) = form_ref.id {
            self.active_form_ids.remove(&oid);
        }
        res
    }

    fn interpret_stream(
        &mut self,
        content: &str,
        scope: &PdfResourceScope,
        initial_ctm: [f64; 6],
        bounds_wh: (f64, f64),
        stream_label: &str,
        form_depth: usize,
    ) -> Result<Vec<VectorGroupBucket>, ImportError> {
        for (res_name, msg) in &scope.warnings {
            self.report.warning(
                "pdf-shading-approximated",
                format!("{stream_label}::{res_name}"),
                msg.clone(),
            );
        }

        let tokens = tokenize_pdf_content(content)?;
        let mut buckets: Vec<VectorGroupBucket> = Vec::new();
        let mut current_bucket = VectorGroupBucket {
            name: format!("{stream_label} Layer 1"),
            elements: Vec::new(),
            mask_elements: Vec::new(),
            mask_mode: MaskMode::Geometry,
        };

        let mut operand_stack: Vec<String> = Vec::new();
        let mut gstate = PdfGState {
            ctm: initial_ctm,
            ..PdfGState::default()
        };
        let mut gstate_stack: Vec<PdfGState> = Vec::new();
        let mut current_path: Vec<PathCommand> = Vec::new();
        let mut cur_pt = (0.0, 0.0);
        let mut subpath_start = (0.0, 0.0);
        let mut pending_clip: Option<FillRule> = None;
        let mut text_builder = PdfTextBlockBuilder::default();

        let pop_f64 = |stack: &mut Vec<String>| -> Option<f64> { stack.pop()?.parse::<f64>().ok() };

        let sync_clip_bucket = |current_bucket: &mut VectorGroupBucket,
                                buckets: &mut Vec<VectorGroupBucket>,
                                effective_clip: Option<&Element>,
                                mode: MaskMode| {
            let cur_mask_id = current_bucket.mask_elements.first().map(|e| e.id.as_str());
            let want_mask_id = effective_clip.map(|e| e.id.as_str());
            if cur_mask_id != want_mask_id || current_bucket.mask_mode != mode {
                if !current_bucket.elements.is_empty() {
                    let next = VectorGroupBucket {
                        name: current_bucket.name.clone(),
                        elements: Vec::new(),
                        mask_elements: Vec::new(),
                        mask_mode: mode,
                    };
                    buckets.push(std::mem::replace(current_bucket, next));
                }
                current_bucket.mask_mode = mode;
                current_bucket.mask_elements = effective_clip.cloned().into_iter().collect();
            }
        };

        for tok in tokens {
            let is_num = tok.parse::<f64>().is_ok();
            if is_num
                || tok.starts_with('(')
                || tok.starts_with('<')
                || tok == ">>"
                || tok.starts_with('/')
                || tok == "["
                || tok == "]"
            {
                if operand_stack.len() >= 4096 {
                    return Err(ImportError::Container(format!(
                        "{stream_label}: PDF operand stack exceeds 4096 tokens."
                    )));
                }
                operand_stack.push(tok);
                continue;
            }

            self.total_ops += 1;
            let loc = format!("{stream_label}::{tok}");

            let is_known_shading = tok == "sh"
                && operand_stack
                    .last()
                    .and_then(|name| scope.shadings.get(name.trim_start_matches('/')))
                    .is_some();
            let effective_clip = if is_known_shading {
                None
            } else {
                gstate.clip.as_deref()
            };
            if matches!(
                tok.as_str(),
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "ET" | "sh" | "Do"
            ) {
                sync_clip_bucket(
                    &mut current_bucket,
                    &mut buckets,
                    effective_clip,
                    gstate.mask_mode,
                );
            }

            match tok.as_str() {
                "q" => {
                    if gstate_stack.len() < 128 {
                        gstate_stack.push(gstate.clone());
                    }
                    operand_stack.clear();
                }
                "Q" => {
                    if text_builder.in_text {
                        let els = text_builder.flush_elements(
                            gstate.ctm,
                            gstate.blend_mode,
                            self.options,
                            &loc,
                            self.report,
                        );
                        current_bucket.elements.extend(els);
                    }
                    if let Some(prev) = gstate_stack.pop() {
                        gstate = prev;
                    }
                    operand_stack.clear();
                }
                "cm" => {
                    if operand_stack.len() >= 6 {
                        let f = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let e = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let d = pop_f64(&mut operand_stack).unwrap_or(1.0);
                        let c = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let b = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let a = pop_f64(&mut operand_stack).unwrap_or(1.0);
                        gstate.ctm = affine_multiply(gstate.ctm, [a, b, c, d, e, f]);
                    }
                    operand_stack.clear();
                }
                "w" => {
                    if let Some(lw) = pop_f64(&mut operand_stack) {
                        gstate.line_width = lw.max(0.0);
                    }
                    operand_stack.clear();
                }
                "J" => {
                    if let Some(cap) = pop_f64(&mut operand_stack) {
                        gstate.line_cap = (cap.round() as i64).clamp(0, 2);
                    }
                    operand_stack.clear();
                }
                "j" => {
                    if let Some(join) = pop_f64(&mut operand_stack) {
                        gstate.line_join = (join.round() as i64).clamp(0, 2);
                    }
                    operand_stack.clear();
                }
                "M" => {
                    if let Some(ml) = pop_f64(&mut operand_stack) {
                        gstate.miter_limit = ml.max(1.0);
                    }
                    operand_stack.clear();
                }
                "d" => {
                    // `[ d0 d1 ... ] phase d`
                    if let Some(phase) = pop_f64(&mut operand_stack) {
                        let pattern: Vec<f64> = operand_stack
                            .iter()
                            .filter(|t| *t != "[" && *t != "]")
                            .filter_map(|t| t.parse::<f64>().ok())
                            .filter(|&v| v >= 0.0)
                            .collect();
                        gstate.dash_pattern = pattern;
                        gstate.dash_phase = phase;
                    }
                    operand_stack.clear();
                }
                "gs" => {
                    if let Some(gs_name) = operand_stack
                        .last()
                        .filter(|s| s.starts_with('/'))
                        .map(|s| s.trim_start_matches('/').to_owned())
                        && let Some(ext) = scope.ext_gstates.get(&gs_name).cloned()
                    {
                        if let Some(ca) = ext.fill_alpha {
                            gstate.fill_alpha = ca;
                        }
                        if let Some(stroke_ca) = ext.stroke_alpha {
                            gstate.stroke_alpha = stroke_ca;
                        }
                        if let Some(bm) = ext.blend_mode {
                            gstate.blend_mode = bm;
                        }
                        if let Some(ref warn_msg) = ext.blend_approx {
                            self.report.warning(
                                "pdf-blend-approximated",
                                loc.clone(),
                                warn_msg.clone(),
                            );
                        }
                        if let Some(lw) = ext.line_width {
                            gstate.line_width = lw;
                        }
                        if let Some(lc) = ext.line_cap {
                            gstate.line_cap = lc;
                        }
                        if let Some(lj) = ext.line_join {
                            gstate.line_join = lj;
                        }
                        if let Some(ml) = ext.miter_limit {
                            gstate.miter_limit = ml;
                        }
                        if let Some((pat, ph)) = ext.dash {
                            gstate.dash_pattern = pat;
                            gstate.dash_phase = ph;
                        }
                        if ext.clear_smask {
                            gstate.clip = None;
                            gstate.clip_geom = None;
                            gstate.mask_mode = MaskMode::Geometry;
                        } else if let Some(sm) = ext.smask {
                            if form_depth >= MAX_PDF_FORM_DEPTH {
                                return Err(ImportError::Container(format!(
                                    "PDF SoftMask Form XObject recursion depth exceeds {MAX_PDF_FORM_DEPTH}."
                                )));
                            }
                            if let Some(oid) = sm.form_ref.id
                                && !self.active_form_ids.insert(oid)
                            {
                                return Err(ImportError::Container(
                                    "Cyclic PDF SoftMask Form XObject reference detected.".into(),
                                ));
                            }
                            let sm_res = (|| {
                                let sm_form = scope.resolve_form(&sm.form_ref)?;
                                let [bx0, by0, bx1, by1] = sm_form.bbox;
                                let sm_mat = affine_multiply(gstate.ctm, sm_form.matrix);
                                let sm_buckets = self.interpret_stream(
                                    &sm_form.content,
                                    &sm_form.scope,
                                    sm_mat,
                                    ((bx1 - bx0).abs().max(1.0), (by1 - by0).abs().max(1.0)),
                                    &format!("{stream_label}::SMask[{gs_name}]"),
                                    form_depth + 1,
                                )?;
                                Ok::<_, ImportError>((sm_buckets, bx0, by0, bx1, by1, sm_mat))
                            })();
                            if let Some(oid) = sm.form_ref.id {
                                self.active_form_ids.remove(&oid);
                            }
                            let (sm_buckets, bx0, by0, bx1, by1, sm_mat) = sm_res?;
                            let first_mask_el = sm_buckets
                                .into_iter()
                                .flat_map(|b| b.elements)
                                .next()
                                .or_else(|| {
                                    let rect = bake_matrix_into_commands(
                                        &[
                                            PathCommand::MoveTo { x: bx0, y: by0 },
                                            PathCommand::LineTo { x: bx1, y: by0 },
                                            PathCommand::LineTo { x: bx1, y: by1 },
                                            PathCommand::LineTo { x: bx0, y: by1 },
                                            PathCommand::Close,
                                        ],
                                        sm_mat,
                                    );
                                    validate_path_commands(&rect).ok().map(|()| Element {
                                        id: new_id("element"),
                                        name: format!("PDF SoftMask ({gs_name})"),
                                        transform: Transform::default(),
                                        kind: ElementKind::Path {
                                            commands: rect,
                                            fill_rule: FillRule::NonZero,
                                            fill: [255, 255, 255, 255],
                                            stroke: [0, 0, 0, 0],
                                            stroke_width: 0.0,
                                        },
                                        ..Element::default()
                                    })
                                });
                            if let Some(mel) = first_mask_el {
                                match self.options.mask_handling {
                                    VectorMaskHandling::MaskLayers => {
                                        gstate.mask_mode = sm.mode;
                                        gstate.clip = Some(std::sync::Arc::new(mel));
                                    }
                                    VectorMaskHandling::FlattenClip => {
                                        self.report.warning(
                                            "pdf-smask-flattened",
                                            loc.clone(),
                                            "PDF /SMask soft mask was flattened according to VectorMaskHandling::FlattenClip.",
                                        );
                                    }
                                }
                            }
                        }
                    }
                    operand_stack.clear();
                }
                "rg" => {
                    if operand_stack.len() >= 3 {
                        let b = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        let g = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        let r = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        gstate.fill_color = [r, g, b, 255];
                        gstate.fill_cs_name = Some("DeviceRGB".into());
                        gstate.fill_pattern = None;
                    }
                    operand_stack.clear();
                }
                "RG" => {
                    if operand_stack.len() >= 3 {
                        let b = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        let g = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        let r = (pop_f64(&mut operand_stack).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0)
                            .round() as u8;
                        gstate.stroke_color = [r, g, b, 255];
                        gstate.stroke_cs_name = Some("DeviceRGB".into());
                        gstate.stroke_pattern = None;
                    }
                    operand_stack.clear();
                }
                "g" => {
                    if let Some(gray) = pop_f64(&mut operand_stack) {
                        let v = (gray.clamp(0.0, 1.0) * 255.0).round() as u8;
                        gstate.fill_color = [v, v, v, 255];
                        gstate.fill_cs_name = Some("DeviceGray".into());
                        gstate.fill_pattern = None;
                    }
                    operand_stack.clear();
                }
                "G" => {
                    if let Some(gray) = pop_f64(&mut operand_stack) {
                        let v = (gray.clamp(0.0, 1.0) * 255.0).round() as u8;
                        gstate.stroke_color = [v, v, v, 255];
                        gstate.stroke_cs_name = Some("DeviceGray".into());
                        gstate.stroke_pattern = None;
                    }
                    operand_stack.clear();
                }
                "k" => {
                    if operand_stack.len() >= 4 {
                        let k = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let m = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let c = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        gstate.fill_color = cmyk_to_rgba(c, m, y, k);
                        gstate.fill_cs_name = Some("DeviceCMYK".into());
                        gstate.fill_pattern = None;
                    }
                    operand_stack.clear();
                }
                "K" => {
                    if operand_stack.len() >= 4 {
                        let k = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let m = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let c = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        gstate.stroke_color = cmyk_to_rgba(c, m, y, k);
                        gstate.stroke_cs_name = Some("DeviceCMYK".into());
                        gstate.stroke_pattern = None;
                    }
                    operand_stack.clear();
                }
                "cs" => {
                    let cs_name = operand_stack
                        .last()
                        .filter(|s| s.starts_with('/'))
                        .map(|s| s.trim_start_matches('/').to_owned());
                    if cs_name.as_deref() != Some("Pattern") {
                        gstate.fill_pattern = None;
                    }
                    gstate.fill_cs_name = cs_name;
                    operand_stack.clear();
                }
                "CS" => {
                    let cs_name = operand_stack
                        .last()
                        .filter(|s| s.starts_with('/'))
                        .map(|s| s.trim_start_matches('/').to_owned());
                    if cs_name.as_deref() != Some("Pattern") {
                        gstate.stroke_pattern = None;
                    }
                    gstate.stroke_cs_name = cs_name;
                    operand_stack.clear();
                }
                "sc" | "scn" => {
                    if let Some(pat) = operand_stack
                        .last()
                        .filter(|s| s.starts_with('/'))
                        .and_then(|s| scope.patterns.get(s.trim_start_matches('/')))
                    {
                        gstate.fill_pattern = Some(pat.clone());
                    } else {
                        let nums: Vec<f64> = operand_stack
                            .iter()
                            .filter_map(|s| s.parse::<f64>().ok())
                            .collect();
                        if let Some(col) =
                            resolve_color_in_cs(scope, gstate.fill_cs_name.as_deref(), &nums)
                        {
                            gstate.fill_color = col;
                            gstate.fill_pattern = None;
                        }
                    }
                    operand_stack.clear();
                }
                "SC" | "SCN" => {
                    if let Some(pat) = operand_stack
                        .last()
                        .filter(|s| s.starts_with('/'))
                        .and_then(|s| scope.patterns.get(s.trim_start_matches('/')))
                    {
                        gstate.stroke_pattern = Some(pat.clone());
                    } else {
                        let nums: Vec<f64> = operand_stack
                            .iter()
                            .filter_map(|s| s.parse::<f64>().ok())
                            .collect();
                        if let Some(col) =
                            resolve_color_in_cs(scope, gstate.stroke_cs_name.as_deref(), &nums)
                        {
                            gstate.stroke_color = col;
                            gstate.stroke_pattern = None;
                        }
                    }
                    operand_stack.clear();
                }
                "m" => {
                    if operand_stack.len() >= 2 {
                        if current_path.len() >= MAX_SVG_PATH_COMMANDS {
                            return Err(ImportError::Container(format!(
                                "{stream_label}: PDF path exceeds maximum {MAX_SVG_PATH_COMMANDS} commands."
                            )));
                        }
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::MoveTo { x, y });
                        cur_pt = (x, y);
                        subpath_start = (x, y);
                    }
                    operand_stack.clear();
                }
                "l" => {
                    if operand_stack.len() >= 2 {
                        if current_path.len() >= MAX_SVG_PATH_COMMANDS {
                            return Err(ImportError::Container(format!(
                                "{stream_label}: PDF path exceeds maximum {MAX_SVG_PATH_COMMANDS} commands."
                            )));
                        }
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::LineTo { x, y });
                        cur_pt = (x, y);
                    }
                    operand_stack.clear();
                }
                "c" => {
                    if operand_stack.len() >= 6 {
                        if current_path.len() >= MAX_SVG_PATH_COMMANDS {
                            return Err(ImportError::Container(format!(
                                "{stream_label}: PDF path exceeds maximum {MAX_SVG_PATH_COMMANDS} commands."
                            )));
                        }
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cy2 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cx2 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cy1 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cx1 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::CubicTo {
                            cx1,
                            cy1,
                            cx2,
                            cy2,
                            x,
                            y,
                        });
                        cur_pt = (x, y);
                    }
                    operand_stack.clear();
                }
                "v" => {
                    if operand_stack.len() >= 4 {
                        if current_path.len() >= MAX_SVG_PATH_COMMANDS {
                            return Err(ImportError::Container(format!(
                                "{stream_label}: PDF path exceeds maximum {MAX_SVG_PATH_COMMANDS} commands."
                            )));
                        }
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cy2 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cx2 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::CubicTo {
                            cx1: cur_pt.0,
                            cy1: cur_pt.1,
                            cx2,
                            cy2,
                            x,
                            y,
                        });
                        cur_pt = (x, y);
                    }
                    operand_stack.clear();
                }
                "y" => {
                    if operand_stack.len() >= 4 {
                        if current_path.len() >= MAX_SVG_PATH_COMMANDS {
                            return Err(ImportError::Container(format!(
                                "{stream_label}: PDF path exceeds maximum {MAX_SVG_PATH_COMMANDS} commands."
                            )));
                        }
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cy1 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let cx1 = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::CubicTo {
                            cx1,
                            cy1,
                            cx2: x,
                            cy2: y,
                            x,
                            y,
                        });
                        cur_pt = (x, y);
                    }
                    operand_stack.clear();
                }
                "re" => {
                    if operand_stack.len() >= 4 {
                        if current_path.len() + 5 > MAX_SVG_PATH_COMMANDS {
                            return Err(ImportError::Container(format!(
                                "{stream_label}: PDF path exceeds maximum {MAX_SVG_PATH_COMMANDS} commands."
                            )));
                        }
                        let h = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let w = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let y = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let x = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        current_path.push(PathCommand::MoveTo { x, y });
                        current_path.push(PathCommand::LineTo { x: x + w, y });
                        current_path.push(PathCommand::LineTo { x: x + w, y: y + h });
                        current_path.push(PathCommand::LineTo { x, y: y + h });
                        current_path.push(PathCommand::Close);
                        cur_pt = (x, y);
                        subpath_start = (x, y);
                    }
                    operand_stack.clear();
                }
                "h" => {
                    if !current_path.is_empty() {
                        current_path.push(PathCommand::Close);
                        cur_pt = subpath_start;
                    }
                    operand_stack.clear();
                }
                "W" | "W*" => {
                    pending_clip = Some(if tok == "W*" {
                        FillRule::EvenOdd
                    } else {
                        FillRule::NonZero
                    });
                    operand_stack.clear();
                }
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => {
                    if matches!(tok.as_str(), "s" | "b" | "b*")
                        && !current_path.is_empty()
                        && !matches!(current_path.last(), Some(PathCommand::Close))
                    {
                        current_path.push(PathCommand::Close);
                    }
                    if let Some(clip_rule) = pending_clip.take()
                        && !current_path.is_empty()
                    {
                        let baked = bake_matrix_into_commands(&current_path, gstate.ctm);
                        gstate.clip_geom = Some((baked.clone(), clip_rule));
                        gstate.mask_mode = MaskMode::Geometry;
                        match self.options.mask_handling {
                            VectorMaskHandling::MaskLayers => {
                                gstate.clip = Some(std::sync::Arc::new(Element {
                                    id: new_id("element"),
                                    name: "PDF Clip Path".to_owned(),
                                    transform: Transform::default(),
                                    kind: ElementKind::Path {
                                        commands: baked,
                                        fill_rule: clip_rule,
                                        fill: [255, 255, 255, 255],
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                    },
                                    ..Element::default()
                                }));
                                self.report.diagnostics.push(Diagnostic {
                                    severity: Severity::Info,
                                    code: "pdf-clipping-path-mapped".to_owned(),
                                    location: loc.clone(),
                                    message:
                                        "Mapped PDF clipping path (W) to a Timeline Mask layer."
                                            .to_owned(),
                                });
                            }
                            VectorMaskHandling::FlattenClip => {
                                self.report.warning(
                                    "pdf-clipping-path-flattened",
                                    loc.clone(),
                                    "PDF clipping path (W) was flattened according to VectorMaskHandling::FlattenClip.",
                                );
                            }
                        }
                        if tok != "n" {
                            sync_clip_bucket(
                                &mut current_bucket,
                                &mut buckets,
                                gstate.clip.as_deref(),
                                gstate.mask_mode,
                            );
                        }
                    }

                    if tok != "n" && !current_path.is_empty() {
                        let do_fill =
                            matches!(tok.as_str(), "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
                        let do_stroke = matches!(tok.as_str(), "S" | "s" | "B" | "B*" | "b" | "b*");
                        let fill_rule = if tok.ends_with('*') {
                            FillRule::EvenOdd
                        } else {
                            FillRule::NonZero
                        };

                        let dashed_cmds = if do_stroke && !gstate.dash_pattern.is_empty() {
                            Some(dash_subdivide_path(
                                &current_path,
                                &gstate.dash_pattern,
                                gstate.dash_phase,
                            ))
                        } else {
                            None
                        };

                        // If BOTH filled AND dashed-stroked, emit the closed fill path first so
                        // open dash segments do not break the fill interior!
                        if do_fill && dashed_cmds.is_some() {
                            let (fill_final, fill_tf, fill_mat) = match self
                                .options
                                .transform_handling
                            {
                                VectorTransformHandling::BakeTransformsIntoPaths => (
                                    bake_matrix_into_commands(&current_path, gstate.ctm),
                                    Transform::default(),
                                    identity_matrix(),
                                ),
                                VectorTransformHandling::KeepElementTransforms => {
                                    if let Some(tf) = Transform::from_matrix(gstate.ctm) {
                                        (current_path.clone(), tf, gstate.ctm)
                                    } else {
                                        (
                                            bake_matrix_into_commands(&current_path, gstate.ctm),
                                            Transform::default(),
                                            identity_matrix(),
                                        )
                                    }
                                }
                            };
                            let resolve_fill_pat = |pat: &GradientPaint| -> Option<GradientPaint> {
                                let mut p = pat.clone();
                                let stage_mat = affine_multiply(initial_ctm, p.matrix);
                                p.matrix = if fill_mat == identity_matrix() {
                                    stage_mat
                                } else {
                                    affine_multiply(
                                        invert_affine_2x2_with_origin(fill_mat)?,
                                        stage_mat,
                                    )
                                };
                                for st in &mut p.stops {
                                    st.color = with_alpha(st.color, gstate.fill_alpha);
                                }
                                p.validate().ok()?;
                                Some(p)
                            };
                            let fg = gstate.fill_pattern.as_ref().and_then(resolve_fill_pat);
                            if validate_path_commands(&fill_final).is_ok() {
                                let fc = if fg.is_none() {
                                    with_alpha(gstate.fill_color, gstate.fill_alpha)
                                } else {
                                    [0, 0, 0, 0]
                                };
                                let kind = if fg.is_some() {
                                    ElementKind::PaintedPath {
                                        commands: fill_final,
                                        fill_rule,
                                        fill: fc,
                                        stroke: [0, 0, 0, 0],
                                        fill_gradient: fg,
                                        stroke_gradient: None,
                                        stroke_width: 0.0,
                                    }
                                } else {
                                    ElementKind::Path {
                                        commands: fill_final,
                                        fill_rule,
                                        fill: fc,
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                    }
                                };
                                current_bucket.elements.push(Element {
                                    id: new_id("element"),
                                    name: format!("PDF Path {}", current_bucket.elements.len() + 1),
                                    transform: fill_tf,
                                    kind,
                                    blend_mode: gstate.blend_mode,
                                    ..Element::default()
                                });
                                self.report.statistics.elements += 1;
                            }
                        }

                        let src_cmds = dashed_cmds.as_deref().unwrap_or(&current_path);
                        let effective_do_fill = do_fill && dashed_cmds.is_none();

                        let (final_cmds, elem_transform, elem_mat) =
                            match self.options.transform_handling {
                                VectorTransformHandling::BakeTransformsIntoPaths => (
                                    bake_matrix_into_commands(src_cmds, gstate.ctm),
                                    Transform::default(),
                                    identity_matrix(),
                                ),
                                VectorTransformHandling::KeepElementTransforms => {
                                    if let Some(tf) = Transform::from_matrix(gstate.ctm) {
                                        (src_cmds.to_vec(), tf, gstate.ctm)
                                    } else {
                                        (
                                            bake_matrix_into_commands(src_cmds, gstate.ctm),
                                            Transform::default(),
                                            identity_matrix(),
                                        )
                                    }
                                }
                            };
                        let resolve_pat = |pat: &GradientPaint,
                                           alpha_mul: f64|
                         -> Option<GradientPaint> {
                            let mut p = pat.clone();
                            let stage_mat = affine_multiply(initial_ctm, p.matrix);
                            p.matrix = if elem_mat == identity_matrix() {
                                stage_mat
                            } else {
                                affine_multiply(invert_affine_2x2_with_origin(elem_mat)?, stage_mat)
                            };
                            for st in &mut p.stops {
                                st.color = with_alpha(st.color, alpha_mul);
                            }
                            p.validate().ok()?;
                            Some(p)
                        };
                        let fill_grad = if effective_do_fill {
                            gstate
                                .fill_pattern
                                .as_ref()
                                .and_then(|p| resolve_pat(p, gstate.fill_alpha))
                        } else {
                            None
                        };
                        let stroke_grad = if do_stroke {
                            gstate
                                .stroke_pattern
                                .as_ref()
                                .and_then(|p| resolve_pat(p, gstate.stroke_alpha))
                        } else {
                            None
                        };

                        if validate_path_commands(&final_cmds).is_ok() {
                            let fill_col = if effective_do_fill && fill_grad.is_none() {
                                with_alpha(gstate.fill_color, gstate.fill_alpha)
                            } else {
                                [0, 0, 0, 0]
                            };
                            let stroke_col = if do_stroke && stroke_grad.is_none() {
                                with_alpha(gstate.stroke_color, gstate.stroke_alpha)
                            } else {
                                [0, 0, 0, 0]
                            };
                            let sw = if do_stroke {
                                if elem_mat == identity_matrix() {
                                    gstate.line_width * ctm_stroke_scale(gstate.ctm)
                                } else {
                                    gstate.line_width
                                }
                            } else {
                                0.0
                            };
                            let kind = if fill_grad.is_some() || stroke_grad.is_some() {
                                ElementKind::PaintedPath {
                                    commands: final_cmds,
                                    fill_rule,
                                    fill: fill_col,
                                    stroke: stroke_col,
                                    fill_gradient: fill_grad,
                                    stroke_gradient: stroke_grad,
                                    stroke_width: sw,
                                }
                            } else {
                                ElementKind::Path {
                                    commands: final_cmds,
                                    fill_rule,
                                    fill: fill_col,
                                    stroke: stroke_col,
                                    stroke_width: sw,
                                }
                            };
                            current_bucket.elements.push(Element {
                                id: new_id("element"),
                                name: format!("PDF Path {}", current_bucket.elements.len() + 1),
                                transform: elem_transform,
                                kind,
                                blend_mode: gstate.blend_mode,
                                ..Element::default()
                            });
                            self.report.statistics.elements += 1;
                        }
                    }
                    current_path.clear();
                    operand_stack.clear();
                }
                "BT" => {
                    text_builder.begin_text();
                    operand_stack.clear();
                }
                "ET" => {
                    let els = text_builder.flush_elements(
                        gstate.ctm,
                        gstate.blend_mode,
                        self.options,
                        &loc,
                        self.report,
                    );
                    current_bucket.elements.extend(els);
                    operand_stack.clear();
                }
                "Tf" => {
                    if let Some(sz) = pop_f64(&mut operand_stack) {
                        gstate.text_style.font_size = sz.abs().max(0.01);
                    }
                    if let Some(fname) = operand_stack
                        .last()
                        .filter(|s| s.starts_with('/'))
                        .map(|s| s.trim_start_matches('/').to_owned())
                    {
                        gstate.text_style.font_name = fname;
                    }
                    operand_stack.clear();
                }
                "Tc" => {
                    if let Some(v) = pop_f64(&mut operand_stack) {
                        gstate.text_style.char_space = v;
                    }
                    operand_stack.clear();
                }
                "Tw" => {
                    if let Some(v) = pop_f64(&mut operand_stack) {
                        gstate.text_style.word_space = v;
                    }
                    operand_stack.clear();
                }
                "Tz" => {
                    if let Some(v) = pop_f64(&mut operand_stack) {
                        gstate.text_style.horiz_scale = v.clamp(5.0, 2000.0);
                    }
                    operand_stack.clear();
                }
                "TL" => {
                    if let Some(v) = pop_f64(&mut operand_stack) {
                        gstate.text_style.leading = v;
                    }
                    operand_stack.clear();
                }
                "Ts" => {
                    if let Some(v) = pop_f64(&mut operand_stack) {
                        gstate.text_style.text_rise = v;
                    }
                    operand_stack.clear();
                }
                "Tr" => {
                    if let Some(v) = pop_f64(&mut operand_stack) {
                        gstate.text_style.render_mode = v.round() as i64;
                    }
                    operand_stack.clear();
                }
                "Td" | "TD" => {
                    if operand_stack.len() >= 2 {
                        let ty = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let tx = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        if tok == "TD" {
                            gstate.text_style.leading = -ty;
                        }
                        text_builder.move_line(tx, ty);
                    }
                    operand_stack.clear();
                }
                "T*" => {
                    text_builder.next_line(gstate.text_style.leading);
                    operand_stack.clear();
                }
                "Tm" => {
                    if operand_stack.len() >= 6 {
                        let f = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let e = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let d = pop_f64(&mut operand_stack).unwrap_or(1.0);
                        let c = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let b = pop_f64(&mut operand_stack).unwrap_or(0.0);
                        let a = pop_f64(&mut operand_stack).unwrap_or(1.0);
                        text_builder.set_matrix([a, b, c, d, e, f]);
                    }
                    operand_stack.clear();
                }
                "Tj" | "TJ" => {
                    let fc = with_alpha(gstate.fill_color, gstate.fill_alpha);
                    let sc = with_alpha(gstate.stroke_color, gstate.stroke_alpha);
                    text_builder.show_tokens(
                        &operand_stack,
                        &gstate.text_style,
                        &scope.fonts,
                        fc,
                        sc,
                    );
                    operand_stack.clear();
                }
                "'" => {
                    text_builder.next_line(gstate.text_style.leading);
                    let fc = with_alpha(gstate.fill_color, gstate.fill_alpha);
                    let sc = with_alpha(gstate.stroke_color, gstate.stroke_alpha);
                    text_builder.show_tokens(
                        &operand_stack,
                        &gstate.text_style,
                        &scope.fonts,
                        fc,
                        sc,
                    );
                    operand_stack.clear();
                }
                "\"" => {
                    // `aw ac string "`
                    if operand_stack.len() >= 3 {
                        let str_tok = operand_stack.pop().unwrap();
                        if let Some(ac) = pop_f64(&mut operand_stack) {
                            gstate.text_style.char_space = ac;
                        }
                        if let Some(aw) = pop_f64(&mut operand_stack) {
                            gstate.text_style.word_space = aw;
                        }
                        text_builder.next_line(gstate.text_style.leading);
                        let fc = with_alpha(gstate.fill_color, gstate.fill_alpha);
                        let sc = with_alpha(gstate.stroke_color, gstate.stroke_alpha);
                        text_builder.show_tokens(
                            &[str_tok],
                            &gstate.text_style,
                            &scope.fonts,
                            fc,
                            sc,
                        );
                    }
                    operand_stack.clear();
                }
                "BDC" | "BMC" => {
                    if text_builder.in_text {
                        let els = text_builder.flush_elements(
                            gstate.ctm,
                            gstate.blend_mode,
                            self.options,
                            &loc,
                            self.report,
                        );
                        current_bucket.elements.extend(els);
                    }
                    let group_label = operand_stack
                        .iter()
                        .find(|s| s.starts_with('/'))
                        .map(|s| s.trim_start_matches('/').to_owned())
                        .unwrap_or_else(|| format!("Layer {}", buckets.len() + 1));
                    if !current_bucket.elements.is_empty()
                        || !current_bucket.mask_elements.is_empty()
                    {
                        buckets.push(current_bucket);
                    }
                    current_bucket = VectorGroupBucket {
                        name: group_label,
                        elements: Vec::new(),
                        mask_elements: Vec::new(),
                        mask_mode: MaskMode::Geometry,
                    };
                    operand_stack.clear();
                }
                "EMC" => {
                    if text_builder.in_text {
                        let els = text_builder.flush_elements(
                            gstate.ctm,
                            gstate.blend_mode,
                            self.options,
                            &loc,
                            self.report,
                        );
                        current_bucket.elements.extend(els);
                    }
                    if !current_bucket.elements.is_empty()
                        || !current_bucket.mask_elements.is_empty()
                    {
                        buckets.push(current_bucket);
                        current_bucket = VectorGroupBucket {
                            name: format!("{stream_label} Layer {}", buckets.len() + 1),
                            elements: Vec::new(),
                            mask_elements: Vec::new(),
                            mask_mode: MaskMode::Geometry,
                        };
                    }
                    operand_stack.clear();
                }
                "sh" if operand_stack
                    .last()
                    .and_then(|name| scope.shadings.get(name.trim_start_matches('/')))
                    .is_some() =>
                {
                    let mut paint = scope.shadings
                        [operand_stack.last().unwrap().trim_start_matches('/')]
                    .clone();
                    paint.matrix = affine_multiply(gstate.ctm, paint.matrix);
                    for st in &mut paint.stops {
                        st.color = with_alpha(st.color, gstate.fill_alpha);
                    }
                    if paint.validate().is_ok() {
                        let (commands, rule) = if let Some(Element {
                            kind:
                                ElementKind::Path {
                                    commands,
                                    fill_rule,
                                    ..
                                },
                            ..
                        }) = gstate.clip.as_deref()
                        {
                            (commands.clone(), *fill_rule)
                        } else if let Some((commands, fill_rule)) = &gstate.clip_geom {
                            (commands.clone(), *fill_rule)
                        } else {
                            let (w, h) = bounds_wh;
                            (
                                vec![
                                    PathCommand::MoveTo { x: 0., y: 0. },
                                    PathCommand::LineTo { x: w, y: 0. },
                                    PathCommand::LineTo { x: w, y: h },
                                    PathCommand::LineTo { x: 0., y: h },
                                    PathCommand::Close,
                                ],
                                FillRule::NonZero,
                            )
                        };
                        let name = match paint.kind {
                            GradientKind::Radial => "PDF radial shading",
                            _ => "PDF axial shading",
                        };
                        current_bucket.elements.push(Element {
                            id: new_id("element"),
                            name: name.into(),
                            kind: ElementKind::PaintedPath {
                                commands,
                                fill_rule: rule,
                                fill: [0; 4],
                                stroke: [0; 4],
                                stroke_width: 0.,
                                fill_gradient: Some(paint),
                                stroke_gradient: None,
                            },
                            blend_mode: gstate.blend_mode,
                            ..Default::default()
                        });
                        self.report.statistics.elements += 1;
                    } else {
                        self.report.warning(
                            "pdf-unsupported-operator",
                            loc,
                            "PDF shading transform became singular and was skipped.".to_owned(),
                        );
                    }
                    operand_stack.clear();
                }
                "Do" => {
                    let xobj_name = operand_stack
                        .last()
                        .filter(|s| s.starts_with('/'))
                        .map(|s| s.trim_start_matches('/').to_owned());
                    if let Some(ref name) = xobj_name
                        && let Some(form_ref) = scope.forms.get(name)
                    {
                        let (sym_id, [bx0, _by0, _bx1, by1], form_matrix) = self
                            .build_or_reuse_form_symbol(
                                scope,
                                form_ref,
                                stream_label,
                                form_depth,
                            )?;
                        // Undo internal symbol Y-flip `[1, 0, 0, -1, -bx0, by1]` -> `[1, 0, 0, -1, bx0, by1]`
                        let inv_internal = [1.0, 0.0, 0.0, -1.0, bx0, by1];
                        let inst_mat =
                            affine_multiply(affine_multiply(gstate.ctm, form_matrix), inv_internal);
                        if let Some(mut tf) = Transform::from_matrix(inst_mat) {
                            tf.alpha = gstate.fill_alpha.clamp(0.0, 1.0);
                            current_bucket.elements.push(Element {
                                id: new_id("element"),
                                name: format!("PDF Form ({name})"),
                                transform: tf,
                                kind: ElementKind::Instance {
                                    symbol_id: sym_id,
                                    playback: InstancePlayback::Independent,
                                },
                                blend_mode: gstate.blend_mode,
                                ..Element::default()
                            });
                            self.report.statistics.elements += 1;
                        }
                    } else if let Some(ref name) = xobj_name
                        && let Some(img) = scope.images.get(name)
                    {
                        let res_id = self.intern_png_resource(&img.png_bytes)?;
                        // PDF Image XObject paints onto unit square `[0,0]..[1,1]` with top row at `y = 1`
                        let img_to_unit = [
                            1.0 / f64::from(img.width.max(1)),
                            0.0,
                            0.0,
                            -1.0 / f64::from(img.height.max(1)),
                            0.0,
                            1.0,
                        ];
                        let elem_mat = affine_multiply(gstate.ctm, img_to_unit);
                        if let Some(mut tf) = Transform::from_matrix(elem_mat) {
                            tf.alpha = gstate.fill_alpha.clamp(0.0, 1.0);
                            current_bucket.elements.push(Element {
                                id: new_id("element"),
                                name: format!("PDF Image ({name})"),
                                transform: tf,
                                kind: ElementKind::Bitmap {
                                    resource_id: res_id,
                                    width: img.width,
                                    height: img.height,
                                    smoothing: true,
                                },
                                blend_mode: gstate.blend_mode,
                                ..Element::default()
                            });
                            self.report.statistics.elements += 1;
                        }
                    } else {
                        let detail = xobj_name
                            .as_deref()
                            .and_then(|n| scope.unsupported_xobjects.get(n))
                            .cloned()
                            .unwrap_or_else(|| {
                                "external XObject or shading dictionary is not directly supported"
                                    .to_owned()
                            });
                        self.report.warning(
                            "pdf-unsupported-operator",
                            loc,
                            format!(
                                "PDF operator 'Do' ({detail}) was recorded in the fidelity report."
                            ),
                        );
                    }
                    operand_stack.clear();
                }
                "sh" => {
                    self.report.warning(
                        "pdf-unsupported-operator",
                        loc,
                        "PDF operator 'sh' (external XObject or shading dictionary) is not directly supported and was recorded in the fidelity report.".to_owned(),
                    );
                    operand_stack.clear();
                }
                _ => {
                    operand_stack.clear();
                }
            }
        }

        if text_builder.in_text {
            let els = text_builder.flush_elements(
                gstate.ctm,
                gstate.blend_mode,
                self.options,
                stream_label,
                self.report,
            );
            current_bucket.elements.extend(els);
        }

        if !current_bucket.elements.is_empty() || !current_bucket.mask_elements.is_empty() {
            buckets.push(current_bucket);
        }
        Ok(buckets)
    }
}

pub(super) fn import_pdf_ai_with_bounds(
    project: &mut Project,
    bytes: &[u8],
    source_name: &str,
    options: &VectorImportOptions,
    max_input_bytes: usize,
) -> Result<(ImportReport, [f64; 4]), ImportError> {
    if bytes.len() > max_input_bytes {
        return Err(ImportError::Container(format!(
            "Vector input exceeds {} MiB.",
            max_input_bytes / (1024 * 1024)
        )));
    }
    if bytes.starts_with(eps::MAGIC) {
        return eps::import_with_bounds(project, bytes, source_name, options);
    }
    let header_str = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    if header_str.trim_start().starts_with("%!PS-Adobe") && !header_str.contains("%PDF-") {
        return eps::import_with_bounds(project, bytes, source_name, options);
    }
    if !header_str.contains("%PDF-") && !header_str.contains("%!PS-Adobe") {
        return Err(ImportError::Container(
            "Source is not a recognized PDF or Adobe Illustrator document (missing %PDF- or %!PS-Adobe header).".into(),
        ));
    }

    let pages = match pdf::pages(bytes) {
        Ok(pages) => pages,
        // Older NeoFlash exchange snippets have no xref or page tree, just textual operators.
        Err(_)
            if bytes.len() <= 1024 * 1024
                && !bytes.windows(9).any(|w| w == b"startxref")
                && !bytes.windows(7).any(|w| w == b"/Filter") =>
        {
            let text = String::from_utf8_lossy(bytes);
            let (w, h) = parse_pdf_media_box(&text).unwrap_or((project.width, project.height));
            vec![Page {
                content: extract_pdf_streams(bytes).join("\n"),
                bounds: [0., 0., f64::from(w), f64::from(h)],
                media_box: Some([0., 0., f64::from(w), f64::from(h)]),
                rotation: 0,
                scope: PdfResourceScope::empty(),
            }]
        }
        Err(e) => return Err(e),
    };

    let stage_bounds = pages[0].stage_bounds();
    if options.resize_stage_to_viewbox {
        (project.width, project.height) = pages[0].size_and_transform().0;
    }

    let mut report = ImportReport {
        source: source_name.to_owned(),
        format: ImportFormat::Unknown,
        version: Some("PDF/AI".to_owned()),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    let mut all_buckets = Vec::new();
    let total_ops = {
        let mut builder = PdfSharedBuild {
            project,
            options,
            report: &mut report,
            form_symbol_cache: HashMap::new(),
            active_form_ids: HashSet::new(),
            png_resource_cache: HashMap::new(),
            total_ops: 0,
        };
        for (stream_idx, page) in pages.iter().enumerate() {
            let ((pw, ph), page_matrix) = page.size_and_transform();
            let mut page_buckets = builder.interpret_stream(
                &page.content,
                &page.scope,
                page_matrix,
                (f64::from(pw), f64::from(ph)),
                &format!("{source_name}::stream[{stream_idx}]"),
                0,
            )?;
            for (bi, b) in page_buckets.iter_mut().enumerate() {
                if b.name.contains("::stream[") {
                    b.name = format!("{source_name} Layer {}", all_buckets.len() + bi + 1);
                }
            }
            all_buckets.extend(page_buckets);
        }
        builder.total_ops
    };

    report.diagnostics.push(Diagnostic {
        severity: Severity::Info,
        code: "pdf-content-stream-parsed".to_owned(),
        location: source_name.to_owned(),
        message: format!(
            "Parsed {} PDF/AI vector operator(s) across {} page(s).",
            total_ops,
            pages.len()
        ),
    });

    assemble_vector_buckets(project, all_buckets, source_name, options, &mut report);
    report.can_import = true;
    Ok((report, stage_bounds))
}

fn wrap_masked_root_layers_for_clipboard(project: &mut Project) {
    let root_id = project.root.clone();
    let Some(old_layers) = project.symbol(&root_id).map(|s| s.timeline.layers.clone()) else {
        return;
    };
    let has_mask = old_layers
        .iter()
        .any(|l| l.properties.role == LayerRole::Mask || l.properties.mask.is_some());
    if !has_mask {
        return;
    }

    let mut mask_layers_by_id: HashMap<String, Layer> = HashMap::new();
    for l in &old_layers {
        if l.properties.role == LayerRole::Mask {
            mask_layers_by_id.insert(l.id.clone(), l.clone());
        }
    }

    let mut new_root_layers = Vec::new();
    let mut new_symbols = Vec::new();
    let mut group_index = 1usize;

    for layer in old_layers {
        if layer.properties.role == LayerRole::Mask {
            continue;
        }
        if let Some(ref mask_id) = layer.properties.mask
            && let Some(mask_layer) = mask_layers_by_id.get(mask_id).cloned()
        {
            let sym_id = new_id("symbol");
            let sym_name = format!("Clipped Group {group_index}");
            group_index += 1;
            new_symbols.push(Symbol {
                id: sym_id.clone(),
                name: sym_name.clone(),
                folder: "Clipped Artwork".to_owned(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    frame_count: 1,
                    layers: vec![mask_layer, layer],
                    scripts: Vec::new(),
                    guides: Vec::new(),
                },
                class_name: None,
                linkage_id: None,
            });
            new_root_layers.push(Layer {
                id: new_id("layer"),
                name: sym_name.clone(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: new_id("element"),
                        name: sym_name,
                        transform: Transform::default(),
                        kind: ElementKind::Instance {
                            symbol_id: sym_id,
                            playback: InstancePlayback::Independent,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
                properties: LayerProperties::default(),
            });
        } else {
            new_root_layers.push(layer);
        }
    }

    if !new_symbols.is_empty() {
        if !project
            .library_folders
            .iter()
            .any(|f| f == "Clipped Artwork")
        {
            project.library_folders.push("Clipped Artwork".to_owned());
        }
        project.symbols.extend(new_symbols);
        if let Some(root) = project.symbol_mut(&root_id) {
            root.timeline.layers = new_root_layers;
        }
        project.upgrade_format_for_features();
    }
}

pub fn import_pdf_clipboard(
    bytes: &[u8],
    source_name: &str,
) -> Result<VectorClipboardImport, ImportError> {
    if bytes.len() > MAX_STAGE_VECTOR_CLIPBOARD_BYTES {
        return Err(ImportError::Container(
            "PDF clipboard payload exceeds the 64 MiB limit.".into(),
        ));
    }
    let header_slice = &bytes[..bytes.len().min(1024)];
    if !header_slice.windows(5).any(|w| w == b"%PDF-") {
        return Err(ImportError::Container(
            "PDF clipboard payload is missing a %PDF- header within the first 1024 bytes.".into(),
        ));
    }
    let options = VectorImportOptions {
        layer_mapping: VectorLayerMapping::PreserveGroupsAsLayers,
        text_handling: VectorTextHandling::EditableText,
        mask_handling: VectorMaskHandling::MaskLayers,
        transform_handling: VectorTransformHandling::BakeTransformsIntoPaths,
        resize_stage_to_viewbox: true,
    };
    let mut project = Project::blank();
    project.name = source_name.to_owned();
    let (report, stage_bounds) = import_pdf_ai_with_bounds(
        &mut project,
        bytes,
        source_name,
        &options,
        MAX_STAGE_VECTOR_CLIPBOARD_BYTES,
    )?;
    wrap_masked_root_layers_for_clipboard(&mut project);
    let has_content = project.symbol(&project.root).is_some_and(|s| {
        s.timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .any(|k| !k.elements.is_empty())
    });
    if !has_content {
        return Err(ImportError::Container(
            "PDF clipboard contains no importable vector, text, or bitmap elements.".into(),
        ));
    }
    Ok(VectorClipboardImport {
        project,
        report,
        stage_bounds,
    })
}

pub fn import_eps_clipboard(
    bytes: &[u8],
    source_name: &str,
) -> Result<VectorClipboardImport, ImportError> {
    if bytes.len() > MAX_STAGE_VECTOR_CLIPBOARD_BYTES {
        return Err(ImportError::Container(
            "EPS/AICB clipboard payload exceeds the 64 MiB limit.".into(),
        ));
    }
    let header_slice = &bytes[..bytes.len().min(1024)];
    let is_binary_eps = bytes.starts_with(eps::MAGIC);
    let is_ascii_ps = header_slice.windows(10).any(|w| w == b"%!PS-Adobe");
    if !is_binary_eps && !is_ascii_ps {
        return Err(ImportError::Container(
            "EPS/AICB clipboard payload is missing %!PS-Adobe or DOS binary EPS header.".into(),
        ));
    }
    let options = VectorImportOptions {
        layer_mapping: VectorLayerMapping::PreserveGroupsAsLayers,
        text_handling: VectorTextHandling::EditableText,
        mask_handling: VectorMaskHandling::MaskLayers,
        transform_handling: VectorTransformHandling::BakeTransformsIntoPaths,
        resize_stage_to_viewbox: true,
    };
    let mut project = Project::blank();
    project.name = source_name.to_owned();
    let (report, stage_bounds) =
        eps::import_with_bounds(&mut project, bytes, source_name, &options)?;
    wrap_masked_root_layers_for_clipboard(&mut project);
    let has_content = project.symbol(&project.root).is_some_and(|s| {
        s.timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .any(|k| !k.elements.is_empty())
    });
    if !has_content {
        return Err(ImportError::Container(
            "EPS/AICB clipboard contains no importable vector or preview elements.".into(),
        ));
    }
    Ok(VectorClipboardImport {
        project,
        report,
        stage_bounds,
    })
}
