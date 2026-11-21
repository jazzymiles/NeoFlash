//! Native binary authoring import. Source records remain embedded verbatim;
//! unsupported visual or executable records stop conversion with a location.
mod assets;
mod compiled_clip;
mod instance_props;
mod shape;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_fidelity;
#[cfg(test)]
mod text;

use super::{BinaryTimelineDocument, contents::*, timeline::*};
use crate::ImportReport;
use neoflash_core::*;
use std::collections::BTreeMap;

pub(super) fn read_string(bytes: &[u8], value: &ContentsString) -> Result<String, String> {
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
        Ok(decode_ansi(raw))
    }
}

thread_local! {
    /// Code pages guessed while converting the current document (bit 0: 1251, bit 1: 1252).
    static ANSI_GUESSES: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// Legacy writers stored 8-bit text in the authoring system's ANSI code page,
/// which the archive does not record. Cyrillic text (whole words of high bytes)
/// is decoded as Windows-1251, anything else as Windows-1252. The guess is
/// reported; the original bytes stay in the embedded source.
pub(super) fn decode_ansi(raw: &[u8]) -> String {
    if raw.is_ascii() {
        return String::from_utf8(raw.to_vec()).unwrap();
    }
    let letters = raw
        .iter()
        .filter(|b| b.is_ascii_alphabetic() || **b >= 0xc0)
        .count();
    let high = raw
        .iter()
        .filter(|b| **b >= 0xc0 || matches!(**b, 0xa8 | 0xb8))
        .count();
    let cyrillic = high * 10 >= letters * 3;
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

    if empty_revision_count > 0 || truncated_count > 0 {
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
                    .and_then(|ca| ca.reconstructed.as_ref())
                    .is_some_and(|rec| rec.has_avm1_scripts),
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
        contents_revision: c.revision,
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
    // The exact original is embedded when it fits the project resource budget,
    // split into parts of at most one resource each. Larger sources stay
    // external; provenance records their size and digest either way.
    let parts: Vec<_> = document.original_bytes.chunks(MAX_RESOURCE_BYTES).collect();
    let part_ids: Vec<String> = if parts.len() <= 1 {
        vec!["fla-original".into()]
    } else {
        (0..parts.len())
            .map(|i| format!("fla-original-{i}"))
            .collect()
    };
    if !document.inspection.container_tolerances.is_empty() {
        context.report.warning("fla_container_compatibility","CFB directory",format!("The source container has non-canonical metadata outside its live streams ({}). Streams were read from their directory chains unchanged; the original FLA is retained.",document.inspection.container_tolerances.join("; ")))
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
    // Saved projects are pretty-printed JSON; measure that form.
    let encoded = |bytes: usize| bytes.div_ceil(3) * 4;
    let measure_budget = |proj: &Project, placements: &[serde_json::Value]| {
        let doc_json = serde_json::to_string_pretty(proj).map_or(usize::MAX, |v| v.len());
        let used_bytes: usize = proj
            .resources
            .iter()
            .map(|r| r.data_base64.len() / 4 * 3 + 3)
            .sum();
        let prov_val = serde_json::json!({
            "schema": 2,
            "source_resources": part_ids,
            "source_bytes": document.original_bytes.len(),
            "source_fnv1a64": format!("{:016x}", fnv1a64(&document.original_bytes)),
            "authoring_version": document.inspection.authoring_version,
            "contents_revision": c.revision,
            "language": language,
            "publish_hidden_layers": include_hidden,
            "container_tolerances": document.inspection.container_tolerances,
            "descriptors": &c.descriptors,
            "folders": c.extensions.as_ref().map(|e| &e.folders),
            "placements": placements,
            "note": "Offsets refer to original logical streams. Concatenating source_resources in order reproduces the original FLA. Source bytes retain authoring properties, editor visibility, guides, metadata and unresolved source references."
        });
        let prov_len = serde_json::to_vec(&prov_val).map_or(usize::MAX, |v| v.len());
        let fits_source = !parts.is_empty()
            && proj.resources.len() + parts.len() < MAX_RESOURCES
            && used_bytes + document.original_bytes.len() + prov_len + 4096
                <= MAX_RESOURCE_TOTAL_BYTES
            && doc_json
                .saturating_add(encoded(document.original_bytes.len()))
                .saturating_add(encoded(prov_len))
                .saturating_add(1 << 20)
                <= MAX_PROJECT_JSON_BYTES;
        (doc_json, fits_source)
    };
    let (mut document_json, mut embedded) = measure_budget(&project, &context.provenance);
    if (!embedded || document_json > MAX_PROJECT_JSON_BYTES)
        && deduplicate_project_for_budget(&mut project, &mut context.provenance)
    {
        (document_json, embedded) = measure_budget(&project, &context.provenance);
    }
    let _ = document_json;
    let provenance = |embedded: bool, detail: ProvenanceDetail| {
        let (descriptors, folders, placements) = match detail {
            ProvenanceDetail::Full => (
                serde_json::to_value(&c.descriptors).unwrap_or(serde_json::Value::Null),
                serde_json::to_value(c.extensions.as_ref().map(|e| &e.folders))
                    .unwrap_or(serde_json::Value::Null),
                serde_json::Value::Array(context.provenance.clone()),
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
        serde_json::json!({
            "schema": 2,
            "source_resources": if embedded { part_ids.clone() } else { vec![] },
            "source_bytes": document.original_bytes.len(),
            "source_fnv1a64": format!("{:016x}", fnv1a64(&document.original_bytes)),
            "authoring_version": document.inspection.authoring_version,
            "contents_revision": c.revision,
            "language": language,
            "publish_hidden_layers": include_hidden,
            "container_tolerances": document.inspection.container_tolerances,
            "descriptors": descriptors,
            "folders": folders,
            "placements": placements,
            "note": "Offsets refer to original logical streams. Concatenating source_resources in order reproduces the original FLA. Source bytes retain authoring properties, editor visibility, guides, metadata and unresolved source references."
        })
    };
    if embedded {
        for (i, (part, id)) in parts.iter().zip(&part_ids).enumerate() {
            let path = if parts.len() == 1 {
                "legacy/source.fla".to_owned()
            } else {
                format!("legacy/source.fla.part{i:02}")
            };
            project.resources.push(
                ResourceUnit::from_bytes(id, path, "application/x-neoflash-fla-source", part)
                    .map_err(|e| e.to_string())?,
            );
        }
    } else {
        context.report.warning("fla_source_not_embedded","Original FLA",format!("The original FLA ({} MiB) does not fit the project's limits next to the converted document (64 MiB of resources, 128 MiB in total), so it is not embedded. Keep the original file; provenance records its size and FNV-1a digest.",document.original_bytes.len()/(1024*1024)));
    }
    let mut prov_detail = ProvenanceDetail::Full;
    let mut prov_warned = false;
    update_provenance_resource(
        &mut project,
        &mut prov_detail,
        &mut prov_warned,
        embedded,
        !context.provenance.is_empty(),
        context.report,
        &provenance,
    )?;
    project.upgrade_format_for_features();
    let mut json = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
    if json.len() > MAX_PROJECT_JSON_BYTES {
        fit_project_json_budget(
            &mut project,
            &mut json,
            &mut embedded,
            &mut prov_detail,
            &mut prov_warned,
            document.original_bytes.len(),
            !context.provenance.is_empty(),
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

struct Context<'a> {
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
    contents_revision: Option<u8>,
    generated: Vec<Symbol>,
    inline_resources: Vec<ResourceUnit>,
    compiled_clip_sources: Vec<SourceUnit>,
    provenance: Vec<serde_json::Value>,
    missing_bitmaps: BTreeMap<u16, String>,
}
impl Context<'_> {
    fn ensure_placeholder_symbol(&mut self) {
        if !self.generated.iter().any(|g| g.id == PLACEHOLDER) {
            self.generated.push(group_symbol(
                PLACEHOLDER,
                "Unsupported source object (see import report)".into(),
                vec![],
            ));
        }
    }
    fn placeholder_element(&mut self, id: String, transform: Transform) -> Element {
        self.ensure_placeholder_symbol();
        Element {
            id,
            name: String::new(),
            transform,
            kind: ElementKind::Instance {
                symbol_id: PLACEHOLDER.into(),
                playback: InstancePlayback::SingleFrame(0),
            },
            scripts: vec![],
            accessibility: None,
            ..Element::default()
        }
    }
    fn frame_sound(
        &mut self,
        s: &BinaryTimelineStream,
        f: &TimelineFrame,
        at: &str,
    ) -> Option<FrameSound> {
        let id = f.sound_media_id.unwrap_or(0);
        if id == 0 {
            return None;
        }
        let Some(asset) = self.sounds.get(&id).cloned() else {
            self.report.warning(
                "fla_frame_sound",
                at,
                format!(
                    "Timeline sound references media #{id}, which could not be imported; the sound and its sync settings remain in the embedded source."
                ),
            );
            return None;
        };
        let sync = match f.sound_sync.unwrap_or(0) {
            0 => SoundSync::Event,
            1 => SoundSync::Start,
            2 => SoundSync::Stop,
            3 => SoundSync::Stream,
            other => {
                self.report.warning(
                    "fla_frame_sound",
                    at,
                    format!("Unknown frame-sound sync mode {other}; Event sync is used."),
                );
                SoundSync::Event
            }
        };
        let raw_loop = f.sound_loop_count.unwrap_or(1);
        let (loops, loop_forever) = if raw_loop == 0x7fff {
            (1, true)
        } else {
            let n = u32::from(raw_loop).max(1);
            if n > FrameSound::MAX_LOOPS {
                self.report.warning(
                    "fla_frame_sound",
                    at,
                    format!(
                        "Frame-sound repeat count {n} exceeds {}; clamped to {}.",
                        FrameSound::MAX_LOOPS,
                        FrameSound::MAX_LOOPS
                    ),
                );
                (FrameSound::MAX_LOOPS, false)
            } else {
                (n, false)
            }
        };
        let envelope_points = || -> Option<Vec<(u32, u16, u16)>> {
            let count = usize::from(f.sound_envelope_count.unwrap_or(0));
            if count == 0 {
                return Some(vec![]);
            }
            let span = f.sound_envelope?;
            let raw = s.original_bytes.get(span.start..span.end)?;
            if raw.len() != count * 8 {
                return None;
            }
            Some(
                raw.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| {
                        (
                            u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                            u16::from_le_bytes([c[4], c[5]]),
                            u16::from_le_bytes([c[6], c[7]]),
                        )
                    })
                    .collect(),
            )
        };
        let full_span_fade = |p0: u32, p1: u32| -> bool {
            let dur = u64::from(asset.duration_44);
            p0 <= 64
                && if dur > 0 {
                    u64::from(p1).abs_diff(dur) <= 64
                } else {
                    p1 > 0
                }
        };
        let classify_envelope = |pts: &[(u32, u16, u16)]| -> Option<SoundEffect> {
            if pts.is_empty() || pts.iter().all(|&(_, l, r)| l == 0x8000 && r == 0x8000) {
                Some(SoundEffect::None)
            } else if pts.iter().all(|&(_, l, r)| l == 0x8000 && r == 0) {
                Some(SoundEffect::LeftChannel)
            } else if pts.iter().all(|&(_, l, r)| l == 0 && r == 0x8000) {
                Some(SoundEffect::RightChannel)
            } else if let [(p0, 0, 0), (p1, 0x8000, 0x8000)] = *pts
                && full_span_fade(p0, p1)
            {
                Some(SoundEffect::FadeIn)
            } else if let [(p0, 0x8000, 0x8000), (p1, 0, 0)] = *pts
                && full_span_fade(p0, p1)
            {
                Some(SoundEffect::FadeOut)
            } else if let [(p0, 0x8000, 0), (p1, 0, 0x8000)] = *pts
                && full_span_fade(p0, p1)
            {
                Some(SoundEffect::FadeLeftToRight)
            } else if let [(p0, 0, 0x8000), (p1, 0x8000, 0)] = *pts
                && full_span_fade(p0, p1)
            {
                Some(SoundEffect::FadeRightToLeft)
            } else {
                None
            }
        };
        let (effect, envelope) = match f.sound_effect {
            Some(1) => (SoundEffect::LeftChannel, None),
            Some(2) => (SoundEffect::RightChannel, None),
            Some(3) => (SoundEffect::FadeLeftToRight, None),
            Some(4) => (SoundEffect::FadeRightToLeft, None),
            Some(5) => (SoundEffect::FadeIn, None),
            Some(6) => (SoundEffect::FadeOut, None),
            Some(0) | Some(7) | None => match envelope_points() {
                Some(mut pts) => match classify_envelope(&pts) {
                    Some(preset) => (preset, None),
                    None => {
                        pts.sort_by_key(|p| p.0);
                        if pts.len() > FrameSound::MAX_ENVELOPE_POINTS {
                            self.report.warning(
                                "fla_frame_sound",
                                at,
                                format!(
                                    "Custom frame-sound envelope has {} points; truncated to {} points and full envelope data remains in the embedded source.",
                                    pts.len(),
                                    FrameSound::MAX_ENVELOPE_POINTS
                                ),
                            );
                            pts.truncate(FrameSound::MAX_ENVELOPE_POINTS);
                        }
                        let max_mark = pts.iter().map(|p| u64::from(p.0)).max().unwrap_or(0);
                        let denom = u64::from(asset.duration_44).max(max_mark);
                        if pts.iter().any(|&(_, l, r)| l > 0x8000 || r > 0x8000) {
                            self.report.warning(
                                "fla_frame_sound",
                                at,
                                "Custom frame-sound envelope gain above 100% was clamped to 1.0; original envelope levels remain in the embedded source.",
                            );
                        }
                        let mut prev_pos = 0.0_f64;
                        let points = pts
                            .into_iter()
                            .map(|(mark44, l, r)| {
                                let pos = if denom == 0 {
                                    0.0
                                } else {
                                    (u64::from(mark44) as f64 / denom as f64).clamp(0.0, 1.0)
                                }
                                .max(prev_pos);
                                prev_pos = pos;
                                let left = (f64::from(l) / 32768.0).clamp(0.0, 1.0);
                                let right = (f64::from(r) / 32768.0).clamp(0.0, 1.0);
                                [pos, left, right]
                            })
                            .collect();
                        (SoundEffect::Custom, Some(points))
                    }
                },
                None => {
                    self.report.warning(
                        "fla_frame_sound",
                        at,
                        "Corrupt frame-sound envelope span could not be decoded; imported without gain automation and original bytes remain in the embedded source.",
                    );
                    (SoundEffect::None, None)
                }
            },
            Some(other) => {
                self.report.warning(
                    "fla_frame_sound",
                    at,
                    format!(
                        "Unknown frame-sound effect {other}; imported without an effect preset."
                    ),
                );
                (SoundEffect::None, None)
            }
        };
        let in_trimmed = f.sound_in_point.unwrap_or(0) > 0;
        let out_raw = f.sound_out_point.unwrap_or(0);
        let out_trimmed = out_raw > 0
            && out_raw < 0x3fff_ffff
            && asset.duration_44 > 0
            && out_raw.saturating_add(8) < asset.duration_44;
        let mut trim_invalid = false;
        let in_point = if in_trimmed {
            let sec = f64::from(f.sound_in_point.unwrap_or(0)) / 44_100.0;
            if sec.is_finite() && (0.0..=FrameSound::MAX_SOUND_TRIM_SECONDS).contains(&sec) {
                Some(sec)
            } else {
                trim_invalid = true;
                None
            }
        } else {
            None
        };
        let out_point = if out_trimmed {
            let sec = f64::from(out_raw) / 44_100.0;
            if sec.is_finite()
                && (0.0..=FrameSound::MAX_SOUND_TRIM_SECONDS).contains(&sec)
                && in_point.is_none_or(|inp| sec > inp)
            {
                Some(sec)
            } else {
                trim_invalid = true;
                None
            }
        } else {
            None
        };
        if trim_invalid {
            self.report.warning(
                "fla_frame_sound",
                at,
                "Frame-sound in/out trim points are outside the supported range and were omitted; trim offsets remain in the embedded source.",
            );
        }
        Some(FrameSound {
            resource: asset.resource_id,
            sync,
            loops,
            loop_forever,
            effect,
            envelope,
            in_point,
            out_point,
        })
    }
    fn actions(
        &mut self,
        stream: &BinaryTimelineStream,
        actions: Option<&TimelineActions>,
    ) -> Result<Option<Script>, String> {
        let Some(a) = actions else { return Ok(None) };
        let loc = format!("stream #{} @0x{:x}", stream.index, a.encoded.start);
        match a.presence {
            TimelineActionPresence::Empty => Ok(None),
            TimelineActionPresence::Source if a.complete => Ok(Some(Script {
                language: self.language,
                source: read_string(
                    &stream.original_bytes,
                    a.source.as_ref().ok_or("Missing action source")?,
                )?,
            })),
            TimelineActionPresence::Source => {
                if let Some(span) = a.source.as_ref()
                    && let Ok(source) = read_string(&stream.original_bytes, span)
                {
                    self.report.warning(
                        "fla_actions_partial",
                        &loc,
                        "Source action record stopped after reading its script text; the recovered script is preserved.",
                    );
                    Ok((!source.trim().is_empty()).then_some(Script {
                        language: self.language,
                        source,
                    }))
                } else {
                    self.report.warning(
                        "fla_actions_partial",
                        &loc,
                        "Source action record stopped before its script string completed; the incomplete record remains in the embedded source.",
                    );
                    Ok(None)
                }
            }
            // Historical and compiled-mode command lists carry no source text;
            // the reader's ActionScript 1 translation stands in for it.
            TimelineActionPresence::Historical | TimelineActionPresence::Compiled if a.complete => {
                let decoded = a
                    .decoded
                    .as_ref()
                    .ok_or("Missing decoded action commands")?;
                if decoded.needs_review {
                    self.report.warning(
                        "fla_actions_translated",
                        loc,
                        format!(
                            "ActionScript 1 translation of legacy action commands needs review: {} of {} commands remain as marked comments (or a block was repaired); the original commands stay in the embedded FLA.",
                            decoded.untranslated.len(),
                            decoded.commands.len()
                        ),
                    );
                }
                Ok((!decoded.source.trim().is_empty()).then(|| Script {
                    language: ScriptLanguage::ActionScript1,
                    source: decoded.source.clone(),
                }))
            }
            TimelineActionPresence::Historical | TimelineActionPresence::Compiled => {
                if let Some(decoded) = a.decoded.as_ref()
                    && let Some((mut source, complete_count, untranslated_count)) =
                        crate::legacy::mfc::translate_partial_actions(
                            &stream.original_bytes,
                            decoded,
                        )
                {
                    source.push_str(&format!(
                        "// NeoFlash: action record stopped at @0x{:x} after {complete_count} complete command(s)\n",
                        a.encoded.end
                    ));
                    self.report.warning(
                        "fla_actions_partial",
                        &loc,
                        format!(
                            "Legacy action command list stopped before completion; {complete_count} complete command(s) before the stop were translated to ActionScript 1 ({untranslated_count} kept as comments)."
                        ),
                    );
                    Ok(Some(Script {
                        language: ScriptLanguage::ActionScript1,
                        source,
                    }))
                } else {
                    self.report.warning(
                        "fla_actions_partial",
                        &loc,
                        "Legacy action command list stopped before any complete statement could be translated; the raw bytes remain in the embedded source.",
                    );
                    Ok(None)
                }
            }
            TimelineActionPresence::Unknown => {
                self.report.warning(
                    "fla_actions_partial",
                    &loc,
                    "Action record stopped before its encoding mode was identified; the raw bytes remain in the embedded source.",
                );
                Ok(None)
            }
        }
    }
    fn timeline(&mut self, s: &BinaryTimelineStream) -> Result<Timeline, String> {
        const MAX_TIMELINE_FRAMES: u32 = 1_000_000;
        let Some(page) = s.root.and_then(|r| object(s, r).ok()) else {
            if !self.stream_stops.contains(&s.index) {
                self.report.warning(
                    "fla_timeline_empty",
                    format!("stream #{}", s.index),
                    "Timeline stream contains no readable root page record; imported as an empty 1-frame timeline.",
                );
            }
            return Ok(empty_timeline(&format!("fla-layer-{}-0", s.index)));
        };
        let mut timeline = Timeline {
            guides: vec![],
            frame_count: 1,
            layers: vec![],
            scripts: vec![],
        };
        // Layer source IDs and their total authored length, for end markers.
        let mut sources = Vec::new();
        for id in &page.children {
            let o = object(s, *id)?;
            let default_layer = TimelineLayer::default();
            let (l, name) = match &o.payload {
                Some(TimelinePayload::Layer(l)) => {
                    (l, optional_text(&s.original_bytes, l.name.as_ref())?)
                }
                None if o.class == TimelineClass::Layer && !o.children.is_empty() => {
                    let fallback = format!("Layer {}", timeline.layers.len() + 1);
                    self.report.warning(
                        "fla_layer_partial",
                        format!("stream #{} object #{id}", s.index),
                        format!(
                            "Layer record stopped before its settings after reading frame records; recovered frames are imported on default normal layer '{fallback}'."
                        ),
                    );
                    (&default_layer, fallback)
                }
                _ => {
                    self.report.warning("fla_layer_skipped",format!("stream #{} object #{id}",s.index),if o.complete {"A timeline child is not a layer record; it remains in the embedded source."} else {"The layer record stops before its settings; it remains in the embedded source."});
                    continue;
                }
            };
            if !o.complete && o.children.is_empty() {
                self.report.warning(
                    "fla_layer_skipped",
                    format!("stream #{} layer {name}", s.index),
                    "The layer record stops before any frame; it remains in the embedded source.",
                );
                continue;
            }
            let layer_id = lid(s.index, *id);
            let state = l
                .state_flags
                .and_then(|v| s.original_bytes.get(v.start..v.end));
            let editor_visible = state.map_or(l.legacy_flag != Some(0), |v| v.get(1) == Some(&0));
            let locked = state.map_or(l.legacy_flag == Some(1), |v| {
                v.get(2).is_some_and(|v| *v != 0)
            });
            let role = match l.layer_type.unwrap_or(0) {
                0 | 2 | 5 => LayerRole::Normal,
                1 => LayerRole::Guide,
                3 => LayerRole::Folder,
                4 => LayerRole::Mask,
                other => {
                    self.report.warning(
                        "fla_layer_type",
                        format!("stream #{} layer {name}", s.index),
                        format!("Unknown layer type {other}; imported as a normal layer."),
                    );
                    LayerRole::Normal
                }
            };
            let related = s
                .pointers
                .iter()
                .find(|p| {
                    p.source == Some(*id)
                        && matches!(
                            p.relation,
                            TimelineRelation::RelatedLayer | TimelineRelation::LegacyRelatedLayer
                        )
                })
                .and_then(|p| p.target);
            let mut properties = LayerProperties {
                role,
                outline: l.outline.unwrap_or(0) != 0,
                outline_color: l.outline_color_word.map(|v| {
                    let b = v.to_le_bytes();
                    [b[0], b[1], b[2]]
                }),
                collapsed: role == LayerRole::Folder && l.mask_or_open == Some(0),
                ..Default::default()
            };
            if let Some(parent) = related {
                let parent_type = match object(s, parent).map(|o| &o.payload) {
                    Ok(Some(TimelinePayload::Layer(p))) if page.children.contains(&parent) => {
                        p.layer_type
                    }
                    _ => None,
                };
                let parent_id = lid(s.index, parent);
                match parent_type {
                    Some(4) if role == LayerRole::Normal => properties.mask = Some(parent_id),
                    Some(3) => properties.parent = Some(parent_id),
                    Some(1) if role == LayerRole::Normal => {
                        properties.motion_guide = Some(parent_id)
                    }
                    _ => self.report.warning("fla_layer_relation",format!("stream #{} layer {name}",s.index),"The layer's mask/guide/folder relation cannot be resolved; the layer is imported without it."),
                }
            }
            if l.animation_type.unwrap_or(0) != 0 {
                self.report.warning(
                    "fla_layer_animation",
                    format!("stream #{} layer {name}", s.index),
                    "Motion-object or IK layer animation is imported as its stored keyframe poses.",
                );
            }
            self.provenance.push(serde_json::json!({"stream":s.index,"object":id,"target":layer_id,"editor_visible":editor_visible,"source":o.encoded}));
            let mut layer = Layer {
                id: layer_id,
                name,
                visible: editor_visible,
                locked,
                keyframes: vec![],
                properties,
            };
            let is_scene_stream = self.scene_stream_indices.contains(&s.index);
            let layer_has_tween = l.animation_type.unwrap_or(0) != 0
                || o.children
                    .iter()
                    .filter_map(|fid| object(s, *fid).ok())
                    .any(|fr| {
                        matches!(
                            &fr.payload,
                            Some(TimelinePayload::Frame(tf))
                                if (tf.key_mode.unwrap_or(0) & 255) != 0
                                    || tf.morph_shape.is_some()
                                    || tf.motion_xml.as_ref().is_some_and(|x| x.data.end > x.data.start)
                        )
                    });
            let mut cursor = 0u32;
            let mut pending_morphs: Vec<(usize, u32)> = Vec::new();
            for frame_id in &o.children {
                if cursor >= MAX_TIMELINE_FRAMES {
                    self.report.warning(
                        "fla_frame_limit",
                        format!("stream #{} layer {}", s.index, layer.name),
                        format!(
                            "Layer exceeds the {MAX_TIMELINE_FRAMES}-frame timeline limit; trailing keyframes are omitted."
                        ),
                    );
                    break;
                }
                let frame = object(s, *frame_id)?;
                if frame.class != TimelineClass::Frame {
                    self.report.warning(
                        "fla_frame_skipped",
                        format!("stream #{} layer {}", s.index, layer.name),
                        "A layer child is not a frame record; it remains in the embedded source.",
                    );
                    continue;
                }
                let default_frame = TimelineFrame::default();
                let f = match &frame.payload {
                    Some(TimelinePayload::Frame(f)) => f.as_ref(),
                    _ => &default_frame,
                };
                if !frame.complete {
                    let has_recoverable = !frame.children.is_empty()
                        || frame.shape_index.is_some()
                        || f.duration.is_some()
                        || f.label.is_some()
                        || f.sound_media_id.unwrap_or(0) != 0
                        || f.actions
                            .as_ref()
                            .is_some_and(|a| a.presence != TimelineActionPresence::Empty);
                    if !has_recoverable {
                        break;
                    }
                    if frame.payload.is_none() || f.duration.is_none() {
                        self.report.warning(
                            "fla_frame_partial",
                            format!(
                                "stream #{} layer {} frame {}",
                                s.index,
                                layer.name,
                                cursor + 1
                            ),
                            "Keyframe stopped before all frame properties were read; decoded elements, shapes and scripts before the stop are preserved with a 1-frame fallback duration.",
                        );
                    }
                }
                if f.duration == Some(0) {
                    // Flash writes empty zero-length keys at the start of some
                    // layers; they occupy no frame, so the next key starts here too.
                    if !frame.complete
                        || !frame.children.is_empty()
                        || frame.shape_index.is_some_and(|index| {
                            s.shapes
                                .get(index)
                                .is_none_or(|shape| !shape.record.edges.is_empty())
                        })
                        || f.label
                            .as_ref()
                            .is_some_and(|label| label.data.start != label.data.end)
                        || f.sound_media_id.unwrap_or(0) != 0
                        || f.morph_shape.is_some()
                        || f.motion_xml
                            .as_ref()
                            .is_some_and(|xml| xml.data.start != xml.data.end)
                        || f.actions
                            .as_ref()
                            .is_some_and(|a| a.presence != TimelineActionPresence::Empty)
                    {
                        self.report.warning("fla_frame_duration",format!("stream #{} layer {} frame {}",s.index,layer.name,cursor+1),"A zero-length keyframe with content occupies no frame in Flash and is skipped; it remains in the embedded source.");
                    }
                    if !frame.complete {
                        break;
                    }
                    continue;
                }
                let at = format!(
                    "stream #{} layer {} frame {}",
                    s.index,
                    layer.name,
                    cursor + 1
                );
                let sound = self.frame_sound(s, f, &at);
                let mut frame_videos = Vec::new();
                for &cid in &frame.children {
                    let Ok(co) = object(s, cid) else { continue };
                    let Some(TimelinePayload::Video(v)) = &co.payload else {
                        continue;
                    };
                    let Some(mid) = v.media_id else { continue };
                    let Some(asset) = self.videos.get(&mid) else {
                        continue;
                    };
                    let Some(res_id) = asset.resource_id.as_ref() else {
                        continue;
                    };
                    frame_videos.push(TimelineVideoSpan {
                        resource: res_id.clone(),
                        sync: if asset.sync_video {
                            VideoSyncMode::Stream
                        } else {
                            VideoSyncMode::Event
                        },
                        muted: !asset.import_audio,
                        deployment: timeline_video::VideoDeploymentMode::Embedded,
                        ..Default::default()
                    });
                }
                if frame_videos.len() > 1 {
                    self.report.warning(
                        "fla_media_not_imported",
                        &at,
                        format!(
                            "Keyframe places {} embedded video streams simultaneously; the first stream is bound to the keyframe video track and {} additional stream placement(s) remain as stage viewports.",
                            frame_videos.len(),
                            frame_videos.len() - 1
                        ),
                    );
                }
                let video = frame_videos.into_iter().next();
                if f.motion_xml
                    .as_ref()
                    .is_some_and(|v| v.data.end > v.data.start)
                {
                    self.report.warning(
                        "fla_motion_xml",
                        &at,
                        "Motion-tween XML is imported as its stored keyframe poses.",
                    );
                }
                if f.pose_matrices.is_some() && l.animation_type.unwrap_or(0) == 0 {
                    self.report.warning(
                        "fla_layer_animation",
                        &at,
                        "IK pose matrices are retained in the embedded source; the keyframe uses its stored pose.",
                    );
                }
                let mut elements = Vec::new();
                let mut bitmap_fills = Vec::new();
                let mut appearances = Vec::new();
                let mut vector_effects = Vec::new();
                let provenance_start = self.provenance.len();
                for (slot, child) in frame.children.iter().enumerate() {
                    elements.push(self.element_or_placeholder(
                        s,
                        *child,
                        &format!("fla-{}-{}-{}", s.index, id, child),
                        0,
                    ));
                    self.provenance.push(serde_json::json!({"stream":s.index,"object":child,"layer":layer.id,"frame":cursor,"slot":slot}));
                }
                if let Some(index) = frame.shape_index {
                    let frame_is_shape_tween = matches!(ease(f), Ok((Some(_), TweenKind::Shape)))
                        || layer.keyframes.last().is_some_and(|prev| {
                            prev.tween.is_some() && prev.tween_kind == TweenKind::Shape
                        });
                    let shape_id = format!("fla-{}-{}-shape-{frame_id}", s.index, id);
                    let shape_record = &s.shapes[index].record;
                    match shape::convert_shape_with_effects_opts(
                        shape_record,
                        &shape_id,
                        &self.bitmaps,
                        self.report,
                        &format!("stream #{} frame {cursor}", s.index),
                        !frame_is_shape_tween,
                        !is_scene_stream || layer_has_tween,
                    ) {
                        Ok(converted) => {
                            elements.extend(converted.elements);
                            bitmap_fills.extend(converted.bitmap_fills);
                            appearances.extend(converted.appearances);
                            vector_effects.extend(converted.vector_effects);
                        }
                        Err(e) => {
                            self.report.warning(
                                "fla_shape_skipped",
                                &at,
                                format!(
                                    "Frame artwork was not converted: {e}. A placeholder keeps its slot and the original shape remains in the embedded source."
                                ),
                            );
                            if !shape_record.edges.is_empty() {
                                elements
                                    .push(self.placeholder_element(shape_id, Transform::default()));
                            }
                        }
                    }
                }
                let (tween, tween_kind) = match ease(f) {
                    Ok(value) => value,
                    Err(e) => {
                        self.report.warning(
                            "fla_tween_easing",
                            &at,
                            format!("{e}; the tween uses its classic acceleration instead."),
                        );
                        (
                            Some(Easing::Flash {
                                acceleration: f.acceleration.unwrap_or(0) as i16 as f64,
                            }),
                            TweenKind::Classic,
                        )
                    }
                };
                let is_classic_tween = tween.is_some() && tween_kind == TweenKind::Classic;
                let mode = f.key_mode.unwrap_or(0);
                let orient_to_path = is_classic_tween && (mode & 0x100) != 0;
                let sync_symbols = is_classic_tween && (mode & 0x800) != 0;
                let snap_to_guide = is_classic_tween && (mode & 0x1000) != 0;
                let raw_rot_mode = f.rotation_mode.unwrap_or(0);
                let raw_rot_turns = f.rotation_count.unwrap_or(0);
                let (classic_rotate_dir, classic_rotate_turns) = if is_classic_tween {
                    if raw_rot_mode > 2 || raw_rot_turns > Keyframe::MAX_ROTATE_TURNS {
                        self.report.warning(
                            "fla_tween_rotation",
                            &at,
                            "Directed tween rotation exceeds supported mode/turn bounds and was clamped.",
                        );
                    }
                    let dir = match raw_rot_mode {
                        1 => ClassicRotateDirection::Cw,
                        2 => ClassicRotateDirection::Ccw,
                        _ => ClassicRotateDirection::Auto,
                    };
                    (dir, raw_rot_turns.min(Keyframe::MAX_ROTATE_TURNS))
                } else {
                    (ClassicRotateDirection::Auto, 0)
                };
                if let Some(previous) = layer.keyframes.last_mut() {
                    // An explicit classic tween may pair library symbols (MovieClip, Graphic,
                    // Button, including playback/first-frame transitions and symbol swaps),
                    // generated group symbols (with member placement transforms hoisted onto
                    // the outer group instance), or primitive/bitmap/text/video elements.
                    // Empty, multi-element, or unresolved placeholder endpoints hold their
                    // keyframe poses and emit `fla_tween_dropped`.
                    if previous.tween.is_some()
                        && previous.tween_kind == TweenKind::Classic
                        && previous.elements.len() == 1
                        && elements.len() == 1
                    {
                        hoist_generated_group_transform(
                            &mut self.generated,
                            &mut previous.elements[0],
                        );
                        hoist_generated_group_transform(&mut self.generated, &mut elements[0]);
                        deduplicate_tweened_group_endpoint(
                            &mut self.generated,
                            &mut self.provenance,
                            &previous.elements[0],
                            &mut elements[0],
                        );
                    }
                    let is_tweenable_symbol = |sym_id: &str| {
                        self.library.values().any(|id| id == sym_id)
                            || (sym_id.ends_with("-group")
                                && self.generated.iter().any(|s| s.id == sym_id))
                    };
                    let classic_tween_pair = previous.tween.is_some()
                        && previous.tween_kind == TweenKind::Classic
                        && previous.elements.len() == 1
                        && elements.len() == 1
                        && !placement_matches(&previous.elements[0], &elements[0])
                        && match (&previous.elements[0].kind, &elements[0].kind) {
                            (
                                ElementKind::Instance { symbol_id: a, .. },
                                ElementKind::Instance { symbol_id: b, .. },
                            ) => is_tweenable_symbol(a) && is_tweenable_symbol(b),
                            (ElementKind::Rectangle { .. }, ElementKind::Rectangle { .. })
                            | (ElementKind::Ellipse { .. }, ElementKind::Ellipse { .. })
                            | (ElementKind::Bitmap { .. }, ElementKind::Bitmap { .. })
                            | (ElementKind::TextBox { .. }, ElementKind::TextBox { .. })
                            | (ElementKind::Video { .. }, ElementKind::Video { .. }) => true,
                            _ => false,
                        };
                    for (a, b) in previous.elements.iter().zip(elements.iter_mut()) {
                        if placement_matches(a, b) || classic_tween_pair {
                            let source_id = std::mem::replace(&mut b.id, a.id.clone());
                            for mapping in &mut self.provenance[provenance_start..] {
                                if mapping["target"].as_str() == Some(source_id.as_str()) {
                                    mapping["target"] = serde_json::json!(b.id);
                                }
                            }
                        }
                    }
                    if previous.tween.is_some()
                        && previous.tween_kind == TweenKind::Classic
                        && (previous.elements.len() != 1
                            || elements.len() != 1
                            || !elements.iter().any(|e| e.id == previous.elements[0].id))
                    {
                        previous.tween = None;
                        previous.orient_to_path = false;
                        previous.sync_symbols = false;
                        previous.snap_to_guide = false;
                        previous.classic_rotate_dir = ClassicRotateDirection::Auto;
                        previous.classic_rotate_turns = 0;
                        self.report.warning("fla_tween_dropped",&at,"This classic motion tween has empty, multiple or unsupported placement endpoints, so the frames hold their keyframe poses.");
                    }
                }
                if let Some(script) = self.actions(s, f.actions.as_ref())? {
                    timeline.scripts.push(FrameScript {
                        layer: None,
                        frame: cursor,
                        script,
                    })
                }
                if let Some(morph_id) = f.morph_shape {
                    pending_morphs.push((layer.keyframes.len(), morph_id));
                }
                layer.keyframes.push(Keyframe {
                    frame: cursor,
                    elements,
                    tween,
                    tween_kind,
                    orient_to_path,
                    sync_symbols,
                    snap_to_guide,
                    classic_rotate_dir,
                    classic_rotate_turns,
                    sound,
                    video,
                    bitmap_fills,
                    appearances,
                    vector_effects,
                    label: optional_text(&s.original_bytes, f.label.as_ref())?,
                    ..Default::default()
                });
                let duration = u32::from(f.duration.unwrap_or(1)).max(1);
                let next_cursor = u64::from(cursor) + u64::from(duration);
                if next_cursor > u64::from(MAX_TIMELINE_FRAMES) {
                    self.report.warning(
                        "fla_frame_limit",
                        &at,
                        format!(
                            "Keyframe duration extends beyond the {MAX_TIMELINE_FRAMES}-frame timeline limit and was clamped."
                        ),
                    );
                    cursor = MAX_TIMELINE_FRAMES;
                } else {
                    cursor = next_cursor as u32;
                }
                if !frame.complete {
                    break;
                }
            }
            if let Some(last) = layer.keyframes.last_mut()
                && last.tween.is_some()
            {
                // Flash shows an unterminated tween as a held pose.
                last.tween = None;
                last.tween_kind = TweenKind::Classic;
                last.orient_to_path = false;
                last.sync_symbols = false;
                last.snap_to_guide = false;
                last.classic_rotate_dir = ClassicRotateDirection::Auto;
                last.classic_rotate_turns = 0;
            }
            for (key_idx, morph_id) in pending_morphs {
                if key_idx + 1 < layer.keyframes.len()
                    && layer.keyframes[key_idx].tween.is_some()
                    && layer.keyframes[key_idx].tween_kind == TweenKind::Shape
                {
                    let (hints, had_unconverted_hints) = resolve_fla_morph_hints(
                        s,
                        morph_id,
                        &layer.keyframes[key_idx].elements,
                        &layer.keyframes[key_idx + 1].elements,
                    );
                    if had_unconverted_hints {
                        self.report.warning(
                            "fla_morph_hints_approximated",
                            format!(
                                "stream #{} layer {} frame {}",
                                s.index,
                                if layer.name.is_empty() {
                                    "<unnamed>"
                                } else {
                                    &layer.name
                                },
                                layer.keyframes[key_idx].frame
                            ),
                            "One or more binary FLA shape-hint records were incomplete or unsupported; compatible hints were retained and the original morph records remain embedded.",
                        );
                    }
                    if !hints.is_empty() {
                        layer.keyframes[key_idx].shape_hints = hints;
                    }
                }
            }
            if layer.keyframes.is_empty() && layer.properties.role != LayerRole::Folder {
                self.report.warning("fla_layer_skipped",format!("stream #{} layer {}",s.index,layer.name),"No complete frame could be read for this layer; it remains in the embedded source.");
                continue;
            }
            if !editor_visible && self.include_hidden {
                match layer.properties.role {
                    LayerRole::Guide | LayerRole::Folder => {}
                    LayerRole::Mask => {
                        layer.visible = true;
                        self.report.warning(
                            "fla_publish_hidden_layer",
                            format!("stream #{} layer {}", s.index, layer.name),
                            "Layer is visible according to the source publish profile; its original editor eye toggle is retained in provenance.",
                        );
                    }
                    LayerRole::Normal => {
                        let has_layer_published_payload = layer.keyframes.iter().any(|k| {
                            !k.elements.is_empty()
                                || !k.label.is_empty()
                                || k.sound.is_some()
                                || k.video.is_some()
                        });
                        if has_layer_published_payload {
                            layer.visible = true;
                            self.report.warning(
                                "fla_publish_hidden_layer",
                                format!("stream #{} layer {}", s.index, layer.name),
                                "Layer is visible according to the source publish profile; its original editor eye toggle is retained in provenance.",
                            );
                        }
                    }
                }
            }
            timeline.frame_count = timeline.frame_count.max(cursor);
            sources.push(cursor);
            timeline.layers.push(layer);
        }
        // Parent/mask/guide links to layers that were not imported are dropped.
        let ids: Vec<_> = timeline.layers.iter().map(|l| l.id.clone()).collect();
        for layer in &mut timeline.layers {
            let p = &mut layer.properties;
            for link in [&mut p.parent, &mut p.mask, &mut p.motion_guide] {
                if link.as_ref().is_some_and(|id| !ids.contains(id)) {
                    *link = None;
                }
            }
        }
        for (layer, end) in timeline.layers.iter_mut().zip(sources) {
            if end < timeline.frame_count && layer.properties.role != LayerRole::Folder {
                layer.keyframes.push(Keyframe {
                    frame: end,
                    elements: vec![],
                    tween: None,
                    label: String::new(),
                    ..Default::default()
                });
            }
        }
        Ok(timeline)
    }
    /// Convert one display record, or keep its slot with an empty placeholder
    /// instance and an explicit warning when it cannot be represented yet.
    fn element_or_placeholder(
        &mut self,
        s: &BinaryTimelineStream,
        object_id: u32,
        id: &str,
        depth: usize,
    ) -> Element {
        match self.element(s, object_id, id, depth) {
            Ok(element) => element,
            Err(message) => {
                self.report.warning(
                    "fla_object_placeholder",
                    format!("stream #{} object #{object_id}", s.index),
                    format!("{message}. An empty placeholder keeps its place; the original object remains in the embedded source."),
                );
                if !self.generated.iter().any(|g| g.id == PLACEHOLDER) {
                    self.generated.push(group_symbol(
                        PLACEHOLDER,
                        "Unsupported source object (see import report)".into(),
                        vec![],
                    ));
                }
                Element {
                    id: id.into(),
                    name: String::new(),
                    transform: Transform::default(),
                    kind: ElementKind::Instance {
                        symbol_id: PLACEHOLDER.into(),
                        playback: InstancePlayback::SingleFrame(0),
                    },
                    scripts: vec![],
                    accessibility: None,
                    ..Element::default()
                }
            }
        }
    }
    fn element(
        &mut self,
        s: &BinaryTimelineStream,
        object_id: u32,
        id: &str,
        depth: usize,
    ) -> Result<Element, String> {
        if depth > 48 {
            return Err("Nested source group limit exceeded".into());
        }
        let o = object(s, object_id)?;
        let location = format!(
            "stream #{} object #{object_id} @0x{:x}",
            s.index, o.encoded.start
        );
        let mut element = Element {
            id: id.into(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 0.,
                height: 0.,
                fill: [0; 4],
                stroke: [0; 4],
                stroke_width: 0.,
            },
            scripts: vec![],
            accessibility: None,
            ..Element::default()
        };
        match &o.payload {
            Some(TimelinePayload::Instance(i)) => {
                element.filters = convert_filters(&i.filters, &location, self.report);
                element.blend_mode = convert_blend_mode(i.blend_mode, &location, self.report);
                if i.is_visible == Some(0) {
                    element.visible = false;
                }
                let target_supports_avm1_load_script = o.class == TimelineClass::Sprite
                    && i.library_id.is_some_and(|lid| {
                        self.library_kinds.get(&lid) == Some(&SymbolKind::MovieClip)
                    })
                    && i.library_id
                        .and_then(|lid| self.library.get(&lid))
                        .is_some_and(|sid| self.implemented_symbols.contains(sid));
                let (prop_script, prop_lossy) = instance_props::translate_component_properties(
                    &s.original_bytes,
                    i.properties.as_ref(),
                    i.property_count,
                    self.language,
                    target_supports_avm1_load_script,
                );
                if instance_props::has_nonredundant_animation_settings(
                    &i.animation_frame_settings,
                    i.loop_mode,
                    i.first_frame,
                ) || prop_lossy
                {
                    self.report.warning("fla_instance_settings",&location,"Component parameters or per-instance animation settings are not imported; they remain in the embedded source.");
                }
                if let Some(span) = &i.component_xml {
                    let xml = read_string(&s.original_bytes, span)?;
                    if !xml.is_empty() {
                        let document = roxmltree::Document::parse(&xml).ok();
                        if document.as_ref().is_none_or(|document| {
                            let root = document.root_element();
                            root.tag_name().name() != "component"
                                || root.children().any(|n| {
                                    n.is_element()
                                        || n.is_text() && !n.text().unwrap_or("").trim().is_empty()
                                })
                                || root.attributes().any(|a| {
                                    !matches!(
                                        a.name(),
                                        "metaDataFetched"
                                            | "schemaUrl"
                                            | "schemaOperation"
                                            | "sceneRootLabel"
                                            | "oldCopiedComponentPath"
                                    ) || matches!(a.name(), "schemaUrl" | "schemaOperation")
                                        && !a.value().is_empty()
                                })
                        }) {
                            self.report.warning("fla_component_metadata",&location,"Component data-binding metadata is not imported; it remains in the embedded source.");
                        }
                    }
                }
                element.name = i
                    .historical_wide_name
                    .clone()
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        optional_text(
                            &s.original_bytes,
                            i.instance_name.as_ref().or(i.legacy_name.as_ref()),
                        )
                        .ok()
                    })
                    .unwrap_or_default();
                element.transform = Transform::from_matrix(shape::matrix(
                    i.matrix_words.ok_or("Missing instance matrix")?,
                ))
                .ok_or("Invalid instance matrix")?;
                if let Some(m) = i.matrix_3d
                    && m != [
                        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                    ]
                {
                    self.report.warning("fla_3d_transform",&location,"3D transforms are not rendered; the instance uses its 2D placement matrix.");
                    let m64: Vec<f64> = m.iter().map(|&v| f64::from(v)).collect();
                    if m64.iter().all(|v| v.is_finite()) {
                        let persp = (1.0 / (1.0 + m64[14] / 1000.0)).clamp(0.1, 10.0);
                        let mat2d = [
                            m64[0] * persp,
                            m64[1] * persp,
                            m64[4] * persp,
                            m64[5] * persp,
                            if m64[12] != 0.0 {
                                m64[12] / 20.0
                            } else {
                                element.transform.x
                            },
                            if m64[13] != 0.0 {
                                m64[13] / 20.0
                            } else {
                                element.transform.y
                            },
                        ];
                        if let Some(projected) = Transform::from_matrix(mat2d) {
                            element.transform = projected;
                        }
                    }
                } else if let Some([rx, ry, rz]) = i.rotation_3d
                    && [rx, ry, rz].iter().all(|v| v.is_finite())
                    && (rx.abs() > 1e-6 || ry.abs() > 1e-6 || rz.abs() > 1e-6)
                {
                    self.report.warning("fla_3d_transform",&location,"3D transforms are not rendered; the instance uses its 2D placement matrix.");
                    let sx = element.transform.scale_x * ry.to_radians().cos();
                    let sy = element.transform.scale_y * rx.to_radians().cos();
                    let rot = element.transform.rotation + rz;
                    if sx.is_finite()
                        && sy.is_finite()
                        && rot.is_finite()
                        && sx.abs() <= 1e4
                        && sy.abs() <= 1e4
                        && rot.abs() <= 1e9
                    {
                        element.transform.scale_x = sx;
                        element.transform.scale_y = sy;
                        element.transform.rotation = rot;
                    }
                }
                if let Some(c) = &i.color_transform {
                    let mut value = color_transform(c)?;
                    if value.validate().is_err() {
                        let original = value;
                        for v in &mut value.multipliers {
                            *v = v.clamp(-1e4, 1e4);
                        }
                        for v in &mut value.offsets {
                            *v = v.clamp(-255., 255.);
                        }
                        // ±256 offsets saturate exactly like ±255 for nonnegative multipliers.
                        if original
                            .multipliers
                            .iter()
                            .any(|m| !(0.0..=1e4).contains(m))
                            || original.offsets.iter().any(|o| o.abs() > 256.)
                        {
                            self.report.warning("fla_color_transform",&location,format!("Color effect {:?}/{:?} is outside the renderable range and was clamped.",original.multipliers,original.offsets));
                        }
                    }
                    element.transform.color_transform = value;
                }
                let lib_id = i.library_id.ok_or("Missing library identity")?;
                let symbol_id = self
                    .library
                    .get(&lib_id)
                    .ok_or_else(|| format!("{location}: unresolved library reference"))?
                    .clone();
                let inst_kind = match o.class {
                    TimelineClass::Sprite => SymbolKind::MovieClip,
                    TimelineClass::Button => SymbolKind::Button,
                    _ => SymbolKind::Graphic,
                };
                if let Some(&lib_kind) = self.library_kinds.get(&lib_id)
                    && inst_kind != lib_kind
                {
                    element.instance_behavior = Some(inst_kind);
                }
                let first = u32::from(i.first_frame.unwrap_or(0));
                let playback =
                    if o.class == TimelineClass::Sprite || o.class == TimelineClass::Button {
                        InstancePlayback::Independent
                    } else {
                        match i.loop_mode.unwrap_or(0) {
                            0 if first == 0 => InstancePlayback::Loop,
                            0 => InstancePlayback::LoopFrom(first),
                            1 if first == 0 => InstancePlayback::PlayOnce,
                            1 => InstancePlayback::PlayOnceFrom(first),
                            2 => InstancePlayback::SingleFrame(first),
                            other => {
                                self.report.warning(
                                    "fla_graphic_playback",
                                    &location,
                                    format!(
                                        "Unknown graphic playback mode {other}; the graphic loops."
                                    ),
                                );
                                InstancePlayback::LoopFrom(first)
                            }
                        }
                    };
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback,
                };
                if let Some(ps) = prop_script {
                    element.scripts.push(ps);
                }
                if let Some(script) = self.actions(s, i.actions.as_ref())? {
                    if self.language == ScriptLanguage::ActionScript3
                        || o.class == TimelineClass::Symbol
                    {
                        self.report.warning(
                            "fla_actions_partial",
                            &location,
                            "Object-attached instance scripts are not supported on ActionScript 3 or Graphic instances and remain in the embedded source.",
                        );
                    } else {
                        element.scripts.push(ObjectScript {
                            event: ObjectEvent::Load,
                            script,
                        });
                    }
                }
                if let Some(render_mode) = i.bitmap_render_mode {
                    element.cache_as_bitmap = render_mode == 1;
                }
                let acc_role = if o.class == TimelineClass::Button {
                    AccessibleRole::Button
                } else {
                    AccessibleRole::Image
                };
                element.accessibility =
                    convert_accessibility(s, i.accessibility.as_ref(), &element.name, acc_role)?;
            }
            Some(TimelinePayload::Text(t)) => {
                element = convert_timeline_text(s, t, id, "", self.contents_revision, self.report)?;
            }
            Some(TimelinePayload::Bitmap(b)) => {
                let raw_mat = shape::matrix(b.matrix_words.ok_or("Missing bitmap matrix")?);
                if let Some(media) = b.media_id {
                    if let Some(asset) = self.bitmaps.get(&media) {
                        let mut scaled_mat = raw_mat;
                        scaled_mat[0] *= asset.scale_x;
                        scaled_mat[1] *= asset.scale_x;
                        scaled_mat[2] *= asset.scale_y;
                        scaled_mat[3] *= asset.scale_y;
                        element.transform = Transform::from_matrix(scaled_mat)
                            .or_else(|| Transform::from_matrix(raw_mat))
                            .ok_or("Invalid bitmap matrix")?;
                        element.kind = ElementKind::Bitmap {
                            resource_id: asset.resource_id.clone(),
                            width: asset.width as u32,
                            height: asset.height as u32,
                            smoothing: asset.smoothing,
                        };
                    } else {
                        element.transform =
                            Transform::from_matrix(raw_mat).ok_or("Invalid bitmap matrix")?;
                        let symbol_id = if let Some(id) = self.missing_bitmaps.get(&media) {
                            id.clone()
                        } else {
                            let id = format!("fla-missing-bitmap-{media}");
                            self.generated.push(group_symbol(
                                &id,
                                format!("Unresolved bitmap #{media}"),
                                vec![],
                            ));
                            self.missing_bitmaps.insert(media, id.clone());
                            id
                        };
                        element.kind = ElementKind::Instance {
                            symbol_id,
                            playback: InstancePlayback::SingleFrame(0),
                        };
                        element.name = format!("Unresolved source bitmap #{media}");
                        if self.skipped_bitmaps.contains(&media) {
                            self.report.warning(
                                "fla_bitmap_skipped",
                                &location,
                                format!(
                                    "Bitmap #{media} could not be decoded or exceeded project resource limits; an unresolved placeholder preserves its stage transform."
                                ),
                            );
                        } else {
                            self.report.warning("fla_source_bitmap_missing",&location,format!("Bitmap #{media} is already absent from the source archive. The placement and unresolved reference are retained; no image was invented."));
                        }
                    }
                } else if let Some(span) = b.inline_raster
                    && let Some(raw) = s.original_bytes.get(span.start..span.end)
                    && let Ok(decoded) = crate::legacy::media::decode_binary_bitmap(raw, 0)
                    && let Ok(png) = decoded.png_bytes()
                    && png.len() <= MAX_RESOURCE_BYTES
                {
                    element.transform =
                        Transform::from_matrix(raw_mat).ok_or("Invalid bitmap matrix")?;
                    let res_id = format!("legacy-inline-bitmap-{}-{object_id}", s.index);
                    let res_path = format!("legacy/inline-bitmaps/{}-{object_id}.png", s.index);
                    if let Ok(unit) = ResourceUnit::from_bytes(&res_id, res_path, "image/png", &png)
                    {
                        self.inline_resources.push(unit);
                        element.kind = ElementKind::Bitmap {
                            resource_id: res_id,
                            width: decoded.width,
                            height: decoded.height,
                            smoothing: false,
                        };
                    } else {
                        return Err("Inline bitmap resource exceeded limits".into());
                    }
                } else {
                    return Err("Missing bitmap identity".into());
                }
            }
            Some(TimelinePayload::Shape) => {
                if !o.complete && o.children.is_empty() && o.shape_index.is_none() {
                    return Err("Shape record stopped before completion".into());
                }
                let mut members = Vec::new();
                let mut bitmap_fills = Vec::new();
                let mut appearances = Vec::new();
                let mut vector_effects = Vec::new();
                for (slot, child) in o.children.iter().enumerate() {
                    members.push(self.element_or_placeholder(
                        s,
                        *child,
                        &format!("{id}-member-{slot}"),
                        depth + 1,
                    ))
                }
                if let Some(index) = o.shape_index {
                    let paint_id = format!("{id}-paint");
                    let shape_record = &s.shapes[index].record;
                    match shape::convert_shape_with_effects(
                        shape_record,
                        &paint_id,
                        &self.bitmaps,
                        self.report,
                        &location,
                    ) {
                        Ok(converted) => {
                            members.extend(converted.elements);
                            bitmap_fills.extend(converted.bitmap_fills);
                            appearances.extend(converted.appearances);
                            vector_effects.extend(converted.vector_effects);
                        }
                        Err(e) => {
                            self.report.warning(
                                "fla_shape_skipped",
                                &location,
                                format!(
                                    "Group artwork was not converted: {e}. A placeholder keeps its slot and the original shape remains in the embedded source."
                                ),
                            );
                            if !shape_record.edges.is_empty() {
                                members
                                    .push(self.placeholder_element(paint_id, Transform::default()));
                            }
                        }
                    }
                }
                // A one-frame graphic keeps group membership and vector paint order editable.
                let symbol_id = format!("{id}-group");
                self.generated.push(group_symbol_with_effects(
                    &symbol_id,
                    format!("Source group {object_id}"),
                    members,
                    bitmap_fills,
                    appearances,
                    vector_effects,
                ));
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::SingleFrame(0),
                };
            }
            Some(TimelinePayload::Primitive(prim)) => {
                if o.children.is_empty()
                    && let Some(index) = o.shape_index
                    && let Some(shape_rec) = s.shapes.get(index)
                    && let Some((transform, _origin, kind)) =
                        shape::try_convert_primitive(o.class, prim, &shape_rec.record)
                {
                    element.transform = transform;
                    element.kind = kind;
                } else {
                    let mut members = Vec::new();
                    let mut bitmap_fills = Vec::new();
                    let mut appearances = Vec::new();
                    let mut vector_effects = Vec::new();
                    for (slot, child) in o.children.iter().enumerate() {
                        members.push(self.element_or_placeholder(
                            s,
                            *child,
                            &format!("{id}-member-{slot}"),
                            depth + 1,
                        ));
                    }
                    if let Some(index) = o.shape_index
                        && let Some(shape_rec) = s.shapes.get(index)
                    {
                        let paint_id = format!("{id}-paint");
                        match shape::convert_shape_with_effects(
                            &shape_rec.record,
                            &paint_id,
                            &self.bitmaps,
                            self.report,
                            &location,
                        ) {
                            Ok(converted) => {
                                members.extend(converted.elements);
                                bitmap_fills.extend(converted.bitmap_fills);
                                appearances.extend(converted.appearances);
                                vector_effects.extend(converted.vector_effects);
                            }
                            Err(e) => {
                                self.report.warning(
                                    "fla_shape_skipped",
                                    &location,
                                    format!(
                                        "Primitive artwork was not converted: {e}. A placeholder keeps its slot and the original shape remains in the embedded source."
                                    ),
                                );
                                if !shape_rec.record.edges.is_empty() {
                                    members.push(
                                        self.placeholder_element(paint_id, Transform::default()),
                                    );
                                }
                            }
                        }
                    }
                    let symbol_id = format!("{id}-group");
                    self.generated.push(group_symbol_with_effects(
                        &symbol_id,
                        format!("Source primitive {object_id}"),
                        members,
                        bitmap_fills,
                        appearances,
                        vector_effects,
                    ));
                    element.kind = ElementKind::Instance {
                        symbol_id,
                        playback: InstancePlayback::SingleFrame(0),
                    };
                }
            }
            Some(TimelinePayload::Video(v)) => {
                element.name = optional_text(&s.original_bytes, v.instance_name.as_ref())?;
                let matrix = shape::matrix(v.matrix_words.ok_or("Missing video matrix")?);
                element.transform = Transform::from_matrix(matrix).ok_or("Invalid video matrix")?;
                let asset = v.media_id.and_then(|mid| self.videos.get(&mid));
                let mut width = asset.map_or(160.0, |a| a.width);
                let mut height = asset.map_or(120.0, |a| a.height);
                if let Some([left, right, top, bottom]) = v.bounds_words
                    && left != i32::MIN
                {
                    let x = f64::from(left) / 20.0;
                    let y = f64::from(top) / 20.0;
                    let w = (f64::from(right) - f64::from(left)) / 20.0;
                    let h = (f64::from(bottom) - f64::from(top)) / 20.0;
                    let tx = element.transform.x + matrix[0] * x + matrix[2] * y;
                    let ty = element.transform.y + matrix[1] * x + matrix[3] * y;
                    if tx.is_finite() && ty.is_finite() && tx.abs() <= 1e12 && ty.abs() <= 1e12 {
                        element.transform.x = tx;
                        element.transform.y = ty;
                    }
                    if w.is_finite() && w > 0.0 {
                        width = w.clamp(1.0, 8192.0);
                    }
                    if h.is_finite() && h > 0.0 {
                        height = h.clamp(1.0, 8192.0);
                    }
                }
                element.kind = ElementKind::Video { width, height };
            }
            Some(TimelinePayload::CompiledClip(c)) => {
                element.name = optional_text(&s.original_bytes, c.instance_name.as_ref())?;
                element.transform = Transform::from_matrix(shape::matrix(
                    c.matrix_words.ok_or("Missing compiled clip matrix")?,
                ))
                .ok_or("Invalid compiled clip matrix")?;
                let cc_supports_avm1_props = c
                    .media_id
                    .and_then(|mid| self.compiled_clips.get(&mid))
                    .and_then(|cc| cc.reconstructed.as_ref())
                    .is_some_and(|rec| rec.has_avm1_scripts);
                let (prop_script, prop_lossy) = instance_props::translate_component_properties(
                    &s.original_bytes,
                    c.properties.as_ref(),
                    c.property_count,
                    self.language,
                    cc_supports_avm1_props,
                );
                if prop_lossy {
                    self.report.warning(
                        "fla_instance_settings",
                        &location,
                        "Component parameters or per-instance animation settings are not imported; they remain in the embedded source.",
                    );
                }
                if let Some(ps) = prop_script {
                    element.scripts.push(ps);
                }
                if let Some(script) = self.actions(s, c.actions.as_ref())? {
                    if self.language == ScriptLanguage::ActionScript3 {
                        self.report.warning(
                            "fla_actions_partial",
                            &location,
                            "Object-attached scripts on compiled clips are not supported in ActionScript 3 documents and remain in the embedded source.",
                        );
                    } else {
                        element.scripts.push(ObjectScript {
                            event: ObjectEvent::Load,
                            script,
                        });
                    }
                }
                element.accessibility = convert_accessibility(
                    s,
                    c.accessibility.as_ref(),
                    &element.name,
                    AccessibleRole::Button,
                )?;
                let symbol_id = if let Some(media) = c.media_id {
                    let sym_id = format!("fla-compiled-clip-{media}");
                    if !self.generated.iter().any(|g| g.id == sym_id) {
                        if let Some(rec) = self
                            .compiled_clips
                            .get_mut(&media)
                            .and_then(|cc| cc.reconstructed.take())
                        {
                            self.generated.extend(rec.helper_symbols);
                            self.generated.push(rec.main_symbol);
                            self.compiled_clip_sources.extend(rec.sources);
                        } else {
                            let members = if let Some(cc_asset) = self.compiled_clips.get(&media) {
                                if let Some(icon) = &cc_asset.icon_bitmap {
                                    let mut tf = Transform::default();
                                    if icon.width > 0.0 && icon.height > 0.0 {
                                        tf.scale_x =
                                            (cc_asset.width / icon.width).clamp(0.01, 1000.0);
                                        tf.scale_y =
                                            (cc_asset.height / icon.height).clamp(0.01, 1000.0);
                                    }
                                    vec![Element {
                                        id: format!("{sym_id}-el"),
                                        name: String::new(),
                                        transform: tf,
                                        kind: ElementKind::Bitmap {
                                            resource_id: icon.resource_id.clone(),
                                            width: icon.width as u32,
                                            height: icon.height as u32,
                                            smoothing: icon.smoothing,
                                        },
                                        ..Element::default()
                                    }]
                                } else {
                                    vec![Element {
                                        id: format!("{sym_id}-el"),
                                        name: String::new(),
                                        transform: Transform::default(),
                                        kind: ElementKind::Rectangle {
                                            width: cc_asset.width,
                                            height: cc_asset.height,
                                            fill: [180, 180, 180, 64],
                                            stroke: [120, 120, 120, 160],
                                            stroke_width: 1.0,
                                        },
                                        ..Element::default()
                                    }]
                                }
                            } else {
                                self.report.warning(
                                    "fla_media_not_imported",
                                    &location,
                                    format!(
                                        "Compiled clip placement references unresolved media #{media}; a placeholder symbol keeps its placement."
                                    ),
                                );
                                vec![]
                            };
                            let mut sym =
                                group_symbol(&sym_id, format!("Compiled clip #{media}"), members);
                            sym.kind = SymbolKind::MovieClip;
                            self.generated.push(sym);
                        }
                    }
                    sym_id
                } else {
                    self.report.warning(
                        "fla_media_not_imported",
                        &location,
                        "Compiled clip placement has no media identity; a placeholder symbol keeps its placement.",
                    );
                    if !self.generated.iter().any(|g| g.id == PLACEHOLDER) {
                        self.generated.push(group_symbol(
                            PLACEHOLDER,
                            "Unsupported source object (see import report)".into(),
                            vec![],
                        ));
                    }
                    PLACEHOLDER.to_owned()
                };
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::Independent,
                };
            }
            None if matches!(
                o.class,
                TimelineClass::Shape | TimelineClass::Rect | TimelineClass::Oval
            ) && (!o.children.is_empty() || o.shape_index.is_some()) =>
            {
                self.report.warning(
                    "fla_group_partial",
                    &location,
                    "Group or primitive record stopped before completion; decoded child records and artwork before the stop are preserved.",
                );
                let mut members = Vec::new();
                let mut bitmap_fills = Vec::new();
                let mut appearances = Vec::new();
                let mut vector_effects = Vec::new();
                for (slot, child) in o.children.iter().enumerate() {
                    members.push(self.element_or_placeholder(
                        s,
                        *child,
                        &format!("{id}-member-{slot}"),
                        depth + 1,
                    ));
                }
                if let Some(index) = o.shape_index
                    && let Some(shape_rec) = s.shapes.get(index)
                {
                    let paint_id = format!("{id}-paint");
                    match shape::convert_shape_with_effects(
                        &shape_rec.record,
                        &paint_id,
                        &self.bitmaps,
                        self.report,
                        &location,
                    ) {
                        Ok(converted) => {
                            members.extend(converted.elements);
                            bitmap_fills.extend(converted.bitmap_fills);
                            appearances.extend(converted.appearances);
                            vector_effects.extend(converted.vector_effects);
                        }
                        Err(e) => {
                            self.report.warning(
                                "fla_shape_skipped",
                                &location,
                                format!(
                                    "Group artwork was not converted: {e}. A placeholder keeps its slot and the original shape remains in the embedded source."
                                ),
                            );
                            if !shape_rec.record.edges.is_empty() {
                                members
                                    .push(self.placeholder_element(paint_id, Transform::default()));
                            }
                        }
                    }
                }
                let symbol_id = format!("{id}-group");
                self.generated.push(group_symbol_with_effects(
                    &symbol_id,
                    format!("Source group {object_id}"),
                    members,
                    bitmap_fills,
                    appearances,
                    vector_effects,
                ));
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::SingleFrame(0),
                };
            }
            Some(TimelinePayload::Auxiliary(aux)) => {
                let tf = aux
                    .matrix_words
                    .and_then(|m| Transform::from_matrix(shape::matrix(m)))
                    .unwrap_or_default();
                if o.class == TimelineClass::TlfText {
                    self.report.warning(
                        "fla_text_approximated",
                        &location,
                        "TLF text record is retained in the embedded source; an empty placeholder preserves its placement.",
                    );
                } else {
                    self.report.warning(
                        "fla_object_placeholder",
                        &location,
                        format!(
                            "Auxiliary {:?} record is retained in the embedded source; an empty placeholder keeps its place.",
                            o.class
                        ),
                    );
                }
                element = self.placeholder_element(id.into(), tf);
            }
            _ => {
                return Err(format!(
                    "{location}: unsupported display record {:?}",
                    o.class
                ));
            }
        }
        if !matches!(
            &o.payload,
            Some(TimelinePayload::Instance(i)) if i.bitmap_render_mode.is_some()
        ) && o
            .base
            .as_ref()
            .is_some_and(|b| b.cache_as_bitmap.unwrap_or(0) != 0)
        {
            element.cache_as_bitmap = true;
        }
        self.provenance.push(
            serde_json::json!({"stream":s.index,"object":object_id,"target":id,"source":o.encoded}),
        );
        Ok(element)
    }
}
/// Graphic first-frame indices beyond the referenced symbol's length: Flash
/// wraps looping graphics and shows the last frame otherwise.
fn normalize_playback(symbols: &mut [Symbol], report: &mut ImportReport) {
    let lengths: BTreeMap<_, _> = symbols
        .iter()
        .map(|s| (s.id.clone(), s.timeline.frame_count.max(1)))
        .collect();
    for symbol in symbols.iter_mut() {
        for element in symbol
            .timeline
            .layers
            .iter_mut()
            .flat_map(|l| &mut l.keyframes)
            .flat_map(|k| &mut k.elements)
        {
            let ElementKind::Instance {
                symbol_id,
                playback,
            } = &mut element.kind
            else {
                continue;
            };
            let Some(&len) = lengths.get(symbol_id) else {
                continue;
            };
            if len == 1 {
                *playback = match *playback {
                    InstancePlayback::SingleFrame(_) => InstancePlayback::SingleFrame(0),
                    InstancePlayback::PlayOnceFrom(_) => InstancePlayback::PlayOnce,
                    InstancePlayback::LoopFrom(_) => InstancePlayback::Loop,
                    other => other,
                };
                continue;
            }
            if let InstancePlayback::LoopFrom(f) = *playback
                && f >= len
                && f % len == 0
            {
                // Exact multiples of the symbol's loop length normalize cleanly in Flash.
                *playback = InstancePlayback::Loop;
                continue;
            }
            let fixed = match *playback {
                InstancePlayback::LoopFrom(f) if f >= len => InstancePlayback::LoopFrom(f % len),
                InstancePlayback::SingleFrame(f) if f >= len => {
                    InstancePlayback::SingleFrame(len - 1)
                }
                InstancePlayback::PlayOnceFrom(f) if f >= len => {
                    InstancePlayback::PlayOnceFrom(len - 1)
                }
                _ => continue,
            };
            report.warning("fla_graphic_first_frame",&element.id,format!("The graphic's first frame lies beyond the symbol's {len} frames; it is mapped to {fixed:?}."));
            *playback = fixed;
        }
    }
}
fn stream_has_executable_actions(stream: &BinaryTimelineStream) -> bool {
    stream.objects.iter().any(|o| match &o.payload {
        Some(TimelinePayload::Frame(f)) => f
            .actions
            .as_ref()
            .is_some_and(|a| a.presence != TimelineActionPresence::Empty),
        Some(TimelinePayload::Instance(i)) => i
            .actions
            .as_ref()
            .is_some_and(|a| a.presence != TimelineActionPresence::Empty),
        Some(TimelinePayload::CompiledClip(cc)) => cc
            .actions
            .as_ref()
            .is_some_and(|a| a.presence != TimelineActionPresence::Empty),
        _ => false,
    })
}
fn resolve_include_logical_path(parent: &str, requested: &str) -> Option<String> {
    if requested.is_empty() || requested.starts_with('/') || requested.contains(['\\', ':', '\0']) {
        return None;
    }
    let mut parts: Vec<_> = parent
        .rsplit_once('/')
        .map_or("", |(base, _)| base)
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    for part in requested.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            value => parts.push(value),
        }
    }
    let result = parts.join("/");
    let lower = result.to_ascii_lowercase();
    (lower.ends_with(".as") || lower.ends_with(".asi")).then_some(result)
}
fn scan_include_directives(source: &str, parent_logical: &str, out: &mut Vec<String>) {
    for line in source.split(['\r', '\n']) {
        let trimmed = line.trim();
        let raw_arg = if let Some(rest) = trimmed.strip_prefix("#include")
            && (rest.is_empty()
                || rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\''))
        {
            Some(rest.trim())
        } else if let Some(rest) = trimmed.strip_prefix("include")
            && rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        {
            Some(rest.trim())
        } else {
            None
        };
        let Some(raw_arg) = raw_arg else {
            continue;
        };
        let no_comment = raw_arg.split("//").next().unwrap_or(raw_arg).trim();
        let argument = no_comment.strip_suffix(';').map_or(no_comment, str::trim);
        let Some(requested) = argument
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| {
                argument
                    .strip_prefix('\'')
                    .and_then(|v| v.strip_suffix('\''))
            })
            .filter(|v| !v.contains(['"', '\'', '\\']))
        else {
            continue;
        };
        if let Some(logical) = resolve_include_logical_path(parent_logical, requested) {
            out.push(logical);
        }
    }
}
const MAX_COMPANION_INCLUDE_FILE_BYTES: usize = 1 << 20;
const MAX_COMPANION_INCLUDE_TOTAL_BYTES: usize = 4 << 20;

fn resolve_safe_companion_include_path(
    canon_base: &std::path::Path,
    logical: &str,
) -> Option<std::path::PathBuf> {
    let segments: Vec<&str> = logical.split('/').collect();
    if segments.is_empty() {
        return None;
    }
    let mut cur = canon_base.to_path_buf();
    for (idx, seg) in segments.iter().enumerate() {
        if seg.is_empty() || matches!(*seg, "." | "..") || seg.contains(['/', '\\', ':', '\0']) {
            return None;
        }
        let exact = cur.join(seg);
        let next_path = if std::fs::symlink_metadata(&exact).is_ok() {
            exact
        } else {
            let entry = std::fs::read_dir(&cur).ok()?.flatten().find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(seg)
            })?;
            entry.path()
        };
        let meta = std::fs::symlink_metadata(&next_path).ok()?;
        if meta.file_type().is_symlink() {
            return None;
        }
        let is_last = idx + 1 == segments.len();
        if is_last {
            if !meta.file_type().is_file() || meta.len() > MAX_COMPANION_INCLUDE_FILE_BYTES as u64 {
                return None;
            }
            let valid_ext = next_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("as") || e.eq_ignore_ascii_case("asi"));
            if !valid_ext {
                return None;
            }
        } else if !meta.file_type().is_dir() {
            return None;
        }
        cur = next_path;
    }
    let canon = std::fs::canonicalize(&cur).ok()?;
    if !canon.starts_with(canon_base) || canon == *canon_base {
        return None;
    }
    let valid_canon_ext = canon
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("as") || e.eq_ignore_ascii_case("asi"));
    valid_canon_ext.then_some(canon)
}

fn attach_companion_includes(
    source_path: &str,
    symbols: &[Symbol],
    resources: &mut Vec<ResourceUnit>,
) {
    let Some(base_dir) = std::path::Path::new(source_path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && p.is_dir())
    else {
        return;
    };
    let Ok(canon_base) = std::fs::canonicalize(base_dir) else {
        return;
    };
    if !canon_base.is_dir() {
        return;
    }
    let mut queue = Vec::new();
    for symbol in symbols {
        for frame in &symbol.timeline.scripts {
            scan_include_directives(&frame.script.source, "", &mut queue);
        }
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for element in &key.elements {
                    for handler in &element.scripts {
                        scan_include_directives(&handler.script.source, "", &mut queue);
                    }
                }
            }
        }
    }
    if queue.is_empty() {
        return;
    }
    let mut seen_lower = std::collections::BTreeSet::new();
    for existing in resources.iter() {
        seen_lower.insert(existing.path.to_ascii_lowercase());
    }
    let mut used_bytes: usize = resources.iter().map(ResourceUnit::byte_len).sum();
    let mut companion_bytes = 0usize;
    let mut include_idx = 0usize;
    let mut cursor = 0usize;
    while cursor < queue.len() && include_idx < 64 {
        let logical = queue[cursor].clone();
        cursor += 1;
        let lower = logical.to_ascii_lowercase();
        if !seen_lower.insert(lower) || resources.len() >= MAX_RESOURCES {
            continue;
        }
        let Some(safe_path) = resolve_safe_companion_include_path(&canon_base, &logical) else {
            continue;
        };
        if std::fs::symlink_metadata(&safe_path)
            .ok()
            .is_none_or(|m| m.file_type().is_symlink() || !m.file_type().is_file())
        {
            continue;
        }
        let Ok(raw) = std::fs::read(&safe_path) else {
            continue;
        };
        if raw.len() > MAX_COMPANION_INCLUDE_FILE_BYTES {
            continue;
        }
        let without_bom = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw);
        let decoded = match std::str::from_utf8(without_bom) {
            Ok(s) => s.to_owned(),
            Err(_) => encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(without_bom)
                .0
                .into_owned(),
        };
        let normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
        let bytes = normalized.as_bytes();
        if bytes.len() > MAX_COMPANION_INCLUDE_FILE_BYTES
            || companion_bytes.saturating_add(bytes.len()) > MAX_COMPANION_INCLUDE_TOTAL_BYTES
            || used_bytes.saturating_add(bytes.len()) > MAX_RESOURCE_TOTAL_BYTES
        {
            continue;
        }
        include_idx += 1;
        if let Ok(unit) = ResourceUnit::from_bytes(
            format!("fla-include-{include_idx}"),
            logical.clone(),
            "text/x-actionscript",
            bytes,
        ) {
            used_bytes = used_bytes.saturating_add(bytes.len());
            companion_bytes = companion_bytes.saturating_add(bytes.len());
            resources.push(unit);
            scan_include_directives(&normalized, &logical, &mut queue);
        }
    }
}
/// Stable content digest for provenance; not a security hash.
fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
fn group_symbol(id: &str, name: String, elements: Vec<Element>) -> Symbol {
    group_symbol_with_effects(id, name, elements, Vec::new(), Vec::new(), Vec::new())
}
fn group_symbol_with_effects(
    id: &str,
    name: String,
    elements: Vec<Element>,
    bitmap_fills: Vec<AuthoredBitmapFill>,
    appearances: Vec<ElementAppearance>,
    vector_effects: Vec<ElementVectorEffects>,
) -> Symbol {
    Symbol {
        id: id.into(),
        name,
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            guides: vec![],
            frame_count: 1,
            layers: vec![Layer {
                id: format!("{id}-layer"),
                name: "Members".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements,
                    bitmap_fills,
                    appearances,
                    vector_effects,
                    tween: None,
                    label: String::new(),
                    ..Default::default()
                }],
                properties: Default::default(),
            }],
            scripts: vec![],
        },
        class_name: None,
        linkage_id: None,
    }
}
fn invert_affine_2d(m: [f64; 6]) -> Option<[f64; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if !det.is_finite() || det.abs() <= 1e-8 {
        return None;
    }
    let inv_det = 1.0 / det;
    let a = m[3] * inv_det;
    let b = -m[1] * inv_det;
    let c = -m[2] * inv_det;
    let d = m[0] * inv_det;
    let tx = -(a * m[4] + c * m[5]);
    let ty = -(b * m[4] + d * m[5]);
    let inv = [a, b, c, d, tx, ty];
    inv.iter()
        .all(|v| v.is_finite() && v.abs() <= 1e9)
        .then_some(inv)
}

fn hoist_generated_group_transform(generated: &mut [Symbol], el: &mut Element) {
    let ElementKind::Instance { symbol_id, .. } = &el.kind else {
        return;
    };
    if !symbol_id.ends_with("-group") {
        return;
    }
    let Some(sym) = generated.iter_mut().find(|s| s.id == *symbol_id) else {
        return;
    };
    if sym.timeline.layers.len() != 1 || sym.timeline.layers[0].keyframes.len() != 1 {
        return;
    }
    let kf = &mut sym.timeline.layers[0].keyframes[0];
    let Some(first) = kf.elements.first() else {
        return;
    };
    let geom = Transform {
        x: first.transform.x,
        y: first.transform.y,
        scale_x: first.transform.scale_x,
        scale_y: first.transform.scale_y,
        rotation: first.transform.rotation,
        skew: first.transform.skew,
        ..Transform::default()
    };
    if geom == Transform::default() {
        return;
    }
    let g_mat = neoflash_core::transform_matrix(geom);
    let Some(inv_g) = invert_affine_2d(g_mat) else {
        return;
    };
    let outer_mat =
        neoflash_core::affine_multiply(neoflash_core::transform_matrix(el.transform), g_mat);
    let Some(new_outer) = Transform::from_matrix(outer_mat) else {
        return;
    };
    let mut updated_members = Vec::with_capacity(kf.elements.len());
    for member in &kf.elements {
        let rel_mat = neoflash_core::affine_multiply(
            inv_g,
            neoflash_core::transform_matrix(member.transform),
        );
        let Some(rel_tf) = Transform::from_matrix(rel_mat) else {
            return;
        };
        updated_members.push(rel_tf);
    }
    for (member, rel_tf) in kf.elements.iter_mut().zip(updated_members) {
        member.transform.x = rel_tf.x;
        member.transform.y = rel_tf.y;
        member.transform.scale_x = rel_tf.scale_x;
        member.transform.scale_y = rel_tf.scale_y;
        member.transform.rotation = rel_tf.rotation;
        member.transform.skew = rel_tf.skew;
    }
    el.transform.x = new_outer.x;
    el.transform.y = new_outer.y;
    el.transform.scale_x = new_outer.scale_x;
    el.transform.scale_y = new_outer.scale_y;
    el.transform.rotation = new_outer.rotation;
    el.transform.skew = new_outer.skew;
}

fn group_keyframes_structurally_equal(a: &Keyframe, b: &Keyframe) -> bool {
    if a.elements.len() != b.elements.len()
        || a.bitmap_fills.len() != b.bitmap_fills.len()
        || a.appearances.len() != b.appearances.len()
        || a.vector_effects.len() != b.vector_effects.len()
    {
        return false;
    }
    for (ea, eb) in a.elements.iter().zip(&b.elements) {
        if ea.name != eb.name
            || ea.transform != eb.transform
            || ea.kind != eb.kind
            || ea.visible != eb.visible
            || ea.blend_mode != eb.blend_mode
            || ea.filters != eb.filters
        {
            return false;
        }
    }
    for (bfa, bfb) in a.bitmap_fills.iter().zip(&b.bitmap_fills) {
        if bfa.resource_id != bfb.resource_id
            || bfa.matrix != bfb.matrix
            || bfa.repeat != bfb.repeat
            || bfa.smoothing != bfb.smoothing
        {
            return false;
        }
    }
    for (apa, apb) in a.appearances.iter().zip(&b.appearances) {
        if apa.entries != apb.entries {
            return false;
        }
    }
    for (va, vb) in a.vector_effects.iter().zip(&b.vector_effects) {
        if va.procedural_effects != vb.procedural_effects
            || va.freeform_gradient != vb.freeform_gradient
            || va.gradient_mesh != vb.gradient_mesh
            || va.envelope_warp != vb.envelope_warp
            || va.vector_blend != vb.vector_blend
        {
            return false;
        }
    }
    true
}

fn deduplicate_tweened_group_endpoint(
    generated: &mut Vec<Symbol>,
    provenance: &mut [serde_json::Value],
    prev_el: &Element,
    curr_el: &mut Element,
) {
    let (
        ElementKind::Instance {
            symbol_id: a_id, ..
        },
        ElementKind::Instance {
            symbol_id: b_id,
            playback,
        },
    ) = (&prev_el.kind, &curr_el.kind)
    else {
        return;
    };
    if a_id == b_id || !a_id.ends_with("-group") || !b_id.ends_with("-group") {
        return;
    }
    let Some(a_prefix) = a_id.strip_suffix("-group").map(str::to_owned) else {
        return;
    };
    let Some(b_prefix) = b_id.strip_suffix("-group").map(str::to_owned) else {
        return;
    };
    let Some(sym_a) = generated.iter().find(|s| s.id == *a_id) else {
        return;
    };
    let Some(sym_b) = generated.iter().find(|s| s.id == *b_id) else {
        return;
    };
    if sym_a.timeline.layers.len() == 1
        && sym_b.timeline.layers.len() == 1
        && sym_a.timeline.layers[0].keyframes.len() == 1
        && sym_b.timeline.layers[0].keyframes.len() == 1
        && group_keyframes_structurally_equal(
            &sym_a.timeline.layers[0].keyframes[0],
            &sym_b.timeline.layers[0].keyframes[0],
        )
    {
        let old_b = b_id.clone();
        let reused_a = a_id.clone();
        let pb = *playback;
        let b_member_prefix = format!("{b_prefix}-member-");
        let a_member_prefix = format!("{a_prefix}-member-");
        let b_paint_id = format!("{b_prefix}-paint");
        let a_paint_id = format!("{a_prefix}-paint");
        generated.retain(|s| {
            s.id != old_b && !(s.id.starts_with(&b_member_prefix) && s.id.ends_with("-group"))
        });
        for entry in provenance.iter_mut() {
            let Some(current) = entry.get("target").and_then(|v| v.as_str()) else {
                continue;
            };
            if current == b_paint_id {
                entry["target"] = serde_json::Value::String(a_paint_id.clone());
            } else if let Some(rest) = current.strip_prefix(&b_member_prefix) {
                entry["target"] = serde_json::Value::String(format!("{a_member_prefix}{rest}"));
            }
        }
        curr_el.kind = ElementKind::Instance {
            symbol_id: reused_a,
            playback: pb,
        };
    }
}

fn placement_matches(a: &Element, b: &Element) -> bool {
    a.name == b.name
        && match (&a.kind, &b.kind) {
            (
                ElementKind::Instance { symbol_id: a, .. },
                ElementKind::Instance { symbol_id: b, .. },
            ) => a == b,
            (
                ElementKind::Bitmap { resource_id: a, .. },
                ElementKind::Bitmap { resource_id: b, .. },
            ) => a == b,
            (
                ElementKind::TextBox {
                    text: ta,
                    width: wa,
                    height: ha,
                    format: fa,
                    word_wrap: wwa,
                    multiline: ma,
                },
                ElementKind::TextBox {
                    text: tb,
                    width: wb,
                    height: hb,
                    format: fb,
                    word_wrap: wwb,
                    multiline: mb,
                },
            ) => {
                ta == tb
                    && (wa - wb).abs() < 1e-6
                    && (ha - hb).abs() < 1e-6
                    && fa == fb
                    && wwa == wwb
                    && ma == mb
            }
            _ => false,
        }
}
fn color_transform(c: &TimelineColorTransform) -> Result<ColorTransform, String> {
    let mut result = ColorTransform::default();
    match c.effect_type.unwrap_or(0) {
        0 => {}
        1 => {
            let p = c.effect_percent.unwrap_or(0) as f64 / 100.;
            for k in 0..3 {
                result.multipliers[k] = 1. - p.abs();
                result.offsets[k] = p.max(0.) * 255.;
            }
        }
        2 => {
            let p = c.effect_percent.unwrap_or(0) as f64 / 100.;
            let color = shape::color(c.effect_rgba.unwrap_or(0));
            for (k, ch) in color.iter().enumerate().take(3) {
                result.multipliers[k] = 1. - p;
                result.offsets[k] = p * f64::from(*ch);
            }
        }
        4 => result.multipliers[3] = c.multipliers[3] as f64 / 256.,
        3 => {
            result.multipliers = c.multipliers.map(|v| v as f64 / 256.);
            result.offsets = c.offsets.map(f64::from);
        }
        _ => return Err("Unknown color effect".into()),
    }
    Ok(result)
}
/// Tween easing and interpolation kind from the frame key mode
/// (low byte: 0 none, 1 classic motion, 2 shape, 3 motion object, 4 IK pose).
fn ease(f: &TimelineFrame) -> Result<(Option<Easing>, TweenKind), String> {
    let classic = || Easing::Flash {
        acceleration: f.acceleration.unwrap_or(0) as i16 as f64,
    };
    match f.key_mode.unwrap_or(0) & 255 {
        0 if f.morph_shape.is_some() => Ok((Some(classic()), TweenKind::Shape)),
        0 => Ok((None, TweenKind::Classic)),
        1 => {
            if f.custom_ease_present.unwrap_or(0) != 0 {
                if f.single_ease_curve != Some(1) {
                    return Err("Separate per-property easing curves are not supported".into());
                }
                let c = f
                    .custom_ease
                    .get(5)
                    .ok_or("Missing global custom-ease curve")?;
                if c.len() != 2 || c[0][2..4] != [0., 0.] || c[1][2..4] != [1., 1.] {
                    return Err("Multi-segment custom easing is not supported".into());
                }
                Ok((
                    Some(Easing::CubicBezier {
                        x1: c[0][4],
                        y1: c[0][5],
                        x2: c[1][0],
                        y2: c[1][1],
                    }),
                    TweenKind::Classic,
                ))
            } else {
                Ok((Some(classic()), TweenKind::Classic))
            }
        }
        2 => Ok((Some(classic()), TweenKind::Shape)),
        // Motion-object and IK spans keep their stored keyframe poses.
        _ => Ok((None, TweenKind::Classic)),
    }
}

fn apply_affine_point(m: [f64; 6], pt: [f64; 2]) -> [f64; 2] {
    [
        m[0] * pt[0] + m[2] * pt[1] + m[4],
        m[1] * pt[0] + m[3] * pt[1] + m[5],
    ]
}

fn point_dist_sq(a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

fn best_hint_stage_point(
    geom: Option<&ShapeHintGeometry>,
    raw_px: [f64; 2],
    matrix: Option<[f64; 6]>,
) -> [f64; 2] {
    let (Some(g), Some(m)) = (geom, matrix) else {
        return raw_px;
    };
    let transformed = apply_affine_point(m, raw_px);
    if (transformed[0] - raw_px[0]).abs() < 1e-6 && (transformed[1] - raw_px[1]).abs() < 1e-6 {
        return raw_px;
    }
    let d_raw = point_dist_sq(
        raw_px,
        g.nearest_with_snap(raw_px, DEFAULT_HINT_VERTEX_SNAP_RADIUS),
    );
    let d_xform = point_dist_sq(
        transformed,
        g.nearest_with_snap(transformed, DEFAULT_HINT_VERTEX_SNAP_RADIUS),
    );
    if d_xform + 1e-6 < d_raw {
        transformed
    } else {
        raw_px
    }
}

fn point_to_hint_uv(geom: Option<&ShapeHintGeometry>, pt: [f64; 2]) -> [f64; 2] {
    if !pt[0].is_finite() || !pt[1].is_finite() {
        return [0.5, 0.5];
    }
    if let Some(g) = geom {
        let diag = (g.high[0] - g.low[0]).hypot(g.high[1] - g.low[1]);
        let snap_tol = (diag * 0.15).clamp(2.0, 24.0);
        let snapped = g.nearest_with_snap(pt, DEFAULT_HINT_VERTEX_SNAP_RADIUS);
        if point_dist_sq(pt, snapped) <= snap_tol * snap_tol {
            return g.snapped_uv(pt);
        }
        return std::array::from_fn(|axis| {
            let size = g.high[axis] - g.low[axis];
            if size > 1e-9 {
                ((pt[axis] - g.low[axis]) / size).clamp(0.0, 1.0)
            } else {
                0.5
            }
        });
    }
    [
        (pt[0] / 550.0).clamp(0.0, 1.0),
        (pt[1] / 400.0).clamp(0.0, 1.0),
    ]
}

fn resolve_fla_morph_hints(
    s: &BinaryTimelineStream,
    morph_id: u32,
    start_elements: &[Element],
    end_elements: &[Element],
) -> (Vec<ShapeHint>, bool) {
    let Ok(morph_obj) = object(s, morph_id) else {
        return (Vec::new(), true);
    };
    let Some(TimelinePayload::Morph(morph)) = &morph_obj.payload else {
        return (Vec::new(), true);
    };
    if morph.hints.is_empty() {
        return (Vec::new(), false);
    }
    let start_geom = ShapeHintGeometry::from_elements(start_elements);
    let end_geom = ShapeHintGeometry::from_elements(end_elements);
    let mat_a = morph.matrix_words_start.map(shape::matrix);
    let mat_b = morph.matrix_words_end.map(shape::matrix);

    let mut seen_labels = std::collections::HashSet::new();
    let mut hints = Vec::new();
    let mut had_unconverted = morph.hints.len() > ShapeHint::MAX_HINTS;
    for raw_hint_id in &morph.hints {
        if hints.len() >= ShapeHint::MAX_HINTS {
            had_unconverted = true;
            break;
        }
        let Some(hint_id) = raw_hint_id else {
            had_unconverted = true;
            continue;
        };
        let Ok(hint_obj) = object(s, *hint_id) else {
            had_unconverted = true;
            continue;
        };
        let Some(TimelinePayload::MorphHint(hint)) = &hint_obj.payload else {
            had_unconverted = true;
            continue;
        };
        let (Some(pa), Some(pb)) = (hint.point_a, hint.point_b) else {
            had_unconverted = true;
            continue;
        };
        let label = hint
            .index
            .and_then(|idx| (idx < 26).then(|| (b'a' + idx as u8) as char))
            .filter(|c| !seen_labels.contains(c))
            .or_else(|| ('a'..='z').find(|c| !seen_labels.contains(c)));
        let Some(label) = label else {
            break;
        };
        seen_labels.insert(label);

        let raw_a = [pa[0] as f64 / 20.0, pa[1] as f64 / 20.0];
        let raw_b = [pb[0] as f64 / 20.0, pb[1] as f64 / 20.0];
        let pt_a = best_hint_stage_point(start_geom.as_ref(), raw_a, mat_a);
        let pt_b = best_hint_stage_point(end_geom.as_ref(), raw_b, mat_b);
        hints.push(ShapeHint {
            label,
            start_uv: point_to_hint_uv(start_geom.as_ref(), pt_a),
            end_uv: point_to_hint_uv(end_geom.as_ref(), pt_b),
        });
    }
    hints.sort_by_key(|h| h.label);
    (hints, had_unconverted)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProvenanceDetail {
    Full,
    NoPlacements,
    Minimal,
}

fn update_provenance_resource(
    project: &mut Project,
    detail: &mut ProvenanceDetail,
    warned: &mut bool,
    embedded: bool,
    has_placements: bool,
    report: &mut ImportReport,
    make_provenance: &dyn Fn(bool, ProvenanceDetail) -> serde_json::Value,
) -> Result<(), String> {
    let other_decoded: usize = project
        .resources
        .iter()
        .filter(|r| r.id != "fla-provenance")
        .map(ResourceUnit::byte_len)
        .sum();
    let candidates: &[ProvenanceDetail] = match *detail {
        ProvenanceDetail::Full => &[
            ProvenanceDetail::Full,
            ProvenanceDetail::NoPlacements,
            ProvenanceDetail::Minimal,
        ],
        ProvenanceDetail::NoPlacements => {
            &[ProvenanceDetail::NoPlacements, ProvenanceDetail::Minimal]
        }
        ProvenanceDetail::Minimal => &[ProvenanceDetail::Minimal],
    };
    let mut chosen = None;
    for &candidate in candidates {
        let bytes =
            serde_json::to_vec(&make_provenance(embedded, candidate)).map_err(|e| e.to_string())?;
        if bytes.len() <= MAX_RESOURCE_BYTES
            && other_decoded.saturating_add(bytes.len()) <= MAX_RESOURCE_TOTAL_BYTES
        {
            chosen = Some((candidate, bytes));
            break;
        }
    }
    let (chosen_detail, bytes) =
        chosen.ok_or_else(|| "Import provenance exceeds the project resource budget".to_owned())?;
    if chosen_detail != ProvenanceDetail::Full && !*warned && has_placements {
        *warned = true;
        report.warning(
            "fla_provenance_compacted",
            "legacy/provenance.json",
            "Per-placement source offsets were omitted from provenance to fit project size limits; source digest, stream descriptors and container metadata are retained.",
        );
    }
    *detail = chosen_detail;
    let unit = ResourceUnit::from_bytes(
        "fla-provenance",
        "legacy/provenance.json",
        "application/json",
        &bytes,
    )
    .map_err(|e| e.to_string())?;
    if let Some(slot) = project
        .resources
        .iter_mut()
        .find(|r| r.id == "fla-provenance")
    {
        *slot = unit;
    } else {
        project.resources.push(unit);
    }
    Ok(())
}

fn replace_group_prefix(id: &mut String, old_prefix: &str) {
    if let Some(rest) = id.strip_prefix(old_prefix) {
        *id = format!("$G{rest}");
    }
}

fn normalized_group_timeline_signature(timeline: &Timeline, prefix: &str) -> Option<String> {
    let mut clone = timeline.clone();
    for layer in &mut clone.layers {
        replace_group_prefix(&mut layer.id, prefix);
        for key in &mut layer.keyframes {
            for element in &mut key.elements {
                replace_group_prefix(&mut element.id, prefix);
            }
            for bf in &mut key.bitmap_fills {
                replace_group_prefix(&mut bf.element_id, prefix);
            }
            for app in &mut key.appearances {
                replace_group_prefix(&mut app.element_id, prefix);
            }
            for fx in &mut key.vector_effects {
                replace_group_prefix(&mut fx.element_id, prefix);
            }
        }
    }
    serde_json::to_string(&clone).ok()
}

fn deduplicate_project_for_budget(
    project: &mut Project,
    provenance: &mut [serde_json::Value],
) -> bool {
    let mut seen_media: std::collections::HashMap<(String, std::sync::Arc<str>), String> =
        std::collections::HashMap::new();
    let mut resource_remap: BTreeMap<String, String> = BTreeMap::new();
    for res in &project.resources {
        if !(res.id.starts_with("legacy-sound-") || res.id.starts_with("legacy-bitmap-")) {
            continue;
        }
        let key = (res.media_type.clone(), res.data_base64.clone());
        if let Some(canon) = seen_media.get(&key) {
            resource_remap.insert(res.id.clone(), canon.clone());
        } else {
            seen_media.insert(key, res.id.clone());
        }
    }
    if !resource_remap.is_empty() {
        project
            .resources
            .retain(|r| !resource_remap.contains_key(&r.id));
        for symbol in &mut project.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    if let Some(sound) = &mut key.sound
                        && let Some(canon) = resource_remap.get(&sound.resource)
                    {
                        sound.resource = canon.clone();
                    }
                    for bf in &mut key.bitmap_fills {
                        if let Some(canon) = resource_remap.get(&bf.resource_id) {
                            bf.resource_id = canon.clone();
                        }
                    }
                    for element in &mut key.elements {
                        if let ElementKind::Bitmap { resource_id, .. } = &mut element.kind
                            && let Some(canon) = resource_remap.get(resource_id)
                        {
                            *resource_id = canon.clone();
                        }
                    }
                }
            }
        }
    }

    let protected: std::collections::HashSet<String> = std::iter::once(project.root.clone())
        .chain(project.scenes.iter().cloned())
        .collect();
    let mut seen_groups: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    let mut group_remap: BTreeMap<String, (String, String, String)> = BTreeMap::new();

    for symbol in &mut project.symbols {
        if !group_remap.is_empty() {
            for element in symbol
                .timeline
                .layers
                .iter_mut()
                .flat_map(|l| &mut l.keyframes)
                .flat_map(|k| &mut k.elements)
            {
                if let ElementKind::Instance { symbol_id, .. } = &mut element.kind
                    && let Some((canon_id, _, _)) = group_remap.get(symbol_id)
                {
                    *symbol_id = canon_id.clone();
                }
            }
        }
        if symbol.kind != SymbolKind::Graphic
            || symbol.class_name.is_some()
            || symbol.linkage_id.is_some()
            || protected.contains(&symbol.id)
            || symbol.id == PLACEHOLDER
            || symbol.id.starts_with("fla-missing-bitmap-")
        {
            continue;
        }
        let Some(prefix) = symbol.id.strip_suffix("-group").map(str::to_owned) else {
            continue;
        };
        let Some(sig) = normalized_group_timeline_signature(&symbol.timeline, &prefix) else {
            continue;
        };
        if let Some((canon_id, canon_prefix)) = seen_groups.get(&sig) {
            group_remap.insert(
                symbol.id.clone(),
                (canon_id.clone(), prefix, canon_prefix.clone()),
            );
        } else {
            seen_groups.insert(sig, (symbol.id.clone(), prefix));
        }
    }

    if !group_remap.is_empty() {
        project.symbols.retain(|s| !group_remap.contains_key(&s.id));
        for symbol in &mut project.symbols {
            for element in symbol
                .timeline
                .layers
                .iter_mut()
                .flat_map(|l| &mut l.keyframes)
                .flat_map(|k| &mut k.elements)
            {
                if let ElementKind::Instance { symbol_id, .. } = &mut element.kind
                    && let Some((canon_id, _, _)) = group_remap.get(symbol_id)
                {
                    *symbol_id = canon_id.clone();
                }
            }
        }
        let prefix_pairs: Vec<(String, String)> = group_remap
            .values()
            .map(|(_, old_p, new_p)| (format!("{old_p}-member-"), format!("{new_p}-member-")))
            .collect();
        for entry in provenance.iter_mut() {
            let Some(current) = entry.get("target").and_then(|v| v.as_str()) else {
                continue;
            };
            let mut rewritten = current.to_owned();
            let mut changed = false;
            for _ in 0..48 {
                let mut step = false;
                for (old_member, new_member) in &prefix_pairs {
                    if let Some(rest) = rewritten.strip_prefix(old_member) {
                        rewritten = format!("{new_member}{rest}");
                        changed = true;
                        step = true;
                        break;
                    }
                }
                if !step {
                    break;
                }
            }
            if changed {
                entry["target"] = serde_json::Value::String(rewritten);
            }
        }
    }

    !resource_remap.is_empty() || !group_remap.is_empty()
}

#[allow(clippy::too_many_arguments)]
fn fit_project_json_budget(
    project: &mut Project,
    json: &mut String,
    embedded: &mut bool,
    prov_detail: &mut ProvenanceDetail,
    prov_warned: &mut bool,
    original_len: usize,
    has_placements: bool,
    report: &mut ImportReport,
    make_provenance: &dyn Fn(bool, ProvenanceDetail) -> serde_json::Value,
) -> Result<(), String> {
    if *embedded {
        project
            .resources
            .retain(|r| r.media_type != "application/x-neoflash-fla-source");
        *embedded = false;
        report.warning(
            "fla_source_not_embedded",
            "Original FLA",
            format!(
                "The original FLA ({} MiB) does not fit the project's limits next to the converted document (64 MiB of resources, 128 MiB in total), so it is not embedded. Keep the original file; provenance records its size and FNV-1a digest.",
                original_len / (1024 * 1024)
            ),
        );
        update_provenance_resource(
            project,
            prov_detail,
            prov_warned,
            false,
            has_placements,
            report,
            make_provenance,
        )?;
        *json = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    }

    // 0. Shed imported video resources (legacy-video-*, largest first) before audio/bitmaps/shapes.
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let excess = json.len() - MAX_PROJECT_JSON_BYTES;
        if !shed_video_resources(project, report, excess) {
            break;
        }
        project.format_version = 5;
        project.upgrade_format_for_features();
        *json = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    }

    // 1. Shed imported audio resources (legacy-sound-*, largest first).
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let excess = json.len() - MAX_PROJECT_JSON_BYTES;
        if !shed_audio_resources(project, report, excess) {
            break;
        }
        project.format_version = 5;
        project.upgrade_format_for_features();
        *json = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    }

    // 1b. Downscale/compress imported bitmap resources (legacy-bitmap-*, largest first)
    //     while preserving Stage placement size before shedding any bitmap to a placeholder.
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let excess = json.len() - MAX_PROJECT_JSON_BYTES;
        if !downscale_bitmap_resources_for_json(project, report, excess) {
            break;
        }
        project.format_version = 5;
        project.upgrade_format_for_features();
        *json = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    }

    // 2. Shed imported bitmap resources (legacy-bitmap-*, largest first).
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let excess = json.len() - MAX_PROJECT_JSON_BYTES;
        if !shed_bitmap_resources(project, report, excess) {
            break;
        }
        project.format_version = 5;
        project.upgrade_format_for_features();
        *json = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    }

    // Compact bulky provenance placement mappings before degrading vector artwork.
    if json.len() > MAX_PROJECT_JSON_BYTES && *prov_detail == ProvenanceDetail::Full {
        *prov_detail = ProvenanceDetail::NoPlacements;
        update_provenance_resource(
            project,
            prov_detail,
            prov_warned,
            *embedded,
            has_placements,
            report,
            make_provenance,
        )?;
        *json = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    }

    // 3. Degrade excess vector shape elements in later symbols/frames to placeholders.
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let target = (json.len() - MAX_PROJECT_JSON_BYTES).saturating_add(65_536);
        if !degrade_excess_vector_shapes(project, report, target) {
            break;
        }
        project.format_version = 5;
        project.upgrade_format_for_features();
        *json = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    }

    if json.len() > MAX_PROJECT_JSON_BYTES && *prov_detail != ProvenanceDetail::Minimal {
        *prov_detail = ProvenanceDetail::Minimal;
        update_provenance_resource(
            project,
            prov_detail,
            prov_warned,
            *embedded,
            has_placements,
            report,
            make_provenance,
        )?;
        *json = serde_json::to_string_pretty(project).map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn shed_video_resources(project: &mut Project, report: &mut ImportReport, excess: usize) -> bool {
    let mut candidates: Vec<(usize, u16, String)> = project
        .resources
        .iter()
        .filter_map(|r| {
            let suffix = r.id.strip_prefix("legacy-video-")?;
            let media_id = suffix.parse::<u16>().ok()?;
            Some((r.data_base64.len(), media_id, r.id.clone()))
        })
        .collect();
    if candidates.is_empty() {
        return false;
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut shed = Vec::new();
    let mut saved = 0usize;
    for (b64_len, media_id, res_id) in candidates {
        shed.push((res_id, media_id));
        saved = saved.saturating_add(b64_len.saturating_add(80));
        if saved >= excess {
            break;
        }
    }
    let shed_ids: std::collections::HashSet<&str> =
        shed.iter().map(|(id, _)| id.as_str()).collect();
    project
        .resources
        .retain(|r| !shed_ids.contains(r.id.as_str()));
    for (res_id, media_id) in &shed {
        report.warning(
            "fla_media_not_imported",
            format!("media {media_id}"),
            "Decoded video stream exceeds the project's 128 MiB encoded JSON budget alongside converted timelines; this video remains in the original FLA.",
        );
        for symbol in &mut project.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    if key
                        .video
                        .as_ref()
                        .is_some_and(|video| &video.resource == res_id)
                    {
                        key.video = None;
                    }
                }
            }
        }
    }
    true
}

fn shed_audio_resources(project: &mut Project, report: &mut ImportReport, excess: usize) -> bool {
    let mut candidates: Vec<(usize, u16, String)> = project
        .resources
        .iter()
        .filter_map(|r| {
            let suffix = r.id.strip_prefix("legacy-sound-").or_else(|| {
                r.path
                    .strip_prefix("legacy/sounds/")
                    .and_then(|s| s.split('/').next())
            })?;
            let media_id = suffix.parse::<u16>().ok()?;
            Some((r.data_base64.len(), media_id, r.id.clone()))
        })
        .collect();
    if candidates.is_empty() {
        return false;
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut shed = Vec::new();
    let mut saved = 0usize;
    for (b64_len, media_id, res_id) in candidates {
        shed.push((res_id, media_id));
        saved = saved.saturating_add(b64_len.saturating_add(80));
        if saved >= excess {
            break;
        }
    }
    let shed_ids: std::collections::HashSet<&str> =
        shed.iter().map(|(id, _)| id.as_str()).collect();
    project
        .resources
        .retain(|r| !shed_ids.contains(r.id.as_str()));
    for (res_id, media_id) in &shed {
        report.warning(
            "fla_media_not_imported",
            format!("media {media_id}"),
            "Decoded sound exceeds the project's 128 MiB encoded JSON budget alongside converted timelines; this sound remains in the original FLA.",
        );
        for symbol in &mut project.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    if key
                        .sound
                        .as_ref()
                        .is_some_and(|sound| &sound.resource == res_id)
                    {
                        key.sound = None;
                        report.warning(
                            "fla_frame_sound",
                            format!("{} layer {} frame {}", symbol.name, layer.name, key.frame + 1),
                            format!(
                                "Timeline sound references media #{media_id}, which was omitted to fit the 128 MiB project JSON budget."
                            ),
                        );
                    }
                }
            }
        }
    }
    true
}

fn downscale_one_project_bitmap_resource(
    project: &mut Project,
    res_id: &str,
    min_side: u32,
    require_b64_shrink: bool,
    report: &mut ImportReport,
    reason_budget: &str,
) -> Option<usize> {
    use image::{
        ExtendedColorType, ImageEncoder,
        codecs::jpeg::JpegEncoder,
        codecs::png::{CompressionType, FilterType, PngEncoder},
    };
    let res_idx = project.resources.iter().position(|r| r.id == res_id)?;
    let old_b64_len = project.resources[res_idx].data_base64.len();
    let old_media_type = project.resources[res_idx].media_type.clone();
    let old_path = project.resources[res_idx].path.clone();
    let decoded = project.resources[res_idx].decode_image().ok()?;
    let cur_max = decoded.width.max(decoded.height);
    if cur_max <= min_side {
        return None;
    }
    let target_max = (cur_max / 2).max(min_side);
    let (new_w, new_h, resized) = crate::legacy::media::resize_rgba_proportional(
        &decoded.rgba,
        decoded.width,
        decoded.height,
        target_max,
    )?;
    let sx = f64::from(decoded.width) / f64::from(new_w);
    let sy = f64::from(decoded.height) / f64::from(new_h);

    // Preflight all ElementKind::Bitmap and AuthoredBitmapFill bounds before mutating.
    for symbol in &project.symbols {
        for layer in &symbol.timeline.layers {
            for key in &layer.keyframes {
                for bf in &key.bitmap_fills {
                    if bf.resource_id == res_id {
                        let m = [
                            bf.matrix[0] * sx,
                            bf.matrix[1] * sx,
                            bf.matrix[2] * sy,
                            bf.matrix[3] * sy,
                        ];
                        if !m.iter().all(|v| v.is_finite() && v.abs() <= 1e9) {
                            return None;
                        }
                    }
                }
                for elem in &key.elements {
                    if matches!(
                        &elem.kind,
                        ElementKind::Bitmap { resource_id, .. } if resource_id == res_id
                    ) {
                        if let Some([px, py]) = elem.transform.pivot {
                            let npx = px / sx;
                            let npy = py / sy;
                            if !npx.is_finite()
                                || !npy.is_finite()
                                || npx.abs() > 1e9
                                || npy.abs() > 1e9
                            {
                                return None;
                            }
                        }
                        let geom = Transform {
                            x: elem.transform.x,
                            y: elem.transform.y,
                            scale_x: elem.transform.scale_x,
                            scale_y: elem.transform.scale_y,
                            rotation: elem.transform.rotation,
                            skew: elem.transform.skew,
                            ..Transform::default()
                        };
                        let mut mat = neoflash_core::transform_matrix(geom);
                        mat[0] *= sx;
                        mat[1] *= sx;
                        mat[2] *= sy;
                        mat[3] *= sy;
                        Transform::from_matrix(mat)?;
                    }
                }
            }
        }
    }

    let is_opaque = resized.as_chunks::<4>().0.iter().all(|px| px[3] == 255);
    let use_jpeg = old_media_type == "image/jpeg" || (require_b64_shrink && is_opaque);
    let (encoded_bytes, new_mime, new_path) = if use_jpeg {
        let mut rgb = Vec::with_capacity((new_w as usize) * (new_h as usize) * 3);
        for px in resized.as_chunks::<4>().0 {
            rgb.extend_from_slice(&px[..3]);
        }
        let mut jpg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpg, 85)
            .encode(&rgb, new_w, new_h, ExtendedColorType::Rgb8)
            .ok()?;
        let path = if old_media_type == "image/jpeg" {
            old_path
        } else {
            let used_lower: std::collections::BTreeSet<String> = project
                .resources
                .iter()
                .enumerate()
                .filter(|&(i, _)| i != res_idx)
                .map(|(_, r)| r.path.to_ascii_lowercase())
                .collect();
            let stem = old_path
                .strip_suffix(".png")
                .or_else(|| old_path.strip_suffix(".PNG"))
                .unwrap_or(&old_path);
            let mut seq = 1usize;
            loop {
                let cand = if seq == 1 {
                    format!("{stem}.jpg")
                } else {
                    format!("{stem}-{seq}.jpg")
                };
                if !used_lower.contains(&cand.to_ascii_lowercase()) {
                    break cand;
                }
                seq += 1;
            }
        };
        (jpg, "image/jpeg", path)
    } else {
        let mut png = Vec::new();
        PngEncoder::new_with_quality(&mut png, CompressionType::Best, FilterType::Adaptive)
            .write_image(&resized, new_w, new_h, ExtendedColorType::Rgba8)
            .ok()?;
        (png, "image/png", old_path)
    };

    let updated = ResourceUnit::from_bytes(res_id, new_path, new_mime, &encoded_bytes).ok()?;
    let new_b64_len = updated.data_base64.len();
    if require_b64_shrink && new_b64_len >= old_b64_len {
        return None;
    }

    project.resources[res_idx] = updated;
    for symbol in &mut project.symbols {
        for layer in &mut symbol.timeline.layers {
            for key in &mut layer.keyframes {
                for bf in &mut key.bitmap_fills {
                    if bf.resource_id == res_id {
                        bf.matrix[0] *= sx;
                        bf.matrix[1] *= sx;
                        bf.matrix[2] *= sy;
                        bf.matrix[3] *= sy;
                    }
                }
                for elem in &mut key.elements {
                    if let ElementKind::Bitmap {
                        resource_id,
                        width,
                        height,
                        ..
                    } = &mut elem.kind
                        && resource_id == res_id
                    {
                        *width = new_w;
                        *height = new_h;
                        if let Some(pivot) = &mut elem.transform.pivot {
                            pivot[0] /= sx;
                            pivot[1] /= sy;
                        }
                        let geom = Transform {
                            x: elem.transform.x,
                            y: elem.transform.y,
                            scale_x: elem.transform.scale_x,
                            scale_y: elem.transform.scale_y,
                            rotation: elem.transform.rotation,
                            skew: elem.transform.skew,
                            ..Transform::default()
                        };
                        let mut mat = neoflash_core::transform_matrix(geom);
                        mat[0] *= sx;
                        mat[1] *= sx;
                        mat[2] *= sy;
                        mat[3] *= sy;
                        if let Some(new_tf) = Transform::from_matrix(mat) {
                            elem.transform.x = new_tf.x;
                            elem.transform.y = new_tf.y;
                            elem.transform.scale_x = new_tf.scale_x;
                            elem.transform.scale_y = new_tf.scale_y;
                            elem.transform.rotation = new_tf.rotation;
                            elem.transform.skew = new_tf.skew;
                        }
                    }
                }
            }
        }
    }

    let loc = res_id
        .strip_prefix("legacy-bitmap-")
        .map(|mid| format!("media {mid}"))
        .unwrap_or_else(|| res_id.to_owned());
    report.warning(
        "fla_bitmap_downscaled",
        loc,
        format!(
            "Bitmap was downscaled from {}x{} px to {new_w}x{new_h} px (preserving Stage dimensions via transform scale) to fit the {reason_budget}; full-resolution bytes remain in the original FLA.",
            decoded.width, decoded.height
        ),
    );
    Some(old_b64_len.saturating_sub(new_b64_len))
}

fn downscale_bitmap_resources_for_json(
    project: &mut Project,
    report: &mut ImportReport,
    excess: usize,
) -> bool {
    let mut saved = 0usize;
    for _ in 0..512 {
        let mut candidates: Vec<(usize, u16, String)> = project
            .resources
            .iter()
            .filter_map(|r| {
                let suffix = r.id.strip_prefix("legacy-bitmap-")?;
                let media_id = suffix.parse::<u16>().ok()?;
                if r.data_base64.len() <= 16 * 1024 {
                    return None;
                }
                let raw = r.decoded_bytes().ok()?;
                let (w, h) = image::ImageReader::new(std::io::Cursor::new(&raw))
                    .with_guessed_format()
                    .ok()?
                    .into_dimensions()
                    .ok()?;
                if w.max(h) <= 64 {
                    return None;
                }
                Some((r.data_base64.len(), media_id, r.id.clone()))
            })
            .collect();
        if candidates.is_empty() {
            break;
        }
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        let mut progressed = false;
        for (_, _, res_id) in candidates {
            if let Some(delta) = downscale_one_project_bitmap_resource(
                project,
                &res_id,
                64,
                true,
                report,
                "128 MiB project JSON budget",
            ) && delta > 0
            {
                saved = saved.saturating_add(delta);
                progressed = true;
                break;
            }
        }
        if !progressed || saved >= excess {
            break;
        }
    }
    saved > 0
}

fn shed_bitmap_resources(project: &mut Project, report: &mut ImportReport, excess: usize) -> bool {
    let mut placement_counts = BTreeMap::<String, usize>::new();
    for key in project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
    {
        for element in &key.elements {
            if let ElementKind::Bitmap { resource_id, .. } = &element.kind {
                *placement_counts.entry(resource_id.clone()).or_default() += 1;
            }
        }
        for bf in &key.bitmap_fills {
            *placement_counts.entry(bf.resource_id.clone()).or_default() += 1;
        }
    }
    let mut candidates: Vec<(usize, u16, String)> = project
        .resources
        .iter()
        .filter_map(|r| {
            let suffix = r.id.strip_prefix("legacy-bitmap-")?;
            let media_id = suffix.parse::<u16>().ok()?;
            Some((r.data_base64.len(), media_id, r.id.clone()))
        })
        .collect();
    if candidates.is_empty() {
        return false;
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let mut shed = Vec::new();
    let mut saved = 0usize;
    for (b64_len, media_id, res_id) in candidates {
        let uses = placement_counts.get(&res_id).copied().unwrap_or(0);
        let net = b64_len.saturating_sub(600usize.saturating_add(uses.saturating_mul(40)));
        shed.push((res_id, media_id));
        saved = saved.saturating_add(net.max(1));
        if saved >= excess {
            break;
        }
    }
    let shed_ids: std::collections::HashSet<&str> =
        shed.iter().map(|(id, _)| id.as_str()).collect();
    project
        .resources
        .retain(|r| !shed_ids.contains(r.id.as_str()));
    let allowed_leaf = symbols_allowing_leaf_instance(&project.symbols);
    for (res_id, media_id) in &shed {
        report.warning(
            "fla_bitmap_skipped",
            format!("media {media_id}"),
            "Decoded bitmap exceeds the project's 128 MiB encoded JSON budget alongside converted timelines; this bitmap becomes an unresolved placeholder.",
        );
        let missing_id = format!("fla-missing-bitmap-{media_id}");
        let placeholder_symbol = if project.symbols.iter().any(|s| s.id == missing_id) {
            Some(missing_id)
        } else if project.symbols.len() < 10_000 {
            project.symbols.push(group_symbol(
                &missing_id,
                format!("Unresolved bitmap #{media_id}"),
                vec![],
            ));
            Some(missing_id)
        } else if project.symbols.iter().any(|s| s.id == PLACEHOLDER) {
            Some(PLACEHOLDER.to_owned())
        } else {
            None
        };
        for symbol in &mut project.symbols {
            let can_nest = allowed_leaf.contains(&symbol.id);
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    key.bitmap_fills.retain(|bf| &bf.resource_id != res_id);
                    for element in &mut key.elements {
                        if matches!(
                            &element.kind,
                            ElementKind::Bitmap { resource_id, .. } if resource_id == res_id
                        ) {
                            element.name = format!("Unresolved source bitmap #{media_id}");
                            element.kind = match (&placeholder_symbol, can_nest) {
                                (Some(sid), true) => ElementKind::Instance {
                                    symbol_id: sid.clone(),
                                    playback: InstancePlayback::SingleFrame(0),
                                },
                                _ => ElementKind::Path {
                                    commands: vec![],
                                    fill_rule: FillRule::EvenOdd,
                                    fill: [0; 4],
                                    stroke: [0; 4],
                                    stroke_width: 0.0,
                                },
                            };
                        }
                    }
                }
            }
        }
    }
    true
}

fn is_vector_path(element: &Element) -> bool {
    matches!(
        element.kind,
        ElementKind::PaintedPath { .. } | ElementKind::Path { .. }
    )
}

fn pretty_element_bytes(element: &Element) -> usize {
    match serde_json::to_string_pretty(element) {
        Ok(s) => {
            let lines = s.bytes().filter(|&b| b == b'\n').count() + 1;
            s.len() + 18 * lines + 2
        }
        Err(_) => 512,
    }
}

fn symbols_allowing_leaf_instance(symbols: &[Symbol]) -> std::collections::HashSet<String> {
    let mut parents: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for symbol in symbols {
        for element in symbol
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
        {
            if let ElementKind::Instance { symbol_id, .. } = &element.kind {
                parents
                    .entry(symbol_id.as_str())
                    .or_default()
                    .push(symbol.id.as_str());
            }
        }
    }
    fn ancestor_depth<'a>(
        id: &'a str,
        parents: &std::collections::HashMap<&'a str, Vec<&'a str>>,
        active: &mut std::collections::HashSet<&'a str>,
        memo: &mut std::collections::HashMap<&'a str, usize>,
    ) -> usize {
        if let Some(&d) = memo.get(id) {
            return d;
        }
        if !active.insert(id) || active.len() > 64 {
            active.remove(id);
            return 65;
        }
        let mut best = 1usize;
        if let Some(ps) = parents.get(id) {
            for &p in ps {
                best = best.max(1 + ancestor_depth(p, parents, active, memo));
                if best >= 65 {
                    break;
                }
            }
        }
        active.remove(id);
        memo.insert(id, best);
        best
    }
    let mut active = std::collections::HashSet::new();
    let mut memo = std::collections::HashMap::new();
    symbols
        .iter()
        .filter(|s| ancestor_depth(&s.id, &parents, &mut active, &mut memo) < 64)
        .map(|s| s.id.clone())
        .collect()
}

fn degrade_excess_vector_shapes(
    project: &mut Project,
    report: &mut ImportReport,
    target_savings: usize,
) -> bool {
    if !project.symbols.iter().any(|s| s.id == PLACEHOLDER) && project.symbols.len() < 10_000 {
        project.symbols.push(group_symbol(
            PLACEHOLDER,
            "Unsupported source object (see import report)".into(),
            vec![],
        ));
    }
    let has_placeholder = project.symbols.iter().any(|s| s.id == PLACEHOLDER);
    let allowed_leaf = symbols_allowing_leaf_instance(&project.symbols);
    let mut saved = 0usize;

    for preserve_special_layers in [true, false] {
        for symbol in project.symbols.iter_mut().rev() {
            if symbol.id == PLACEHOLDER || symbol.id.starts_with("fla-missing-bitmap-") {
                continue;
            }
            let is_group_symbol = symbol.id.ends_with("-group");
            let use_placeholder = has_placeholder && allowed_leaf.contains(&symbol.id);
            for layer in symbol.timeline.layers.iter_mut().rev() {
                if preserve_special_layers
                    && matches!(layer.properties.role, LayerRole::Guide | LayerRole::Mask)
                {
                    continue;
                }
                for key in layer.keyframes.iter_mut().rev() {
                    if !key.elements.iter().any(is_vector_path) {
                        continue;
                    }
                    let path_bytes: usize = key
                        .elements
                        .iter()
                        .filter(|e| is_vector_path(e))
                        .map(pretty_element_bytes)
                        .sum();
                    if is_group_symbol {
                        key.elements.retain(|e| !is_vector_path(e));
                        let kept_ids: std::collections::HashSet<&str> = key
                            .elements
                            .iter()
                            .filter(|e| is_vector_path(e))
                            .map(|e| e.id.as_str())
                            .collect();
                        key.bitmap_fills
                            .retain(|bf| kept_ids.contains(bf.element_id.as_str()));
                        key.appearances
                            .retain(|app| kept_ids.contains(app.element_id.as_str()));
                        key.vector_effects
                            .retain(|fx| kept_ids.contains(fx.element_id.as_str()));
                        report.warning(
                            "fla_shape_approximated",
                            &symbol.name,
                            "Vector path geometry was omitted as a placeholder to fit the project's 128 MiB encoded JSON limit; original artwork remains in the source FLA.",
                        );
                        saved = saved.saturating_add(path_bytes);
                    } else {
                        let first_id = key
                            .elements
                            .iter()
                            .find(|e| is_vector_path(e))
                            .unwrap()
                            .id
                            .clone();
                        let replacement = Element {
                            id: first_id,
                            name: String::new(),
                            transform: Transform::default(),
                            kind: if use_placeholder {
                                ElementKind::Instance {
                                    symbol_id: PLACEHOLDER.into(),
                                    playback: InstancePlayback::SingleFrame(0),
                                }
                            } else {
                                ElementKind::Path {
                                    commands: vec![],
                                    fill_rule: FillRule::EvenOdd,
                                    fill: [0; 4],
                                    stroke: [0; 4],
                                    stroke_width: 0.0,
                                }
                            },
                            scripts: vec![],
                            accessibility: None,
                            ..Element::default()
                        };
                        let repl_bytes = pretty_element_bytes(&replacement);
                        if path_bytes <= repl_bytes {
                            continue;
                        }
                        let mut replaced = false;
                        key.elements.retain_mut(|element| {
                            if !is_vector_path(element) {
                                return true;
                            }
                            if !replaced {
                                *element = replacement.clone();
                                replaced = true;
                                true
                            } else {
                                false
                            }
                        });
                        let kept_ids: std::collections::HashSet<&str> = key
                            .elements
                            .iter()
                            .filter(|e| is_vector_path(e))
                            .map(|e| e.id.as_str())
                            .collect();
                        key.bitmap_fills
                            .retain(|bf| kept_ids.contains(bf.element_id.as_str()));
                        key.appearances
                            .retain(|app| kept_ids.contains(app.element_id.as_str()));
                        key.vector_effects
                            .retain(|fx| kept_ids.contains(fx.element_id.as_str()));
                        report.warning(
                            "fla_shape_approximated",
                            format!("{} layer {} frame {}", symbol.name, layer.name, key.frame + 1),
                            "Vector path geometry was replaced by a placeholder to fit the project's 128 MiB encoded JSON limit; original artwork remains in the source FLA.",
                        );
                        saved = saved.saturating_add(path_bytes - repl_bytes);
                    }
                    if saved >= target_savings {
                        return true;
                    }
                }
            }
        }
    }
    saved > 0
}

fn empty_timeline(layer_id: &str) -> Timeline {
    Timeline {
        guides: vec![],
        frame_count: 1,
        layers: vec![Layer {
            id: layer_id.into(),
            name: "Layer 1".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![],
                tween: None,
                label: String::new(),
                ..Default::default()
            }],
            properties: LayerProperties::default(),
        }],
        scripts: vec![],
    }
}

fn is_valid_qualified_class_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && name.split('.').all(|part| {
            !part.is_empty()
                && part.chars().enumerate().all(|(i, ch)| {
                    ch == '_' || ch == '$' || ch.is_alphabetic() || (i > 0 && ch.is_numeric())
                })
        })
}

fn leaf_placeholder_element(id: String, transform: Transform) -> Element {
    Element {
        id,
        name: String::new(),
        transform,
        kind: ElementKind::Path {
            commands: vec![],
            fill_rule: FillRule::EvenOdd,
            fill: [0; 4],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        scripts: vec![],
        accessibility: None,
        ..Element::default()
    }
}

fn prune_keyframe_side_tables(key: &mut Keyframe) {
    let kept: std::collections::HashSet<&str> =
        key.elements.iter().map(|e| e.id.as_str()).collect();
    key.bitmap_fills
        .retain(|bf| kept.contains(bf.element_id.as_str()));
    key.appearances
        .retain(|app| kept.contains(app.element_id.as_str()));
    key.vector_effects
        .retain(|fx| kept.contains(fx.element_id.as_str()));
    key.asset_warps
        .retain(|w| kept.contains(w.element_id.as_str()));
    key.element_parents
        .retain(|ep| kept.contains(ep.child_id.as_str()) && kept.contains(ep.parent_id.as_str()));
}

fn enforce_project_structural_limits(
    project: &mut Project,
    provenance: &mut [serde_json::Value],
    report: &mut ImportReport,
) {
    const MAX_SYMBOLS: usize = 10_000;
    const MAX_NESTING_DEPTH: usize = 64;
    const MAX_PLACED_BITMAPS: usize = 256;
    const MAX_AUTHORED_ELEMENTS: usize = 100_000;
    const MAX_EXPANDED_NODES: usize = 100_000;

    // 1. Break symbol cycles and nesting deeper than 64 levels first so later
    //    bottom-up traversals operate on a bounded DAG.
    loop {
        let index_by_id: std::collections::HashMap<String, usize> = project
            .symbols
            .iter()
            .enumerate()
            .map(|(idx, s)| (s.id.clone(), idx))
            .collect();
        let mut active = std::collections::HashSet::<String>::new();
        let mut good_max_depth = std::collections::HashMap::<String, usize>::new();
        let mut bad_edge: Option<(usize, usize, usize, usize, String)> = None;

        fn find_bad_instance(
            sym_idx: usize,
            depth: usize,
            symbols: &[Symbol],
            index_by_id: &std::collections::HashMap<String, usize>,
            active: &mut std::collections::HashSet<String>,
            good_max_depth: &mut std::collections::HashMap<String, usize>,
        ) -> Result<usize, (usize, usize, usize, usize, String)> {
            let sym = &symbols[sym_idx];
            if let Some(&sub_height) = good_max_depth.get(&sym.id)
                && depth + sub_height <= MAX_NESTING_DEPTH + 1
            {
                return Ok(sub_height);
            }
            active.insert(sym.id.clone());
            let mut max_sub = 1usize;
            for (li, layer) in sym.timeline.layers.iter().enumerate() {
                for (ki, key) in layer.keyframes.iter().enumerate() {
                    for (ei, elem) in key.elements.iter().enumerate() {
                        if let ElementKind::Instance { symbol_id, .. } = &elem.kind {
                            if active.contains(symbol_id) {
                                active.remove(&sym.id);
                                return Err((
                                    sym_idx,
                                    li,
                                    ki,
                                    ei,
                                    "cyclic symbol reference".into(),
                                ));
                            }
                            if depth >= MAX_NESTING_DEPTH {
                                active.remove(&sym.id);
                                return Err((
                                    sym_idx,
                                    li,
                                    ki,
                                    ei,
                                    format!("symbol nesting exceeds {MAX_NESTING_DEPTH} levels"),
                                ));
                            }
                            if let Some(&child_idx) = index_by_id.get(symbol_id) {
                                let child_h = find_bad_instance(
                                    child_idx,
                                    depth + 1,
                                    symbols,
                                    index_by_id,
                                    active,
                                    good_max_depth,
                                )?;
                                max_sub = max_sub.max(1 + child_h);
                            }
                        }
                    }
                }
            }
            active.remove(&sym.id);
            good_max_depth.insert(sym.id.clone(), max_sub);
            Ok(max_sub)
        }

        for idx in 0..project.symbols.len() {
            if let Err(found) = find_bad_instance(
                idx,
                1,
                &project.symbols,
                &index_by_id,
                &mut active,
                &mut good_max_depth,
            ) {
                bad_edge = Some(found);
                break;
            }
        }
        let Some((si, li, ki, ei, reason)) = bad_edge else {
            break;
        };
        let sym_name = project.symbols[si].name.clone();
        let layer_name = project.symbols[si].timeline.layers[li].name.clone();
        let frame_no = project.symbols[si].timeline.layers[li].keyframes[ki].frame + 1;
        let elem = &mut project.symbols[si].timeline.layers[li].keyframes[ki].elements[ei];
        *elem = leaf_placeholder_element(elem.id.clone(), elem.transform);
        report.warning(
            "fla_symbol_nesting_limit",
            format!("{sym_name} layer {layer_name} frame {frame_no}"),
            format!(
                "Symbol placement replaced by a placeholder ({reason}); original symbol remains in the library."
            ),
        );
    }

    // 2. Enforce runtime placed-bitmap budgets (256 distinct bitmap buffers and
    //    MAX_BITMAP_BYTES = 128 MiB of decoded RGBA pixels). First proportionally
    //    downscale the largest placed bitmaps so visible Stage bitmaps stay within
    //    the 128 MiB pixel budget before falling back to placeholders.
    for _ in 0..512 {
        let mut seen = std::collections::HashSet::<String>::new();
        let mut total_rgba = 0usize;
        let mut largest: Option<(usize, String)> = None;
        for sym in &project.symbols {
            for layer in &sym.timeline.layers {
                for key in &layer.keyframes {
                    for elem in &key.elements {
                        if let ElementKind::Bitmap {
                            resource_id,
                            width,
                            height,
                            ..
                        } = &elem.kind
                        {
                            if seen.len() >= MAX_PLACED_BITMAPS && !seen.contains(resource_id) {
                                continue;
                            }
                            if seen.insert(resource_id.clone()) {
                                let rgba = (*width as usize)
                                    .saturating_mul(*height as usize)
                                    .saturating_mul(4);
                                total_rgba = total_rgba.saturating_add(rgba);
                                if (*width).max(*height) > 64
                                    && resource_id.starts_with("legacy-")
                                    && largest.as_ref().is_none_or(|(best, _)| rgba > *best)
                                {
                                    largest = Some((rgba, resource_id.clone()));
                                }
                            }
                        }
                    }
                }
            }
        }
        if total_rgba <= MAX_BITMAP_BYTES {
            break;
        }
        let Some((_, res_id)) = largest else {
            break;
        };
        if downscale_one_project_bitmap_resource(
            project,
            &res_id,
            64,
            false,
            report,
            "128 MiB runtime pixel budget",
        )
        .is_none()
        {
            break;
        }
    }

    let mut kept_placed_bitmaps = std::collections::HashSet::<String>::new();
    let mut placed_rgba_bytes = 0usize;
    let mut degraded_bitmap_placements = Vec::<(usize, usize, usize, usize, String)>::new();
    for (si, sym) in project.symbols.iter().enumerate() {
        for (li, layer) in sym.timeline.layers.iter().enumerate() {
            for (ki, key) in layer.keyframes.iter().enumerate() {
                for (ei, elem) in key.elements.iter().enumerate() {
                    if let ElementKind::Bitmap {
                        resource_id,
                        width,
                        height,
                        ..
                    } = &elem.kind
                    {
                        if kept_placed_bitmaps.contains(resource_id) {
                            continue;
                        }
                        let rgba = (*width as usize)
                            .saturating_mul(*height as usize)
                            .saturating_mul(4);
                        if kept_placed_bitmaps.len() < MAX_PLACED_BITMAPS
                            && placed_rgba_bytes
                                .checked_add(rgba)
                                .is_some_and(|n| n <= MAX_BITMAP_BYTES)
                        {
                            kept_placed_bitmaps.insert(resource_id.clone());
                            placed_rgba_bytes += rgba;
                        } else {
                            degraded_bitmap_placements.push((si, li, ki, ei, resource_id.clone()));
                        }
                    }
                }
            }
        }
    }
    if !degraded_bitmap_placements.is_empty() {
        if !project.symbols.iter().any(|s| s.id == PLACEHOLDER)
            && project.symbols.len() < MAX_SYMBOLS
        {
            project.symbols.push(group_symbol(
                PLACEHOLDER,
                "Unsupported source object (see import report)".into(),
                vec![],
            ));
        }
        let has_placeholder = project.symbols.iter().any(|s| s.id == PLACEHOLDER);
        let allowed_leaf = symbols_allowing_leaf_instance(&project.symbols);
        for (si, li, ki, ei, res_id) in degraded_bitmap_placements {
            let sym_id = project.symbols[si].id.clone();
            let sym_name = project.symbols[si].name.clone();
            let layer_name = project.symbols[si].timeline.layers[li].name.clone();
            let frame_no = project.symbols[si].timeline.layers[li].keyframes[ki].frame + 1;
            let use_inst = has_placeholder && allowed_leaf.contains(&sym_id);
            let elem = &mut project.symbols[si].timeline.layers[li].keyframes[ki].elements[ei];
            elem.name = format!("Budget-limited bitmap ({res_id})");
            elem.kind = if use_inst {
                ElementKind::Instance {
                    symbol_id: PLACEHOLDER.into(),
                    playback: InstancePlayback::SingleFrame(0),
                }
            } else {
                ElementKind::Path {
                    commands: vec![],
                    fill_rule: FillRule::EvenOdd,
                    fill: [0; 4],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                }
            };
            report.warning(
                "fla_bitmap_skipped",
                format!("{sym_name} layer {layer_name} frame {frame_no}"),
                "Placed bitmap exceeds the 256-bitmap or 128 MiB runtime pixel budget; a placeholder preserves its stage transform.",
            );
        }
    }

    // 3. Deduplicate groups/media first whenever symbol, element or expanded-node
    //    counts approach document limits.
    let authored_count = |proj: &Project| -> usize {
        proj.symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .map(|k| k.elements.len())
            .sum()
    };
    let symbol_expanded_bound =
        |sym: &Symbol, child_bounds: &std::collections::HashMap<String, usize>| -> usize {
            let cap = MAX_EXPANDED_NODES + 1;
            let mut count = 1usize;
            for layer in &sym.timeline.layers {
                let mut max_key = 0usize;
                for key in &layer.keyframes {
                    let mut key_count = 0usize;
                    for elem in &key.elements {
                        let nodes = match &elem.kind {
                            ElementKind::Instance { symbol_id, .. } => {
                                child_bounds.get(symbol_id).copied().unwrap_or(1)
                            }
                            _ => 1,
                        };
                        key_count = key_count.saturating_add(nodes).min(cap);
                    }
                    max_key = max_key.max(key_count);
                }
                count = count.saturating_add(max_key).min(cap);
            }
            count
        };
    let compute_expanded_bounds = |symbols: &[Symbol]| -> std::collections::HashMap<String, usize> {
        let by_id: std::collections::HashMap<&str, &Symbol> =
            symbols.iter().map(|s| (s.id.as_str(), s)).collect();
        let mut memo = std::collections::HashMap::<String, usize>::new();
        let mut active = std::collections::HashSet::<String>::new();
        fn eval(
            id: &str,
            by_id: &std::collections::HashMap<&str, &Symbol>,
            active: &mut std::collections::HashSet<String>,
            memo: &mut std::collections::HashMap<String, usize>,
        ) -> usize {
            if let Some(&v) = memo.get(id) {
                return v;
            }
            let Some(&sym) = by_id.get(id) else {
                return 1;
            };
            if !active.insert(id.to_owned()) {
                return 1;
            }
            let cap = MAX_EXPANDED_NODES + 1;
            let mut count = 1usize;
            for layer in &sym.timeline.layers {
                let mut max_key = 0usize;
                for key in &layer.keyframes {
                    let mut key_count = 0usize;
                    for elem in &key.elements {
                        let nodes = match &elem.kind {
                            ElementKind::Instance { symbol_id, .. } => {
                                eval(symbol_id, by_id, active, memo)
                            }
                            _ => 1,
                        };
                        key_count = key_count.saturating_add(nodes).min(cap);
                    }
                    max_key = max_key.max(key_count);
                }
                count = count.saturating_add(max_key).min(cap);
            }
            active.remove(id);
            memo.insert(id.to_owned(), count);
            count
        }
        for s in symbols {
            eval(&s.id, &by_id, &mut active, &mut memo);
        }
        memo
    };

    if project.symbols.len() > MAX_SYMBOLS
        || authored_count(project) > MAX_AUTHORED_ELEMENTS
        || compute_expanded_bounds(&project.symbols)
            .values()
            .any(|&n| n > MAX_EXPANDED_NODES)
    {
        deduplicate_project_for_budget(project, provenance);
    }

    // 3a. Cap symbol count at 10,000.
    if project.symbols.len() > MAX_SYMBOLS {
        let protected: std::collections::HashSet<String> = std::iter::once(project.root.clone())
            .chain(project.scenes.iter().cloned())
            .chain(std::iter::once(PLACEHOLDER.to_owned()))
            .collect();
        let protected_present = project
            .symbols
            .iter()
            .filter(|sym| protected.contains(&sym.id))
            .count();
        let reserve_placeholder = usize::from(
            !project
                .symbols
                .iter()
                .any(|sym| sym.id.as_str() == PLACEHOLDER),
        );
        let max_unprotected = MAX_SYMBOLS.saturating_sub(protected_present + reserve_placeholder);
        let mut unprotected_kept = 0usize;
        let mut kept = Vec::with_capacity(MAX_SYMBOLS);
        let mut dropped = std::collections::HashSet::<String>::new();
        for sym in project.symbols.drain(..) {
            if protected.contains(&sym.id) {
                kept.push(sym);
            } else if unprotected_kept < max_unprotected {
                unprotected_kept += 1;
                kept.push(sym);
            } else {
                report.warning(
                    "fla_symbol_limit",
                    &sym.name,
                    format!(
                        "Symbol omitted because the document exceeds the {MAX_SYMBOLS}-symbol limit; placements become placeholders."
                    ),
                );
                dropped.insert(sym.id);
            }
        }
        project.symbols = kept;
        for sym in &mut project.symbols {
            for layer in &mut sym.timeline.layers {
                for key in &mut layer.keyframes {
                    for elem in &mut key.elements {
                        if let ElementKind::Instance { symbol_id, .. } = &elem.kind
                            && dropped.contains(symbol_id)
                        {
                            *elem = leaf_placeholder_element(elem.id.clone(), elem.transform);
                        }
                    }
                }
            }
        }
    }

    // 3b. Cap total authored elements at 100,000.
    let mut total_elements = authored_count(project);
    if total_elements > MAX_AUTHORED_ELEMENTS {
        for sym in project.symbols.iter_mut().rev() {
            if total_elements <= MAX_AUTHORED_ELEMENTS {
                break;
            }
            for layer in sym.timeline.layers.iter_mut().rev() {
                if total_elements <= MAX_AUTHORED_ELEMENTS {
                    break;
                }
                for key in layer.keyframes.iter_mut().rev() {
                    if total_elements <= MAX_AUTHORED_ELEMENTS {
                        break;
                    }
                    if key.elements.len() <= 1 {
                        continue;
                    }
                    let removed = key.elements.len() - 1;
                    let first = &key.elements[0];
                    key.elements =
                        vec![leaf_placeholder_element(first.id.clone(), first.transform)];
                    prune_keyframe_side_tables(key);
                    total_elements = total_elements.saturating_sub(removed);
                    report.warning(
                        "fla_element_limit",
                        format!("{} layer {} frame {}", sym.name, layer.name, key.frame + 1),
                        format!(
                            "Keyframe elements collapsed into a placeholder to fit the {MAX_AUTHORED_ELEMENTS}-element project limit."
                        ),
                    );
                }
            }
        }
        if total_elements > MAX_AUTHORED_ELEMENTS {
            for sym in project.symbols.iter_mut().rev() {
                for layer in sym.timeline.layers.iter_mut().rev() {
                    for key in layer.keyframes.iter_mut().rev() {
                        if total_elements <= MAX_AUTHORED_ELEMENTS {
                            break;
                        }
                        if !key.elements.is_empty() {
                            total_elements = total_elements.saturating_sub(key.elements.len());
                            key.elements.clear();
                            prune_keyframe_side_tables(key);
                            report.warning(
                                "fla_element_limit",
                                format!("{} layer {} frame {}", sym.name, layer.name, key.frame + 1),
                                format!(
                                    "Trailing keyframe cleared to fit the {MAX_AUTHORED_ELEMENTS}-element project limit."
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    // 3c. Cap expanded scene/symbol node bound at 100,000 (bottom-up over the DAG).
    loop {
        let bounds = compute_expanded_bounds(&project.symbols);
        let Some(target_idx) = (0..project.symbols.len()).find(|&idx| {
            let sym = &project.symbols[idx];
            bounds.get(&sym.id).copied().unwrap_or(0) > MAX_EXPANDED_NODES
                && sym
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .flat_map(|k| &k.elements)
                    .all(|e| match &e.kind {
                        ElementKind::Instance { symbol_id, .. } => {
                            bounds.get(symbol_id).copied().unwrap_or(0) <= MAX_EXPANDED_NODES
                        }
                        _ => true,
                    })
        }) else {
            break;
        };
        let sym_name = project.symbols[target_idx].name.clone();
        // First degrade heavy child instances in keyframes whose per-keyframe bound contributes
        // to the over-budget layer maximum, recomputing the exact symbol bound after each step.
        while symbol_expanded_bound(&project.symbols[target_idx], &bounds) > MAX_EXPANDED_NODES {
            // Find the layer with the largest `max_key`, and within it the last keyframe
            // holding a child instance with `child_cost > 1`.
            let mut best: Option<(usize, usize, usize, usize)> = None; // (layer_max, li, ki, ei)
            for (li, layer) in project.symbols[target_idx]
                .timeline
                .layers
                .iter()
                .enumerate()
            {
                for (ki, key) in layer.keyframes.iter().enumerate() {
                    let mut key_cost = 0usize;
                    let mut last_heavy_elem = None;
                    for (ei, elem) in key.elements.iter().enumerate() {
                        let nodes = match &elem.kind {
                            ElementKind::Instance { symbol_id, .. } => {
                                let c = bounds.get(symbol_id).copied().unwrap_or(1);
                                if c > 1 {
                                    last_heavy_elem = Some(ei);
                                }
                                c
                            }
                            _ => 1,
                        };
                        key_cost = key_cost.saturating_add(nodes);
                    }
                    if let Some(ei) = last_heavy_elem
                        && best.is_none_or(|(b_cost, ..)| key_cost >= b_cost)
                    {
                        best = Some((key_cost, li, ki, ei));
                    }
                }
            }
            let Some((_, li, ki, ei)) = best else {
                break;
            };
            let layer_name = project.symbols[target_idx].timeline.layers[li].name.clone();
            let frame_no = project.symbols[target_idx].timeline.layers[li].keyframes[ki].frame + 1;
            let elem =
                &mut project.symbols[target_idx].timeline.layers[li].keyframes[ki].elements[ei];
            *elem = leaf_placeholder_element(elem.id.clone(), elem.transform);
            report.warning(
                "fla_expanded_node_limit",
                format!("{sym_name} layer {layer_name} frame {frame_no}"),
                format!(
                    "Symbol instance replaced by a placeholder so expanded scene/symbol nodes stay within {MAX_EXPANDED_NODES}."
                ),
            );
        }
        // If leaf elements alone still exceed 100,000, collapse the heaviest keyframes.
        while symbol_expanded_bound(&project.symbols[target_idx], &bounds) > MAX_EXPANDED_NODES {
            let mut best_kf: Option<(usize, usize, usize)> = None; // (len, li, ki)
            for (li, layer) in project.symbols[target_idx]
                .timeline
                .layers
                .iter()
                .enumerate()
            {
                for (ki, key) in layer.keyframes.iter().enumerate() {
                    if !key.elements.is_empty()
                        && best_kf.is_none_or(|(b_len, ..)| key.elements.len() >= b_len)
                    {
                        best_kf = Some((key.elements.len(), li, ki));
                    }
                }
            }
            let Some((len, li, ki)) = best_kf else {
                break;
            };
            let layer_name = project.symbols[target_idx].timeline.layers[li].name.clone();
            let key = &mut project.symbols[target_idx].timeline.layers[li].keyframes[ki];
            if len > 1 {
                let first = &key.elements[0];
                key.elements = vec![leaf_placeholder_element(first.id.clone(), first.transform)];
                prune_keyframe_side_tables(key);
                report.warning(
                    "fla_expanded_node_limit",
                    format!("{sym_name} layer {layer_name} frame {}", key.frame + 1),
                    format!(
                        "Keyframe collapsed to a placeholder so expanded nodes stay within {MAX_EXPANDED_NODES}."
                    ),
                );
            } else {
                key.elements.clear();
                prune_keyframe_side_tables(key);
            }
        }
    }

    // 4. Sanitize Folder layers carrying stage artwork and broken/cyclic layer relations.
    for sym in &mut project.symbols {
        let sym_name = sym.name.clone();
        let layers = &mut sym.timeline.layers;
        for li in 0..layers.len() {
            if layers[li].properties.role == LayerRole::Folder
                && layers[li].keyframes.iter().any(|k| !k.elements.is_empty())
            {
                let folder_id = layers[li].id.clone();
                let has_children = layers.iter().enumerate().any(|(j, other)| {
                    j != li && other.properties.parent.as_deref() == Some(folder_id.as_str())
                });
                let loc = format!("{sym_name} layer {}", layers[li].name);
                if !has_children {
                    layers[li].properties.role = LayerRole::Normal;
                    layers[li].properties.collapsed = false;
                    report.warning(
                        "fla_layer_type",
                        loc,
                        "Folder layer contains stage elements and has no child layers; imported as a normal layer.",
                    );
                } else {
                    for key in &mut layers[li].keyframes {
                        key.elements.clear();
                        key.tween = None;
                        key.tween_kind = TweenKind::Classic;
                        key.keyed_properties = None;
                        key.orient_to_path = false;
                        key.sync_symbols = false;
                        key.snap_to_guide = false;
                        key.classic_rotate_dir = ClassicRotateDirection::Auto;
                        key.classic_rotate_turns = 0;
                        key.shape_hints.clear();
                        prune_keyframe_side_tables(key);
                    }
                    report.warning(
                        "fla_layer_type",
                        loc,
                        "Folder layer cannot contain stage artwork; elements on the folder row were omitted.",
                    );
                }
            }
        }
        for li in 0..layers.len() {
            if layers[li].properties.role != LayerRole::Mask {
                layers[li].properties.mask_mode = MaskMode::Geometry;
                layers[li].properties.mask_invert = false;
            }
            if let Some(mask_id) = layers[li].properties.mask.clone() {
                let valid = layers[li].properties.role == LayerRole::Normal
                    && mask_id != layers[li].id
                    && layers
                        .iter()
                        .any(|l| l.id == mask_id && l.properties.role == LayerRole::Mask);
                if !valid {
                    layers[li].properties.mask = None;
                    report.warning(
                        "fla_layer_relation",
                        format!("{sym_name} layer {}", layers[li].name),
                        "The layer's mask relation is invalid and was cleared.",
                    );
                }
            }
            if let Some(guide_id) = layers[li].properties.motion_guide.clone() {
                let valid = layers[li].properties.role == LayerRole::Normal
                    && guide_id != layers[li].id
                    && layers
                        .iter()
                        .any(|l| l.id == guide_id && l.properties.role == LayerRole::Guide);
                if !valid {
                    layers[li].properties.motion_guide = None;
                    report.warning(
                        "fla_layer_relation",
                        format!("{sym_name} layer {}", layers[li].name),
                        "The layer's motion-guide relation is invalid and was cleared.",
                    );
                }
            }
            if layers[li].properties.parent.is_some() {
                let mut seen = std::collections::HashSet::from([layers[li].id.clone()]);
                let mut next = layers[li].properties.parent.clone();
                let mut valid = true;
                while let Some(pid) = next {
                    if !seen.insert(pid.clone()) {
                        valid = false;
                        break;
                    }
                    let Some(parent_layer) = layers
                        .iter()
                        .find(|l| l.id == pid && l.properties.role == LayerRole::Folder)
                    else {
                        valid = false;
                        break;
                    };
                    next = parent_layer.properties.parent.clone();
                }
                if !valid {
                    layers[li].properties.parent = None;
                    report.warning(
                        "fla_layer_relation",
                        format!("{sym_name} layer {}", layers[li].name),
                        "The layer's folder ancestry is missing or cyclic and was moved to the timeline root.",
                    );
                }
            }
        }
    }

    // 5. Validate mask layers to a fixpoint mirroring `authored_layers::validate_layers`:
    //    upgrade bitmap-containing Geometry masks to Alpha, or demote non-maskable
    //    mask layers to Normal (resetting mask_mode, mask_invert, and sibling mask links).
    fn mask_source_ok<'a>(
        kind: &'a ElementKind,
        mode: MaskMode,
        by_id: &std::collections::HashMap<&'a str, &'a Symbol>,
        decodable_bitmaps: &std::collections::HashSet<String>,
        visiting: &mut std::collections::HashSet<&'a str>,
    ) -> bool {
        match kind {
            ElementKind::Instance { symbol_id, .. } => {
                let Some(&sym) = by_id.get(symbol_id.as_str()) else {
                    return false;
                };
                if !visiting.insert(sym.id.as_str()) {
                    return false;
                }
                let valid = sym
                    .timeline
                    .layers
                    .iter()
                    .filter(|l| l.properties.role != LayerRole::Guide)
                    .all(|layer| {
                        let child_mode = if layer.properties.role == LayerRole::Mask {
                            layer.properties.mask_mode
                        } else {
                            mode
                        };
                        layer.keyframes.iter().flat_map(|k| &k.elements).all(|e| {
                            mask_source_ok(&e.kind, child_mode, by_id, decodable_bitmaps, visiting)
                        })
                    });
                visiting.remove(sym.id.as_str());
                valid
            }
            ElementKind::Bitmap { resource_id, .. } if mode.uses_paint_coverage() => {
                decodable_bitmaps.contains(resource_id.as_str())
            }
            ElementKind::Rectangle { width, height, .. }
            | ElementKind::Ellipse { width, height, .. } => *width > 0.0 && *height > 0.0,
            ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => true,
            _ => false,
        }
    }

    let decodable_bitmaps: std::collections::HashSet<String> = project
        .resources
        .iter()
        .filter(|r| r.decode_image().is_ok())
        .map(|r| r.id.clone())
        .collect();
    loop {
        let mut mask_fixes = Vec::<(usize, usize, bool)>::new();
        {
            let by_id: std::collections::HashMap<&str, &Symbol> =
                project.symbols.iter().map(|s| (s.id.as_str(), s)).collect();
            for (si, sym) in project.symbols.iter().enumerate() {
                for (li, layer) in sym.timeline.layers.iter().enumerate() {
                    if layer.properties.role != LayerRole::Mask {
                        continue;
                    }
                    let current_ok = layer
                        .keyframes
                        .iter()
                        .flat_map(|k| &k.elements)
                        .all(|elem| {
                            let mut visiting = std::collections::HashSet::new();
                            mask_source_ok(
                                &elem.kind,
                                layer.properties.mask_mode,
                                &by_id,
                                &decodable_bitmaps,
                                &mut visiting,
                            )
                        });
                    if current_ok {
                        continue;
                    }
                    if layer.properties.mask_mode.is_geometry() {
                        let alpha_ok =
                            layer
                                .keyframes
                                .iter()
                                .flat_map(|k| &k.elements)
                                .all(|elem| {
                                    let mut visiting = std::collections::HashSet::new();
                                    mask_source_ok(
                                        &elem.kind,
                                        MaskMode::Alpha,
                                        &by_id,
                                        &decodable_bitmaps,
                                        &mut visiting,
                                    )
                                });
                        mask_fixes.push((si, li, alpha_ok));
                    } else {
                        mask_fixes.push((si, li, false));
                    }
                }
            }
        }
        if mask_fixes.is_empty() {
            break;
        }
        for (si, li, promote_to_alpha) in mask_fixes {
            let sym = &mut project.symbols[si];
            let mask_id = sym.timeline.layers[li].id.clone();
            let loc = format!("{} layer {}", sym.name, sym.timeline.layers[li].name);
            if promote_to_alpha {
                sym.timeline.layers[li].properties.mask_mode = MaskMode::Alpha;
                report.warning(
                    "fla_mask_layer_approximated",
                    loc,
                    "Mask layer contains bitmap elements and was promoted to alpha masking.",
                );
            } else {
                sym.timeline.layers[li].properties.role = LayerRole::Normal;
                sym.timeline.layers[li].properties.mask_mode = MaskMode::Geometry;
                sym.timeline.layers[li].properties.mask_invert = false;
                for sibling in &mut sym.timeline.layers {
                    if sibling.properties.mask.as_deref() == Some(mask_id.as_str()) {
                        sibling.properties.mask = None;
                    }
                }
                report.warning(
                    "fla_mask_layer_approximated",
                    loc,
                    "Mask layer contains text, video or unresolved elements not supported as a mask and is imported as a normal layer.",
                );
            }
        }
    }
}

const KNOWN_CPIC_TEXT_FLAGS: u32 = 0x0003_ffff;
const KNOWN_FONT_TECH_FLAGS: u32 = 0xc03f_0061;

fn convert_accessibility(
    stream: &BinaryTimelineStream,
    acc: Option<&TimelineAccessibility>,
    fallback_name: &str,
    role: AccessibleRole,
) -> Result<Option<Accessibility>, String> {
    let Some(acc) = acc else {
        return Ok(None);
    };
    if acc.revision == 0 {
        return Ok(None);
    }
    let raw_name = acc
        .historical_wide_name
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            acc.name
                .as_ref()
                .and_then(|sp| optional_text(&stream.original_bytes, Some(sp)).ok())
        })
        .unwrap_or_default();
    let raw_desc = acc
        .historical_wide_description
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            acc.description
                .as_ref()
                .and_then(|sp| optional_text(&stream.original_bytes, Some(sp)).ok())
        })
        .unwrap_or_default();
    let raw_shortcut = optional_text(&stream.original_bytes, acc.shortcut.as_ref())?;
    let raw_tab = optional_text(&stream.original_bytes, acc.tab_index.as_ref())?;
    let tab_order = raw_tab.trim().parse::<u32>().unwrap_or(0).min(10_000);
    let disabled = acc.flags.unwrap_or(0) != 0;
    if raw_name.trim().is_empty()
        && raw_desc.trim().is_empty()
        && raw_shortcut.trim().is_empty()
        && tab_order == 0
        && !disabled
    {
        return Ok(None);
    }
    let chosen_name = if !raw_name.trim().is_empty() {
        raw_name.trim()
    } else if !fallback_name.trim().is_empty() {
        fallback_name.trim()
    } else {
        "Accessible Item"
    };
    let mut name: String = chosen_name
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect();
    if name.trim().is_empty() {
        name = "Accessible Item".to_owned();
    }
    let description: String = raw_desc
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(4096)
        .collect();
    Ok(Some(Accessibility {
        name,
        description,
        role,
        tab_order,
        disabled,
    }))
}

fn decode_cpic_text_style(
    stream: &BinaryTimelineStream,
    style: &TimelineTextStyle,
    lossy: &mut Vec<&'static str>,
) -> Result<(TextFormat, f64), String> {
    if style.revision < 2 {
        return Err(
            "Text has no archived font or size; a revision-only style cannot be reconstructed."
                .into(),
        );
    }
    if style.rotation.unwrap_or(0) != 0 {
        lossy.push("rotated characters are laid out horizontally");
    }
    let face = optional_text(&stream.original_bytes, style.font_face.as_ref())?;
    let family = optional_text(&stream.original_bytes, style.font_family.as_ref())?;
    let [red, green, blue, alpha] = style
        .color_rgba
        .ok_or("Text style has no archived color")?
        .to_le_bytes();
    let (align, align_mode) = match style.alignment.unwrap_or(0) {
        0 => (TextAlign::Left, None),
        1 => (TextAlign::Right, None),
        2 => (TextAlign::Center, None),
        3 => (TextAlign::Left, Some("justify".to_owned())),
        _ => {
            lossy.push("unknown alignment is shown left-aligned");
            (TextAlign::Left, None)
        }
    };
    let twips = |value: Option<i16>| f64::from(value.unwrap_or(0)) / 20.;
    let mut value = TextFormat {
        font: if face.is_empty() { family } else { face },
        size: f64::from(
            style
                .font_size_twips
                .ok_or("Text style has no archived size")?,
        ) / 20.,
        color: (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue),
        bold: style.bold.unwrap_or(0) != 0,
        italic: style.italic.unwrap_or(0) != 0,
        leading: twips(style.line_spacing),
        align,
        left_margin: twips(style.left_margin),
        right_margin: twips(style.right_margin),
        indent: twips(style.indent),
        letter_spacing: twips(style.letter_spacing),
        kerning: style.auto_kern.map(|value| value != 0),
        url: optional_text(
            &stream.original_bytes,
            style.url_override.as_ref().or(style.url.as_ref()),
        )?,
        target: optional_text(&stream.original_bytes, style.target.as_ref())?,
        ..TextFormat::default()
    };
    let target_needs_clean = value.target.len() > 256 || value.target.chars().any(char::is_control);
    if value.validate().is_err() || target_needs_clean {
        if value.font.trim().is_empty()
            || value.font.len() > 1024
            || value.font.chars().any(char::is_control)
        {
            let cleaned: String = value
                .font
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect();
            value.font = if cleaned.trim().is_empty() {
                "Times New Roman".into()
            } else {
                cleaned
            };
        }
        if value.url.len() > 4096 || value.url.chars().any(char::is_control) {
            value.url = clean_bounded_text(&value.url, 4096);
        }
        if target_needs_clean {
            value.target = clean_bounded_text(&value.target, 256);
        }
        value.size = value.size.clamp(1., 9999.);
        value.leading = value.leading.clamp(-9999., 9999.);
        value.left_margin = value.left_margin.clamp(0., 9999.);
        value.right_margin = value.right_margin.clamp(0., 9999.);
        value.indent = value.indent.clamp(-9999., 9999.);
        value.letter_spacing = value.letter_spacing.clamp(-1000., 1000.);
        if value.size * 1.2 + value.leading <= 0. {
            value.leading = (-value.size * 1.2 + 0.5).clamp(-9999., 9999.);
        }
        lossy.push("out-of-range size, spacing or margins are clamped");
    }
    if let Some(mode) = align_mode {
        value.set_align_mode(Some(mode));
    }
    value.validate().map_err(|error| error.to_string())?;
    Ok((
        value,
        if style.revision < 4 {
            1.
        } else {
            f64::from(alpha) / 255.
        },
    ))
}

fn clean_bounded_text(value: &str, max_bytes: usize) -> String {
    let mut cleaned = String::with_capacity(value.len().min(max_bytes));
    for character in value.chars().filter(|character| !character.is_control()) {
        if cleaned.len() + character.len_utf8() > max_bytes {
            break;
        }
        cleaned.push(character);
    }
    cleaned
}

pub(super) fn convert_timeline_text(
    stream: &BinaryTimelineStream,
    text: &TimelineText,
    id: &str,
    name: &str,
    _contents_revision: Option<u8>,
    report: &mut ImportReport,
) -> Result<Element, String> {
    let flags = text.flags.unwrap_or(0);
    let is_dynamic_or_input = (flags & 0x0001) != 0;
    let is_input = is_dynamic_or_input && (flags & 0x0002) == 0;
    let display_as_password = is_dynamic_or_input && (flags & 0x0004) != 0;
    let mut lossy = Vec::new();
    if (flags & !KNOWN_CPIC_TEXT_FLAGS) != 0 {
        lossy.push("unrecognized text flag bits remain in the embedded source");
    }
    let raw_variable = optional_text(&stream.original_bytes, text.variable_name.as_ref())?;
    if !is_dynamic_or_input && !raw_variable.is_empty() {
        lossy.push("the field's variable binding is not connected");
    }
    let clean_variable: String = raw_variable
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect();
    let instance_name = optional_text(&stream.original_bytes, text.instance_name.as_ref())?;

    let bounds = text.bounds_words.ok_or("Text has no archived bounds")?;
    let [left, right, top, bottom] = bounds.map(|value| f64::from(value) / 20.);
    let dimensions = TextBoxSize {
        width: right - left,
        height: bottom - top,
    };
    dimensions.validate().map_err(|error| error.to_string())?;

    let style = text
        .runs
        .first()
        .map(|run| &run.style)
        .or(text.default_style.as_ref())
        .ok_or("Text has no run or default style")?;

    let (mut format, alpha) = decode_cpic_text_style(stream, style, &mut lossy)?;
    let html = is_dynamic_or_input && (flags & (0x8000 | 0x0080)) != 0;
    let mut characters = String::new();
    let mut format_runs: Vec<TextFormatRun> = Vec::new();
    let mut nonempty_styles: Vec<&TimelineTextStyle> = Vec::new();
    let mut utf16_cursor = 0u32;
    let mut prev_ended_cr = false;
    for run in &text.runs {
        let (run_format, run_alpha) = decode_cpic_text_style(stream, &run.style, &mut lossy)?;
        if (run_alpha - alpha).abs() > 1e-4 {
            lossy.push("per-run alpha uses the first run's alpha");
        }
        // A Flash paragraph break can end one style run and continue with LF
        // in the next. Count UTF-16 offsets after normalizing that boundary.
        let raw_chars = read_string(&stream.original_bytes, &run.characters)?;
        let trimmed = if prev_ended_cr {
            raw_chars.strip_prefix('\n').unwrap_or(&raw_chars)
        } else {
            &raw_chars
        };
        if !raw_chars.is_empty() {
            prev_ended_cr = raw_chars.ends_with('\r');
        }
        let run_chars = normalize_text(trimmed);
        let run_units = run_chars.encode_utf16().count() as u32;
        if run_units > 0 {
            nonempty_styles.push(&run.style);
            if let Some(last) = format_runs.last_mut()
                && last.end == utf16_cursor
                && last.format == run_format
            {
                last.end = utf16_cursor + run_units;
            } else {
                format_runs.push(TextFormatRun {
                    start: utf16_cursor,
                    end: utf16_cursor + run_units,
                    format: run_format,
                });
            }
            utf16_cursor += run_units;
        }
        characters.push_str(&run_chars);
    }
    if format_runs.len() > 1 {
        format = format_runs[0].format.clone();
    } else if let Some(single) = format_runs.pop() {
        format = single.format;
    }

    if nonempty_styles.is_empty() {
        nonempty_styles.push(style);
    }
    let first_pos = nonempty_styles[0].character_position.unwrap_or(0);
    let (uniform_superscript, uniform_subscript) = if nonempty_styles
        .iter()
        .all(|s| s.character_position.unwrap_or(0) == first_pos)
    {
        match first_pos {
            0 => (false, false),
            1 => (true, false),
            2 => (false, true),
            _ => {
                lossy.push("superscript/subscript is shown on the baseline");
                (false, false)
            }
        }
    } else {
        lossy.push("mixed per-run superscript/subscript uses uniform baseline");
        (false, false)
    };

    let first_vert = nonempty_styles[0].vertical.unwrap_or(0) != 0;
    let uniform_vertical = if nonempty_styles
        .iter()
        .all(|s| (s.vertical.unwrap_or(0) != 0) == first_vert)
    {
        first_vert && nonempty_styles.iter().all(|s| s.rotation.unwrap_or(0) == 0)
    } else {
        lossy.push("mixed vertical/horizontal runs are laid out horizontally");
        false
    };
    let uniform_rtl = nonempty_styles
        .iter()
        .all(|s| s.right_to_left.unwrap_or(0) != 0);

    let decode_style_aa = |st: &TimelineTextStyle| -> Option<(u8, f64, f64)> {
        if (flags & 0x20000) != 0 || st.bitmap_rendering.unwrap_or(0) != 0 {
            return None;
        }
        let aa_span = st.anti_alias?;
        let aa_bytes = stream.original_bytes.get(aa_span.start..aa_span.end)?;
        if aa_bytes.len() < 2 {
            return None;
        }
        let rev = aa_bytes[0];
        let mode = aa_bytes[1];
        let float_offset = if rev == 1 { 8 } else { 2 };
        if mode == 2 && aa_bytes.len() >= float_offset + 8 {
            let raw_thickness =
                f32::from_le_bytes(aa_bytes[float_offset..float_offset + 4].try_into().unwrap());
            let raw_sharpness = f32::from_le_bytes(
                aa_bytes[float_offset + 4..float_offset + 8]
                    .try_into()
                    .unwrap(),
            );
            let t = if raw_thickness.is_finite() {
                f64::from(raw_thickness).clamp(-200.0, 200.0)
            } else {
                0.0
            };
            let s = if raw_sharpness.is_finite() {
                f64::from(raw_sharpness).clamp(-400.0, 400.0)
            } else {
                0.0
            };
            Some((2, s, t))
        } else if mode == 1 {
            Some((1, 0.0, 0.0))
        } else {
            Some((0, 0.0, 0.0))
        }
    };

    let has_bitmap_aa = (flags & 0x20000) == 0
        && nonempty_styles
            .iter()
            .any(|s| s.bitmap_rendering.unwrap_or(0) != 0);
    if has_bitmap_aa {
        lossy.push("bitmap-aliased text rendering uses normal antialiasing");
    }
    let decoded_aas: Vec<(u8, f64, f64)> = nonempty_styles
        .iter()
        .filter_map(|st| decode_style_aa(st))
        .collect();
    if decoded_aas.windows(2).any(|w| w[0].0 != w[1].0) {
        lossy.push("mixed per-run anti-aliasing uses uniform field anti-aliasing");
    }
    let (custom_aa, aa_sharpness, aa_thickness) =
        if let Some(&(_, s, t)) = decoded_aas.iter().find(|&&(mode, _, _)| mode == 2) {
            (true, s, t)
        } else if decoded_aas.iter().any(|&(mode, _, _)| mode == 1) {
            (true, 0.0, 0.0)
        } else {
            (false, 0.0, 0.0)
        };

    let localization_id = optional_text(&stream.original_bytes, text.localization_id.as_ref())?;
    if !localization_id.is_empty() {
        lossy.push("text localization string ID remains in the embedded source");
    }

    let name = if name.is_empty() {
        instance_name.as_str()
    } else {
        name
    };
    let accessibility = convert_accessibility(
        stream,
        text.accessibility.as_ref(),
        name,
        AccessibleRole::Button,
    )?;
    let words = text.matrix_words.ok_or("Text has no archived matrix")?;
    let [a, b, c, d] =
        [words[0], words[1], words[2], words[3]].map(|value| f64::from(value) / 65536.);
    let mut transform = Transform::from_matrix([
        a,
        b,
        c,
        d,
        f64::from(words[4]) / 20. + a * left + c * top,
        f64::from(words[5]) / 20. + b * left + d * top,
    ])
    .ok_or("Text matrix is outside supported transform bounds")?;
    transform.alpha = alpha;

    let location = if name.is_empty() { id } else { name };
    if !lossy.is_empty() {
        lossy.dedup();
        let var_suffix = if !is_dynamic_or_input && !raw_variable.is_empty() {
            format!(" (variable \"{raw_variable}\")")
        } else {
            String::new()
        };
        report.warning(
            "fla_text_approximated",
            location,
            format!(
                "Text imported with approximations: {}{var_suffix}. The original text record is retained in the embedded source.",
                lossy.join("; "),
            ),
        );
    }
    if !report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "fla_text_metrics")
    {
        report.warning(
            "fla_text_metrics",
            location,
            "Text, font names, style, bounds and placement are retained. Shared text layout uses host font metrics and text-box padding, so static glyph placement/antialiasing may differ from Flash.",
        );
    }
    if text
        .runs
        .iter()
        .any(|run| (run.style.font_flags.unwrap_or(0) & !KNOWN_FONT_TECH_FLAGS) != 0)
    {
        report.warning(
            "fla_text_font_flags",
            location,
            "Additional archived font flags remain in the embedded source; named font, size, bold and italic are imported.",
        );
    }

    let filters = convert_filters(&text.filters, location, report);
    let has_newlines = characters.contains(['\r', '\n']);
    let static_fixed_wrap =
        !is_dynamic_or_input && text.auto_expand.unwrap_or(0) == 0 && !uniform_vertical;
    let multiline = has_newlines
        || static_fixed_wrap
        || (is_dynamic_or_input && !display_as_password && (flags & 0x0010) != 0);
    let word_wrap = if is_dynamic_or_input {
        !display_as_password && (flags & 0x0008) != 0
    } else {
        static_fixed_wrap
    };
    let has_typography_opts = uniform_superscript
        || uniform_subscript
        || uniform_vertical
        || custom_aa
        || (is_dynamic_or_input && uniform_rtl);

    if is_dynamic_or_input || !format_runs.is_empty() || html || has_typography_opts {
        let mut opts = AuthoredTextFieldOptions {
            field_type: if is_input {
                TextFieldType::Input
            } else {
                TextFieldType::Dynamic
            },
            selectable: if is_input {
                true
            } else if is_dynamic_or_input {
                (flags & 0x10000) != 0 || (flags & 0x0080) == 0
            } else {
                true
            },
            display_as_password,
            max_chars: if is_dynamic_or_input {
                u32::from(text.max_characters.unwrap_or(0))
            } else {
                0
            },
            variable: if is_dynamic_or_input && !clean_variable.is_empty() {
                Some(clean_variable)
            } else {
                None
            },
            auto_size: if is_dynamic_or_input && text.auto_expand.unwrap_or(0) != 0 {
                if format.align_mode() == Some("justify") {
                    "left".into()
                } else {
                    match format.align {
                        TextAlign::Left => "left".into(),
                        TextAlign::Center => "center".into(),
                        TextAlign::Right => "right".into(),
                    }
                }
            } else {
                "none".into()
            },
            border: is_dynamic_or_input && (flags & 0x0040) != 0,
            background: is_dynamic_or_input && (flags & 0x0040) != 0,
            html,
            embed_fonts: is_dynamic_or_input
                && (flags & 0x20000) == 0
                && ((flags & (0x0020 | 0x3f00)) != 0
                    || text.embed_variant_glyphs.unwrap_or(0) != 0),
            orientation: if uniform_vertical {
                "vertical".into()
            } else {
                "horizontal".into()
            },
            direction: if (is_dynamic_or_input || uniform_vertical) && uniform_rtl {
                "rtl".into()
            } else {
                "ltr".into()
            },
            superscript: uniform_superscript,
            subscript: uniform_subscript,
            anti_alias_type: if custom_aa {
                "advanced".into()
            } else {
                "normal".into()
            },
            sharpness: aa_sharpness,
            thickness: aa_thickness,
            format_runs,
            ..AuthoredTextFieldOptions::default()
        };
        if opts.field_type == TextFieldType::Input && format.bullet {
            opts.field_type = TextFieldType::Dynamic;
        }
        format.set_field_options((!opts.is_default()).then_some(opts));
    }

    Ok(Element {
        id: id.into(),
        name: name.into(),
        transform,
        kind: ElementKind::TextBox {
            text: characters,
            width: dimensions.width,
            height: dimensions.height,
            format,
            word_wrap,
            multiline,
        },
        scripts: vec![],
        accessibility,
        filters,
        ..Element::default()
    })
}
