//! Portable font policies, @font-face embedding, deterministic subsetting, and glyph outline conversion.

use ab_glyph::{Font, FontRef, OutlineCurve, PxScale, ScaleFont};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use neoflash_core::{
    ElementKind, FillRule, PathCommand, Project, ResourceUnit, TextFieldState, TextFieldType,
    TextFormat,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const FONT_EMBEDDING_METADATA_PATH: &str = "fonts/embedding.json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontExportPolicy {
    Device,
    #[default]
    Embed,
    Subset,
    Outlines,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontSubsetStats {
    pub family: String,
    pub original_bytes: usize,
    pub subset_bytes: usize,
    pub codepoint_count: usize,
    /// False when NeoFlash preserves the original browser-loadable font rather than emitting an
    /// internal metrics blob under a font MIME type.
    #[serde(default)]
    pub subset_applied: bool,
    /// Why a complete font was preserved, or which conservative boundary was used. This is an
    /// export diagnostic, not a claim that NeoFlash implements every OpenType shaping behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FontBundle {
    pub policy: FontExportPolicy,
    pub css_rules: String,
    pub metrics: BTreeMap<String, serde_json::Value>,
    pub subset_stats: Vec<FontSubsetStats>,
    pub outlined_elements: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outline_diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct EmbeddedFontMetadata {
    #[serde(default)]
    id: String,
    #[serde(default)]
    family: String,
    #[serde(default)]
    style: String,
    #[serde(default)]
    ranges: Vec<bool>,
    #[serde(default)]
    extra_chars: String,
}

fn embedding_metadata(project: &Project) -> BTreeMap<String, EmbeddedFontMetadata> {
    project
        .resources
        .iter()
        .find(|resource| {
            resource
                .path
                .eq_ignore_ascii_case(FONT_EMBEDDING_METADATA_PATH)
        })
        .and_then(|resource| resource.decoded_bytes().ok())
        .and_then(|bytes| serde_json::from_slice::<Vec<EmbeddedFontMetadata>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| !entry.id.is_empty())
        .map(|entry| (entry.id.clone(), entry))
        .collect()
}

fn requested_style(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    }
}

fn face_key(family: &str, style: &str) -> String {
    format!(
        "{}\u{1f}{}",
        family.trim().to_ascii_lowercase(),
        style.trim().to_ascii_lowercase()
    )
}

fn metadata_codepoints(entry: &EmbeddedFontMetadata) -> BTreeSet<u32> {
    let mut codepoints = BTreeSet::from([32]);
    let enabled = |index| entry.ranges.get(index).copied().unwrap_or(false);
    if enabled(0) {
        codepoints.extend('A' as u32..='Z' as u32);
    }
    if enabled(1) {
        codepoints.extend('a' as u32..='z' as u32);
    }
    if enabled(2) {
        codepoints.extend('0' as u32..='9' as u32);
    }
    if enabled(3) {
        codepoints.extend(0x20..=0x2f);
        codepoints.extend(0x3a..=0x40);
        codepoints.extend(0x5b..=0x60);
        codepoints.extend(0x7b..=0x7e);
    }
    if enabled(4) {
        codepoints.extend(0x20..=0x7e);
    }
    if enabled(5) {
        codepoints.extend(0x0400..=0x04ff);
    }
    if enabled(6) {
        codepoints.extend(0x0100..=0x017f);
    }
    if enabled(7) {
        codepoints.extend(0x0370..=0x03ff);
    }
    codepoints.extend(entry.extra_chars.chars().map(|character| character as u32));
    codepoints
}

fn metadata_has_explicit_character_set(entry: &EmbeddedFontMetadata) -> bool {
    entry.ranges.iter().any(|enabled| *enabled) || !entry.extra_chars.is_empty()
}

pub fn is_font_resource(resource: &ResourceUnit) -> bool {
    let mime = resource.media_type.trim().to_ascii_lowercase();
    if matches!(
        mime.as_str(),
        "font/ttf"
            | "font/otf"
            | "font/woff"
            | "font/woff2"
            | "application/font-sfnt"
            | "application/x-font-ttf"
            | "application/x-font-opentype"
            | "application/vnd.ms-opentype"
            | "application/font-woff"
    ) {
        return true;
    }
    let path_lower = resource.path.to_ascii_lowercase();
    path_lower.ends_with(".ttf")
        || path_lower.ends_with(".otf")
        || path_lower.ends_with(".woff")
        || path_lower.ends_with(".woff2")
}

pub fn font_family_for_resource(resource: &ResourceUnit) -> String {
    let stem = Path::new(&resource.path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&resource.id)
        .trim();
    if stem.is_empty() {
        resource.id.clone()
    } else {
        stem.to_string()
    }
}

#[derive(Default)]
struct FontUsage {
    by_family: BTreeMap<String, BTreeSet<u32>>,
    by_face: BTreeMap<String, BTreeSet<u32>>,
    runtime_mutable_families: BTreeSet<String>,
    runtime_mutable_faces: BTreeSet<String>,
}

fn add_character_usage(usage: &mut FontUsage, format: &TextFormat, character: char) {
    usage
        .by_family
        .entry(format.font.clone())
        .or_default()
        .extend([32, character as u32]);
    usage
        .by_face
        .entry(face_key(
            &format.font,
            requested_style(format.bold, format.italic),
        ))
        .or_default()
        .extend([32, character as u32]);
}

fn mark_runtime_mutable_face(usage: &mut FontUsage, format: &TextFormat) {
    usage
        .runtime_mutable_families
        .insert(format.font.trim().to_ascii_lowercase());
    usage.runtime_mutable_faces.insert(face_key(
        &format.font,
        requested_style(format.bold, format.italic),
    ));
}

fn collect_text_box_usage(usage: &mut FontUsage, field: &TextFieldState) {
    let total_units = field.text.encode_utf16().count() as u32;
    let runs = field.materialized_runs(total_units);
    let runtime_mutable = field.field_type == TextFieldType::Input
        || field.selectable
        || field.variable.as_ref().is_some_and(|name| !name.is_empty());

    if runs.is_empty() {
        usage
            .by_family
            .entry(field.format.font.clone())
            .or_default()
            .insert(32);
        usage
            .by_face
            .entry(face_key(
                &field.format.font,
                requested_style(field.format.bold, field.format.italic),
            ))
            .or_default()
            .insert(32);
        if runtime_mutable {
            mark_runtime_mutable_face(usage, &field.format);
        }
        return;
    }

    if runtime_mutable {
        // A later script/user edit may use the field's default face even when every current
        // character is covered by an authored rich-text run.
        mark_runtime_mutable_face(usage, &field.format);
        for run in &runs {
            mark_runtime_mutable_face(usage, &run.format);
        }
    }

    let mut cursor_utf16 = 0u32;
    let mut run_index = 0usize;
    for character in field.text.chars() {
        while run_index + 1 < runs.len() && cursor_utf16 >= runs[run_index].end {
            run_index += 1;
        }
        let format = runs
            .get(run_index)
            .filter(|run| run.start <= cursor_utf16 && cursor_utf16 < run.end)
            .map_or(&field.format, |run| &run.format);
        add_character_usage(usage, format, character);
        cursor_utf16 = cursor_utf16.saturating_add(character.len_utf16() as u32);
    }
}

fn collect_font_usage(project: &Project) -> FontUsage {
    let mut usage = FontUsage::default();
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for keyframe in &layer.keyframes {
                for element in &keyframe.elements {
                    match &element.kind {
                        ElementKind::Text {
                            text, font_family, ..
                        } => {
                            let format = TextFormat {
                                font: font_family.clone(),
                                ..TextFormat::default()
                            };
                            usage
                                .by_family
                                .entry(format.font.clone())
                                .or_default()
                                .insert(32);
                            usage
                                .by_face
                                .entry(face_key(&format.font, "Regular"))
                                .or_default()
                                .insert(32);
                            for ch in text.chars() {
                                add_character_usage(&mut usage, &format, ch);
                            }
                        }
                        ElementKind::TextBox { .. } => {
                            if let Some(field) = TextFieldState::from_kind(&element.kind) {
                                collect_text_box_usage(&mut usage, &field);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    usage
}

pub fn collect_used_codepoints(project: &Project) -> BTreeMap<String, BTreeSet<u32>> {
    collect_font_usage(project).by_family
}

pub fn embedded_font_families(project: &Project) -> BTreeSet<String> {
    let metadata = embedding_metadata(project);
    let mut families = BTreeSet::new();
    for res in &project.resources {
        if is_font_resource(res) {
            let fam = metadata
                .get(&res.id)
                .map(|entry| entry.family.trim())
                .filter(|family| !family.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| font_family_for_resource(res));
            families.insert(fam.to_ascii_lowercase());
            families.insert(fam);
            families.insert(res.id.to_ascii_lowercase());
            families.insert(res.id.clone());
        }
    }
    families
}

/// Whether the project carries a font resource for the exact authored TextField face.
/// Legacy resources without embedding metadata retain their historical family-only match.
pub fn has_embedded_font_face(project: &Project, family: &str, bold: bool, italic: bool) -> bool {
    let metadata = embedding_metadata(project);
    let style = requested_style(bold, italic);
    project.resources.iter().any(|resource| {
        if !is_font_resource(resource) {
            return false;
        }
        if let Some(entry) = metadata.get(&resource.id) {
            entry.family.eq_ignore_ascii_case(family) && entry.style.eq_ignore_ascii_case(style)
        } else {
            font_family_for_resource(resource).eq_ignore_ascii_case(family)
        }
    })
}

/// Resolves the raw font bytes from `project.resources` matching `(family, bold, italic)`.
pub fn resolve_project_font_bytes(
    project: &Project,
    family: &str,
    bold: bool,
    italic: bool,
) -> Option<Vec<u8>> {
    let metadata = embedding_metadata(project);
    let target_key = face_key(family, requested_style(bold, italic));
    let mut fallback: Option<Vec<u8>> = None;
    for res in &project.resources {
        if !is_font_resource(res) {
            continue;
        }
        let Ok(bytes) = res.decoded_bytes() else {
            continue;
        };
        let entry = metadata.get(&res.id);
        let res_family = entry
            .map(|e| e.family.trim())
            .filter(|f| !f.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| font_family_for_resource(res));
        let res_style = entry.map_or("Regular", |e| e.style.as_str());
        if face_key(&res_family, res_style) == target_key {
            return Some(bytes);
        }
        if fallback.is_none()
            && (res_family.eq_ignore_ascii_case(family) || res.id.eq_ignore_ascii_case(family))
        {
            fallback = Some(bytes);
        }
    }
    fallback
}

/// Converts a single glyph's `ab_glyph::Outline` curves (`OutlineCurve::{Line, Quad, Cubic}`)
/// from font design units (Y-up) into stage `PathCommand`s (Y-down) at `(origin_x, baseline_y)`.
pub fn outline_glyph(
    font: &FontRef<'_>,
    ch: char,
    origin_x: f64,
    top_y: f64,
    font_size: f64,
) -> Option<Vec<PathCommand>> {
    if ch.is_whitespace() {
        return Some(Vec::new());
    }
    let gid = font.glyph_id(ch);
    if gid.0 == 0 {
        return None;
    }
    // Verify `outline_glyph` succeeds at the target pixel scale as well as raw `outline`.
    let px_scale = PxScale::from((font_size.max(1.0)) as f32);
    let _ = font.outline_glyph(gid.with_scale(px_scale))?;
    let outline = font.outline(gid)?;
    if outline.curves.is_empty() {
        return None;
    }
    let upm = f64::from(font.units_per_em().unwrap_or(1000.0)).max(1.0);
    let scale = font_size.max(1.0) / upm;
    let ascent = f64::from(font.as_scaled(PxScale::from(upm as f32)).ascent()).max(upm * 0.8);
    let baseline = top_y + ascent * scale;

    let mut commands = Vec::with_capacity(outline.curves.len() + 4);
    let mut current_pos: Option<(f64, f64)> = None;
    let mut contour_start: Option<(f64, f64)> = None;
    let map_pt = |pt: ab_glyph::Point| -> (f64, f64) {
        (
            origin_x + f64::from(pt.x) * scale,
            baseline - f64::from(pt.y) * scale,
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

/// Extracts vector outline `PathCommand`s for static `text`, supporting TrueType (`glyf`),
/// composite TrueType glyphs, and OpenType/CFF (`OTTO` / `CFF ` / `CFF2`) faces via `ab_glyph`.
/// When `font_bytes` contains unsupported bitmap/color tables (`CBDT`, `sbix`, `COLR`) or lacks
/// glyph coverage for any non-whitespace character in `text`, returns `Err(diagnostics)` with
/// actionable located diagnostics so the caller retains visible text fallback rather than dropping glyphs.
pub fn extract_text_outline_commands_with_diagnostics(
    location: &str,
    text: &str,
    font_family: &str,
    font_size: f64,
    font_bytes: Option<&[u8]>,
) -> Result<Vec<PathCommand>, Vec<String>> {
    let Some(bytes) = font_bytes else {
        return Ok(TextFieldState::static_text_to_outline_commands(
            text,
            font_family,
            font_size,
            None,
        ));
    };

    let table_diags =
        TextFormat::validate_text_outlines_for_font(location, font_family, text, bytes);
    if !table_diags.is_empty() {
        return Err(table_diags);
    }

    if let Ok(font) = FontRef::try_from_slice(bytes) {
        let upm = f64::from(font.units_per_em().unwrap_or(1000.0)).max(1.0);
        let scaled = font.as_scaled(PxScale::from(upm as f32));
        let scale = font_size.max(1.0) / upm;
        let line_h = font_size.max(1.0) * 1.2;

        let mut missing_chars = Vec::new();
        let mut commands = Vec::new();
        for (line_idx, line) in text.split('\n').enumerate() {
            let top_y = (line_idx as f64) * line_h;
            let mut cursor_x = 0.0;
            for ch in line.chars() {
                if ch == '\r' {
                    continue;
                }
                let gid = font.glyph_id(ch);
                let adv = f64::from(scaled.h_advance(gid)) * scale;
                if ch.is_whitespace() {
                    cursor_x += adv.max(font_size * 0.25);
                    continue;
                }
                match outline_glyph(&font, ch, cursor_x, top_y, font_size) {
                    Some(glyph_cmds) => {
                        commands.extend(glyph_cmds);
                    }
                    None => {
                        let cp = ch as u32;
                        let item = format!("U+{cp:04X} ('{ch}')");
                        if !missing_chars.contains(&item) {
                            missing_chars.push(item);
                        }
                    }
                }
                cursor_x += adv;
            }
        }
        if !missing_chars.is_empty() {
            return Err(vec![format!(
                "{location}: font '{font_family}' is missing outline glyph coverage for {}; retaining visible text fallback",
                missing_chars.join(", ")
            )]);
        }
        return Ok(commands);
    }

    // Fallback for minimal SFNT streams without full `name`/`OS/2` tables (e.g., synthetic TrueType + composite glyf).
    TextFormat::extract_sfnt_outline_commands(font_family, text, font_size, bytes).map_err(
        |missing_chars| {
            vec![format!(
                "{location}: font '{font_family}' is missing outline glyph coverage for {}; retaining visible text fallback",
                missing_chars.join(", ")
            )]
        },
    )
}

/// Extracts vector outline `PathCommand`s for a `TextFieldState`, validating embedded font tables
/// and glyph coverage and returning actionable located diagnostics on failure.
pub fn extract_text_field_outline_commands_with_diagnostics(
    location: &str,
    field: &TextFieldState,
    font_bytes: Option<&[u8]>,
) -> Result<Vec<PathCommand>, Vec<String>> {
    let Some(bytes) = font_bytes else {
        return Ok(field.to_outline_commands(None));
    };
    let table_diags = TextFormat::validate_text_outlines_for_font(
        location,
        &field.format.font,
        &field.text,
        bytes,
    );
    if !table_diags.is_empty() {
        return Err(table_diags);
    }
    extract_text_outline_commands_with_diagnostics(
        location,
        &field.text,
        &field.format.font,
        field.format.size,
        Some(bytes),
    )
}

/// Converts static text elements in `project` to `ElementKind::Path` outlines while emitting
/// actionable located diagnostics (`symbol/layer/frame/element`) and retaining visible text
/// fallback whenever a font uses unsupported bitmap/color tables (`CBDT`, `sbix`, `COLR`) or
/// is missing glyph coverage.
pub fn convert_project_text_to_outlines_with_diagnostics(
    project: &mut Project,
) -> (usize, Vec<String>) {
    let metadata = embedding_metadata(project);
    let mut font_bytes_by_face: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut font_bytes_by_family: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for res in &project.resources {
        if is_font_resource(res)
            && let Ok(bytes) = res.decoded_bytes()
        {
            let entry = metadata.get(&res.id);
            let family = entry
                .map(|entry| entry.family.trim())
                .filter(|family| !family.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| font_family_for_resource(res));
            let style = entry.map_or("Regular", |entry| entry.style.as_str());
            font_bytes_by_face.insert(face_key(&family, style), bytes.clone());
            font_bytes_by_family
                .entry(family.to_ascii_lowercase())
                .or_insert_with(|| bytes.clone());
            font_bytes_by_family.insert(res.id.to_ascii_lowercase(), bytes);
        }
    }
    let mut converted = 0usize;
    let mut diagnostics = Vec::new();
    for symbol in &mut project.symbols {
        let sym_name = symbol.id.clone();
        for (layer_idx, layer) in symbol.timeline.layers.iter_mut().enumerate() {
            for keyframe in &mut layer.keyframes {
                let frame_no = keyframe.frame;
                for element in &mut keyframe.elements {
                    let location = format!(
                        "{sym_name}/layer[{layer_idx}]/frame[{frame_no}]/element[{}]",
                        element.id
                    );
                    let replacement = match &element.kind {
                        ElementKind::Text {
                            text,
                            font_family,
                            font_size,
                            color,
                        } => {
                            let bytes = font_bytes_by_face
                                .get(&face_key(font_family, "Regular"))
                                .or_else(|| {
                                    font_bytes_by_family.get(&font_family.to_ascii_lowercase())
                                })
                                .map(Vec::as_slice);
                            match extract_text_outline_commands_with_diagnostics(
                                &location,
                                text,
                                font_family,
                                *font_size,
                                bytes,
                            ) {
                                Ok(commands) => Some(ElementKind::Path {
                                    commands,
                                    fill_rule: FillRule::NonZero,
                                    fill: *color,
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                }),
                                Err(diags) => {
                                    diagnostics.extend(diags);
                                    None
                                }
                            }
                        }
                        ElementKind::TextBox { .. } if element.name.trim().is_empty() => {
                            if let Some(field) = TextFieldState::from_kind(&element.kind) {
                                let bytes = font_bytes_by_face
                                    .get(&face_key(
                                        &field.format.font,
                                        requested_style(field.format.bold, field.format.italic),
                                    ))
                                    .or_else(|| {
                                        font_bytes_by_family
                                            .get(&field.format.font.to_ascii_lowercase())
                                    })
                                    .map(Vec::as_slice);
                                match extract_text_field_outline_commands_with_diagnostics(
                                    &location, &field, bytes,
                                ) {
                                    Ok(commands) => {
                                        let rgb = field.format.color;
                                        let fill = [
                                            ((rgb >> 16) & 0xFF) as u8,
                                            ((rgb >> 8) & 0xFF) as u8,
                                            (rgb & 0xFF) as u8,
                                            field.alpha,
                                        ];
                                        Some(ElementKind::Path {
                                            commands,
                                            fill_rule: FillRule::NonZero,
                                            fill,
                                            stroke: [0, 0, 0, 0],
                                            stroke_width: 0.0,
                                        })
                                    }
                                    Err(diags) => {
                                        diagnostics.extend(diags);
                                        None
                                    }
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some(next_kind) = replacement {
                        element.kind = next_kind;
                        converted += 1;
                    }
                }
            }
        }
    }
    (converted, diagnostics)
}

pub fn convert_project_text_to_outlines(project: &mut Project) -> usize {
    convert_project_text_to_outlines_with_diagnostics(project).0
}

fn sfnt_checksum(bytes: &[u8]) -> u32 {
    let mut sum = 0u32;
    let (chunks, rem) = bytes.as_chunks::<4>();
    for chunk in chunks {
        sum = sum.wrapping_add(u32::from_be_bytes(*chunk));
    }
    if !rem.is_empty() {
        let mut tail = [0u8; 4];
        tail[..rem.len()].copy_from_slice(rem);
        sum = sum.wrapping_add(u32::from_be_bytes(tail));
    }
    sum
}

/// Deterministically subsets a TrueType SFNT font (`0x00010000` or `true`), retaining `.notdef`,
/// whitespace, all requested `codepoints`, and any transitive composite glyph components while
/// pruning unused glyph outlines from `glyf`/`loca`. Original OpenType tables are preserved except
/// for `DSIG` (invalidated by any rewrite) and a compact glyph-name-free `post` table. If a table
/// can introduce glyphs that are not directly reachable from Unicode (for example `GSUB`, color
/// layers, SVG glyphs, or mathematical variants), all glyph outlines are conservatively retained;
/// this keeps layout behavior valid without claiming that this routine performs full shaping-aware
/// subsetting.
/// Returns `None` if `raw_bytes` is not a TrueType-outline SFNT or if subsetting does not shrink
/// the binary.
pub fn subset_ttf_font_bytes(raw_bytes: &[u8], codepoints: &BTreeSet<u32>) -> Option<Vec<u8>> {
    if raw_bytes.len() < 12 {
        return None;
    }
    let sfnt_version = u32::from_be_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]]);
    if sfnt_version != 0x0001_0000 && sfnt_version != 0x7472_7565 {
        return None;
    }
    let num_tables = u16::from_be_bytes([raw_bytes[4], raw_bytes[5]]) as usize;
    let dir_end = 12usize.checked_add(num_tables.checked_mul(16)?)?;
    if raw_bytes.len() < dir_end {
        return None;
    }

    let mut tables = BTreeMap::<[u8; 4], &[u8]>::new();
    for i in 0..num_tables {
        let rec = &raw_bytes[12 + i * 16..12 + (i + 1) * 16];
        let tag = [rec[0], rec[1], rec[2], rec[3]];
        let offset = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as usize;
        let length = u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as usize;
        let end = offset.checked_add(length)?;
        if end > raw_bytes.len() {
            return None;
        }
        tables.insert(tag, &raw_bytes[offset..end]);
    }

    let head = *tables.get(b"head")?;
    let maxp = *tables.get(b"maxp")?;
    let loca = *tables.get(b"loca")?;
    let glyf = *tables.get(b"glyf")?;
    if head.len() < 54
        || maxp.len() < 6
        || !tables.contains_key(b"cmap")
        || !tables.contains_key(b"hhea")
        || !tables.contains_key(b"hmtx")
    {
        return None;
    }

    let num_glyphs = u16::from_be_bytes([maxp[4], maxp[5]]) as usize;
    if num_glyphs == 0 {
        return None;
    }

    let font = FontRef::try_from_slice(raw_bytes).ok()?;
    let mut retained = BTreeSet::<u16>::new();
    retained.insert(0);
    if num_glyphs > 1 {
        retained.insert(1);
    }
    for cp in codepoints.iter().copied().chain([9u32, 10, 13, 32]) {
        if let Some(ch) = char::from_u32(cp) {
            let gid = font.glyph_id(ch).0;
            if (gid as usize) < num_glyphs {
                retained.insert(gid);
            }
        }
    }

    // These tables can select or compose glyphs that are not directly named by cmap. Parsing every
    // OpenType/AAT lookup format here would be a separate shaping engine, so keep every outline when
    // one is present. We still rewrite a valid deterministic SFNT and compact the `post` table.
    const INDIRECT_GLYPH_TABLES: [&[u8; 4]; 10] = [
        b"GSUB", b"morx", b"mort", b"COLR", b"SVG ", b"sbix", b"CBDT", b"EBDT", b"bdat", b"MATH",
    ];
    if INDIRECT_GLYPH_TABLES
        .iter()
        .any(|tag| tables.contains_key(*tag))
    {
        retained.extend((0..num_glyphs).map(|glyph| glyph as u16));
    }

    let index_to_loc_format = i16::from_be_bytes([head[50], head[51]]);
    let mut orig_offsets = Vec::with_capacity(num_glyphs + 1);
    match index_to_loc_format {
        0 => {
            if loca.len() < (num_glyphs + 1) * 2 {
                return None;
            }
            for i in 0..=num_glyphs {
                let off = (u16::from_be_bytes([loca[i * 2], loca[i * 2 + 1]]) as usize) * 2;
                orig_offsets.push(off);
            }
        }
        1 => {
            if loca.len() < (num_glyphs + 1) * 4 {
                return None;
            }
            for i in 0..=num_glyphs {
                let off = u32::from_be_bytes([
                    loca[i * 4],
                    loca[i * 4 + 1],
                    loca[i * 4 + 2],
                    loca[i * 4 + 3],
                ]) as usize;
                orig_offsets.push(off);
            }
        }
        _ => return None,
    }
    if !orig_offsets.windows(2).all(|w| w[0] <= w[1]) || *orig_offsets.last()? > glyf.len() {
        return None;
    }

    // Transitively retain component glyphs referenced by composite glyphs.
    let mut queue: Vec<u16> = retained.iter().copied().collect();
    while let Some(gid) = queue.pop() {
        let start = orig_offsets[gid as usize];
        let end = orig_offsets[gid as usize + 1];
        if end <= start || end - start < 10 {
            continue;
        }
        let glyph_data = &glyf[start..end];
        let num_contours = i16::from_be_bytes([glyph_data[0], glyph_data[1]]);
        if num_contours < 0 {
            let mut pos = 10usize;
            loop {
                if pos + 4 > glyph_data.len() {
                    return None;
                }
                let flags = u16::from_be_bytes([glyph_data[pos], glyph_data[pos + 1]]);
                let comp_gid = u16::from_be_bytes([glyph_data[pos + 2], glyph_data[pos + 3]]);
                if (comp_gid as usize) < num_glyphs && retained.insert(comp_gid) {
                    queue.push(comp_gid);
                }
                pos += 4;
                pos += if (flags & 0x0001) != 0 { 4 } else { 2 };
                if (flags & 0x0008) != 0 {
                    pos += 2;
                } else if (flags & 0x0040) != 0 {
                    pos += 4;
                } else if (flags & 0x0080) != 0 {
                    pos += 8;
                }
                if pos > glyph_data.len() {
                    return None;
                }
                if (flags & 0x0020) == 0 {
                    break;
                }
            }
        }
    }

    let mut new_glyf = Vec::<u8>::new();
    let mut new_offsets = Vec::<u32>::with_capacity(num_glyphs + 1);
    for gid in 0..num_glyphs {
        new_offsets.push(new_glyf.len() as u32);
        if retained.contains(&(gid as u16)) {
            let start = orig_offsets[gid];
            let end = orig_offsets[gid + 1];
            if end > start {
                new_glyf.extend_from_slice(&glyf[start..end]);
                while !new_glyf.len().is_multiple_of(4) {
                    new_glyf.push(0);
                }
            }
        }
    }
    new_offsets.push(new_glyf.len() as u32);
    if new_glyf.is_empty() {
        new_glyf.extend_from_slice(&[0u8; 4]);
    }

    let use_short_loca = *new_offsets.last()? <= (u16::MAX as u32) * 2;
    let new_loca_format: i16 = if use_short_loca { 0 } else { 1 };
    let mut new_loca =
        Vec::<u8>::with_capacity((num_glyphs + 1) * if use_short_loca { 2 } else { 4 });
    for &off in &new_offsets {
        if use_short_loca {
            new_loca.extend_from_slice(&((off / 2) as u16).to_be_bytes());
        } else {
            new_loca.extend_from_slice(&off.to_be_bytes());
        }
    }

    let mut new_head = head.to_vec();
    new_head[8..12].copy_from_slice(&[0, 0, 0, 0]);
    new_head[50..52].copy_from_slice(&new_loca_format.to_be_bytes());

    let new_post = tables.get(b"post").map(|post| {
        if post.len() >= 32 {
            let mut compact = post[..32].to_vec();
            compact[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
            compact
        } else {
            post.to_vec()
        }
    });

    let mut out_tables = Vec::<([u8; 4], Vec<u8>)>::new();
    for (tag, original) in &tables {
        if tag == b"DSIG" {
            continue;
        }
        let data = match tag {
            b"glyf" => new_glyf.clone(),
            b"loca" => new_loca.clone(),
            b"head" => new_head.clone(),
            b"post" => new_post.clone().unwrap_or_else(|| original.to_vec()),
            _ => original.to_vec(),
        };
        out_tables.push((*tag, data));
    }
    out_tables.sort_by_key(|(tag, _)| *tag);

    let table_count = out_tables.len() as u16;
    if table_count == 0 {
        return None;
    }
    let entry_selector = 15u16.saturating_sub(table_count.leading_zeros() as u16);
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = table_count * 16 - search_range;

    let mut out = Vec::<u8>::new();
    out.extend_from_slice(&sfnt_version.to_be_bytes());
    out.extend_from_slice(&table_count.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    let dir_pos = out.len();
    out.resize(dir_pos + (table_count as usize) * 16, 0);

    let mut head_table_offset = None;
    for (idx, (tag, data)) in out_tables.iter().enumerate() {
        let offset = out.len() as u32;
        let length = data.len() as u32;
        let checksum = sfnt_checksum(data);
        if tag == b"head" {
            head_table_offset = Some(offset as usize);
        }
        let rec_pos = dir_pos + idx * 16;
        out[rec_pos..rec_pos + 4].copy_from_slice(tag);
        out[rec_pos + 4..rec_pos + 8].copy_from_slice(&checksum.to_be_bytes());
        out[rec_pos + 8..rec_pos + 12].copy_from_slice(&offset.to_be_bytes());
        out[rec_pos + 12..rec_pos + 16].copy_from_slice(&length.to_be_bytes());

        out.extend_from_slice(data);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }

    let head_off = head_table_offset?;
    let total_sum = sfnt_checksum(&out);
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(total_sum);
    out[head_off + 8..head_off + 12].copy_from_slice(&adjustment.to_be_bytes());

    if out.len() >= raw_bytes.len() {
        return None;
    }
    let subset_font = FontRef::try_from_slice(&out).ok()?;
    for cp in codepoints.iter().copied() {
        if let Some(ch) = char::from_u32(cp)
            && subset_font.glyph_id(ch) != font.glyph_id(ch)
        {
            return None;
        }
    }
    Some(out)
}

pub fn apply_font_policy(project: &mut Project, policy: FontExportPolicy) -> FontBundle {
    let usage = collect_font_usage(project);
    let used_by_family = &usage.by_family;
    let metadata = embedding_metadata(project);
    let mut bundle = FontBundle {
        policy,
        css_rules: String::new(),
        metrics: BTreeMap::new(),
        subset_stats: Vec::new(),
        outlined_elements: 0,
        outline_diagnostics: Vec::new(),
    };

    for family in used_by_family.keys() {
        if let Ok(json_str) = TextFormat::portable_font_metrics_json(family, None)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str)
        {
            bundle.metrics.insert(family.clone(), val);
        }
    }

    if policy == FontExportPolicy::Outlines {
        let (outlined, diags) = convert_project_text_to_outlines_with_diagnostics(project);
        bundle.outlined_elements = outlined;
        bundle.outline_diagnostics = diags;
        return bundle;
    }

    if policy == FontExportPolicy::Device {
        return bundle;
    }

    for res in &mut project.resources {
        if !is_font_resource(res) {
            continue;
        }
        let face = metadata.get(&res.id);
        let family = face
            .map(|entry| entry.family.trim())
            .filter(|family| !family.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| font_family_for_resource(res));
        let Ok(raw_bytes) = res.decoded_bytes() else {
            continue;
        };
        let original_bytes = raw_bytes.len();
        let style = face
            .map(|entry| entry.style.trim())
            .filter(|style| !style.is_empty())
            .unwrap_or("Regular");
        let active_bytes = if policy == FontExportPolicy::Subset {
            let key = face_key(&family, style);
            let explicit_character_set = face.is_some_and(metadata_has_explicit_character_set);
            let runtime_mutable = usage.runtime_mutable_faces.contains(&key)
                || (face.is_none()
                    && (usage
                        .runtime_mutable_families
                        .contains(&family.trim().to_ascii_lowercase())
                        || usage
                            .runtime_mutable_families
                            .contains(&res.id.trim().to_ascii_lowercase())));

            // Metadata declares the allowed future repertoire, while current authored text must
            // always remain renderable even if it falls outside the selected range checkboxes.
            let mut codepoints = face.map(metadata_codepoints).unwrap_or_default();
            let authored_usage = if face.is_some() {
                usage.by_face.get(&key)
            } else {
                used_by_family
                    .get(&family)
                    .or_else(|| used_by_family.get(&res.id))
                    .or_else(|| {
                        used_by_family.iter().find_map(|(candidate, codepoints)| {
                            (candidate.eq_ignore_ascii_case(&family)
                                || candidate.eq_ignore_ascii_case(&res.id))
                            .then_some(codepoints)
                        })
                    })
            };
            if let Some(authored_usage) = authored_usage {
                codepoints.extend(authored_usage);
            }
            if codepoints.is_empty() {
                codepoints.extend(32u32..=126);
            }

            if runtime_mutable && !explicit_character_set {
                bundle.subset_stats.push(FontSubsetStats {
                    family: family.clone(),
                    original_bytes,
                    subset_bytes: raw_bytes.len(),
                    codepoint_count: codepoints.len(),
                    subset_applied: false,
                    diagnostic: Some(
                        "Complete font preserved: runtime-editable Dynamic/Input text has no explicit embedded-character ranges, so future script or user text cannot be safely predicted."
                            .to_owned(),
                    ),
                });
                raw_bytes
            } else if let Some(subset_bytes) = subset_ttf_font_bytes(&raw_bytes, &codepoints) {
                res.data_base64 = STANDARD.encode(&subset_bytes).into();
                bundle.subset_stats.push(FontSubsetStats {
                    family: family.clone(),
                    original_bytes,
                    subset_bytes: subset_bytes.len(),
                    codepoint_count: codepoints.len(),
                    subset_applied: true,
                    diagnostic: Some(if runtime_mutable {
                        "Runtime-editable Dynamic/Input text is limited to the explicitly authored embedded-character ranges plus current text. OpenType layout tables are preserved; this does not claim complete shaping-aware subsetting."
                            .to_owned()
                    } else {
                        "OpenType layout tables and composite dependencies are preserved; fonts with indirect glyph-selection tables conservatively retain every outline. This does not claim complete shaping-aware subsetting."
                            .to_owned()
                    }),
                });
                subset_bytes
            } else {
                bundle.subset_stats.push(FontSubsetStats {
                    family: family.clone(),
                    original_bytes,
                    subset_bytes: raw_bytes.len(),
                    codepoint_count: codepoints.len(),
                    subset_applied: false,
                    diagnostic: Some(
                        "Complete font preserved because it could not be rewritten as a smaller validated TrueType SFNT without risking glyph or layout loss."
                            .to_owned(),
                    ),
                });
                raw_bytes
            }
        } else {
            raw_bytes
        };

        if let Ok(json_str) = TextFormat::portable_font_metrics_json(&family, Some(&active_bytes))
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str)
        {
            bundle.metrics.insert(family.clone(), val);
        }

        let css_family = family.replace('\\', "\\\\").replace('"', "\\\"");
        let style_lower = style.to_ascii_lowercase();
        let css_weight = if style_lower.contains("semi bold") || style_lower.contains("semibold") {
            600
        } else if style_lower.contains("bold") {
            700
        } else {
            400
        };
        let css_style = if style_lower.contains("italic") {
            "italic"
        } else {
            "normal"
        };
        let mime = if res.media_type.trim().is_empty() {
            "font/ttf"
        } else {
            res.media_type.trim()
        };
        bundle.css_rules.push_str(&format!(
            "@font-face {{ font-family: \"{}\"; font-style: {}; font-weight: {}; src: url('data:{};base64,{}'); font-display: block; }}\n",
            css_family, css_style, css_weight, mime, res.data_base64
        ));
    }

    bundle
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{AuthoredTextFieldOptions, Element, TextFormatRun};

    const TEST_FONT: &[u8] = include_bytes!("../../neoflash-ui/assets/fonts/Inter-Regular.ttf");

    fn add_font(project: &mut Project, id: &str, file_name: &str) {
        project.resources.push(
            ResourceUnit::from_bytes(id, format!("fonts/{file_name}"), "font/ttf", TEST_FONT)
                .expect("test font resource"),
        );
    }

    fn add_text_box(project: &mut Project, id: &str, name: &str, text: &str, format: TextFormat) {
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: id.to_owned(),
                name: name.to_owned(),
                kind: ElementKind::TextBox {
                    text: text.to_owned(),
                    width: 240.0,
                    height: 40.0,
                    format,
                    word_wrap: false,
                    multiline: false,
                },
                ..Element::default()
            });
    }

    fn sfnt_table_tags(bytes: &[u8]) -> BTreeSet<[u8; 4]> {
        let count = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        (0..count)
            .map(|index| {
                let start = 12 + index * 16;
                bytes[start..start + 4]
                    .try_into()
                    .expect("complete SFNT table tag")
            })
            .collect()
    }

    #[test]
    fn subset_collects_every_rich_text_face_and_preserves_layout_tables() {
        let mut project = Project::blank();
        add_font(&mut project, "font-alpha", "Alpha.ttf");
        add_font(&mut project, "font-beta", "Beta-Bold.ttf");
        let metadata = serde_json::json!([
            {
                "id": "font-alpha",
                "family": "Alpha",
                "style": "Regular",
                "ranges": [],
                "extra_chars": ""
            },
            {
                "id": "font-beta",
                "family": "Beta",
                "style": "Bold",
                "ranges": [],
                "extra_chars": ""
            }
        ]);
        project.resources.push(
            ResourceUnit::from_bytes(
                "font-embedding-metadata",
                FONT_EMBEDDING_METADATA_PATH,
                "application/json",
                &serde_json::to_vec(&metadata).expect("embedding metadata"),
            )
            .expect("metadata resource"),
        );

        let alpha = TextFormat {
            font: "Alpha".to_owned(),
            ..TextFormat::default()
        };
        let beta_bold = TextFormat {
            font: "Beta".to_owned(),
            bold: true,
            ..TextFormat::default()
        };
        let mut field_format = alpha.clone();
        field_format.set_field_options(Some(AuthoredTextFieldOptions {
            selectable: false,
            format_runs: vec![
                TextFormatRun {
                    start: 0,
                    end: 1,
                    format: alpha,
                },
                TextFormatRun {
                    start: 1,
                    end: 2,
                    format: beta_bold,
                },
            ],
            ..AuthoredTextFieldOptions::default()
        }));
        add_text_box(&mut project, "mixed", "", "AΩ", field_format);

        let usage = collect_font_usage(&project);
        assert_eq!(
            usage.by_face.get(&face_key("Alpha", "Regular")),
            Some(&BTreeSet::from([32, 'A' as u32]))
        );
        assert_eq!(
            usage.by_face.get(&face_key("Beta", "Bold")),
            Some(&BTreeSet::from([32, 'Ω' as u32]))
        );

        let mut published = project.clone();
        let bundle = apply_font_policy(&mut published, FontExportPolicy::Subset);
        assert_eq!(bundle.subset_stats.len(), 2);
        for stats in &bundle.subset_stats {
            assert!(stats.subset_applied, "{stats:?}");
            assert_eq!(stats.codepoint_count, 2, "{stats:?}");
            assert!(
                stats
                    .diagnostic
                    .as_deref()
                    .is_some_and(|message| message.contains("does not claim complete shaping")),
                "{stats:?}"
            );
        }

        let original_tags = sfnt_table_tags(TEST_FONT);
        for resource in published
            .resources
            .iter()
            .filter(|resource| is_font_resource(resource))
        {
            let subset = resource.decoded_bytes().expect("subset font bytes");
            FontRef::try_from_slice(&subset).expect("valid rewritten font");
            let subset_tags = sfnt_table_tags(&subset);
            for tag in [*b"GDEF", *b"GPOS", *b"GSUB", *b"kern"] {
                if original_tags.contains(&tag) {
                    assert!(subset_tags.contains(&tag), "lost table {tag:?}");
                }
            }
        }
    }

    #[test]
    fn subset_preserves_complete_font_for_dynamic_and_input_without_declared_ranges() {
        for (label, options) in [
            ("dynamic", None),
            (
                "input",
                Some(AuthoredTextFieldOptions {
                    field_type: TextFieldType::Input,
                    ..AuthoredTextFieldOptions::default()
                }),
            ),
        ] {
            let mut project = Project::blank();
            add_font(&mut project, "runtime-font", "Runtime.ttf");
            let mut format = TextFormat {
                font: "Runtime".to_owned(),
                ..TextFormat::default()
            };
            if let Some(options) = options {
                format.set_field_options(Some(options));
            }
            add_text_box(&mut project, label, label, "A", format);

            let before = project.resources[0]
                .decoded_bytes()
                .expect("complete font bytes");
            let bundle = apply_font_policy(&mut project, FontExportPolicy::Subset);
            let stats = bundle.subset_stats.first().expect("font subset stats");
            assert!(!stats.subset_applied, "{label}: {stats:?}");
            assert_eq!(stats.original_bytes, stats.subset_bytes, "{label}");
            assert_eq!(
                project.resources[0]
                    .decoded_bytes()
                    .expect("preserved font bytes"),
                before,
                "{label} field must retain future glyphs"
            );
            assert!(
                stats.diagnostic.as_deref().is_some_and(|message| {
                    message.contains("Dynamic/Input") && message.contains("future")
                }),
                "{label}: {stats:?}"
            );
        }
    }

    #[test]
    fn outlines_policy_extracts_ab_glyph_curves_and_retains_fallback_on_bitmap_or_missing_glyphs() {
        let mut project = Project::blank();
        add_font(&mut project, "inter-regular", "Inter.ttf");
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .clear();
        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "valid-heading".to_owned(),
                name: String::new(),
                kind: ElementKind::Text {
                    text: "Hi".to_owned(),
                    font_family: "Inter".to_owned(),
                    font_size: 24.0,
                    color: [20, 30, 40, 255],
                },
                ..Element::default()
            });
        // U+1F680 ROCKET is not covered by Inter-Regular.ttf
        add_text_box(
            &mut project,
            "missing-glyph-box",
            "",
            "Hi \u{1F680}",
            TextFormat {
                font: "Inter".to_owned(),
                size: 20.0,
                ..TextFormat::default()
            },
        );

        let bundle = apply_font_policy(&mut project, FontExportPolicy::Outlines);
        assert_eq!(bundle.outlined_elements, 1);
        assert_eq!(bundle.outline_diagnostics.len(), 1);
        assert!(bundle.outline_diagnostics[0].contains("missing-glyph-box"));
        assert!(bundle.outline_diagnostics[0].contains("U+1F680"));

        let elements = &project.symbols[0].timeline.layers[0].keyframes[0].elements;
        assert!(
            matches!(elements[0].kind, ElementKind::Path { ref commands, .. } if !commands.is_empty()),
            "supported text must convert to real ab_glyph Path commands"
        );
        assert!(
            matches!(elements[1].kind, ElementKind::TextBox { ref text, .. } if text == "Hi \u{1F680}"),
            "element with missing glyph coverage must retain visible text fallback"
        );
    }
}

#[path = "html.rs"]
pub mod html;
pub use html::*;
