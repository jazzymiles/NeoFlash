//! Native binary authoring import. Source records remain embedded verbatim;
//! unsupported visual or executable records stop conversion with a location.
mod assets;
mod budget_shedding;
mod compiled_clip;
mod instance_props;
mod provenance_archive;
mod shape;
mod structural_limits;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_fidelity;
#[cfg(test)]
mod text;
mod text_convert;
mod timeline_elements;
mod timeline_frames;
mod tweens_convert;

use super::{BinaryTimelineDocument, contents::*, timeline::*};
use crate::ImportReport;
use neoflash_core::*;
use std::collections::BTreeMap;

pub(super) fn read_string(bytes: &[u8], value: &ContentsString) -> Result<String, String> {
    read_string_with_charset(bytes, value, None)
}

pub(super) fn read_string_with_charset(
    bytes: &[u8],
    value: &ContentsString,
    char_set: Option<u8>,
) -> Result<String, String> {
    let raw = bytes
        .get(value.data.start..value.data.end)
        .ok_or("String span outside source")?;
    if value.utf16 {
        if raw.len() % 2 != 0 {
            return Err("Odd UTF-16 byte count".into());
        }
        String::from_utf16(
            &raw.as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| "Invalid UTF-16 source".into())
    } else {
        Ok(decode_ansi_with_charset(raw, char_set))
    }
}

fn read_text_run_string(
    bytes: &[u8],
    value: &ContentsString,
    char_set: Option<u8>,
) -> Result<String, String> {
    let raw = bytes
        .get(value.data.start..value.data.end)
        .ok_or("String span outside source")?;
    if value.utf16 {
        if raw.len() % 2 != 0 {
            return Err("Odd UTF-16 byte count".into());
        }
        Ok(char::decode_utf16(
            raw.as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]])),
        )
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect())
    } else {
        Ok(decode_ansi_with_charset(raw, char_set))
    }
}

fn read_action_source_string(
    bytes: &[u8],
    revision: Option<u32>,
    value: &ContentsString,
) -> Result<String, String> {
    if revision == Some(3) && value.utf16 {
        let raw = bytes
            .get(value.data.start..value.data.end)
            .ok_or("String span outside source")?;
        let even_raw = &raw[..raw.len() & !1];
        return Ok(char::decode_utf16(
            even_raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]])),
        )
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect());
    }
    read_string(bytes, value)
}

thread_local! {
    /// Code pages guessed while converting the current document (bit 0: 1251, bit 1: 1252).
    static ANSI_GUESSES: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    /// Deterministic document-level ANSI/MBCS code page inferred from font `lfCharSet` or `[MS-OLEPS]`.
    static DOC_ANSI_ENCODING: std::cell::Cell<Option<&'static encoding_rs::Encoding>> =
        const { std::cell::Cell::new(None) };
}

struct AnsiDecodeScope;
impl Drop for AnsiDecodeScope {
    fn drop(&mut self) {
        DOC_ANSI_ENCODING.with(|e| e.set(None));
    }
}

/// Legacy writers stored 8-bit text in the authoring system's ANSI/MBCS code page.
/// When a run's `LOGFONT.lfCharSet` or unambiguous document font/summary metadata
/// identifies a deterministic `encoding_rs` encoding without replacement errors, use
/// it directly; otherwise fall back to Cyrillic (`Windows-1251`) vs Western
/// (`Windows-1252`) content heuristics and report `fla_ansi_code_page`.
fn decode_ansi_with_charset(raw: &[u8], char_set: Option<u8>) -> String {
    if raw.is_ascii() {
        return String::from_utf8(raw.to_vec()).unwrap();
    }
    if let Some(enc) = char_set.and_then(assets::encoding_from_gdi_charset) {
        let (cow, had_errors) = enc.decode_without_bom_handling(raw);
        if !had_errors {
            return cow.into_owned();
        }
    }
    if let Some(enc) = DOC_ANSI_ENCODING.with(std::cell::Cell::get) {
        let (cow, had_errors) = enc.decode_without_bom_handling(raw);
        if !had_errors {
            return cow.into_owned();
        }
    }
    let letters = raw
        .iter()
        .filter(|b| b.is_ascii_alphabetic() || **b >= 0xc0)
        .count();
    let high = raw
        .iter()
        .filter(|b| **b >= 0xc0 || matches!(**b, 0xa8 | 0xb8))
        .count();
    let has_yo = raw.iter().any(|b| matches!(*b, 0xa8 | 0xb8));
    let cyrillic = (high >= 2 || has_yo) && high * 10 >= letters * 3;
    ANSI_GUESSES.with(|g| g.set(g.get() | if cyrillic { 1 } else { 2 }));
    let encoding = if cyrillic {
        encoding_rs::WINDOWS_1251
    } else {
        encoding_rs::WINDOWS_1252
    };
    encoding.decode_without_bom_handling(raw).0.into_owned()
}
fn sid(index: usize) -> String {
    format!("fla-symbol-{index}")
}
fn lid(index: usize, id: u32) -> String {
    format!("fla-layer-{index}-{id}")
}
fn object(stream: &BinaryTimelineStream, id: u32) -> Result<&TimelineObject, String> {
    stream
        .objects
        .binary_search_by_key(&id, |o| o.object_id)
        .ok()
        .map(|i| &stream.objects[i])
        .ok_or_else(|| format!("Unresolved object #{id}"))
}
fn optional_text(bytes: &[u8], span: Option<&ContentsString>) -> Result<String, String> {
    span.map(|v| read_string(bytes, v))
        .transpose()
        .map(Option::unwrap_or_default)
}
fn optional_text_with_charset(
    bytes: &[u8],
    span: Option<&ContentsString>,
    char_set: Option<u8>,
) -> Result<String, String> {
    span.map(|v| read_string_with_charset(bytes, v, char_set))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(super) fn convert_blend_mode(
    raw: Option<u8>,
    location: &str,
    report: &mut ImportReport,
) -> BlendMode {
    let Some(code) = raw else {
        return BlendMode::Normal;
    };
    let Some(decoded) = TimelineBlendMode::from_archive(code) else {
        report.warning(
            "fla_blend_mode",
            location,
            format!(
                "Unknown blend mode code {code} is drawn normally; the raw value remains in the embedded source."
            ),
        );
        return BlendMode::Normal;
    };
    let blend = match decoded {
        TimelineBlendMode::Normal => BlendMode::Normal,
        TimelineBlendMode::Layer => BlendMode::Layer,
        TimelineBlendMode::Multiply => BlendMode::Multiply,
        TimelineBlendMode::Screen => BlendMode::Screen,
        TimelineBlendMode::Lighten => BlendMode::Lighten,
        TimelineBlendMode::Darken => BlendMode::Darken,
        TimelineBlendMode::Difference => BlendMode::Difference,
        TimelineBlendMode::Add => BlendMode::Add,
        TimelineBlendMode::Subtract => BlendMode::Subtract,
        TimelineBlendMode::Invert => BlendMode::Invert,
        TimelineBlendMode::Alpha => BlendMode::Alpha,
        TimelineBlendMode::Erase => BlendMode::Erase,
        TimelineBlendMode::Overlay => BlendMode::Overlay,
        TimelineBlendMode::Hardlight => BlendMode::Hardlight,
    };
    if !blend.has_canvas_compositing() {
        report.warning(
            "fla_blend_mode",
            location,
            format!(
                "Blend mode '{}' is stored on the instance, while HTML5 Canvas 2D approximates it as normal compositing.",
                blend.label()
            ),
        );
    }
    blend
}

pub(super) fn convert_filters(
    filters: &[TimelineFilter],
    location: &str,
    report: &mut ImportReport,
) -> Vec<FilterSpec> {
    if filters.is_empty() {
        return Vec::new();
    }
    let unpack_rgba = |word: u32| -> (u32, f64) {
        let [r, g, b, a] = word.to_le_bytes();
        let rgb = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        (rgb, f64::from(a) / 255.0)
    };
    let finite_clamp = |v: Option<f32>, default: f64, lo: f64, hi: f64| -> f64 {
        v.map(f64::from)
            .filter(|x| x.is_finite())
            .unwrap_or(default)
            .clamp(lo, hi)
    };
    let angle_deg = |rad: Option<f32>| -> f64 {
        rad.map(|r| f64::from(r).to_degrees())
            .filter(|d| d.is_finite())
            .unwrap_or(45.0)
            .clamp(-1e9, 1e9)
    };
    let placement_str = |p: &TimelineFilterParams| -> String {
        if p.full.unwrap_or(0) != 0 {
            "full".into()
        } else if p.inner.unwrap_or(0) != 0 {
            "inner".into()
        } else {
            "outer".into()
        }
    };
    let gradient_stops = |stops: &[TimelineFilterGradientStop]| -> (Vec<u32>, Vec<f64>, Vec<u8>) {
        let mut entries: Vec<(u8, u32, f64)> = stops
            .iter()
            .take(16)
            .map(|s| {
                let (rgb, alpha) = unpack_rgba(s.color_rgba);
                (s.ratio.min(255) as u8, rgb, alpha)
            })
            .collect();
        entries.sort_by_key(|e| e.0);
        (
            entries.iter().map(|e| e.1).collect(),
            entries.iter().map(|e| e.2).collect(),
            entries.iter().map(|e| e.0).collect(),
        )
    };

    let mut out = Vec::new();
    let mut empty_revision_count = 0usize;
    let mut truncated_count = 0usize;
    let mut clamped_convolution_count = 0usize;

    for f in filters {
        if f.revision == 0
            || (matches!(
                f.kind,
                TimelineFilterKind::GradientGlow | TimelineFilterKind::GradientBevel
            ) && f.params.gradient_revision == Some(0))
        {
            empty_revision_count += 1;
            continue;
        }
        if out.len() >= MAX_OBJECT_FILTERS {
            truncated_count += 1;
            continue;
        }
        let p = &f.params;
        let blur_x = finite_clamp(p.blur_x, 4.0, 0.0, 255.0);
        let blur_y = finite_clamp(p.blur_y.or(p.blur_x), blur_x, 0.0, 255.0);
        let quality = p.quality.unwrap_or(1).min(15) as u8;
        let strength = (f64::from(p.strength_percent.unwrap_or(100)) / 100.0).clamp(0.0, 255.0);
        let inner = p.inner.unwrap_or(0) != 0;
        let knockout = p.knockout.unwrap_or(0) != 0;

        let spec = match f.kind {
            TimelineFilterKind::Blur => FilterSpec::Blur {
                blur_x,
                blur_y,
                quality,
            },
            TimelineFilterKind::Glow => {
                let (color, alpha) = unpack_rgba(p.color_rgba.unwrap_or(0xff00_00ff));
                FilterSpec::Glow {
                    color,
                    alpha,
                    blur_x,
                    blur_y,
                    strength,
                    quality,
                    inner,
                    knockout,
                }
            }
            TimelineFilterKind::DropShadow => {
                let (color, alpha) = unpack_rgba(p.color_rgba.unwrap_or(0xff00_0000));
                FilterSpec::DropShadow {
                    distance: finite_clamp(p.distance, 4.0, -8192.0, 8192.0),
                    angle: angle_deg(p.angle_radians),
                    color,
                    alpha,
                    blur_x,
                    blur_y,
                    strength,
                    quality,
                    inner,
                    knockout,
                    hide_object: p.hide_object.unwrap_or(0) != 0,
                }
            }
            TimelineFilterKind::Bevel => {
                let (shadow_color, shadow_alpha) = unpack_rgba(p.color_rgba.unwrap_or(0xff00_0000));
                let (highlight_color, highlight_alpha) =
                    unpack_rgba(p.highlight_color_rgba.unwrap_or(0xffff_ffff));
                FilterSpec::Bevel {
                    distance: finite_clamp(p.distance, 4.0, -8192.0, 8192.0),
                    angle: angle_deg(p.angle_radians),
                    highlight_color,
                    highlight_alpha,
                    shadow_color,
                    shadow_alpha,
                    blur_x,
                    blur_y,
                    strength,
                    quality,
                    placement: placement_str(p),
                    knockout,
                }
            }
            TimelineFilterKind::GradientGlow => {
                let (colors, alphas, ratios) = gradient_stops(&p.gradient_stops);
                FilterSpec::GradientGlow {
                    distance: finite_clamp(p.distance, 4.0, -8192.0, 8192.0),
                    angle: angle_deg(p.angle_radians),
                    colors,
                    alphas,
                    ratios,
                    blur_x,
                    blur_y,
                    strength,
                    quality,
                    placement: placement_str(p),
                    knockout,
                }
            }
            TimelineFilterKind::GradientBevel => {
                let (colors, alphas, ratios) = gradient_stops(&p.gradient_stops);
                FilterSpec::GradientBevel {
                    distance: finite_clamp(p.distance, 4.0, -8192.0, 8192.0),
                    angle: angle_deg(p.angle_radians),
                    colors,
                    alphas,
                    ratios,
                    blur_x,
                    blur_y,
                    strength,
                    quality,
                    placement: placement_str(p),
                    knockout,
                }
            }
            TimelineFilterKind::Convolution => {
                let mx = p.matrix_x.unwrap_or(0);
                let my = p.matrix_y.unwrap_or(0);
                let mut clamped = false;
                let mut matrix = Vec::with_capacity(p.convolution_matrix.len());
                for &coeff in &p.convolution_matrix {
                    let v = f64::from(coeff);
                    if !v.is_finite() {
                        clamped = true;
                        matrix.push(0.0);
                    } else if v.abs() > 65_536.0 {
                        clamped = true;
                        matrix.push(v.clamp(-65_536.0, 65_536.0));
                    } else {
                        matrix.push(v);
                    }
                }
                let raw_div = f64::from(p.divisor.unwrap_or(1.0));
                let divisor = if !raw_div.is_finite() {
                    clamped = true;
                    1.0
                } else if raw_div == 0.0 {
                    let sum: f64 = matrix.iter().sum();
                    if sum.is_finite() && sum.abs() > 1e-9 && sum.abs() <= 65_536.0 {
                        sum
                    } else {
                        1.0
                    }
                } else if raw_div.abs() > 65_536.0 {
                    clamped = true;
                    raw_div.clamp(-65_536.0, 65_536.0)
                } else {
                    raw_div
                };
                let raw_bias = f64::from(p.bias.unwrap_or(0.0));
                let bias = if !raw_bias.is_finite() {
                    clamped = true;
                    0.0
                } else if raw_bias.abs() > 65_536.0 {
                    clamped = true;
                    raw_bias.clamp(-65_536.0, 65_536.0)
                } else {
                    raw_bias
                };
                if clamped {
                    clamped_convolution_count += 1;
                }
                let (color, alpha) = unpack_rgba(p.color_rgba.unwrap_or(0));
                let flags = p.convolution_flags.unwrap_or(0x03);
                FilterSpec::Convolution {
                    matrix_x: mx,
                    matrix_y: my,
                    matrix,
                    divisor,
                    bias,
                    preserve_alpha: (flags & 0x01) != 0,
                    clamp: (flags & 0x02) != 0,
                    color,
                    alpha,
                }
            }
            TimelineFilterKind::AdjustColor => FilterSpec::ColorMatrix {
                matrix: adjust_color_matrix(
                    finite_clamp(p.brightness, 0.0, -100.0, 100.0),
                    finite_clamp(p.contrast, 0.0, -100.0, 100.0),
                    finite_clamp(p.saturation, 0.0, -100.0, 100.0),
                    finite_clamp(p.hue, 0.0, -180.0, 180.0),
                ),
            },
        }
        .with_enabled(f.enabled != Some(false));
        if spec.validate().is_ok() {
            out.push(spec);
        }
    }

    if empty_revision_count > 0 || truncated_count > 0 || clamped_convolution_count > 0 {
        let mut notes = Vec::new();
        if empty_revision_count > 0 {
            notes.push(format!(
                "{empty_revision_count} empty revision-0 filter(s) skipped"
            ));
        }
        if truncated_count > 0 {
            notes.push(format!(
                "{truncated_count} filter(s) beyond the {MAX_OBJECT_FILTERS}-filter limit omitted"
            ));
        }
        if clamped_convolution_count > 0 {
            notes.push(format!(
                "{clamped_convolution_count} convolution filter(s) clamped to +/-65536"
            ));
        }
        report.warning(
            "fla_filter_approximated",
            location,
            format!(
                "Display filters imported with approximations ({}); original records remain in the embedded source.",
                notes.join(", ")
            ),
        );
    }

    out
}

pub(crate) fn convert(
    document: &BinaryTimelineDocument,
    report: &mut ImportReport,
) -> Option<Project> {
    report.binary_fla = Some(document.inspection.clone());
    // Streams that stop early are reported here. Timeline content read before
    // the stop is still converted; what follows stays only in the embedded source.
    for s in &document.inspection.streams {
        let (Some(code), Some(message)) = (&s.stop_code, &s.stop_message) else {
            continue;
        };
        let location = format!("CFB stream #{} @0x{:x}", s.index, s.bytes_consumed);
        match s.role {
            super::BinaryStreamRole::Media => {}
            super::BinaryStreamRole::Other => report.warning(
                "fla_stream_retained",
                location,
                "Unrecognized auxiliary stream; it is retained unchanged in the embedded source.",
            ),
            super::BinaryStreamRole::Contents => report.warning(code, location, message),
            _ => report.warning(
                code,
                location,
                format!(
                    "{message} Records before this point are imported; the rest of this timeline remains only in the embedded source."
                ),
            ),
        }
    }
    ANSI_GUESSES.with(|g| g.set(0));
    DOC_ANSI_ENCODING.with(|e| e.set(assets::infer_deterministic_document_encoding(document)));
    let _ansi_scope = AnsiDecodeScope;
    let built = build(document, report);
    let guesses = ANSI_GUESSES.with(|g| g.replace(0));
    if guesses != 0 {
        let pages = match guesses {
            1 => "Windows-1251 (Cyrillic)",
            2 => "Windows-1252 (Western)",
            _ => "Windows-1251 (Cyrillic) or Windows-1252 (Western) per string",
        };
        report.warning("fla_ansi_code_page","Legacy text",format!("8-bit names, text and scripts were decoded as {pages}, guessed from their content because the FLA does not record its code page. Original bytes remain in the embedded source."));
    }
    collapse_repeated_warnings(report);
    match built {
        Ok(project) => {
            if let Some(binary) = &mut report.binary_fla {
                for stream in &mut binary.streams {
                    if stream.role == super::BinaryStreamRole::Media {
                        stream.stop_code = None;
                        stream.stop_message = None;
                        stream.bytes_consumed = stream.byte_length as usize;
                        stream.structure_complete = true;
                    }
                }
            }
            Some(project)
        }
        Err(message) => {
            report.error("fla_semantic_conversion", "Binary FLA", message);
            None
        }
    }
}

/// Keep one entry per repeated warning code, with the count and first location,
/// so large documents produce a readable report. Errors are never merged.
fn collapse_repeated_warnings(report: &mut ImportReport) {
    let mut counts = BTreeMap::<String, usize>::new();
    for d in &report.diagnostics {
        if d.severity == crate::Severity::Warning {
            *counts.entry(d.code.clone()).or_default() += 1;
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    report.diagnostics.retain_mut(|d| {
        let n = counts.get(&d.code).copied().unwrap_or(1);
        if d.severity != crate::Severity::Warning || n <= 3 {
            return true;
        }
        if !seen.insert(d.code.clone()) {
            return false;
        }
        d.location = format!("{} places, first at {}", n, d.location);
        true
    });
}

fn build(document: &BinaryTimelineDocument, report: &mut ImportReport) -> Result<Project, String> {
    let c = &document.contents;
    // The library descriptors and document settings precede every optional
    // Contents suffix; without them there is no document to build.
    let settings = c
        .settings
        .as_ref()
        .ok_or("Contents stops before the library descriptors and document settings")?;
    if c.stop.is_some() {
        report.warning("fla_contents_partial","Contents","Library and document settings were read; later Contents records (publish profiles, folders, metadata) use defaults and remain in the embedded source.");
    }
    if c.extensions.as_ref().is_some_and(|e| e.profiles.len() > 1) {
        report.warning(
            "fla_publish_profile",
            "Contents",
            "Several publish profiles exist; the first profile supplies language and hidden-layer settings.",
        );
    }
    let mut scene_indices = Vec::new();
    for id in &c.scenes {
        let descriptor = c
            .descriptors
            .iter()
            .find(|d| d.object_id == *id)
            .ok_or("Missing scene descriptor")?;
        scene_indices.push(descriptor.stream_index.ok_or("Missing scene stream")?);
    }
    let root_index = *scene_indices.first().ok_or("The document has no scene")?;
    let has_any_scene_root = scene_indices.iter().any(|&idx| {
        document
            .streams
            .iter()
            .find(|s| s.index == idx)
            .and_then(|s| s.root.and_then(|r| object(s, r).ok()))
            .is_some()
    });
    if !has_any_scene_root {
        return Err("Missing root timeline object".into());
    }
    let size = settings.size_twips().ok_or("Invalid stage bounds")?;
    if settings.bounds_words[0] != 0 || settings.bounds_words[2] != 0 {
        report.warning("fla_stage_origin","Document settings","The stage bounds start away from 0,0; content keeps its authored coordinates relative to a stage at the origin.");
    }
    if size.iter().any(|s| s % 20 != 0) || settings.frame_rate().fract() != 0. {
        report.warning("fla_stage_rounded","Document settings",format!("Stage size or frame rate is fractional ({} x {} px, {} fps); the project uses the nearest whole values.",size[0] as f64/20.,size[1] as f64/20.,settings.frame_rate()));
    }
    let language = match c
        .publish_property(0, "Vector::ActionScriptVersion")
        .as_deref()
    {
        Some("1") => ScriptLanguage::ActionScript1,
        Some("2") => ScriptLanguage::ActionScript2,
        Some("3") => ScriptLanguage::ActionScript3,
        None if c.revision.is_some_and(|r| r <= 0x29) => ScriptLanguage::ActionScript1,
        other => {
            report.warning("fla_script_language","Publish settings",format!("ActionScript version {:?} is not recorded in a supported form; scripts are imported as ActionScript 2.",other));
            ScriptLanguage::ActionScript2
        }
    };
    let include_hidden = match c.publish_property(0, "Vector::Invisible Layer").as_deref() {
        Some("0") => false,
        Some("1") | None => true,
        Some(other) => {
            report.warning(
                "fla_publish_hidden_layers",
                "Publish settings",
                format!(
                    "Unknown hidden-layer publish flag {other:?}; hidden layers are published."
                ),
            );
            true
        }
    };
    let (folders, folder_paths) = match assets::folders(c) {
        Ok((mut list, paths)) => {
            if list.len() > 10_000 {
                report.warning(
                    "fla_library_folder",
                    "Contents",
                    format!(
                        "Library contains {} folders (limit 10000); excess folder entries are omitted.",
                        list.len()
                    ),
                );
                list.truncate(10_000);
            }
            (list, paths)
        }
        Err(reason) => {
            report.warning(
                "fla_library_folder",
                "Contents",
                format!(
                    "Library folder hierarchy could not be reconstructed ({reason}); items are placed at the library root."
                ),
            );
            (Vec::new(), BTreeMap::new())
        }
    };
    let mut library = BTreeMap::new();
    let mut library_kinds = BTreeMap::new();
    let mut reserved_ids = std::collections::BTreeSet::from([
        "root".to_owned(),
        "fla-original".to_owned(),
        "fla-provenance".to_owned(),
        PLACEHOLDER.to_owned(),
    ]);
    for &sidx in &scene_indices {
        reserved_ids.insert(sid(sidx));
    }
    for d in &c.descriptors {
        if d.class != ContentsDescriptorClass::DocumentPage || c.scenes.contains(&d.object_id) {
            continue;
        }
        let sym_kind = match d.symbol_type {
            Some(0) => SymbolKind::Graphic,
            Some(1) => SymbolKind::Button,
            _ => SymbolKind::MovieClip,
        };
        if let Some(id) = d.library_id {
            let sym_id = match d.stream_index {
                Some(index) => sid(index),
                None => format!("fla-symbol-missing-{}", d.object_id),
            };
            reserved_ids.insert(sym_id.clone());
            library_kinds.insert(id, sym_kind);
            if library.insert(id, sym_id).is_some() {
                return Err("Duplicate library ID".into());
            }
        }
    }
    let assets::ExtractedAssets {
        mut resources,
        bitmaps,
        sounds,
        videos,
        compiled_clips,
        skipped_bitmaps,
    } = assets::resources(document, &folder_paths, &reserved_ids, language, report)?;
    let scene_stream_indices: std::collections::BTreeSet<usize> =
        scene_indices.iter().copied().collect();
    let mut implemented_symbols = std::collections::BTreeSet::new();
    for d in &c.descriptors {
        if d.class != ContentsDescriptorClass::DocumentPage || c.scenes.contains(&d.object_id) {
            continue;
        }
        if matches!(d.symbol_type, Some(0) | Some(1)) {
            continue;
        }
        let sym_id = match d.stream_index {
            Some(index) => sid(index),
            None => continue,
        };
        let linkage = d
            .linkage
            .as_ref()
            .or_else(|| d.library_metadata.as_ref().and_then(|m| m.linkage.as_ref()));
        let has_class = linkage
            .and_then(|l| l.class_name.as_ref())
            .and_then(|cn| read_string(&c.original_bytes, cn).ok())
            .is_some_and(|s| !s.trim().is_empty());
        let has_stream_actions = d
            .stream_index
            .and_then(|idx| document.streams.iter().find(|s| s.index == idx))
            .is_some_and(stream_has_executable_actions);
        if has_class || has_stream_actions {
            implemented_symbols.insert(sym_id);
        }
    }
    loop {
        let mut changed = false;
        for d in &c.descriptors {
            if d.class != ContentsDescriptorClass::DocumentPage
                || c.scenes.contains(&d.object_id)
                || matches!(d.symbol_type, Some(0) | Some(1))
            {
                continue;
            }
            let Some(index) = d.stream_index else {
                continue;
            };
            let sym_id = sid(index);
            if implemented_symbols.contains(&sym_id) {
                continue;
            }
            let Some(stream) = document.streams.iter().find(|s| s.index == index) else {
                continue;
            };
            let has_implemented_child = stream.objects.iter().any(|o| match &o.payload {
                Some(TimelinePayload::Instance(inst)) => inst
                    .library_id
                    .and_then(|lid| library.get(&lid))
                    .is_some_and(|cid| implemented_symbols.contains(cid)),
                Some(TimelinePayload::CompiledClip(cc)) => cc
                    .media_id
                    .and_then(|mid| compiled_clips.get(&mid))
                    .is_some_and(|ca| ca.has_avm1_scripts),
                _ => false,
            });
            if has_implemented_child {
                implemented_symbols.insert(sym_id);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let stream_stops: std::collections::BTreeSet<usize> = document
        .inspection
        .streams
        .iter()
        .filter(|s| s.stop_code.is_some())
        .map(|s| s.index)
        .collect();
    let (view_angle_3d, vanishing_point_3d) = parse_contents_3d_perspective(c);
    let stage_size_px = [size[0] as f64 / 20.0, size[1] as f64 / 20.0];
    let mut context = Context {
        report,
        library,
        library_kinds,
        implemented_symbols,
        scene_stream_indices,
        bitmaps,
        sounds,
        videos,
        compiled_clips,
        skipped_bitmaps,
        stream_stops,
        language,
        include_hidden,
        motion_pose_budget: crate::motion_xml::DOCUMENT_POSE_BUDGET,
        contents_revision: c.revision,
        motion_3d: (stage_size_px, view_angle_3d, vanishing_point_3d),
        generated: vec![],
        inline_resources: vec![],
        compiled_clip_sources: vec![],
        provenance: vec![],
        missing_bitmaps: BTreeMap::new(),
    };
    let mut symbols = Vec::new();
    let mut seen_export_ids = std::collections::HashSet::new();
    let mut seen_export_classes = std::collections::HashSet::new();
    for d in &c.descriptors {
        if d.class != ContentsDescriptorClass::DocumentPage {
            continue;
        }
        let name = read_string(&c.original_bytes, &d.name)?;
        let is_scene = d.stream_index.is_some_and(|i| scene_indices.contains(&i));
        let kind = if is_scene {
            SymbolKind::MovieClip
        } else {
            match d.symbol_type {
                Some(0) => SymbolKind::Graphic,
                Some(1) => SymbolKind::Button,
                Some(2) => SymbolKind::MovieClip,
                other => {
                    context.report.warning(
                        "fla_symbol_type",
                        &name,
                        format!("Unknown symbol type {other:?}; imported as a MovieClip."),
                    );
                    SymbolKind::MovieClip
                }
            }
        };
        let parent = d
            .library_metadata
            .as_ref()
            .and_then(|m| m.parent)
            .filter(|id| *id != [0, 0]);
        let folder = if d.stream_index == Some(root_index) {
            String::new()
        } else {
            match parent.map(|id| folder_paths.get(&id)) {
                Some(Some(path)) if valid_library_folder(path) => path.clone(),
                Some(Some(_)) | Some(None) => {
                    context.report.warning(
                        "fla_library_folder",
                        &name,
                        "Library folder record is unresolved; the item is placed at the library root.",
                    );
                    String::new()
                }
                None => String::new(),
            }
        };
        let linkage = d
            .linkage
            .as_ref()
            .or_else(|| d.library_metadata.as_ref().and_then(|m| m.linkage.as_ref()));
        let mut linkage_id = linkage
            .map(|l| read_string(&c.original_bytes, &l.identifier))
            .transpose()?
            .filter(|v| !v.is_empty());
        let mut class_name = optional_text(
            &c.original_bytes,
            linkage.and_then(|l| l.class_name.as_ref()),
        )
        .map(|s| (!s.is_empty()).then_some(s))?;
        if let Some(lid_val) = &linkage_id {
            let valid_export_target = !is_scene
                && d.stream_index != Some(root_index)
                && matches!(kind, SymbolKind::MovieClip | SymbolKind::Button)
                && lid_val.len() <= 512
                && !lid_val.chars().any(char::is_control)
                && seen_export_ids.insert(lid_val.clone());
            if !valid_export_target {
                context.report.warning(
                    "fla_linkage_approximated",
                    &name,
                    "Symbol linkage identifier is invalid, duplicate or attached to a scene/Graphic symbol and was omitted.",
                );
                linkage_id = None;
            }
        }
        if let Some(cls) = &class_name {
            let valid_cls = is_valid_qualified_class_name(cls)
                && (linkage_id.is_none() || seen_export_classes.insert(cls.clone()));
            if !valid_cls {
                context.report.warning(
                    "fla_linkage_approximated",
                    &name,
                    "Symbol ActionScript class name is invalid or duplicated across exported symbols and was omitted.",
                );
                class_name = None;
            }
        }
        let (sym_id, timeline) = match d.stream_index.and_then(|index| {
            document
                .streams
                .iter()
                .find(|s| s.index == index)
                .map(|s| (index, s))
        }) {
            Some((index, stream)) => (sid(index), context.timeline(stream)?),
            None if !is_scene => {
                context.report.warning(
                    "fla_timeline_empty",
                    &name,
                    "Library symbol has no resolved timeline stream in the archive; imported as an empty 1-frame symbol.",
                );
                let sym_id = match d.stream_index {
                    Some(index) => sid(index),
                    None => format!("fla-symbol-missing-{}", d.object_id),
                };
                (sym_id.clone(), empty_timeline(&format!("{sym_id}-layer")))
            }
            None => return Err("Missing scene timeline stream".into()),
        };
        symbols.push(Symbol {
            id: sym_id,
            name,
            folder,
            kind,
            timeline,
            class_name,
            linkage_id,
        });
    }
    symbols.append(&mut context.generated);
    for inline_res in context.inline_resources.drain(..) {
        let used_total: usize = resources.iter().map(ResourceUnit::byte_len).sum();
        if resources.len() < MAX_RESOURCES - 2
            && used_total
                .checked_add(inline_res.byte_len())
                .is_some_and(|n| n <= MAX_RESOURCE_TOTAL_BYTES - 512 * 1024)
        {
            resources.push(inline_res);
        }
    }
    normalize_playback(&mut symbols, context.report);
    attach_companion_includes(&context.report.source, &symbols, &mut resources);
    if !document.inspection.container_tolerances.is_empty() {
        context.report.warning("fla_container_compatibility","CFB directory",format!("The source container has non-canonical metadata outside its live streams ({}). Streams were read from their original directory chains.",document.inspection.container_tolerances.join("; ")))
    }
    let mut imported_sources = Vec::new();
    let mut seen_source_paths = std::collections::HashSet::new();
    if let Some(ext) = &c.extensions {
        for (idx, prop) in ext.document_properties.iter().enumerate() {
            let Some(key) = c.string_text(&prop.key) else {
                continue;
            };
            let Some(rest) = key.strip_prefix("NeoFlash::Source::") else {
                continue;
            };
            let Some((lang_tag, rel_path)) = rest.split_once("::") else {
                continue;
            };
            if rel_path.is_empty()
                || rel_path.len() > 1024
                || rel_path.contains(['\\', '\0', ':'])
                || rel_path
                    .split('/')
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
                || !seen_source_paths.insert(rel_path.to_lowercase())
            {
                continue;
            }
            let Some(source_text) = c.string_text(&prop.value) else {
                continue;
            };
            let src_lang = match lang_tag {
                "as1" => ScriptLanguage::ActionScript1,
                "as2" => ScriptLanguage::ActionScript2,
                "as3" => ScriptLanguage::ActionScript3,
                "js" => ScriptLanguage::JavaScript,
                "ts" => ScriptLanguage::TypeScript,
                _ => language,
            };
            imported_sources.push(SourceUnit {
                id: format!("fla-source-{}", idx + 1),
                path: rel_path.to_owned(),
                language: src_lang,
                source: source_text,
            });
        }
    }
    for src in context.compiled_clip_sources {
        if !src.path.is_empty()
            && src.path.len() <= 1024
            && !src.path.contains(['\\', '\0', ':'])
            && !src
                .path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
            && seen_source_paths.insert(src.path.to_lowercase())
        {
            imported_sources.push(src);
        }
    }
    let document_class = c
        .publish_property(0, "Vector::DocumentClass")
        .or_else(|| c.publish_property(0, "DocumentClass"))
        .map(|s| s.trim().to_owned())
        .filter(|name| is_valid_qualified_class_name(name));
    if let Some(root_sym) = symbols.iter_mut().find(|s| s.id == sid(root_index))
        && root_sym.class_name.is_some()
        && document_class.is_some()
        && root_sym.class_name != document_class
    {
        root_sym.class_name = None;
    }
    let raw_width = ((size[0] + 10) / 20).max(1);
    let raw_height = ((size[1] + 10) / 20).max(1);
    let raw_fps = if settings.frame_rate().is_finite() {
        (settings.frame_rate().round() as i64).max(1)
    } else {
        24
    };
    if raw_width > 65_535 || raw_height > 65_535 || raw_fps > 1_000 {
        context.report.warning(
            "fla_stage_clamped",
            "Document settings",
            format!(
                "Stage size or frame rate ({raw_width} x {raw_height} px, {raw_fps} fps) exceeds project bounds and was clamped to 1..=65535 px and 1..=1000 fps."
            ),
        );
    }
    let mut project = Project {
        format_version: 5,
        name: std::path::Path::new(&context.report.source)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        width: raw_width.clamp(1, 65_535) as u32,
        height: raw_height.clamp(1, 65_535) as u32,
        fps: raw_fps.clamp(1, 1_000) as u32,
        background: shape::color(settings.background_rgba_word),
        root: sid(root_index),
        symbols,
        library_folders: folders,
        sources: imported_sources,
        resources,
        document_class,
        scenes: if scene_indices.len() > 1 {
            scene_indices.iter().map(|i| sid(*i)).collect()
        } else {
            vec![]
        },
        script_language: Some(context.language),
        also_compile_compatible_scripts: true,
    };
    enforce_project_structural_limits(&mut project, &mut context.provenance, context.report);
    // Size the converted document first: the original shares both the resource
    // budget and the project's encoded JSON limit with the converted content.
    // The normal saver uses compact JSON when indentation would exceed the file
    // limit. Measure that representation before omitting any source or artwork.
    let library_fonts: Vec<_> = c
        .extensions
        .iter()
        .flat_map(|ext| &ext.fonts)
        .map(|font| {
            let cs = font.style.char_set;
            let label =
                |s: Option<&ContentsString>| optional_text_with_charset(&c.original_bytes, s, cs);
            Ok::<_, String>(serde_json::json!({
                "name": label(Some(&font.name))?,
                "id": font.font_id,
                "family": label(font.style.font_family.as_ref())?,
                "face": label(font.style.font_face.as_ref())?,
                "bold": font.style.bold,
                "italic": font.style.italic,
                "flags": font.flags,
                "linkage": optional_text(
                    &c.original_bytes,
                    font.metadata.linkage.as_ref().map(|l| &l.identifier),
                )?,
                "record": font.encoded,
            }))
        })
        .collect::<Result<_, _>>()?;
    if !library_fonts.is_empty() {
        context.report.warning("fla_font_library", "Contents / font library",
            format!("{} font library descriptor(s) retained in provenance. Text uses the installed family; the descriptor supplies no portable font file.", library_fonts.len()));
    }
    let source_len = document.original_bytes.len();
    let source_digest = format!("{:016x}", fnv1a64(&document.original_bytes));
    let archive_budget =
        |proj: &Project, placements: &[serde_json::Value]| -> Result<(usize, usize), String> {
            let doc_json = serde_json::to_string(proj)
                .map_err(|e| e.to_string())?
                .len();
            let used_bytes: usize = proj.resources.iter().map(ResourceUnit::byte_len).sum();
            let prov_len = serde_json::to_vec(&serde_json::json!({
                "descriptors": &c.descriptors,
                "folders": c.extensions.as_ref().map(|e| &e.folders),
                "placements": placements,
                "library_fonts": &library_fonts,
            }))
            .map_err(|e| e.to_string())?
            .len();
            // Reserve room for the provenance resource and its scalar fields, the
            // source part IDs, headers and alignment at individual Base64 boundaries.
            let prov_bytes = prov_len.saturating_add(8192);
            let json_left = MAX_PROJECT_JSON_BYTES
                .saturating_sub(doc_json)
                .saturating_sub(prov_bytes.div_ceil(3) * 4)
                .saturating_sub(1 << 20);
            let res_budget = MAX_RESOURCE_TOTAL_BYTES
                .saturating_sub(used_bytes)
                .saturating_sub(prov_bytes)
                .min(
                    MAX_RESOURCES
                        .saturating_sub(proj.resources.len() + 1)
                        .saturating_mul(MAX_RESOURCE_BYTES),
                );
            let budget = res_budget.min(json_left / 4 * 3);
            Ok((budget, res_budget))
        };
    let (mut budget, mut res_budget) = archive_budget(&project, &context.provenance)?;
    if source_len > budget && deduplicate_project_for_budget(&mut project, &mut context.provenance)
    {
        (budget, res_budget) = archive_budget(&project, &context.provenance)?;
    }
    enum SourceArchiveEncoding {
        Raw,
        Gzip(Vec<u8>),
        ResourceDeltaGzip {
            data: Vec<u8>,
            reference_resources: Vec<String>,
        },
        External,
    }
    let compress_within =
        |cap: usize, resources: &[ResourceUnit]| -> Result<SourceArchiveEncoding, String> {
            use std::io::Read;
            let mut data = Vec::new();
            flate2::read::GzEncoder::new(
                document.original_bytes.as_slice(),
                flate2::Compression::fast(),
            )
            .take(cap as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
            if data.len() <= cap {
                Ok(SourceArchiveEncoding::Gzip(data))
            } else if let Some((delta_gz, ref_ids)) =
                super::encode_resource_delta_gzip(&document.original_bytes, resources, cap)?
            {
                Ok(SourceArchiveEncoding::ResourceDeltaGzip {
                    data: delta_gz,
                    reference_resources: ref_ids,
                })
            } else {
                Ok(SourceArchiveEncoding::External)
            }
        };
    let archive_encoding = if !document.original_bytes.is_empty() && source_len <= budget {
        SourceArchiveEncoding::Raw
    } else if !document.original_bytes.is_empty() && res_budget > 0 {
        // Limit speculative compression by available resource storage, then run lossless
        // repeated-shape Graphic compaction only when the 128 MiB JSON cap (not the 64 MiB
        // resource cap) prevents embedding the archive.
        let candidate = compress_within(res_budget, &project.resources)?;
        let cand_len = match &candidate {
            SourceArchiveEncoding::Gzip(d)
            | SourceArchiveEncoding::ResourceDeltaGzip { data: d, .. } => Some(d.len()),
            _ => None,
        };
        if cand_len.is_some_and(|len| len <= budget) {
            candidate
        } else if (cand_len.is_some() || source_len <= res_budget)
            && budget < res_budget
            && deduplicate_repeated_vector_paths_for_budget(
                &mut project,
                &mut context.provenance,
                true,
            )
        {
            project.format_version = 5;
            project.upgrade_format_for_features();
            budget = archive_budget(&project, &context.provenance)?.0;
            if source_len <= budget {
                SourceArchiveEncoding::Raw
            } else if cand_len.is_some_and(|len| len <= budget) {
                candidate
            } else {
                SourceArchiveEncoding::External
            }
        } else {
            SourceArchiveEncoding::External
        }
    } else {
        SourceArchiveEncoding::External
    };
    let source_data: &[u8] = match &archive_encoding {
        SourceArchiveEncoding::Raw => &document.original_bytes,
        SourceArchiveEncoding::Gzip(data) => data.as_slice(),
        SourceArchiveEncoding::ResourceDeltaGzip { data, .. } => data.as_slice(),
        SourceArchiveEncoding::External => &[],
    };
    let parts = source_data.chunks(MAX_RESOURCE_BYTES);
    let part_ids: Vec<_> = (0..parts.len())
        .map(|i| {
            if parts.len() == 1 {
                "fla-original".to_owned()
            } else {
                format!("fla-original-{i}")
            }
        })
        .collect();
    let mut embedded = !matches!(archive_encoding, SourceArchiveEncoding::External)
        && !source_data.is_empty()
        && source_data.len() <= budget;
    let lazy_sha256: std::cell::OnceCell<String> = std::cell::OnceCell::new();
    let get_source_sha256 = || -> &str {
        lazy_sha256
            .get_or_init(|| neoflash_core::extensions::sha256_hex(&document.original_bytes))
            .as_str()
    };
    let provenance = |embedded: bool,
                      detail: ProvenanceDetail,
                      placements_slice: &[serde_json::Value]| {
        let (descriptors, folders, placements) = match detail {
            ProvenanceDetail::Full => (
                serde_json::to_value(&c.descriptors).unwrap_or(serde_json::Value::Null),
                serde_json::to_value(c.extensions.as_ref().map(|e| &e.folders))
                    .unwrap_or(serde_json::Value::Null),
                serde_json::Value::Array(placements_slice.to_vec()),
            ),
            ProvenanceDetail::NoPlacements => (
                serde_json::to_value(&c.descriptors).unwrap_or(serde_json::Value::Null),
                serde_json::to_value(c.extensions.as_ref().map(|e| &e.folders))
                    .unwrap_or(serde_json::Value::Null),
                serde_json::Value::Array(vec![]),
            ),
            ProvenanceDetail::Minimal => (
                serde_json::Value::Array(vec![]),
                serde_json::Value::Null,
                serde_json::Value::Array(vec![]),
            ),
        };
        let is_compressed = matches!(
            archive_encoding,
            SourceArchiveEncoding::Gzip(_) | SourceArchiveEncoding::ResourceDeltaGzip { .. }
        );
        let archived = embedded && is_compressed;
        let mut result = serde_json::json!({
            "schema": if archived { 3 } else { 2 },
            "source_resources": if embedded && !archived { &part_ids[..] } else { &[] },
            "source_bytes": document.original_bytes.len(),
            "source_fnv1a64": source_digest,
            "authoring_version": document.inspection.authoring_version,
            "contents_revision": c.revision,
            "language": language,
            "publish_hidden_layers": include_hidden,
            "container_tolerances": document.inspection.container_tolerances,
            "descriptors": descriptors,
            "folders": folders,
            "placements": placements,
            "note": "Offsets refer to original logical streams. Concatenating source_resources in order reproduces the original FLA. Source bytes retain authoring properties, editor visibility, guides, metadata and unresolved source references."
        });
        if !library_fonts.is_empty() && detail != ProvenanceDetail::Minimal {
            result["library_fonts"] = serde_json::json!(library_fonts);
        }
        if archived {
            let sha256 = get_source_sha256();
            result["source_sha256"] = serde_json::json!(sha256);
            match &archive_encoding {
                SourceArchiveEncoding::Gzip(_) => {
                    result["source_archive"] = serde_json::json!({
                        "encoding": "gzip",
                        "resources": part_ids,
                        "source_sha256": sha256,
                        "source_byte_len": source_len,
                    });
                    result["note"] = serde_json::json!(
                        "Concatenate source_archive.resources in order and decompress the gzip stream to recover the original FLA. Offsets refer to the original logical streams."
                    );
                }
                SourceArchiveEncoding::ResourceDeltaGzip {
                    reference_resources,
                    ..
                } => {
                    result["source_archive"] = serde_json::json!({
                        "encoding": "resource-delta-gzip",
                        "resources": part_ids,
                        "reference_resources": reference_resources,
                        "source_sha256": sha256,
                        "source_byte_len": source_len,
                    });
                    result["note"] = serde_json::json!(
                        "Use neoflash_import::reconstruct_legacy_fla_bytes (decompressing source_archive.resources and resolving reference_resources slices) to recover the byte-exact original FLA."
                    );
                }
                _ => {}
            }
        } else if !embedded {
            let sha256 = get_source_sha256();
            result["source_sha256"] = serde_json::json!(sha256);
            result["source_byte_len"] = serde_json::json!(source_len);
            result["source_archive"] = serde_json::json!({
                "encoding": "external",
                "resources": [],
                "source_sha256": sha256,
                "source_byte_len": source_len,
            });
            result["note"] = serde_json::json!(
                "Original FLA exceeded project resource/JSON limits and remains external; source_sha256 and source_byte_len identify the exact source archive."
            );
        }
        result
    };
    if embedded {
        let (base, mime) = if matches!(archive_encoding, SourceArchiveEncoding::Raw) {
            ("legacy/source.fla", "application/x-neoflash-fla-source")
        } else {
            ("legacy/source.fla.gz", "application/gzip")
        };
        for (i, (part, id)) in parts.zip(&part_ids).enumerate() {
            let path = if part_ids.len() == 1 {
                base.to_owned()
            } else {
                format!("{base}.part{i:02}")
            };
            project
                .resources
                .push(ResourceUnit::from_bytes(id, path, mime, part).map_err(|e| e.to_string())?);
        }
    } else {
        context.report.warning("fla_source_not_embedded","Original FLA",format!("The original FLA ({} MiB) does not fit the project's limits next to the converted document (64 MiB of resources, 128 MiB in total), so it is not embedded. Keep the original file; provenance records its size, SHA-256 digest, and FNV-1a digest.",document.original_bytes.len()/(1024*1024)));
    }
    let mut prov_detail = ProvenanceDetail::Full;
    let mut prov_warned = false;
    update_provenance_resource(
        &mut project,
        &mut prov_detail,
        &mut prov_warned,
        embedded,
        &context.provenance,
        context.report,
        &provenance,
    )?;
    project.upgrade_format_for_features();
    let mut json = serde_json::to_string(&project).map_err(|e| e.to_string())?;
    if json.len() > MAX_PROJECT_JSON_BYTES {
        fit_project_json_budget(
            &mut project,
            &mut json,
            &mut embedded,
            &mut prov_detail,
            &mut prov_warned,
            document.original_bytes.len(),
            &mut context.provenance,
            context.report,
            &provenance,
        )?;
    }
    project
        .validate()
        .map_err(|e| format!("Converted document validation: {e}"))?;
    if json.len() > MAX_PROJECT_JSON_BYTES {
        return Err("Invalid project: document exceeds the 128 MiB encoded JSON limit".into());
    }
    // Import reserves authored IDs through the same parser as opening .nflash.
    let project = Project::from_json(&json).map_err(|e| e.to_string())?;
    context.report.statistics.symbols = project.symbols.len();
    for symbol in &project.symbols {
        context.report.statistics.layers += symbol.timeline.layers.len();
        context.report.statistics.scripts += symbol.timeline.scripts.len();
        for layer in &symbol.timeline.layers {
            context.report.statistics.keyframes += layer.keyframes.len();
            for key in &layer.keyframes {
                context.report.statistics.elements += key.elements.len();
                context.report.statistics.scripts +=
                    key.elements.iter().map(|e| e.scripts.len()).sum::<usize>();
            }
        }
    }
    Ok(project)
}

const PLACEHOLDER: &str = "fla-unsupported-object";

pub(super) struct Context<'a> {
    report: &'a mut ImportReport,
    library: BTreeMap<u32, String>,
    library_kinds: BTreeMap<u32, SymbolKind>,
    implemented_symbols: std::collections::BTreeSet<String>,
    scene_stream_indices: std::collections::BTreeSet<usize>,
    bitmaps: BTreeMap<u16, assets::BitmapAsset>,
    sounds: BTreeMap<u16, assets::SoundAsset>,
    videos: BTreeMap<u16, assets::VideoAsset>,
    compiled_clips: BTreeMap<u16, assets::CompiledClipAsset>,
    skipped_bitmaps: std::collections::BTreeSet<u16>,
    stream_stops: std::collections::BTreeSet<usize>,
    language: ScriptLanguage,
    include_hidden: bool,
    motion_pose_budget: usize,
    contents_revision: Option<u8>,
    motion_3d: ([f64; 2], Option<f64>, Option<[f64; 2]>),
    generated: Vec<Symbol>,
    inline_resources: Vec<ResourceUnit>,
    compiled_clip_sources: Vec<SourceUnit>,
    provenance: Vec<serde_json::Value>,
    missing_bitmaps: BTreeMap<u16, String>,
}

#[allow(unused_imports)]
pub(super) use self::text_convert::convert_timeline_text;
#[allow(unused_imports)]
use self::{
    budget_shedding::*, provenance_archive::*, structural_limits::*, text_convert::*,
    timeline_elements::*, timeline_frames::*, tweens_convert::*,
};
