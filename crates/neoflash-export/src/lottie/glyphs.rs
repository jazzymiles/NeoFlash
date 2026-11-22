//! Vector-glyph Bezier outline shape-group (`ty: "gr"` / `ty: "sh"`) and Bodymovin `chars`
//! emitter for Lottie / dotLottie text elements.

use super::{
    LottieBuilder, LottieNoteKind, LottieTextMode, round4,
    shapes::{apply_rgb_only, identity_group_tr, path_commands_to_lottie_beziers},
    tracks::layer_element_name,
};
use crate::ExportError;
use ab_glyph::{Font, FontRef, OutlineCurve};
use neoflash_core::{
    AppearancePaintEntry, Color, Element, ElementKind, Layer, PathCommand, TextAlign,
    TextFieldState, TextFieldType, TextFormat, Timeline, Transform,
};
use serde_json::{Value, json};

struct ResolvedOutlineFace {
    bytes: Vec<u8>,
    face_index: u32,
}

impl ResolvedOutlineFace {
    fn as_font_ref(&self) -> Option<FontRef<'_>> {
        FontRef::try_from_slice_and_index(&self.bytes, self.face_index).ok()
    }
}

fn covers_all_chars(bytes: &[u8], face_index: u32, text: &str) -> bool {
    let Ok(font) = FontRef::try_from_slice_and_index(bytes, face_index) else {
        return false;
    };
    text.chars()
        .all(|ch| ch == '\r' || ch.is_whitespace() || font.glyph_id(ch).0 != 0)
}

fn resolve_outline_face_for_text(
    project: &neoflash_core::Project,
    family: &str,
    bold: bool,
    italic: bool,
    text: &str,
) -> Option<ResolvedOutlineFace> {
    // 1. Embedded FontResource in `project.resources` (supports raw SFNT, `.ttc`, and WOFF1).
    // If an embedded font is subsetted and lacks one or more glyphs in `text`, fall through
    // to system/fallback fonts rather than aborting outline resolution immediately.
    if let Some(raw_bytes) = crate::fonts::resolve_project_font_bytes(project, family, bold, italic)
    {
        let unpacked = neoflash_core::sfnt_view_bytes(&raw_bytes)
            .map(std::borrow::Cow::into_owned)
            .unwrap_or(raw_bytes);
        let face_count = neoflash_core::sfnt_face_count(&unpacked).max(1);
        let preferred = neoflash_core::select_best_sfnt_face_index(&unpacked, family, bold, italic);
        let mut candidate_indices = vec![preferred];
        for idx in 0..face_count {
            if idx != preferred {
                candidate_indices.push(idx);
            }
        }
        for fidx in candidate_indices {
            if TextFormat::validate_text_outlines_for_font_face(
                "LottieText",
                family,
                text,
                &unpacked,
                fidx,
            )
            .is_empty()
                && covers_all_chars(&unpacked, fidx, text)
            {
                return Some(ResolvedOutlineFace {
                    bytes: unpacked,
                    face_index: fidx,
                });
            }
        }
    }

    // 2. System font via cached `fontdb::Database` (including large CJK `.ttc` collections via `face.index`).
    let db = crate::fonts::system_font_db();
    for face in crate::fonts::matching_system_faces(family, bold, italic) {
        let idx = face.index;
        if let Some(Some((bytes, out_idx))) = db.with_face_data(face.id, |data, _| {
            if data.len() > 512 * 1024 * 1024 {
                return None;
            }
            if !TextFormat::validate_text_outlines_for_font_face(
                "LottieText",
                family,
                text,
                data,
                idx,
            )
            .is_empty()
            {
                return None;
            }
            if !covers_all_chars(data, idx, text) {
                return None;
            }
            if (idx > 0 || data.starts_with(b"ttcf"))
                && let Some(single_sfnt) = neoflash_core::extract_ttc_face_to_sfnt(data, idx)
            {
                return Some((single_sfnt, 0));
            }
            Some((data.to_vec(), idx))
        }) {
            return Some(ResolvedOutlineFace {
                bytes,
                face_index: out_idx,
            });
        }
    }

    // 3. Built-in portable SFNT fallback (`Inter` / `RobotoMono`).
    let fallback = crate::fonts::fallback_font(family, bold);
    if TextFormat::validate_text_outlines_for_font_face("LottieText", family, text, fallback, 0)
        .is_empty()
        && covers_all_chars(fallback, 0, text)
    {
        return Some(ResolvedOutlineFace {
            bytes: fallback.to_vec(),
            face_index: 0,
        });
    }

    None
}

fn glyph_outline_commands(
    font: &FontRef<'_>,
    ch: char,
    origin_x: f64,
    baseline_y: f64,
    font_size: f64,
) -> Option<Vec<PathCommand>> {
    if ch.is_whitespace() {
        return Some(Vec::new());
    }
    let gid = font.glyph_id(ch);
    if gid.0 == 0 {
        return None;
    }
    let outline = font.outline(gid)?;
    if outline.curves.is_empty() {
        return None;
    }
    let upm = f64::from(font.units_per_em().unwrap_or(1000.0)).max(1.0);
    let scale = font_size.max(1.0) / upm;

    let mut commands = Vec::with_capacity(outline.curves.len() + 4);
    let mut current_pos: Option<(f64, f64)> = None;
    let mut contour_start: Option<(f64, f64)> = None;
    let map_pt = |pt: ab_glyph::Point| -> (f64, f64) {
        (
            origin_x + f64::from(pt.x) * scale,
            baseline_y - f64::from(pt.y) * scale,
        )
    };
    let same_pt = |a: (f64, f64), b: (f64, f64)| -> bool {
        (a.0 - b.0).abs() <= 1e-4 && (a.1 - b.1).abs() <= 1e-4
    };

    for curve in &outline.curves {
        match *curve {
            OutlineCurve::Line(p0, p1) => {
                let s = map_pt(p0);
                let e = map_pt(p1);
                if current_pos.is_none_or(|cur| !same_pt(cur, s)) {
                    if contour_start.is_some() {
                        commands.push(PathCommand::Close);
                    }
                    commands.push(PathCommand::MoveTo { x: s.0, y: s.1 });
                    contour_start = Some(s);
                }
                commands.push(PathCommand::LineTo { x: e.0, y: e.1 });
                current_pos = Some(e);
            }
            OutlineCurve::Quad(p0, c, p1) => {
                let s = map_pt(p0);
                let ctrl = map_pt(c);
                let e = map_pt(p1);
                if current_pos.is_none_or(|cur| !same_pt(cur, s)) {
                    if contour_start.is_some() {
                        commands.push(PathCommand::Close);
                    }
                    commands.push(PathCommand::MoveTo { x: s.0, y: s.1 });
                    contour_start = Some(s);
                }
                commands.push(PathCommand::QuadraticTo {
                    cx: ctrl.0,
                    cy: ctrl.1,
                    x: e.0,
                    y: e.1,
                });
                current_pos = Some(e);
            }
            OutlineCurve::Cubic(p0, c1, c2, p1) => {
                let s = map_pt(p0);
                let ctrl1 = map_pt(c1);
                let ctrl2 = map_pt(c2);
                let e = map_pt(p1);
                if current_pos.is_none_or(|cur| !same_pt(cur, s)) {
                    if contour_start.is_some() {
                        commands.push(PathCommand::Close);
                    }
                    commands.push(PathCommand::MoveTo { x: s.0, y: s.1 });
                    contour_start = Some(s);
                }
                commands.push(PathCommand::CubicTo {
                    cx1: ctrl1.0,
                    cy1: ctrl1.1,
                    cx2: ctrl2.0,
                    cy2: ctrl2.1,
                    x: e.0,
                    y: e.1,
                });
                current_pos = Some(e);
            }
        }
    }
    if contour_start.is_some() {
        commands.push(PathCommand::Close);
    }
    Some(commands)
}

fn resolve_element_appearance_stroke(
    layer: &Layer,
    frame: u32,
    element_id: &str,
) -> Option<(Color, f64)> {
    let kf = layer.key_at(frame)?;
    let app = kf.appearance(element_id)?;
    for entry in &app.entries {
        if let AppearancePaintEntry::Stroke {
            color,
            width,
            opacity,
            ..
        } = entry
            && *width > 0.0
            && *opacity > 0.0
        {
            let mut c = *color;
            c[3] = ((f64::from(c[3]) * opacity.clamp(0.0, 1.0)).round() as u8).max(1);
            return Some((c, *width));
        }
    }
    None
}

fn build_glyph_shape_group(
    ch: char,
    commands: &[PathCommand],
    fill: Color,
    stroke: Option<(Color, f64)>,
) -> Option<Value> {
    let contours = path_commands_to_lottie_beziers(commands);
    if contours.is_empty() {
        return None;
    }
    let mut items: Vec<Value> = contours
        .into_iter()
        .enumerate()
        .map(|(idx, bez)| {
            let dir = bez.lottie_direction();
            json!({
                "ty": "sh",
                "d": dir,
                "nm": format!("Contour {}", idx + 1),
                "ks": {
                    "a": 0,
                    "k": bez.to_json()
                }
            })
        })
        .collect();

    if fill[3] > 0 {
        let [r, g, b, a] = fill;
        items.push(json!({
            "ty": "fl",
            "nm": "Fill",
            "c": {
                "a": 0,
                "k": [
                    round4(f64::from(r) / 255.0),
                    round4(f64::from(g) / 255.0),
                    round4(f64::from(b) / 255.0),
                    1.0
                ]
            },
            "o": { "a": 0, "k": round4(f64::from(a) / 255.0 * 100.0) },
            "r": 1,
            "bm": 0
        }));
    }

    if let Some((sc, sw)) = stroke
        && sw > 0.0
        && sc[3] > 0
    {
        let [r, g, b, a] = sc;
        items.push(json!({
            "ty": "st",
            "nm": "Stroke",
            "c": {
                "a": 0,
                "k": [
                    round4(f64::from(r) / 255.0),
                    round4(f64::from(g) / 255.0),
                    round4(f64::from(b) / 255.0),
                    1.0
                ]
            },
            "o": { "a": 0, "k": round4(f64::from(a) / 255.0 * 100.0) },
            "w": { "a": 0, "k": round4(sw) },
            "lc": 2,
            "lj": 2,
            "ml": 4,
            "bm": 0
        }));
    }

    items.push(identity_group_tr());
    Some(json!({
        "ty": "gr",
        "nm": format!("Glyph '{ch}'"),
        "it": items,
        "bm": 0
    }))
}

#[derive(Clone, PartialEq)]
struct StyledChar {
    ch: char,
    family: String,
    size: f64,
    bold: bool,
    italic: bool,
    letter_spacing: f64,
    color: Color,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GlyphAlignMode {
    Left,
    Center,
    Right,
    Justify,
    JustifyAll,
}

#[derive(Clone, PartialEq)]
struct StyledCharsSpec {
    chars: Vec<StyledChar>,
    box_width: Option<f64>,
    word_wrap: bool,
    leading: f64,
    align_mode: GlyphAlignMode,
    stroke: Option<(Color, f64)>,
}

fn build_timed_visibility_group(
    nm: String,
    mut items: Vec<Value>,
    span_start: u32,
    span_end: u32,
    first_frame: u32,
    last_frame: u32,
) -> Value {
    let mut kfs = Vec::new();
    if span_start > first_frame {
        kfs.push(json!({"t": first_frame, "s": [0.0], "h": 1}));
    }
    kfs.push(json!({"t": span_start, "s": [100.0], "h": 1}));
    if span_end < last_frame {
        kfs.push(json!({"t": span_end, "s": [0.0], "h": 1}));
    }
    let mut tr = identity_group_tr();
    if let Some(obj) = tr.as_object_mut() {
        obj.insert("o".to_owned(), json!({"a": 1, "k": kfs}));
    }
    items.push(tr);
    json!({
        "ty": "gr",
        "nm": nm,
        "it": items,
        "bm": 0
    })
}

impl<'a> LottieBuilder<'a> {
    pub(super) fn should_outline_text_element(&self, element: &Element) -> bool {
        match &element.kind {
            ElementKind::Text { font_family, .. } => match self.options.outline_text_glyphs {
                Some(explicit) => explicit,
                None => {
                    self.options.text_mode == LottieTextMode::Shapes
                        || crate::fonts::has_embedded_font_face(
                            self.project,
                            font_family,
                            false,
                            false,
                        )
                }
            },
            ElementKind::TextBox { format, .. } => match self.options.outline_text_glyphs {
                Some(explicit) => explicit,
                None => {
                    if self.options.text_mode == LottieTextMode::Shapes {
                        return true;
                    }
                    let runtime_mutable =
                        TextFieldState::from_kind(&element.kind).is_some_and(|tf| {
                            !element.name.trim().is_empty()
                                || tf.field_type == TextFieldType::Input
                                || tf.selectable
                                || tf.variable.as_ref().is_some_and(|v| !v.is_empty())
                        });
                    !runtime_mutable
                        && crate::fonts::has_embedded_font_face(
                            self.project,
                            &format.font,
                            format.bold,
                            format.italic,
                        )
                }
            },
            _ => false,
        }
    }

    pub(super) fn record_glyph_chars_for_text(
        &mut self,
        text: &str,
        family: &str,
        font_size: f64,
        bold: bool,
        italic: bool,
    ) {
        if !self.options.emit_glyph_chars {
            return;
        }
        let clean_family = if family.trim().is_empty() {
            "sans-serif"
        } else {
            family.trim()
        };
        let style = match (bold, italic) {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            (false, false) => "Regular",
        };
        let Some(resolved) =
            resolve_outline_face_for_text(self.project, clean_family, bold, italic, text)
        else {
            return;
        };
        let Some(font) = resolved.as_font_ref() else {
            return;
        };
        let upm = f64::from(font.units_per_em().unwrap_or(1000.0)).max(1.0);
        let size_norm = round4(font_size.max(1.0));
        let size_key = size_norm.to_bits();

        for ch in text.chars() {
            if ch == '\r' || ch == '\n' {
                continue;
            }
            let key = (
                ch.to_string(),
                clean_family.to_owned(),
                style.to_owned(),
                size_key,
            );
            if self.glyph_chars.contains_key(&key) {
                continue;
            }
            let gid = font.glyph_id(ch);
            let w = round4(f64::from(font.h_advance_unscaled(gid)) / upm * 100.0);
            let shapes = if ch.is_whitespace() {
                Vec::new()
            } else if let Some(cmds) = glyph_outline_commands(&font, ch, 0.0, 0.0, 100.0) {
                let contours = path_commands_to_lottie_beziers(&cmds);
                if contours.is_empty() {
                    Vec::new()
                } else {
                    let mut it: Vec<Value> = contours
                        .into_iter()
                        .enumerate()
                        .map(|(i, bez)| {
                            json!({
                                "ty": "sh",
                                "d": bez.lottie_direction(),
                                "nm": format!("Path {}", i + 1),
                                "ks": { "a": 0, "k": bez.to_json() }
                            })
                        })
                        .collect();
                    it.push(identity_group_tr());
                    vec![json!({
                        "ty": "gr",
                        "nm": ch.to_string(),
                        "it": it,
                        "bm": 0
                    })]
                }
            } else {
                continue;
            };
            self.glyph_chars.insert(
                key,
                json!({
                    "ch": ch.to_string(),
                    "size": size_norm,
                    "style": style,
                    "w": w,
                    "data": { "shapes": shapes },
                    "fFamily": clean_family
                }),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn try_emit_outlined_text_layer(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        layer: &Layer,
        samples: &[(u32, Element)],
        representative: &Element,
        first_frame: u32,
        last_frame: u32,
        bm: u32,
        orient_auto: bool,
        force_dense_ks: bool,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        comp_origin_offset: [f64; 2],
        world_samples_for_mask: &[(u32, Transform)],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Option<Vec<Value>>, ExportError> {
        self.check_cancelled()?;
        if !self.should_outline_text_element(representative) {
            return Ok(None);
        }

        let build_spec_for_sample =
            |sample_frame: u32, el: &Element| -> Option<(StyledCharsSpec, Option<&'static str>)> {
                let ct = el.transform.color_transform;
                let app_stroke = resolve_element_appearance_stroke(layer, sample_frame, &el.id)
                    .map(|(sc, sw)| (apply_rgb_only(ct, sc, false), sw));
                match &el.kind {
                    ElementKind::Text {
                        text,
                        font_family,
                        font_size,
                        color,
                    } => {
                        let tinted = apply_rgb_only(ct, *color, false);
                        let chars: Vec<StyledChar> = text
                            .chars()
                            .map(|ch| StyledChar {
                                ch,
                                family: font_family.clone(),
                                size: *font_size,
                                bold: false,
                                italic: false,
                                letter_spacing: 0.0,
                                color: tinted,
                            })
                            .collect();
                        Some((
                            StyledCharsSpec {
                                chars,
                                box_width: None,
                                word_wrap: false,
                                leading: 0.0,
                                align_mode: GlyphAlignMode::Left,
                                stroke: app_stroke,
                            },
                            None,
                        ))
                    }
                    ElementKind::TextBox { width, format, .. } => {
                        let tf_state = TextFieldState::from_kind(&el.kind)?;
                        let dyn_label = if tf_state.field_type == TextFieldType::Input
                            || !el.name.trim().is_empty()
                        {
                            Some(match tf_state.field_type {
                                TextFieldType::Input => "Input",
                                TextFieldType::Dynamic => "Dynamic",
                            })
                        } else {
                            None
                        };
                        let total_units = tf_state.text.encode_utf16().count() as u32;
                        let runs = tf_state.materialized_runs(total_units);
                        let mut cursor_u16 = 0u32;
                        let mut run_idx = 0usize;
                        let mut chars = Vec::new();
                        for ch in tf_state.text.chars() {
                            while run_idx + 1 < runs.len() && cursor_u16 >= runs[run_idx].end {
                                run_idx += 1;
                            }
                            let f = runs
                                .get(run_idx)
                                .filter(|r| r.start <= cursor_u16 && cursor_u16 < r.end)
                                .map_or(&tf_state.format, |r| &r.format);
                            let raw_col = [
                                ((f.color >> 16) & 0xFF) as u8,
                                ((f.color >> 8) & 0xFF) as u8,
                                (f.color & 0xFF) as u8,
                                tf_state.alpha,
                            ];
                            chars.push(StyledChar {
                                ch,
                                family: f.font.clone(),
                                size: f.size,
                                bold: f.bold,
                                italic: f.italic,
                                letter_spacing: f.letter_spacing,
                                color: apply_rgb_only(ct, raw_col, false),
                            });
                            cursor_u16 = cursor_u16.saturating_add(ch.len_utf16() as u32);
                        }
                        let align_mode = match format.effective_align_mode() {
                            "right" | "end" => GlyphAlignMode::Right,
                            "center" => GlyphAlignMode::Center,
                            "justify" => GlyphAlignMode::Justify,
                            "justify_all" => GlyphAlignMode::JustifyAll,
                            _ => match format.align {
                                TextAlign::Left => GlyphAlignMode::Left,
                                TextAlign::Center => GlyphAlignMode::Center,
                                TextAlign::Right => GlyphAlignMode::Right,
                            },
                        };
                        Some((
                            StyledCharsSpec {
                                chars,
                                box_width: Some(*width),
                                word_wrap: tf_state.word_wrap,
                                leading: format.leading,
                                align_mode,
                                stroke: app_stroke,
                            },
                            dyn_label,
                        ))
                    }
                    _ => None,
                }
            };

        let fallback_sample;
        let effective_samples: &[(u32, Element)] = if samples.is_empty() {
            fallback_sample = [(first_frame, representative.clone())];
            &fallback_sample
        } else {
            samples
        };

        let mut dynamic_kind_label: Option<&'static str> = None;
        let mut spans: Vec<(u32, StyledCharsSpec)> = Vec::new();
        for (sample_frame, sample_el) in effective_samples {
            let Some((spec, dyn_lbl)) = build_spec_for_sample(*sample_frame, sample_el) else {
                return Ok(None);
            };
            if dynamic_kind_label.is_none() {
                dynamic_kind_label = dyn_lbl;
            }
            if spans.last().is_none_or(|(_, prev)| prev != &spec) {
                spans.push((*sample_frame, spec));
            }
        }
        if spans.is_empty() {
            return Ok(None);
        }

        if let Some(kind_label) = dynamic_kind_label {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_DYNAMIC_INPUT_TEXT"),
                format!(
                    "Layer '{}' · {}",
                    layer.name,
                    layer_element_name(layer, representative)
                ),
                format!(
                    "{kind_label} text field interactivity/runtime binding is not supported by Lottie players; static vector outlines were exported."
                ),
                Some(symbol_id.to_owned()),
                Some(layer.id.clone()),
                Some(first_frame),
            );
        }

        let glyph_groups = if spans.len() == 1 {
            let (_, spec) = &spans[0];
            self.layout_styled_chars_to_glyph_groups(
                &spec.chars,
                spec.box_width,
                spec.word_wrap,
                spec.leading,
                spec.align_mode,
                spec.stroke,
            )
        } else {
            let mut timed_groups = Vec::with_capacity(spans.len());
            let mut all_ok = true;
            for (idx, (s_start, spec)) in spans.iter().enumerate() {
                let s_end = spans.get(idx + 1).map_or(last_frame, |(next_f, _)| *next_f);
                if let Some(span_glyphs) = self.layout_styled_chars_to_glyph_groups(
                    &spec.chars,
                    spec.box_width,
                    spec.word_wrap,
                    spec.leading,
                    spec.align_mode,
                    spec.stroke,
                ) {
                    timed_groups.push(build_timed_visibility_group(
                        format!("Text Keyframe {s_start}"),
                        span_glyphs,
                        *s_start,
                        s_end,
                        first_frame,
                        last_frame,
                    ));
                } else {
                    all_ok = false;
                    break;
                }
            }
            all_ok.then_some(timed_groups)
        };

        let Some(glyph_groups) = glyph_groups else {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_TEXT_OUTLINE_FALLBACK"),
                format!(
                    "Layer '{}' · {}",
                    layer.name,
                    layer_element_name(layer, representative)
                ),
                "Font lacks vector glyph outlines for one or more characters; retained Lottie text layer fallback.",
                Some(symbol_id.to_owned()),
                Some(layer.id.clone()),
                Some(first_frame),
            );
            return Ok(None);
        };

        let ks = self.build_animated_ks(
            timeline,
            layer,
            samples,
            [0.0, 0.0],
            origin_offset,
            force_dense_ks,
            orient_auto,
        )?;
        let ind = *next_ind;
        *next_ind += 1;
        let obj = json!({
            "ddd": 0,
            "ind": ind,
            "ty": 4,
            "nm": layer_element_name(layer, representative),
            "sr": 1,
            "ks": ks,
            "ao": if orient_auto { 1 } else { 0 },
            "shapes": glyph_groups,
            "ip": first_frame,
            "op": last_frame,
            "st": first_frame,
            "bm": bm
        });
        let wrapped = self.attach_mask_or_matte(
            symbol_id,
            timeline,
            mask_layer,
            obj,
            first_frame,
            last_frame,
            world_samples_for_mask,
            comp_origin_offset,
            stack,
            next_ind,
        )?;
        self.outlined_text_count += 1;
        self.exported_count += 1;
        Ok(Some(wrapped))
    }

    fn layout_styled_chars_to_glyph_groups(
        &self,
        chars: &[StyledChar],
        box_width: Option<f64>,
        word_wrap: bool,
        leading: f64,
        align_mode: GlyphAlignMode,
        stroke: Option<(Color, f64)>,
    ) -> Option<Vec<Value>> {
        // Split into explicit paragraphs on '\n' (ignoring '\r').
        let mut paragraphs: Vec<Vec<&StyledChar>> = vec![Vec::new()];
        for sc in chars {
            if sc.ch == '\r' {
                continue;
            }
            if sc.ch == '\n' {
                paragraphs.push(Vec::new());
            } else if let Some(cur) = paragraphs.last_mut() {
                cur.push(sc);
            }
        }

        // Cache resolved faces per (family, bold, italic) across the element.
        let mut face_cache: std::collections::BTreeMap<(String, bool, bool), ResolvedOutlineFace> =
            std::collections::BTreeMap::new();
        for sc in chars {
            if sc.ch == '\r' || sc.ch == '\n' || sc.ch.is_whitespace() {
                continue;
            }
            let key = (sc.family.clone(), sc.bold, sc.italic);
            if let std::collections::btree_map::Entry::Vacant(e) = face_cache.entry(key) {
                let sub_text: String = chars
                    .iter()
                    .filter(|c| c.family == sc.family && c.bold == sc.bold && c.italic == sc.italic)
                    .map(|c| c.ch)
                    .collect();
                let resolved = resolve_outline_face_for_text(
                    self.project,
                    &sc.family,
                    sc.bold,
                    sc.italic,
                    &sub_text,
                )?;
                e.insert(resolved);
            }
        }

        let measure_line = |line: &[&StyledChar]| -> (Vec<(f64, f64)>, f64, f64) {
            let max_size = line
                .iter()
                .map(|c| c.size.max(1.0))
                .fold(0.0f64, f64::max)
                .max(chars.first().map(|c| c.size.max(1.0)).unwrap_or(16.0));
            let mut advances = Vec::with_capacity(line.len());
            let mut max_ascent = max_size * 0.8;
            let mut prev_face_gid: Option<((String, bool, bool), ab_glyph::GlyphId, f64)> = None;

            for sc in line {
                let sz = sc.size.max(1.0);
                let key = (sc.family.clone(), sc.bold, sc.italic);
                let face_opt = face_cache.get(&key).or_else(|| face_cache.values().next());
                if let Some(face) = face_opt
                    && let Some(font) = face.as_font_ref()
                {
                    let upm = f64::from(font.units_per_em().unwrap_or(1000.0)).max(1.0);
                    let scale = sz / upm;
                    let asc_u = f64::from(font.ascent_unscaled());
                    if asc_u.is_finite() && asc_u > 0.0 {
                        max_ascent = max_ascent.max(asc_u * scale);
                    }
                    let gid = font.glyph_id(sc.ch);
                    let kern = if let Some((ref pk, prev_gid, prev_scale)) = prev_face_gid
                        && pk == &key
                    {
                        f64::from(font.kern_unscaled(prev_gid, gid)) * prev_scale
                    } else {
                        0.0
                    };
                    let raw_adv = f64::from(font.h_advance_unscaled(gid)) * scale;
                    let adv = if sc.ch.is_whitespace() {
                        raw_adv.max(sz * 0.25)
                    } else {
                        raw_adv
                    } + sc.letter_spacing;
                    advances.push((kern, adv));
                    prev_face_gid = Some((key, gid, scale));
                } else {
                    advances.push((0.0, sz * 0.3 + sc.letter_spacing));
                    prev_face_gid = None;
                }
            }
            (advances, max_size, max_ascent)
        };

        let mut visual_lines: Vec<(Vec<&StyledChar>, bool)> = Vec::new();
        for para in paragraphs {
            if let Some(max_w) = box_width.filter(|w| word_wrap && *w > 0.0) {
                if para.is_empty() {
                    visual_lines.push((Vec::new(), false));
                    continue;
                }
                let (para_advs, _, _) = measure_line(&para);
                let mut cur_line: Vec<&StyledChar> = Vec::new();
                let mut cur_width = 0.0f64;
                let mut idx = 0usize;
                while idx < para.len() {
                    if para[idx].ch.is_whitespace() {
                        if !cur_line.is_empty() {
                            cur_width += para_advs[idx].0 + para_advs[idx].1;
                            cur_line.push(para[idx]);
                        }
                        idx += 1;
                        continue;
                    }
                    let word_start = idx;
                    while idx < para.len() && !para[idx].ch.is_whitespace() {
                        idx += 1;
                    }
                    let word_end = idx;
                    let word_total: f64 = (word_start..word_end)
                        .map(|k| para_advs[k].0 + para_advs[k].1)
                        .sum();
                    let word_vis = (word_total - para[word_end - 1].letter_spacing).max(0.0);

                    if !cur_line.is_empty() && cur_width + word_vis > max_w + 1e-4 {
                        while cur_line.last().is_some_and(|c| c.ch.is_whitespace()) {
                            cur_line.pop();
                        }
                        visual_lines.push((std::mem::take(&mut cur_line), true));
                        cur_width = 0.0;
                    }

                    if cur_line.is_empty() && word_vis > max_w + 1e-4 {
                        for k in word_start..word_end {
                            let ch_adv = para_advs[k].0 + para_advs[k].1;
                            let ch_vis = (ch_adv - para[k].letter_spacing).max(0.0);
                            if !cur_line.is_empty() && cur_width + ch_vis > max_w + 1e-4 {
                                visual_lines.push((std::mem::take(&mut cur_line), true));
                                cur_width = 0.0;
                            }
                            cur_line.push(para[k]);
                            cur_width += ch_adv;
                        }
                    } else {
                        for k in word_start..word_end {
                            cur_line.push(para[k]);
                            cur_width += para_advs[k].0 + para_advs[k].1;
                        }
                    }
                }
                visual_lines.push((cur_line, false));
            } else {
                visual_lines.push((para, false));
            }
        }

        let mut groups = Vec::new();
        let mut top_y = 0.0;
        for (line, is_wrapped_non_final) in visual_lines {
            let (advances, max_size, max_ascent) = measure_line(&line);
            let last_non_ws = line.iter().rposition(|sc| !sc.ch.is_whitespace());
            let first_non_ws = line
                .iter()
                .position(|sc| !sc.ch.is_whitespace())
                .unwrap_or(0);
            let visible_width: f64 = if let Some(last_idx) = last_non_ws {
                let sum: f64 = advances[..=last_idx].iter().map(|(k, a)| k + a).sum();
                (sum - line[last_idx].letter_spacing).max(0.0)
            } else {
                0.0
            };
            let space_count = if let Some(last_idx) = last_non_ws
                && last_idx > first_non_ws
            {
                (first_non_ws..last_idx)
                    .filter(|&i| line[i].ch.is_whitespace())
                    .count()
            } else {
                0
            };
            let do_justify = match align_mode {
                GlyphAlignMode::JustifyAll => true,
                GlyphAlignMode::Justify => is_wrapped_non_final,
                _ => false,
            };
            let (mut cursor_x, extra_per_space) = if do_justify
                && space_count > 0
                && let Some(w) = box_width.filter(|w| *w > visible_width)
            {
                (0.0, (w - visible_width) / (space_count as f64))
            } else {
                let start_x = match (box_width, align_mode) {
                    (Some(w), GlyphAlignMode::Center) if w > 0.0 => (w - visible_width) * 0.5,
                    (Some(w), GlyphAlignMode::Right) if w > 0.0 => w - visible_width,
                    _ => 0.0,
                };
                (start_x, 0.0)
            };
            let baseline_y = top_y + max_ascent;

            for (idx, (sc, (kern, adv))) in line.iter().zip(advances).enumerate() {
                cursor_x += kern;
                if !sc.ch.is_whitespace() {
                    let key = (sc.family.clone(), sc.bold, sc.italic);
                    let face = face_cache.get(&key)?;
                    let font = face.as_font_ref()?;
                    let cmds = glyph_outline_commands(
                        &font,
                        sc.ch,
                        cursor_x,
                        baseline_y,
                        sc.size.max(1.0),
                    )?;
                    if let Some(grp) = build_glyph_shape_group(sc.ch, &cmds, sc.color, stroke) {
                        groups.push(grp);
                    }
                }
                cursor_x += adv;
                if extra_per_space > 0.0
                    && sc.ch.is_whitespace()
                    && idx > first_non_ws
                    && last_non_ws.is_some_and(|l| idx < l)
                {
                    cursor_x += extra_per_space;
                }
            }

            top_y += (max_size * 1.2 + leading).max(1.0);
        }

        Some(groups)
    }
}
