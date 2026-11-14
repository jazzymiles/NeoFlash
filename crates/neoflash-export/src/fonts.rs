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
    neoflash_core::is_font_resource_unit(resource)
}

pub fn font_family_for_resource(resource: &ResourceUnit) -> String {
    neoflash_core::font_family_for_resource_unit(resource)
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

fn collect_text_box_usage(usage: &mut FontUsage, field: &TextFieldState, has_instance_name: bool) {
    let total_units = field.text.encode_utf16().count() as u32;
    let runs = field.materialized_runs(total_units);
    let runtime_mutable = has_instance_name
        || field.field_type == TextFieldType::Input
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
                                collect_text_box_usage(
                                    &mut usage,
                                    &field,
                                    !element.name.trim().is_empty(),
                                );
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
    neoflash_core::resolve_project_font_bytes(project, family, bold, italic)
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

    let sfnt_bytes = neoflash_core::sfnt_view_bytes(bytes);
    let outline_slice = sfnt_bytes.as_deref().unwrap_or(bytes);
    let table_diags =
        TextFormat::validate_text_outlines_for_font(location, font_family, text, outline_slice);
    if !table_diags.is_empty() {
        return Err(table_diags);
    }

    if let Ok(font) = FontRef::try_from_slice(outline_slice) {
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
    TextFormat::extract_sfnt_outline_commands(font_family, text, font_size, outline_slice).map_err(
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
    let sfnt_bytes = neoflash_core::sfnt_view_bytes(bytes);
    let outline_slice = sfnt_bytes.as_deref().unwrap_or(bytes);
    let table_diags = TextFormat::validate_text_outlines_for_font(
        location,
        &field.format.font,
        &field.text,
        outline_slice,
    );
    if !table_diags.is_empty() {
        return Err(table_diags);
    }
    extract_text_outline_commands_with_diagnostics(
        location,
        &field.text,
        &field.format.font,
        field.format.size,
        Some(outline_slice),
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
    neoflash_core::sfnt_checksum(bytes)
}

fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn be_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(i16::from_be_bytes([slice[0], slice[1]]))
}

fn be_u24(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 3)?;
    Some(((slice[0] as u32) << 16) | ((slice[1] as u32) << 8) | (slice[2] as u32))
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn parse_coverage_glyphs(table: &[u8]) -> Option<Vec<u16>> {
    const MAX_GLYPHS: usize = 65_536;
    let format = be_u16(table, 0)?;
    match format {
        1 => {
            let count = be_u16(table, 2)? as usize;
            if 4 + count * 2 > table.len() || count > MAX_GLYPHS {
                return None;
            }
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                out.push(be_u16(table, 4 + i * 2)?);
            }
            Some(out)
        }
        2 => {
            let range_count = be_u16(table, 2)? as usize;
            if 4 + range_count * 6 > table.len() {
                return None;
            }
            let mut out = Vec::<u16>::new();
            let mut sparse = BTreeMap::<usize, u16>::new();
            let mut used_sparse = false;
            for i in 0..range_count {
                let rec_off = 4 + i * 6;
                let start = be_u16(table, rec_off)?;
                let end = be_u16(table, rec_off + 2)?;
                let start_cov = be_u16(table, rec_off + 4)? as usize;
                if end < start {
                    return None;
                }
                let span = (end - start) as usize + 1;
                if start_cov + span > MAX_GLYPHS {
                    return None;
                }
                if !used_sparse && start_cov == out.len() {
                    for g in start..=end {
                        out.push(g);
                    }
                } else {
                    if !used_sparse {
                        for (idx, g) in out.drain(..).enumerate() {
                            sparse.insert(idx, g);
                        }
                        used_sparse = true;
                    }
                    for (offset, g) in (start..=end).enumerate() {
                        sparse.insert(start_cov + offset, g);
                    }
                }
            }
            if !used_sparse {
                return Some(out);
            }
            let len = sparse.keys().next_back().map_or(0, |k| k + 1);
            if len > MAX_GLYPHS {
                return None;
            }
            let mut merged = vec![0u16; len];
            for (idx, gid) in sparse {
                merged[idx] = gid;
            }
            Some(merged)
        }
        _ => None,
    }
}

fn validate_class_def(class_def: &[u8]) -> Option<()> {
    match be_u16(class_def, 0)? {
        1 => {
            let count = be_u16(class_def, 4)? as usize;
            if 6 + count * 2 > class_def.len() {
                return None;
            }
            Some(())
        }
        2 => {
            let count = be_u16(class_def, 2)? as usize;
            if 4 + count * 6 > class_def.len() {
                return None;
            }
            for i in 0..count {
                let start = be_u16(class_def, 4 + i * 6)?;
                let end = be_u16(class_def, 4 + i * 6 + 2)?;
                if end < start {
                    return None;
                }
            }
            Some(())
        }
        _ => None,
    }
}

fn has_format14_uvs_match(cmap: &[u8], codepoints: &BTreeSet<u32>) -> Result<bool, String> {
    if cmap.len() < 4 {
        return Err("cmap table is truncated".to_owned());
    }
    let num_tables = be_u16(cmap, 2).ok_or_else(|| "cmap header truncated".to_owned())? as usize;
    let records_end = 4usize
        .checked_add(num_tables * 8)
        .filter(|&end| end <= cmap.len())
        .ok_or_else(|| "cmap encoding records exceed table bounds".to_owned())?;
    let _ = records_end;

    for i in 0..num_tables {
        let off = be_u32(cmap, 4 + i * 8 + 4)
            .ok_or_else(|| "cmap subtable offset out of bounds".to_owned())?
            as usize;
        let Some(subtable) = cmap.get(off..) else {
            continue;
        };
        let Some(format) = be_u16(subtable, 0) else {
            continue;
        };
        if format != 14 {
            continue;
        }
        let num_records = be_u32(subtable, 6)
            .ok_or_else(|| "cmap format 14 header truncated".to_owned())?
            as usize;
        if 10usize
            .checked_add(num_records.saturating_mul(11))
            .is_none_or(|end| end > subtable.len())
        {
            return Err("cmap format 14 records exceed table bounds".to_owned());
        }
        for r in 0..num_records {
            let rec_off = 10 + r * 11;
            let var_selector = be_u24(subtable, rec_off)
                .ok_or_else(|| "cmap format 14 varSelector truncated".to_owned())?;
            if codepoints.contains(&var_selector) {
                return Ok(true);
            }
            let default_off = be_u32(subtable, rec_off + 3).unwrap_or(0) as usize;
            let non_default_off = be_u32(subtable, rec_off + 7).unwrap_or(0) as usize;
            if default_off != 0 {
                let d_slice = subtable
                    .get(default_off..)
                    .ok_or_else(|| "cmap format 14 defaultUVS out of bounds".to_owned())?;
                let count = be_u32(d_slice, 0)
                    .ok_or_else(|| "cmap format 14 defaultUVS truncated".to_owned())?
                    as usize;
                if 4usize
                    .checked_add(count.saturating_mul(4))
                    .is_none_or(|end| end > d_slice.len())
                {
                    return Err("cmap format 14 defaultUVS exceeds bounds".to_owned());
                }
                for u in 0..count {
                    let start = be_u24(d_slice, 4 + u * 4).unwrap_or(0);
                    let add = d_slice[4 + u * 4 + 3] as u32;
                    let end = start.saturating_add(add);
                    if codepoints.range(start..=end).next().is_some() {
                        return Ok(true);
                    }
                }
            }
            if non_default_off != 0 {
                let nd_slice = subtable
                    .get(non_default_off..)
                    .ok_or_else(|| "cmap format 14 nonDefaultUVS out of bounds".to_owned())?;
                let count = be_u32(nd_slice, 0)
                    .ok_or_else(|| "cmap format 14 nonDefaultUVS truncated".to_owned())?
                    as usize;
                if 4usize
                    .checked_add(count.saturating_mul(5))
                    .is_none_or(|end| end > nd_slice.len())
                {
                    return Err("cmap format 14 nonDefaultUVS exceeds bounds".to_owned());
                }
                for u in 0..count {
                    let cp = be_u24(nd_slice, 4 + u * 5).unwrap_or(0);
                    if codepoints.contains(&cp) {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}

fn build_compact_cmap(retained_cmap: &BTreeMap<u32, u16>) -> Option<Vec<u8>> {
    // Build Format 4 (BMP < 0xFFFF) using idDelta segments so glyph IDs remain unchanged.
    let mut segments: Vec<(u16, u16, u16)> = Vec::new();
    for (&cp_u32, &gid) in retained_cmap {
        if cp_u32 >= 0xFFFF {
            continue;
        }
        let cp = cp_u32 as u16;
        let delta = gid.wrapping_sub(cp);
        if let Some((_start, end, prev_delta)) = segments.last_mut()
            && cp == *end + 1
            && delta == *prev_delta
        {
            *end = cp;
        } else {
            segments.push((cp, cp, delta));
        }
    }
    segments.push((0xFFFF, 0xFFFF, 1u16));

    let seg_count = segments.len();
    let f4_len = 16usize.checked_add(seg_count.checked_mul(8)?)?;
    if f4_len > u16::MAX as usize || seg_count == 0 {
        return None;
    }
    let seg_count_u16 = seg_count as u16;
    let seg_count_x2 = seg_count_u16 * 2;
    let entry_selector = 15u16.saturating_sub(seg_count_u16.leading_zeros() as u16);
    let search_range = 2u16 * (1u16 << entry_selector);
    let range_shift = seg_count_x2 - search_range;

    let mut f4 = Vec::with_capacity(f4_len);
    f4.extend_from_slice(&4u16.to_be_bytes());
    f4.extend_from_slice(&(f4_len as u16).to_be_bytes());
    f4.extend_from_slice(&0u16.to_be_bytes()); // language
    f4.extend_from_slice(&seg_count_x2.to_be_bytes());
    f4.extend_from_slice(&search_range.to_be_bytes());
    f4.extend_from_slice(&entry_selector.to_be_bytes());
    f4.extend_from_slice(&range_shift.to_be_bytes());
    for &(_start, end, _delta) in &segments {
        f4.extend_from_slice(&end.to_be_bytes());
    }
    f4.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    for &(start, _end, _delta) in &segments {
        f4.extend_from_slice(&start.to_be_bytes());
    }
    for &(_start, _end, delta) in &segments {
        f4.extend_from_slice(&delta.to_be_bytes());
    }
    for _ in &segments {
        f4.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
    }
    while !f4.len().is_multiple_of(4) {
        f4.push(0);
    }

    let has_non_bmp = retained_cmap.keys().any(|&cp| cp >= 0xFFFF);
    let f12 = if has_non_bmp {
        let mut groups: Vec<(u32, u32, u32)> = Vec::new();
        for (&cp, &gid_u16) in retained_cmap {
            let gid = gid_u16 as u32;
            if let Some((start_cp, end_cp, start_gid)) = groups.last_mut()
                && cp == *end_cp + 1
                && gid == *start_gid + (*end_cp - *start_cp) + 1
            {
                *end_cp = cp;
            } else {
                groups.push((cp, cp, gid));
            }
        }
        let f12_len = 16usize.checked_add(groups.len().checked_mul(12)?)?;
        let mut buf = Vec::with_capacity(f12_len);
        buf.extend_from_slice(&12u16.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
        buf.extend_from_slice(&(f12_len as u32).to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes()); // language
        buf.extend_from_slice(&(groups.len() as u32).to_be_bytes());
        for (start_cp, end_cp, start_gid) in groups {
            buf.extend_from_slice(&start_cp.to_be_bytes());
            buf.extend_from_slice(&end_cp.to_be_bytes());
            buf.extend_from_slice(&start_gid.to_be_bytes());
        }
        Some(buf)
    } else {
        None
    };

    let mut records: Vec<(u16, u16, u32)> = if f12.is_some() {
        vec![(0, 3, 0), (0, 4, 1), (3, 1, 0), (3, 10, 1)]
    } else {
        vec![(0, 3, 0), (3, 1, 0)]
    };
    let header_len = 4 + records.len() * 8;
    let f4_off = header_len as u32;
    let f12_off = (header_len + f4.len()) as u32;
    for rec in &mut records {
        rec.2 = if rec.2 == 0 { f4_off } else { f12_off };
    }

    let mut cmap = Vec::with_capacity(header_len + f4.len() + f12.as_ref().map_or(0, Vec::len));
    cmap.extend_from_slice(&0u16.to_be_bytes()); // version
    cmap.extend_from_slice(&(records.len() as u16).to_be_bytes());
    for (platform_id, encoding_id, offset) in records {
        cmap.extend_from_slice(&platform_id.to_be_bytes());
        cmap.extend_from_slice(&encoding_id.to_be_bytes());
        cmap.extend_from_slice(&offset.to_be_bytes());
    }
    cmap.extend_from_slice(&f4);
    if let Some(f12_bytes) = f12 {
        cmap.extend_from_slice(&f12_bytes);
    }
    Some(cmap)
}

fn detect_complex_gsub_script(gsub: &[u8], codepoints: &BTreeSet<u32>) -> Option<&'static str> {
    let script_list_off = be_u16(gsub, 4)? as usize;
    let script_list = gsub.get(script_list_off..)?;
    let script_count = be_u16(script_list, 0)? as usize;
    if 2 + script_count * 6 > script_list.len() {
        return None;
    }
    let mut tags = BTreeSet::<[u8; 4]>::new();
    for i in 0..script_count {
        let off = 2 + i * 6;
        tags.insert([
            script_list[off],
            script_list[off + 1],
            script_list[off + 2],
            script_list[off + 3],
        ]);
    }
    let has_cp_in = |ranges: &[(u32, u32)]| -> bool {
        ranges
            .iter()
            .any(|&(lo, hi)| codepoints.range(lo..=hi).next().is_some())
    };
    type ScriptCheckRule = (&'static [[u8; 4]], &'static [(u32, u32)], &'static str);
    const RULES: &[ScriptCheckRule] = &[
        (
            &[*b"arab"],
            &[
                (0x0600, 0x06FF),
                (0x0750, 0x077F),
                (0x08A0, 0x08FF),
                (0xFB50, 0xFDFF),
                (0xFE70, 0xFEFF),
            ],
            "Arabic",
        ),
        (&[*b"hebr"], &[(0x0590, 0x05FF), (0xFB1D, 0xFB4F)], "Hebrew"),
        (&[*b"syrc"], &[(0x0700, 0x074F)], "Syriac"),
        (&[*b"thaa"], &[(0x0780, 0x07BF)], "Thaana"),
        (&[*b"nko "], &[(0x07C0, 0x07FF)], "N'Ko"),
        (&[*b"deva", *b"dev2"], &[(0x0900, 0x097F)], "Devanagari"),
        (&[*b"beng", *b"bng2"], &[(0x0980, 0x09FF)], "Bengali"),
        (&[*b"guru", *b"gur2"], &[(0x0A00, 0x0A7F)], "Gurmukhi"),
        (&[*b"gujr", *b"gjr2"], &[(0x0A80, 0x0AFF)], "Gujarati"),
        (&[*b"orya", *b"ory2"], &[(0x0B00, 0x0B7F)], "Oriya"),
        (&[*b"taml", *b"tml2"], &[(0x0B80, 0x0BFF)], "Tamil"),
        (&[*b"telu", *b"tel2"], &[(0x0C00, 0x0C7F)], "Telugu"),
        (&[*b"knda", *b"knd2"], &[(0x0C80, 0x0CFF)], "Kannada"),
        (&[*b"mlym", *b"mlm2"], &[(0x0D00, 0x0D7F)], "Malayalam"),
        (&[*b"sinh"], &[(0x0D80, 0x0DFF)], "Sinhala"),
        (&[*b"thai"], &[(0x0E00, 0x0E7F)], "Thai"),
        (&[*b"laoo"], &[(0x0E80, 0x0EFF)], "Lao"),
        (&[*b"tibt"], &[(0x0F00, 0x0FFF)], "Tibetan"),
        (&[*b"mymr", *b"mym2"], &[(0x1000, 0x109F)], "Myanmar"),
        (&[*b"khmr"], &[(0x1780, 0x17FF)], "Khmer"),
        (&[*b"mong"], &[(0x1800, 0x18AF)], "Mongolian"),
        (
            &[*b"hang", *b"jamo"],
            &[(0x1100, 0x11FF), (0xA960, 0xA97F), (0xD7B0, 0xD7FF)],
            "Hangul Jamo",
        ),
    ];
    for &(script_tags, ranges, label) in RULES {
        if script_tags.iter().any(|tag| tags.contains(tag)) && has_cp_in(ranges) {
            return Some(label);
        }
    }
    None
}

fn validate_subst_lookup_records(
    subtable: &[u8],
    offset: usize,
    subst_count: usize,
    lookup_count: usize,
) -> Result<(), String> {
    let end = offset
        .checked_add(subst_count.saturating_mul(4))
        .filter(|&e| e <= subtable.len())
        .ok_or_else(|| "GSUB SubstLookupRecord array exceeds bounds".to_owned())?;
    let _ = end;
    for i in 0..subst_count {
        let lookup_idx = be_u16(subtable, offset + i * 4 + 2)
            .ok_or_else(|| "GSUB SubstLookupRecord truncated".to_owned())?
            as usize;
        if lookup_idx >= lookup_count {
            return Err(format!(
                "GSUB SubstLookupRecord references invalid lookup index {lookup_idx}"
            ));
        }
    }
    Ok(())
}

fn apply_gsub_subtable(
    lookup_type: u16,
    subtable: &[u8],
    num_glyphs: usize,
    lookup_count: usize,
    retained: &mut BTreeSet<u16>,
    allow_extension: bool,
) -> Result<(), String> {
    let insert_gid = |gid: u16, set: &mut BTreeSet<u16>| {
        if (gid as usize) < num_glyphs {
            set.insert(gid);
        }
    };
    let cov_at = |off: usize| -> Result<Vec<u16>, String> {
        let slice = subtable
            .get(off..)
            .ok_or_else(|| "GSUB coverage offset out of bounds".to_owned())?;
        parse_coverage_glyphs(slice).ok_or_else(|| "GSUB invalid coverage subtable".to_owned())
    };
    let format = be_u16(subtable, 0).ok_or_else(|| "GSUB subtable header truncated".to_owned())?;
    match (lookup_type, format) {
        (1, 1) => {
            let cov_off = be_u16(subtable, 2)
                .ok_or_else(|| "GSUB type 1 format 1 truncated".to_owned())?
                as usize;
            let delta = be_i16(subtable, 4)
                .ok_or_else(|| "GSUB type 1 format 1 delta truncated".to_owned())?;
            for gid in cov_at(cov_off)? {
                if retained.contains(&gid) {
                    insert_gid(gid.wrapping_add_signed(delta), retained);
                }
            }
        }
        (1, 2) => {
            let cov_off = be_u16(subtable, 2)
                .ok_or_else(|| "GSUB type 1 format 2 truncated".to_owned())?
                as usize;
            let count = be_u16(subtable, 4)
                .ok_or_else(|| "GSUB type 1 format 2 glyphCount truncated".to_owned())?
                as usize;
            if 6 + count * 2 > subtable.len() {
                return Err("GSUB type 1 format 2 substitutes exceed bounds".to_owned());
            }
            for (idx, gid) in cov_at(cov_off)?.into_iter().enumerate() {
                if idx < count
                    && retained.contains(&gid)
                    && let Some(sub) = be_u16(subtable, 6 + idx * 2)
                {
                    insert_gid(sub, retained);
                }
            }
        }
        (2, 1) => {
            let cov_off =
                be_u16(subtable, 2).ok_or_else(|| "GSUB type 2 truncated".to_owned())? as usize;
            let seq_count = be_u16(subtable, 4)
                .ok_or_else(|| "GSUB type 2 sequenceCount truncated".to_owned())?
                as usize;
            if 6 + seq_count * 2 > subtable.len() {
                return Err("GSUB type 2 sequence offsets exceed bounds".to_owned());
            }
            for (idx, gid) in cov_at(cov_off)?.into_iter().enumerate() {
                if idx >= seq_count || !retained.contains(&gid) {
                    continue;
                }
                let seq_off = be_u16(subtable, 6 + idx * 2).unwrap_or(0) as usize;
                let seq = subtable
                    .get(seq_off..)
                    .ok_or_else(|| "GSUB sequence offset out of bounds".to_owned())?;
                let glyph_count = be_u16(seq, 0)
                    .ok_or_else(|| "GSUB sequence header truncated".to_owned())?
                    as usize;
                if 2 + glyph_count * 2 > seq.len() {
                    return Err("GSUB sequence glyphs exceed bounds".to_owned());
                }
                for g in 0..glyph_count {
                    insert_gid(be_u16(seq, 2 + g * 2).unwrap_or(0), retained);
                }
            }
        }
        (3, 1) => {
            let cov_off =
                be_u16(subtable, 2).ok_or_else(|| "GSUB type 3 truncated".to_owned())? as usize;
            let alt_count = be_u16(subtable, 4)
                .ok_or_else(|| "GSUB type 3 alternateSetCount truncated".to_owned())?
                as usize;
            if 6 + alt_count * 2 > subtable.len() {
                return Err("GSUB type 3 alternateSet offsets exceed bounds".to_owned());
            }
            for (idx, gid) in cov_at(cov_off)?.into_iter().enumerate() {
                if idx >= alt_count || !retained.contains(&gid) {
                    continue;
                }
                let alt_off = be_u16(subtable, 6 + idx * 2).unwrap_or(0) as usize;
                let alt = subtable
                    .get(alt_off..)
                    .ok_or_else(|| "GSUB alternateSet offset out of bounds".to_owned())?;
                let glyph_count = be_u16(alt, 0)
                    .ok_or_else(|| "GSUB alternateSet header truncated".to_owned())?
                    as usize;
                if 2 + glyph_count * 2 > alt.len() {
                    return Err("GSUB alternateSet glyphs exceed bounds".to_owned());
                }
                for g in 0..glyph_count {
                    insert_gid(be_u16(alt, 2 + g * 2).unwrap_or(0), retained);
                }
            }
        }
        (4, 1) => {
            let cov_off =
                be_u16(subtable, 2).ok_or_else(|| "GSUB type 4 truncated".to_owned())? as usize;
            let set_count = be_u16(subtable, 4)
                .ok_or_else(|| "GSUB type 4 ligatureSetCount truncated".to_owned())?
                as usize;
            if 6 + set_count * 2 > subtable.len() {
                return Err("GSUB type 4 ligatureSet offsets exceed bounds".to_owned());
            }
            for (idx, first_gid) in cov_at(cov_off)?.into_iter().enumerate() {
                if idx >= set_count || !retained.contains(&first_gid) {
                    continue;
                }
                let set_off = be_u16(subtable, 6 + idx * 2).unwrap_or(0) as usize;
                let set_slice = subtable
                    .get(set_off..)
                    .ok_or_else(|| "GSUB ligatureSet offset out of bounds".to_owned())?;
                let lig_count = be_u16(set_slice, 0)
                    .ok_or_else(|| "GSUB ligatureSet header truncated".to_owned())?
                    as usize;
                if 2 + lig_count * 2 > set_slice.len() {
                    return Err("GSUB ligature offsets exceed bounds".to_owned());
                }
                for l in 0..lig_count {
                    let lig_off = be_u16(set_slice, 2 + l * 2).unwrap_or(0) as usize;
                    let lig = set_slice
                        .get(lig_off..)
                        .ok_or_else(|| "GSUB ligature offset out of bounds".to_owned())?;
                    let lig_glyph =
                        be_u16(lig, 0).ok_or_else(|| "GSUB ligature glyph truncated".to_owned())?;
                    let comp_count = be_u16(lig, 2)
                        .ok_or_else(|| "GSUB ligature componentCount truncated".to_owned())?
                        as usize;
                    if comp_count == 0 || 4 + (comp_count - 1) * 2 > lig.len() {
                        return Err("GSUB ligature components exceed bounds".to_owned());
                    }
                    let all_components_retained = (0..(comp_count - 1)).all(|c| {
                        be_u16(lig, 4 + c * 2).is_some_and(|comp| retained.contains(&comp))
                    });
                    if all_components_retained {
                        insert_gid(lig_glyph, retained);
                    }
                }
            }
        }
        (5, 1) => {
            let cov_off = be_u16(subtable, 2)
                .ok_or_else(|| "GSUB type 5 format 1 truncated".to_owned())?
                as usize;
            let _ = cov_at(cov_off)?;
            let count = be_u16(subtable, 4)
                .ok_or_else(|| "GSUB type 5 format 1 count truncated".to_owned())?
                as usize;
            if 6 + count * 2 > subtable.len() {
                return Err("GSUB type 5 format 1 subRuleSet exceeds bounds".to_owned());
            }
            for i in 0..count {
                let set_off = be_u16(subtable, 6 + i * 2).unwrap_or(0) as usize;
                if set_off == 0 {
                    continue;
                }
                let set_slice = subtable
                    .get(set_off..)
                    .ok_or_else(|| "GSUB type 5 subRuleSet out of bounds".to_owned())?;
                let rule_count = be_u16(set_slice, 0)
                    .ok_or_else(|| "GSUB type 5 subRuleSet count truncated".to_owned())?
                    as usize;
                if 2 + rule_count * 2 > set_slice.len() {
                    return Err("GSUB type 5 subRule offsets exceed bounds".to_owned());
                }
                for r in 0..rule_count {
                    let rule_off = be_u16(set_slice, 2 + r * 2).unwrap_or(0) as usize;
                    let rule = set_slice
                        .get(rule_off..)
                        .ok_or_else(|| "GSUB type 5 subRule out of bounds".to_owned())?;
                    let glyph_count = be_u16(rule, 0)
                        .ok_or_else(|| "GSUB type 5 subRule glyphCount truncated".to_owned())?
                        as usize;
                    let subst_count = be_u16(rule, 2)
                        .ok_or_else(|| "GSUB type 5 subRule substCount truncated".to_owned())?
                        as usize;
                    let input_end = 4 + glyph_count.saturating_sub(1) * 2;
                    validate_subst_lookup_records(rule, input_end, subst_count, lookup_count)?;
                }
            }
        }
        (5, 2) => {
            let cov_off = be_u16(subtable, 2)
                .ok_or_else(|| "GSUB type 5 format 2 truncated".to_owned())?
                as usize;
            let class_off = be_u16(subtable, 4)
                .ok_or_else(|| "GSUB type 5 format 2 classDef truncated".to_owned())?
                as usize;
            let _ = cov_at(cov_off)?;
            validate_class_def(
                subtable
                    .get(class_off..)
                    .ok_or_else(|| "GSUB type 5 classDef out of bounds".to_owned())?,
            )
            .ok_or_else(|| "GSUB type 5 invalid classDef".to_owned())?;
            let count = be_u16(subtable, 6)
                .ok_or_else(|| "GSUB type 5 format 2 count truncated".to_owned())?
                as usize;
            if 8 + count * 2 > subtable.len() {
                return Err("GSUB type 5 subClassSet exceeds bounds".to_owned());
            }
            for i in 0..count {
                let set_off = be_u16(subtable, 8 + i * 2).unwrap_or(0) as usize;
                if set_off == 0 {
                    continue;
                }
                let set_slice = subtable
                    .get(set_off..)
                    .ok_or_else(|| "GSUB type 5 subClassSet out of bounds".to_owned())?;
                let rule_count = be_u16(set_slice, 0)
                    .ok_or_else(|| "GSUB type 5 subClassSet count truncated".to_owned())?
                    as usize;
                if 2 + rule_count * 2 > set_slice.len() {
                    return Err("GSUB type 5 subClassRule offsets exceed bounds".to_owned());
                }
                for r in 0..rule_count {
                    let rule_off = be_u16(set_slice, 2 + r * 2).unwrap_or(0) as usize;
                    let rule = set_slice
                        .get(rule_off..)
                        .ok_or_else(|| "GSUB type 5 subClassRule out of bounds".to_owned())?;
                    let glyph_count = be_u16(rule, 0)
                        .ok_or_else(|| "GSUB type 5 subClassRule glyphCount truncated".to_owned())?
                        as usize;
                    let subst_count = be_u16(rule, 2)
                        .ok_or_else(|| "GSUB type 5 subClassRule substCount truncated".to_owned())?
                        as usize;
                    let input_end = 4 + glyph_count.saturating_sub(1) * 2;
                    validate_subst_lookup_records(rule, input_end, subst_count, lookup_count)?;
                }
            }
        }
        (5, 3) => {
            let glyph_count = be_u16(subtable, 2)
                .ok_or_else(|| "GSUB type 5 format 3 truncated".to_owned())?
                as usize;
            let subst_count = be_u16(subtable, 4)
                .ok_or_else(|| "GSUB type 5 format 3 substCount truncated".to_owned())?
                as usize;
            let cov_end = 6 + glyph_count * 2;
            validate_subst_lookup_records(subtable, cov_end, subst_count, lookup_count)?;
            for i in 0..glyph_count {
                let cov_off = be_u16(subtable, 6 + i * 2).unwrap_or(0) as usize;
                let _ = cov_at(cov_off)?;
            }
        }
        (6, 1) => {
            let cov_off = be_u16(subtable, 2)
                .ok_or_else(|| "GSUB type 6 format 1 truncated".to_owned())?
                as usize;
            let _ = cov_at(cov_off)?;
            let count = be_u16(subtable, 4)
                .ok_or_else(|| "GSUB type 6 format 1 count truncated".to_owned())?
                as usize;
            if 6 + count * 2 > subtable.len() {
                return Err("GSUB type 6 chainSubRuleSet exceeds bounds".to_owned());
            }
            for i in 0..count {
                let set_off = be_u16(subtable, 6 + i * 2).unwrap_or(0) as usize;
                if set_off == 0 {
                    continue;
                }
                let set_slice = subtable
                    .get(set_off..)
                    .ok_or_else(|| "GSUB type 6 chainSubRuleSet out of bounds".to_owned())?;
                let rule_count = be_u16(set_slice, 0)
                    .ok_or_else(|| "GSUB type 6 chainSubRuleSet count truncated".to_owned())?
                    as usize;
                if 2 + rule_count * 2 > set_slice.len() {
                    return Err("GSUB type 6 chainSubRule offsets exceed bounds".to_owned());
                }
                for r in 0..rule_count {
                    let rule_off = be_u16(set_slice, 2 + r * 2).unwrap_or(0) as usize;
                    let rule = set_slice
                        .get(rule_off..)
                        .ok_or_else(|| "GSUB type 6 chainSubRule out of bounds".to_owned())?;
                    let back_count = be_u16(rule, 0)
                        .ok_or_else(|| "GSUB type 6 backtrackGlyphCount truncated".to_owned())?
                        as usize;
                    let in_pos = 2 + back_count * 2;
                    let in_count = be_u16(rule, in_pos)
                        .ok_or_else(|| "GSUB type 6 inputGlyphCount truncated".to_owned())?
                        as usize;
                    let look_pos = in_pos + 2 + in_count.saturating_sub(1) * 2;
                    let look_count = be_u16(rule, look_pos)
                        .ok_or_else(|| "GSUB type 6 lookaheadGlyphCount truncated".to_owned())?
                        as usize;
                    let subst_pos = look_pos + 2 + look_count * 2;
                    let subst_count = be_u16(rule, subst_pos)
                        .ok_or_else(|| "GSUB type 6 substCount truncated".to_owned())?
                        as usize;
                    validate_subst_lookup_records(rule, subst_pos + 2, subst_count, lookup_count)?;
                }
            }
        }
        (6, 2) => {
            let cov_off = be_u16(subtable, 2)
                .ok_or_else(|| "GSUB type 6 format 2 truncated".to_owned())?
                as usize;
            let back_cd = be_u16(subtable, 4)
                .ok_or_else(|| "GSUB type 6 format 2 backtrackClassDef truncated".to_owned())?
                as usize;
            let in_cd = be_u16(subtable, 6)
                .ok_or_else(|| "GSUB type 6 format 2 inputClassDef truncated".to_owned())?
                as usize;
            let look_cd = be_u16(subtable, 8)
                .ok_or_else(|| "GSUB type 6 format 2 lookaheadClassDef truncated".to_owned())?
                as usize;
            let _ = cov_at(cov_off)?;
            for cd_off in [back_cd, in_cd, look_cd] {
                if cd_off != 0 {
                    validate_class_def(
                        subtable
                            .get(cd_off..)
                            .ok_or_else(|| "GSUB type 6 classDef out of bounds".to_owned())?,
                    )
                    .ok_or_else(|| "GSUB type 6 invalid classDef".to_owned())?;
                }
            }
            let count = be_u16(subtable, 10)
                .ok_or_else(|| "GSUB type 6 format 2 count truncated".to_owned())?
                as usize;
            if 12 + count * 2 > subtable.len() {
                return Err("GSUB type 6 chainSubClassSet exceeds bounds".to_owned());
            }
            for i in 0..count {
                let set_off = be_u16(subtable, 12 + i * 2).unwrap_or(0) as usize;
                if set_off == 0 {
                    continue;
                }
                let set_slice = subtable
                    .get(set_off..)
                    .ok_or_else(|| "GSUB type 6 chainSubClassSet out of bounds".to_owned())?;
                let rule_count = be_u16(set_slice, 0)
                    .ok_or_else(|| "GSUB type 6 chainSubClassSet count truncated".to_owned())?
                    as usize;
                if 2 + rule_count * 2 > set_slice.len() {
                    return Err("GSUB type 6 chainSubClassRule offsets exceed bounds".to_owned());
                }
                for r in 0..rule_count {
                    let rule_off = be_u16(set_slice, 2 + r * 2).unwrap_or(0) as usize;
                    let rule = set_slice
                        .get(rule_off..)
                        .ok_or_else(|| "GSUB type 6 chainSubClassRule out of bounds".to_owned())?;
                    let back_count = be_u16(rule, 0)
                        .ok_or_else(|| "GSUB type 6 backtrackGlyphCount truncated".to_owned())?
                        as usize;
                    let in_pos = 2 + back_count * 2;
                    let in_count = be_u16(rule, in_pos)
                        .ok_or_else(|| "GSUB type 6 inputGlyphCount truncated".to_owned())?
                        as usize;
                    let look_pos = in_pos + 2 + in_count.saturating_sub(1) * 2;
                    let look_count = be_u16(rule, look_pos)
                        .ok_or_else(|| "GSUB type 6 lookaheadGlyphCount truncated".to_owned())?
                        as usize;
                    let subst_pos = look_pos + 2 + look_count * 2;
                    let subst_count = be_u16(rule, subst_pos)
                        .ok_or_else(|| "GSUB type 6 substCount truncated".to_owned())?
                        as usize;
                    validate_subst_lookup_records(rule, subst_pos + 2, subst_count, lookup_count)?;
                }
            }
        }
        (6, 3) => {
            let back_count = be_u16(subtable, 2)
                .ok_or_else(|| "GSUB type 6 format 3 backtrackGlyphCount truncated".to_owned())?
                as usize;
            let in_pos = 4 + back_count * 2;
            let in_count = be_u16(subtable, in_pos)
                .ok_or_else(|| "GSUB type 6 format 3 inputGlyphCount truncated".to_owned())?
                as usize;
            let look_pos = in_pos + 2 + in_count * 2;
            let look_count = be_u16(subtable, look_pos)
                .ok_or_else(|| "GSUB type 6 format 3 lookaheadGlyphCount truncated".to_owned())?
                as usize;
            let subst_pos = look_pos + 2 + look_count * 2;
            let subst_count = be_u16(subtable, subst_pos)
                .ok_or_else(|| "GSUB type 6 format 3 substCount truncated".to_owned())?
                as usize;
            validate_subst_lookup_records(subtable, subst_pos + 2, subst_count, lookup_count)?;
            for (pos, count) in [
                (4, back_count),
                (in_pos + 2, in_count),
                (look_pos + 2, look_count),
            ] {
                for i in 0..count {
                    let cov_off = be_u16(subtable, pos + i * 2).unwrap_or(0) as usize;
                    let _ = cov_at(cov_off)?;
                }
            }
        }
        (7, 1) => {
            if !allow_extension {
                return Err("Nested GSUB extension lookup (type 7) is invalid".to_owned());
            }
            let ext_type = be_u16(subtable, 2)
                .ok_or_else(|| "GSUB extension lookupType truncated".to_owned())?;
            let ext_off = be_u32(subtable, 4)
                .ok_or_else(|| "GSUB extensionOffset truncated".to_owned())?
                as usize;
            let ext_slice = subtable
                .get(ext_off..)
                .ok_or_else(|| "GSUB extensionOffset out of bounds".to_owned())?;
            apply_gsub_subtable(
                ext_type,
                ext_slice,
                num_glyphs,
                lookup_count,
                retained,
                false,
            )?;
        }
        (8, 1) => {
            let cov_off = be_u16(subtable, 2)
                .ok_or_else(|| "GSUB type 8 coverage truncated".to_owned())?
                as usize;
            let cov = cov_at(cov_off)?;
            let back_count = be_u16(subtable, 4)
                .ok_or_else(|| "GSUB type 8 backtrackGlyphCount truncated".to_owned())?
                as usize;
            let look_pos = 6 + back_count * 2;
            let look_count = be_u16(subtable, look_pos)
                .ok_or_else(|| "GSUB type 8 lookaheadGlyphCount truncated".to_owned())?
                as usize;
            let sub_pos = look_pos + 2 + look_count * 2;
            let sub_count = be_u16(subtable, sub_pos)
                .ok_or_else(|| "GSUB type 8 glyphCount truncated".to_owned())?
                as usize;
            if sub_pos + 2 + sub_count * 2 > subtable.len() {
                return Err("GSUB type 8 substitutes exceed bounds".to_owned());
            }
            let mut context_possible = true;
            for i in 0..back_count {
                let off = be_u16(subtable, 6 + i * 2).unwrap_or(0) as usize;
                let back_cov = cov_at(off)?;
                if !back_cov.iter().any(|g| retained.contains(g)) {
                    context_possible = false;
                    break;
                }
            }
            if context_possible {
                for i in 0..look_count {
                    let off = be_u16(subtable, look_pos + 2 + i * 2).unwrap_or(0) as usize;
                    let look_cov = cov_at(off)?;
                    if !look_cov.iter().any(|g| retained.contains(g)) {
                        context_possible = false;
                        break;
                    }
                }
            }
            if context_possible {
                for (idx, gid) in cov.into_iter().enumerate() {
                    if idx < sub_count
                        && retained.contains(&gid)
                        && let Some(sub) = be_u16(subtable, sub_pos + 2 + idx * 2)
                    {
                        insert_gid(sub, retained);
                    }
                }
            }
        }
        _ => {
            return Err(format!(
                "unsupported GSUB lookupType={lookup_type} format={format}"
            ));
        }
    }
    Ok(())
}

fn expand_composite_glyf_closure(
    glyf: &[u8],
    orig_offsets: &[usize],
    num_glyphs: usize,
    retained: &mut BTreeSet<u16>,
) -> Result<(), String> {
    let mut queue: Vec<u16> = retained.iter().copied().collect();
    while let Some(gid) = queue.pop() {
        let start = *orig_offsets
            .get(gid as usize)
            .ok_or_else(|| "glyf composite gid out of bounds".to_owned())?;
        let end = *orig_offsets
            .get(gid as usize + 1)
            .ok_or_else(|| "glyf composite end out of bounds".to_owned())?;
        if end <= start || end - start < 10 {
            continue;
        }
        let glyph_data = &glyf[start..end];
        let num_contours = i16::from_be_bytes([glyph_data[0], glyph_data[1]]);
        if num_contours < 0 {
            let mut pos = 10usize;
            loop {
                if pos + 4 > glyph_data.len() {
                    return Err("glyf composite glyph component table truncated".to_owned());
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
                    return Err("glyf composite glyph transform flags exceed bounds".to_owned());
                }
                if (flags & 0x0020) == 0 {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn expand_gsub_closure(
    gsub: &[u8],
    glyf: &[u8],
    orig_offsets: &[usize],
    num_glyphs: usize,
    retained: &mut BTreeSet<u16>,
) -> Result<(), String> {
    if gsub.len() < 10 {
        return Err("GSUB header is truncated".to_owned());
    }
    let major = be_u16(gsub, 0).unwrap_or(0);
    if major != 1 {
        return Err(format!("unsupported GSUB majorVersion={major}"));
    }
    let lookup_list_off =
        be_u16(gsub, 8).ok_or_else(|| "GSUB lookupListOffset truncated".to_owned())? as usize;
    let lookup_list = gsub
        .get(lookup_list_off..)
        .ok_or_else(|| "GSUB lookupListOffset out of bounds".to_owned())?;
    let lookup_count =
        be_u16(lookup_list, 0).ok_or_else(|| "GSUB lookupCount truncated".to_owned())? as usize;
    if 2 + lookup_count * 2 > lookup_list.len() {
        return Err("GSUB lookup offsets exceed bounds".to_owned());
    }

    for _pass in 0..64 {
        let before = retained.len();
        expand_composite_glyf_closure(glyf, orig_offsets, num_glyphs, retained)?;
        for idx in 0..lookup_count {
            let lookup_off = be_u16(lookup_list, 2 + idx * 2).unwrap_or(0) as usize;
            let lookup = lookup_list
                .get(lookup_off..)
                .ok_or_else(|| "GSUB lookup offset out of bounds".to_owned())?;
            let lookup_type =
                be_u16(lookup, 0).ok_or_else(|| "GSUB lookupType truncated".to_owned())?;
            let subtable_count = be_u16(lookup, 4)
                .ok_or_else(|| "GSUB subTableCount truncated".to_owned())?
                as usize;
            if 6 + subtable_count * 2 > lookup.len() {
                return Err("GSUB subtableOffsets exceed bounds".to_owned());
            }
            for s in 0..subtable_count {
                let sub_off = be_u16(lookup, 6 + s * 2).unwrap_or(0) as usize;
                let subtable = lookup
                    .get(sub_off..)
                    .ok_or_else(|| "GSUB subtable offset out of bounds".to_owned())?;
                apply_gsub_subtable(
                    lookup_type,
                    subtable,
                    num_glyphs,
                    lookup_count,
                    retained,
                    true,
                )?;
            }
        }
        expand_composite_glyf_closure(glyf, orig_offsets, num_glyphs, retained)?;
        if retained.len() == before {
            return Ok(());
        }
    }
    Err("GSUB glyph closure did not converge within 64 passes".to_owned())
}

enum SubsetOutcome {
    Subsetted {
        bytes: Vec<u8>,
        diagnostic: Option<String>,
    },
    PreservedComplete {
        bytes: Vec<u8>,
        diagnostic: String,
    },
}

fn subset_sfnt_inner(sfnt_bytes: &[u8], codepoints: &BTreeSet<u32>) -> Result<Vec<u8>, String> {
    if sfnt_bytes.len() < 12 {
        return Err("font header is smaller than a 12-byte SFNT directory".to_owned());
    }
    let sfnt_version =
        u32::from_be_bytes([sfnt_bytes[0], sfnt_bytes[1], sfnt_bytes[2], sfnt_bytes[3]]);
    match sfnt_version {
        0x0001_0000 | 0x7472_7565 => {}
        0x4F54_544F => {
            return Err(
                "Complete OpenType/CFF (OTTO) font preserved: CFF/CFF2 outline subsetting is not supported, avoiding glyph corruption."
                    .to_owned(),
            );
        }
        0x7474_6366 => {
            return Err(
                "Complete TrueType Collection (ttcf) preserved: font collections are not subsetted."
                    .to_owned(),
            );
        }
        0x774F_4632 => {
            return Err(
                "Complete WOFF2 font preserved: WOFF2 decompression/re-encoding is not supported for subsetting."
                    .to_owned(),
            );
        }
        _ => {
            return Err(
                "Complete font preserved: resource is not a TrueType SFNT or WOFF1 outline font."
                    .to_owned(),
            );
        }
    }

    let num_tables = u16::from_be_bytes([sfnt_bytes[4], sfnt_bytes[5]]) as usize;
    let dir_end = 12usize
        .checked_add(
            num_tables
                .checked_mul(16)
                .ok_or("SFNT directory overflow")?,
        )
        .ok_or("SFNT directory overflow")?;
    if sfnt_bytes.len() < dir_end {
        return Err("SFNT table directory is truncated".to_owned());
    }

    let mut tables = BTreeMap::<[u8; 4], &[u8]>::new();
    for i in 0..num_tables {
        let rec = &sfnt_bytes[12 + i * 16..12 + (i + 1) * 16];
        let tag = [rec[0], rec[1], rec[2], rec[3]];
        let offset = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as usize;
        let length = u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as usize;
        let end = offset
            .checked_add(length)
            .filter(|&e| e <= sfnt_bytes.len())
            .ok_or_else(|| "SFNT table offset/length exceeds binary bounds".to_owned())?;
        tables.insert(tag, &sfnt_bytes[offset..end]);
    }

    let head = *tables
        .get(b"head")
        .ok_or_else(|| "SFNT is missing required 'head' table".to_owned())?;
    let maxp = *tables
        .get(b"maxp")
        .ok_or_else(|| "SFNT is missing required 'maxp' table".to_owned())?;
    let loca = *tables
        .get(b"loca")
        .ok_or_else(|| "SFNT is missing required 'loca' table".to_owned())?;
    let glyf = *tables
        .get(b"glyf")
        .ok_or_else(|| "SFNT is missing required 'glyf' table".to_owned())?;
    let cmap = *tables
        .get(b"cmap")
        .ok_or_else(|| "SFNT is missing required 'cmap' table".to_owned())?;
    if head.len() < 54
        || maxp.len() < 6
        || !tables.contains_key(b"hhea")
        || !tables.contains_key(b"hmtx")
    {
        return Err(
            "Complete font preserved: required TrueType SFNT tables (head/maxp/cmap/hhea/hmtx) are incomplete."
                .to_owned(),
        );
    }

    const OPAQUE_GLYPH_TABLES: [&[u8; 4]; 9] = [
        b"morx", b"mort", b"COLR", b"SVG ", b"sbix", b"CBDT", b"EBDT", b"bdat", b"MATH",
    ];
    let present_opaque: Vec<String> = OPAQUE_GLYPH_TABLES
        .iter()
        .filter(|tag| tables.contains_key(**tag))
        .map(|tag| String::from_utf8_lossy(*tag).trim().to_owned())
        .collect();
    if !present_opaque.is_empty() {
        return Err(format!(
            "Complete font preserved: font contains opaque color, bitmap, AAT, or math tables ({}) that cannot be safely pruned without risking missing glyphs.",
            present_opaque.join(", ")
        ));
    }

    if has_format14_uvs_match(cmap, codepoints)? {
        return Err(
            "Complete font preserved: requested text uses Unicode variation sequences (cmap format 14) requiring full glyph retention."
                .to_owned(),
        );
    }

    let num_glyphs = u16::from_be_bytes([maxp[4], maxp[5]]) as usize;
    if num_glyphs == 0 {
        return Err("Complete font preserved: font reports zero glyphs.".to_owned());
    }

    let font = FontRef::try_from_slice(sfnt_bytes).map_err(|_| {
        "Complete font preserved: OpenType font metrics parser rejected face.".to_owned()
    })?;
    let mut retained = BTreeSet::<u16>::new();
    retained.insert(0);
    if num_glyphs > 1 {
        retained.insert(1);
    }
    let mut retained_cmap = BTreeMap::<u32, u16>::new();
    for cp in codepoints.iter().copied().chain([9u32, 10, 13, 32]) {
        if let Some(ch) = char::from_u32(cp) {
            let gid = font.glyph_id(ch).0;
            if gid != 0 && (gid as usize) < num_glyphs {
                retained.insert(gid);
                retained_cmap.insert(cp, gid);
            }
        }
    }

    let index_to_loc_format = i16::from_be_bytes([head[50], head[51]]);
    let mut orig_offsets = Vec::with_capacity(num_glyphs + 1);
    match index_to_loc_format {
        0 => {
            if loca.len() < (num_glyphs + 1) * 2 {
                return Err("SFNT short 'loca' table is truncated".to_owned());
            }
            for i in 0..=num_glyphs {
                let off = (u16::from_be_bytes([loca[i * 2], loca[i * 2 + 1]]) as usize) * 2;
                orig_offsets.push(off);
            }
        }
        1 => {
            if loca.len() < (num_glyphs + 1) * 4 {
                return Err("SFNT long 'loca' table is truncated".to_owned());
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
        _ => return Err("SFNT 'head' indexToLocFormat is unsupported".to_owned()),
    }
    if !orig_offsets.windows(2).all(|w| w[0] <= w[1])
        || orig_offsets.last().is_none_or(|&last| last > glyf.len())
    {
        return Err("SFNT 'loca' glyph offsets exceed 'glyf' bounds".to_owned());
    }

    if let Some(gsub) = tables.get(b"GSUB").copied() {
        if let Some(script_label) = detect_complex_gsub_script(gsub, codepoints) {
            return Err(format!(
                "Complete font preserved: text uses complex-script {script_label} shaping via GSUB; retaining full glyph repertoire and OpenType shaping tables."
            ));
        }
        if let Err(reason) =
            expand_gsub_closure(gsub, glyf, &orig_offsets, num_glyphs, &mut retained)
        {
            return Err(format!(
                "Complete font preserved: could not prove full GSUB glyph closure ({reason}); retaining full glyph set to prevent shaping loss."
            ));
        }
    } else {
        expand_composite_glyf_closure(glyf, &orig_offsets, num_glyphs, &mut retained)?;
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

    let use_short_loca = new_offsets
        .last()
        .is_some_and(|&last| last <= (u16::MAX as u32) * 2);
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

    // `glyf` keeps stable glyph IDs but replaces every unretained outline with an empty slot.
    // The matching cmap therefore has to be rewritten even when its sparse encoding is the same
    // size as (or larger than) the original table. Keeping the original cmap in that case would
    // advertise removed characters and make browsers select empty glyph slots instead of using
    // their normal missing-glyph/fallback path.
    let new_cmap = build_compact_cmap(&retained_cmap).ok_or_else(|| {
        "Complete font preserved: retained Unicode mappings cannot be encoded safely in cmap format 4/12."
            .to_owned()
    })?;

    let mut new_head = head.to_vec();
    new_head[8..12].copy_from_slice(&[0, 0, 0, 0]);
    new_head[50..52].copy_from_slice(&new_loca_format.to_be_bytes());

    let new_os2 = tables.get(b"OS/2").map(|os2| {
        let mut updated = os2.to_vec();
        if updated.len() >= 68
            && let (Some(&first), Some(&last)) = (
                retained_cmap.keys().next(),
                retained_cmap.keys().next_back(),
            )
        {
            updated[64..66].copy_from_slice(&(first.min(0xFFFF) as u16).to_be_bytes());
            updated[66..68].copy_from_slice(&(last.min(0xFFFF) as u16).to_be_bytes());
        }
        updated
    });

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
            b"cmap" => new_cmap.clone(),
            b"OS/2" => new_os2.clone().unwrap_or_else(|| original.to_vec()),
            b"head" => new_head.clone(),
            b"post" => new_post.clone().unwrap_or_else(|| original.to_vec()),
            _ => original.to_vec(),
        };
        out_tables.push((*tag, data));
    }
    out_tables.sort_by_key(|(tag, _)| *tag);

    let table_count = out_tables.len() as u16;
    if table_count == 0 {
        return Err("subset font contains zero SFNT tables".to_owned());
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

    let head_off = head_table_offset.ok_or("subset font missing head table")?;
    let total_sum = sfnt_checksum(&out);
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(total_sum);
    out[head_off + 8..head_off + 12].copy_from_slice(&adjustment.to_be_bytes());

    let subset_font = FontRef::try_from_slice(&out)
        .map_err(|_| "subset SFNT failed FontRef verification".to_owned())?;
    for cp in codepoints.iter().copied().chain([32u32]) {
        if let Some(ch) = char::from_u32(cp)
            && subset_font.glyph_id(ch) != font.glyph_id(ch)
        {
            return Err(format!(
                "subset SFNT altered glyph mapping for U+{cp:04X}; preserving complete face"
            ));
        }
    }
    Ok(out)
}

fn subset_font_with_diagnostics(raw_bytes: &[u8], codepoints: &BTreeSet<u32>) -> SubsetOutcome {
    let is_woff1 = raw_bytes.starts_with(b"wOFF");
    let sfnt_cow = if is_woff1 {
        match neoflash_core::unpack_woff_to_sfnt(raw_bytes) {
            Some(sfnt) => std::borrow::Cow::Owned(sfnt),
            None => {
                return SubsetOutcome::PreservedComplete {
                    bytes: raw_bytes.to_vec(),
                    diagnostic: "Complete WOFF1 font preserved: could not unpack WOFF1 table stream cleanly for subsetting."
                        .to_owned(),
                };
            }
        }
    } else {
        std::borrow::Cow::Borrowed(raw_bytes)
    };

    match subset_sfnt_inner(sfnt_cow.as_ref(), codepoints) {
        Ok(subset_sfnt) => {
            let final_bytes = if is_woff1 {
                match neoflash_core::pack_sfnt_to_woff(&subset_sfnt) {
                    Some(woff) => woff,
                    None => {
                        return SubsetOutcome::PreservedComplete {
                            bytes: raw_bytes.to_vec(),
                            diagnostic: "Complete WOFF1 font preserved: could not re-pack subset WOFF1 container cleanly."
                                .to_owned(),
                        };
                    }
                }
            } else {
                subset_sfnt
            };

            if final_bytes.len() >= raw_bytes.len() {
                SubsetOutcome::PreservedComplete {
                    bytes: raw_bytes.to_vec(),
                    diagnostic: "Complete font preserved: subsetting would not reduce encoded font size for the requested glyphs."
                        .to_owned(),
                }
            } else {
                SubsetOutcome::Subsetted {
                    bytes: final_bytes,
                    diagnostic: None,
                }
            }
        }
        Err(reason) => SubsetOutcome::PreservedComplete {
            bytes: raw_bytes.to_vec(),
            diagnostic: reason,
        },
    }
}

/// Deterministically subsets a TrueType SFNT (`0x00010000` / `true`) or compressed WOFF1 (`wOFF`)
/// font, retaining `.notdef`, whitespace, all requested `codepoints`, GSUB-reachable substitution /
/// ligature glyphs (types 1..=8), and transitive composite glyph components while pruning unused
/// glyph outlines from `glyf`/`loca`, compacting `cmap` (format 4 / format 12) and `post` v3.0,
/// updating `OS/2` character bounds, stripping `DSIG`, and recomputing SFNT table/file checksums.
/// Returns `None` if `raw_bytes` cannot be safely subsetted (for example CFF `OTTO`, WOFF2,
/// collections, opaque `COLR`/`CBDT`/`morx` tables, complex shaping scripts, or no size savings).
pub fn subset_ttf_font_bytes(raw_bytes: &[u8], codepoints: &BTreeSet<u32>) -> Option<Vec<u8>> {
    match subset_font_with_diagnostics(raw_bytes, codepoints) {
        SubsetOutcome::Subsetted { bytes, .. } => Some(bytes),
        SubsetOutcome::PreservedComplete { .. } => None,
    }
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

    let mut metrics_has_regular = BTreeSet::<String>::new();
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
            } else {
                match subset_font_with_diagnostics(&raw_bytes, &codepoints) {
                    SubsetOutcome::Subsetted {
                        bytes: subset_bytes,
                        diagnostic,
                    } => {
                        res.data_base64 = STANDARD.encode(&subset_bytes).into();
                        let default_diag = Some(if runtime_mutable {
                            "Runtime-editable Dynamic/Input text is limited to the explicitly authored embedded-character ranges plus current text. GSUB-reachable glyphs, composite dependencies, GDEF, GPOS, and kern tables are preserved; this does not claim complete shaping-aware subsetting."
                                .to_owned()
                        } else {
                            "TrueType glyph outlines and cmap are subsetted while GSUB-reachable glyphs, composite dependencies, GDEF, GPOS, and kern tables are preserved; this does not claim complete shaping-aware subsetting."
                                .to_owned()
                        });
                        bundle.subset_stats.push(FontSubsetStats {
                            family: family.clone(),
                            original_bytes,
                            subset_bytes: subset_bytes.len(),
                            codepoint_count: codepoints.len(),
                            subset_applied: true,
                            diagnostic: diagnostic.or(default_diag),
                        });
                        subset_bytes
                    }
                    SubsetOutcome::PreservedComplete { bytes, diagnostic } => {
                        let preserved_len = bytes.len();
                        bundle.subset_stats.push(FontSubsetStats {
                            family: family.clone(),
                            original_bytes,
                            subset_bytes: preserved_len,
                            codepoint_count: codepoints.len(),
                            subset_applied: false,
                            diagnostic: Some(diagnostic),
                        });
                        bytes
                    }
                }
            }
        } else {
            raw_bytes
        };

        let is_regular = style.eq_ignore_ascii_case("Regular");
        let fam_lower = family.to_ascii_lowercase();
        if (is_regular || !metrics_has_regular.contains(&fam_lower))
            && let Ok(json_str) =
                TextFormat::portable_font_metrics_json(&family, Some(&active_bytes))
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str)
        {
            if is_regular {
                metrics_has_regular.insert(fam_lower);
            }
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
        let mime = neoflash_core::sniff_font_media_type(
            &active_bytes,
            if res.media_type.trim().is_empty() {
                "font/ttf"
            } else {
                res.media_type.trim()
            },
        );
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

    fn read_sfnt_tables_map(bytes: &[u8]) -> BTreeMap<[u8; 4], (u32, usize, &[u8])> {
        let num_tables = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        let mut out = BTreeMap::new();
        for i in 0..num_tables {
            let rec = &bytes[12 + i * 16..12 + (i + 1) * 16];
            let tag = [rec[0], rec[1], rec[2], rec[3]];
            let checksum = u32::from_be_bytes([rec[4], rec[5], rec[6], rec[7]]);
            let offset = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as usize;
            let length = u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as usize;
            out.insert(tag, (checksum, offset, &bytes[offset..offset + length]));
        }
        out
    }

    fn assert_valid_sfnt_checksums_and_structure(bytes: &[u8]) {
        assert!(bytes.len() >= 12);
        let version = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert!(version == 0x0001_0000 || version == 0x7472_7565);
        let num_tables = u16::from_be_bytes([bytes[4], bytes[5]]);
        let entry_selector = 15u16.saturating_sub(num_tables.leading_zeros() as u16);
        let search_range = (1u16 << entry_selector) * 16;
        let range_shift = num_tables * 16 - search_range;
        assert_eq!(u16::from_be_bytes([bytes[6], bytes[7]]), search_range);
        assert_eq!(u16::from_be_bytes([bytes[8], bytes[9]]), entry_selector);
        assert_eq!(u16::from_be_bytes([bytes[10], bytes[11]]), range_shift);

        let mut prev_tag: Option<[u8; 4]> = None;
        for i in 0..(num_tables as usize) {
            let rec = &bytes[12 + i * 16..12 + (i + 1) * 16];
            let tag = [rec[0], rec[1], rec[2], rec[3]];
            if let Some(prev) = prev_tag {
                assert!(
                    prev < tag,
                    "SFNT tags not strictly sorted: {prev:?} >= {tag:?}"
                );
            }
            prev_tag = Some(tag);
            let expected_sum = u32::from_be_bytes([rec[4], rec[5], rec[6], rec[7]]);
            let offset = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as usize;
            let length = u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as usize;
            assert!(offset.is_multiple_of(4), "table {tag:?} unaligned offset");
            assert!(offset + length <= bytes.len());
            let mut slice = bytes[offset..offset + length].to_vec();
            if &tag == b"head" {
                slice[8..12].copy_from_slice(&[0, 0, 0, 0]);
            }
            assert_eq!(
                sfnt_checksum(&slice),
                expected_sum,
                "checksum mismatch for table {:?}",
                String::from_utf8_lossy(&tag)
            );
        }
        assert_eq!(
            sfnt_checksum(bytes),
            0xB1B0_AFBA,
            "SFNT whole-file head.checkSumAdjustment magic checksum failed"
        );
        FontRef::try_from_slice(bytes).expect("ab_glyph FontRef must accept rewritten SFNT");
    }

    fn build_sfnt_with_replaced_table(
        base: &[u8],
        version_override: Option<u32>,
        replace: &[([u8; 4], Option<Vec<u8>>)],
    ) -> Vec<u8> {
        let base_tables = read_sfnt_tables_map(base);
        let mut merged = BTreeMap::<[u8; 4], Vec<u8>>::new();
        for (tag, (_, _, data)) in base_tables {
            merged.insert(tag, data.to_vec());
        }
        for (tag, val) in replace {
            match val {
                Some(bytes) => {
                    merged.insert(*tag, bytes.clone());
                }
                None => {
                    merged.remove(tag);
                }
            }
        }
        let version = version_override
            .unwrap_or_else(|| u32::from_be_bytes([base[0], base[1], base[2], base[3]]));
        let table_count = merged.len() as u16;
        let entry_selector = 15u16.saturating_sub(table_count.leading_zeros() as u16);
        let search_range = (1u16 << entry_selector) * 16;
        let range_shift = table_count * 16 - search_range;
        let mut out = Vec::new();
        out.extend_from_slice(&version.to_be_bytes());
        out.extend_from_slice(&table_count.to_be_bytes());
        out.extend_from_slice(&search_range.to_be_bytes());
        out.extend_from_slice(&entry_selector.to_be_bytes());
        out.extend_from_slice(&range_shift.to_be_bytes());
        let dir_pos = out.len();
        out.resize(dir_pos + (table_count as usize) * 16, 0);
        let mut head_off = None;
        for (idx, (tag, data)) in merged.iter().enumerate() {
            let mut copy = data.clone();
            if tag == b"head" && copy.len() >= 12 {
                copy[8..12].copy_from_slice(&[0, 0, 0, 0]);
                head_off = Some(out.len());
            }
            let offset = out.len() as u32;
            let length = copy.len() as u32;
            let sum = sfnt_checksum(&copy);
            let rec_pos = dir_pos + idx * 16;
            out[rec_pos..rec_pos + 4].copy_from_slice(tag);
            out[rec_pos + 4..rec_pos + 8].copy_from_slice(&sum.to_be_bytes());
            out[rec_pos + 8..rec_pos + 12].copy_from_slice(&offset.to_be_bytes());
            out[rec_pos + 12..rec_pos + 16].copy_from_slice(&length.to_be_bytes());
            out.extend_from_slice(&copy);
            while !out.len().is_multiple_of(4) {
                out.push(0);
            }
        }
        if let Some(hoff) = head_off {
            let adj = 0xB1B0_AFBAu32.wrapping_sub(sfnt_checksum(&out));
            out[hoff + 8..hoff + 12].copy_from_slice(&adj.to_be_bytes());
        }
        out
    }

    #[test]
    fn subset_sfnt_inter_prunes_glyf_cmap_checksums_and_ligatures() {
        let mut project = Project::blank();
        add_font(&mut project, "clean-font", "CleanMachineFont.ttf");
        let mut fmt = TextFormat {
            font: "CleanMachineFont".to_owned(),
            size: 24.0,
            ..TextFormat::default()
        };
        fmt.set_field_options(Some(AuthoredTextFieldOptions {
            selectable: false,
            ..AuthoredTextFieldOptions::default()
        }));
        add_text_box(&mut project, "clean-box", "", "A Ω fi", fmt);

        let bundle = apply_font_policy(&mut project, FontExportPolicy::Subset);
        assert_eq!(bundle.subset_stats.len(), 1);
        let stats = &bundle.subset_stats[0];
        assert!(stats.subset_applied, "{stats:?}");
        assert!(
            stats.subset_bytes * 2 < stats.original_bytes,
            "expected Inter-Regular.ttf subset to drop unused glyf/cmap bytes dramatically: {stats:?}"
        );

        let subset = project.resources[0]
            .decoded_bytes()
            .expect("subset SFNT bytes");
        assert_valid_sfnt_checksums_and_structure(&subset);

        let orig_font = FontRef::try_from_slice(TEST_FONT).unwrap();
        let sub_font = FontRef::try_from_slice(&subset).unwrap();
        let sub_tables = read_sfnt_tables_map(&subset);
        assert!(!sub_tables.contains_key(b"DSIG"));
        assert!(sub_tables.contains_key(b"GSUB"));
        assert!(sub_tables.contains_key(b"GPOS"));
        assert!(sub_tables.contains_key(b"GDEF"));
        assert_eq!(
            u32::from_be_bytes(sub_tables[b"post"].2[0..4].try_into().unwrap()),
            0x0003_0000
        );
        let os2 = sub_tables[b"OS/2"].2;
        assert_eq!(u16::from_be_bytes([os2[64], os2[65]]), 32);
        assert_eq!(u16::from_be_bytes([os2[66], os2[67]]), 'Ω' as u16);

        for ch in ['A', 'Ω', 'f', 'i', ' '] {
            assert_eq!(sub_font.glyph_id(ch), orig_font.glyph_id(ch));
            assert_ne!(sub_font.glyph_id(ch).0, 0);
        }
        // Unrequested BMP characters must be removed from cmap AND pruned from glyf/loca.
        let head = sub_tables[b"head"].2;
        let maxp = sub_tables[b"maxp"].2;
        let loca = sub_tables[b"loca"].2;
        let num_glyphs = u16::from_be_bytes([maxp[4], maxp[5]]) as usize;
        let short_loca = i16::from_be_bytes([head[50], head[51]]) == 0;
        let glyph_span = |gid: usize| -> (usize, usize) {
            if short_loca {
                let s = (u16::from_be_bytes([loca[gid * 2], loca[gid * 2 + 1]]) as usize) * 2;
                let e = (u16::from_be_bytes([loca[(gid + 1) * 2], loca[(gid + 1) * 2 + 1]])
                    as usize)
                    * 2;
                (s, e)
            } else {
                let s = u32::from_be_bytes(loca[gid * 4..gid * 4 + 4].try_into().unwrap()) as usize;
                let e =
                    u32::from_be_bytes(loca[(gid + 1) * 4..(gid + 1) * 4 + 4].try_into().unwrap())
                        as usize;
                (s, e)
            }
        };
        assert!(glyph_span(0).1 > glyph_span(0).0, ".notdef kept");
        for unused in ['B', 'Z', 'g', 'q', '5'] {
            assert_eq!(sub_font.glyph_id(unused).0, 0, "cmap still maps {unused}");
            let orig_gid = orig_font.glyph_id(unused).0 as usize;
            assert!(orig_gid < num_glyphs);
            let (s, e) = glyph_span(orig_gid);
            assert_eq!(
                s, e,
                "unused glyph {unused} (gid {orig_gid}) was not pruned from glyf"
            );
        }
    }

    #[test]
    fn subset_rewrites_cmap_even_when_sparse_table_is_not_smaller() {
        let base_font = FontRef::try_from_slice(TEST_FONT).expect("valid Inter test font");
        let space_gid = base_font.glyph_id(' ').0;
        let a_gid = base_font.glyph_id('A').0;
        assert_ne!(space_gid, 0);
        assert_ne!(a_gid, 0);

        // A..B share one idDelta segment, so the original cmap and the requested
        // Space+A cmap have the same encoded size. The old size-only choice kept
        // this original table after pruning B's glyph slot.
        let original_cmap = build_compact_cmap(&BTreeMap::from([
            (' ' as u32, space_gid),
            ('A' as u32, a_gid),
            ('B' as u32, a_gid + 1),
        ]))
        .expect("compact source cmap");
        let retained_cmap = build_compact_cmap(&BTreeMap::from([
            (' ' as u32, space_gid),
            ('A' as u32, a_gid),
        ]))
        .expect("compact retained cmap");
        assert_eq!(
            original_cmap.len(),
            retained_cmap.len(),
            "fixture must exercise the equal-size cmap branch"
        );

        let source =
            build_sfnt_with_replaced_table(TEST_FONT, None, &[(*b"cmap", Some(original_cmap))]);
        let source_font = FontRef::try_from_slice(&source).expect("valid source font");
        assert_eq!(source_font.glyph_id('A').0, a_gid);
        assert_ne!(source_font.glyph_id('B').0, 0);

        let subset = subset_ttf_font_bytes(&source, &BTreeSet::from(['A' as u32]))
            .expect("source font should still shrink after glyph pruning");
        assert_valid_sfnt_checksums_and_structure(&subset);
        let subset_font = FontRef::try_from_slice(&subset).expect("valid subset font");
        assert_eq!(subset_font.glyph_id('A').0, a_gid);
        assert!(
            outline_glyph(&subset_font, 'A', 0.0, 0.0, 16.0)
                .is_some_and(|commands| !commands.is_empty()),
            "retained A must remain renderable"
        );
        assert_eq!(
            subset_font.glyph_id('B').0,
            0,
            "omitted B must not map to its now-pruned glyph slot"
        );
    }

    #[test]
    fn subset_woff1_font_roundtrips_compressed_woff_and_metrics() {
        let woff_orig = neoflash_core::pack_sfnt_to_woff(TEST_FONT).expect("pack Inter to WOFF1");
        let mut project = Project::blank();
        project.resources.push(
            ResourceUnit::from_bytes(
                "clean-woff",
                "fonts/CleanWoff.woff",
                "font/woff",
                &woff_orig,
            )
            .expect("woff resource"),
        );
        let mut fmt = TextFormat {
            font: "CleanWoff".to_owned(),
            size: 20.0,
            ..TextFormat::default()
        };
        fmt.set_field_options(Some(AuthoredTextFieldOptions {
            selectable: false,
            ..AuthoredTextFieldOptions::default()
        }));
        add_text_box(&mut project, "woff-static", "", "MiM", fmt);

        let bundle = apply_font_policy(&mut project, FontExportPolicy::Subset);
        assert_eq!(bundle.subset_stats.len(), 1);
        assert!(bundle.subset_stats[0].subset_applied);
        assert!(bundle.subset_stats[0].subset_bytes < bundle.subset_stats[0].original_bytes);
        assert!(bundle.css_rules.contains("data:font/woff;base64,"));

        let subset_woff = project.resources[0].decoded_bytes().expect("subset WOFF1");
        assert!(subset_woff.starts_with(b"wOFF"));
        let unpacked =
            neoflash_core::unpack_woff_to_sfnt(&subset_woff).expect("unpack subset WOFF1");
        assert_valid_sfnt_checksums_and_structure(&unpacked);

        let metrics = &bundle.metrics["CleanWoff"];
        let adv_m = metrics["advances"]["77"].as_u64().expect("'M' advance");
        let adv_i = metrics["advances"]["105"].as_u64().expect("'i' advance");
        assert!(
            adv_m > adv_i * 2,
            "expected real Inter WOFF1 M>i metrics: {adv_m} vs {adv_i}"
        );
    }

    #[test]
    fn subset_falls_back_explicitly_with_diagnostics_for_cff_opaque_complex_script_uvs_and_named_dynamic()
     {
        // 1. OpenType CFF (`OTTO`)
        let otto_cff = build_sfnt_with_replaced_table(
            TEST_FONT,
            Some(0x4F54_544F),
            &[
                (*b"glyf", None),
                (*b"loca", None),
                (*b"CFF ", Some(vec![1, 0, 4, 1, 0, 1, 0, 1])),
            ],
        );
        let mut p_cff = Project::blank();
        p_cff.resources.push(
            ResourceUnit::from_bytes("cff-res", "fonts/CffFace.otf", "font/otf", &otto_cff)
                .unwrap(),
        );
        let b_cff = apply_font_policy(&mut p_cff, FontExportPolicy::Subset);
        assert!(!b_cff.subset_stats[0].subset_applied);
        assert_eq!(
            b_cff.subset_stats[0].original_bytes,
            b_cff.subset_stats[0].subset_bytes
        );
        assert!(
            b_cff.subset_stats[0]
                .diagnostic
                .as_deref()
                .is_some_and(|d| d.contains("OTTO") && d.contains("CFF"))
        );

        // 2. Opaque table (`COLR`)
        let colr_ttf =
            build_sfnt_with_replaced_table(TEST_FONT, None, &[(*b"COLR", Some(vec![0; 14]))]);
        let mut p_colr = Project::blank();
        p_colr.resources.push(
            ResourceUnit::from_bytes("colr-res", "fonts/ColrFace.ttf", "font/ttf", &colr_ttf)
                .unwrap(),
        );
        let b_colr = apply_font_policy(&mut p_colr, FontExportPolicy::Subset);
        assert!(!b_colr.subset_stats[0].subset_applied);
        assert!(
            b_colr.subset_stats[0]
                .diagnostic
                .as_deref()
                .is_some_and(|d| d.contains("COLR"))
        );

        // 3. Complex-script GSUB (`arab` + Arabic character)
        let mut arab_gsub = Vec::new();
        arab_gsub.extend_from_slice(&1u16.to_be_bytes());
        arab_gsub.extend_from_slice(&0u16.to_be_bytes());
        arab_gsub.extend_from_slice(&10u16.to_be_bytes()); // ScriptList at 10
        arab_gsub.extend_from_slice(&20u16.to_be_bytes()); // FeatureList
        arab_gsub.extend_from_slice(&20u16.to_be_bytes()); // LookupList
        arab_gsub.extend_from_slice(&1u16.to_be_bytes()); // 1 script
        arab_gsub.extend_from_slice(b"arab");
        arab_gsub.extend_from_slice(&8u16.to_be_bytes());
        let arab_ttf =
            build_sfnt_with_replaced_table(TEST_FONT, None, &[(*b"GSUB", Some(arab_gsub))]);
        let mut p_arab = Project::blank();
        p_arab.resources.push(
            ResourceUnit::from_bytes("arab-res", "fonts/ArabFace.ttf", "font/ttf", &arab_ttf)
                .unwrap(),
        );
        let mut fmt_arab = TextFormat {
            font: "ArabFace".to_owned(),
            ..TextFormat::default()
        };
        fmt_arab.set_field_options(Some(AuthoredTextFieldOptions {
            selectable: false,
            ..AuthoredTextFieldOptions::default()
        }));
        add_text_box(&mut p_arab, "arab-box", "", "ب", fmt_arab);
        let b_arab = apply_font_policy(&mut p_arab, FontExportPolicy::Subset);
        assert!(!b_arab.subset_stats[0].subset_applied);
        assert!(
            b_arab.subset_stats[0]
                .diagnostic
                .as_deref()
                .is_some_and(|d| d.contains("Arabic"))
        );

        // 4. cmap Format 14 variation sequence conflict
        let mut uvs_cmap = Vec::new();
        uvs_cmap.extend_from_slice(&0u16.to_be_bytes());
        uvs_cmap.extend_from_slice(&1u16.to_be_bytes()); // numTables
        uvs_cmap.extend_from_slice(&0u16.to_be_bytes()); // platform 0
        uvs_cmap.extend_from_slice(&5u16.to_be_bytes()); // encoding 5 (UVS)
        uvs_cmap.extend_from_slice(&12u32.to_be_bytes()); // offset 12
        uvs_cmap.extend_from_slice(&14u16.to_be_bytes()); // format 14
        uvs_cmap.extend_from_slice(&21u32.to_be_bytes()); // length
        uvs_cmap.extend_from_slice(&1u32.to_be_bytes()); // numVarSelectorRecords
        uvs_cmap.extend_from_slice(&[0x00, 0xFE, 0x0F]); // U+FE0F
        uvs_cmap.extend_from_slice(&0u32.to_be_bytes()); // defaultUVSOffset
        uvs_cmap.extend_from_slice(&0u32.to_be_bytes()); // nonDefaultUVSOffset
        let uvs_ttf =
            build_sfnt_with_replaced_table(TEST_FONT, None, &[(*b"cmap", Some(uvs_cmap))]);
        let mut p_uvs = Project::blank();
        p_uvs.resources.push(
            ResourceUnit::from_bytes("uvs-res", "fonts/UvsFace.ttf", "font/ttf", &uvs_ttf).unwrap(),
        );
        let mut fmt_uvs = TextFormat {
            font: "UvsFace".to_owned(),
            ..TextFormat::default()
        };
        fmt_uvs.set_field_options(Some(AuthoredTextFieldOptions {
            selectable: false,
            ..AuthoredTextFieldOptions::default()
        }));
        add_text_box(&mut p_uvs, "uvs-box", "", "A\u{FE0F}", fmt_uvs);
        let b_uvs = apply_font_policy(&mut p_uvs, FontExportPolicy::Subset);
        assert!(!b_uvs.subset_stats[0].subset_applied);
        assert!(
            b_uvs.subset_stats[0]
                .diagnostic
                .as_deref()
                .is_some_and(|d| d.contains("variation sequences"))
        );

        // 5. Named Dynamic TextBox even with selectable: false must preserve complete font without declared ranges
        let mut p_named = Project::blank();
        add_font(&mut p_named, "named-font", "NamedDyn.ttf");
        let mut fmt_named = TextFormat {
            font: "NamedDyn".to_owned(),
            ..TextFormat::default()
        };
        fmt_named.set_field_options(Some(AuthoredTextFieldOptions {
            selectable: false,
            ..AuthoredTextFieldOptions::default()
        }));
        add_text_box(&mut p_named, "score-id", "score_txt", "0", fmt_named);
        let b_named = apply_font_policy(&mut p_named, FontExportPolicy::Subset);
        assert!(!b_named.subset_stats[0].subset_applied);
        assert!(
            b_named.subset_stats[0]
                .diagnostic
                .as_deref()
                .is_some_and(|d| d.contains("Dynamic/Input"))
        );
    }
}
