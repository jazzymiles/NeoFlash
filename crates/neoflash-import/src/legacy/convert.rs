//! Native binary authoring import. Source records remain embedded verbatim;
//! unsupported visual or executable records stop conversion with a location.
mod assets;
mod shape;
#[cfg(test)]
mod tests;
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
    let (resources, bitmaps, sounds) = assets::resources(document, report)?;
    let (folders, folder_paths) = assets::folders(c)?;
    let mut library = BTreeMap::new();
    for d in &c.descriptors {
        if let (Some(id), Some(index)) = (d.library_id, d.stream_index)
            && !c.scenes.contains(&d.object_id)
            && library.insert(id, sid(index)).is_some()
        {
            return Err("Duplicate library ID".into());
        }
    }
    let mut context = Context {
        report,
        library,
        bitmaps,
        sounds,
        language,
        include_hidden,
        generated: vec![],
        provenance: vec![],
        missing_bitmaps: BTreeMap::new(),
    };
    let mut symbols = Vec::new();
    for d in &c.descriptors {
        if d.class != ContentsDescriptorClass::DocumentPage {
            continue;
        }
        let index = d.stream_index.ok_or("Library descriptor has no stream")?;
        let stream = document
            .streams
            .iter()
            .find(|s| s.index == index)
            .ok_or("Library timeline is missing")?;
        let name = read_string(&c.original_bytes, &d.name)?;
        let kind = if scene_indices.contains(&index) {
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
        let folder = match parent.map(|id| folder_paths.get(&id)) {
            Some(Some(path)) => path.clone(),
            Some(None) => {
                context.report.warning(
                    "fla_library_folder",
                    &name,
                    "Library folder record is unresolved; the item is placed at the library root.",
                );
                String::new()
            }
            None => String::new(),
        };
        let linkage = d
            .linkage
            .as_ref()
            .or_else(|| d.library_metadata.as_ref().and_then(|m| m.linkage.as_ref()));
        let linkage_id = linkage
            .map(|l| read_string(&c.original_bytes, &l.identifier))
            .transpose()?
            .filter(|v| !v.is_empty());
        let class_name = optional_text(
            &c.original_bytes,
            linkage.and_then(|l| l.class_name.as_ref()),
        )?;
        let timeline = context.timeline(stream)?;
        symbols.push(Symbol {
            id: sid(index),
            name,
            folder,
            kind,
            timeline,
            class_name: (!class_name.is_empty()).then_some(class_name),
            linkage_id,
        });
    }
    symbols.append(&mut context.generated);
    normalize_playback(&mut symbols, context.report);
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
    // The provenance record is sized first: it shares the same budget.
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
    if !document.inspection.container_tolerances.is_empty() {
        context.report.warning("fla_container_compatibility","CFB directory",format!("The source container has non-canonical metadata outside its live streams ({}). Streams were read from their directory chains unchanged; the original FLA is retained.",document.inspection.container_tolerances.join("; ")))
    }
    let mut project = Project {
        format_version: 5,
        name: std::path::Path::new(&context.report.source)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        width: u32::try_from((size[0] + 10) / 20)
            .ok()
            .filter(|v| *v > 0)
            .ok_or("Stage width overflow")?,
        height: u32::try_from((size[1] + 10) / 20)
            .ok()
            .filter(|v| *v > 0)
            .ok_or("Stage height overflow")?,
        fps: (settings.frame_rate().round() as u32).max(1),
        background: shape::color(settings.background_rgba_word),
        root: sid(root_index),
        symbols,
        library_folders: folders,
        sources: vec![],
        resources,
        document_class: None,
        scenes: if scene_indices.len() > 1 {
            scene_indices.iter().map(|i| sid(*i)).collect()
        } else {
            vec![]
        },
    };
    // Size the converted document first: the original shares both the resource
    // budget and the project's encoded JSON limit with the converted content.
    // Saved projects are pretty-printed JSON; measure that form.
    let document_json = serde_json::to_string_pretty(&project).map_or(usize::MAX, |v| v.len());
    let used: usize = project
        .resources
        .iter()
        .map(|r| r.data_base64.len() / 4 * 3 + 3)
        .sum();
    let provenance_json = serde_json::to_vec(&provenance(true, ProvenanceDetail::Full))
        .map_or(usize::MAX, |v| v.len());
    let encoded = |bytes: usize| bytes.div_ceil(3) * 4;
    let mut embedded = !parts.is_empty()
        && project.resources.len() + parts.len() < MAX_RESOURCES
        && used + document.original_bytes.len() + provenance_json + 4096
            <= MAX_RESOURCE_TOTAL_BYTES
        && document_json
            .saturating_add(encoded(document.original_bytes.len()))
            .saturating_add(encoded(provenance_json))
            .saturating_add(1 << 20)
            <= MAX_PROJECT_JSON_BYTES;
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
    bitmaps: BTreeMap<u16, assets::BitmapAsset>,
    sounds: BTreeMap<u16, assets::SoundAsset>,
    language: ScriptLanguage,
    include_hidden: bool,
    generated: Vec<Symbol>,
    provenance: Vec<serde_json::Value>,
    missing_bitmaps: BTreeMap<u16, String>,
}
impl Context<'_> {
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
        let classify_envelope = |pts: &[(u32, u16, u16)]| -> Option<SoundEffect> {
            if pts.is_empty() || pts.iter().all(|&(_, l, r)| l == 0x8000 && r == 0x8000) {
                Some(SoundEffect::None)
            } else if pts.iter().all(|&(_, l, r)| l == 0x8000 && r == 0) {
                Some(SoundEffect::LeftChannel)
            } else if pts.iter().all(|&(_, l, r)| l == 0 && r == 0x8000) {
                Some(SoundEffect::RightChannel)
            } else if let [(0, 0, 0), (_, 0x8000, 0x8000)] = pts {
                Some(SoundEffect::FadeIn)
            } else if let [(_, 0x8000, 0x8000), (_, 0, 0)] = pts {
                Some(SoundEffect::FadeOut)
            } else if let [(_, 0x8000, 0), (_, 0, 0x8000)] = pts {
                Some(SoundEffect::FadeLeftToRight)
            } else if let [(_, 0, 0x8000), (_, 0x8000, 0)] = pts {
                Some(SoundEffect::FadeRightToLeft)
            } else {
                None
            }
        };
        let effect = match f.sound_effect {
            Some(0) => SoundEffect::None,
            Some(1) => SoundEffect::LeftChannel,
            Some(2) => SoundEffect::RightChannel,
            Some(3) => SoundEffect::FadeLeftToRight,
            Some(4) => SoundEffect::FadeRightToLeft,
            Some(5) => SoundEffect::FadeIn,
            Some(6) => SoundEffect::FadeOut,
            Some(7) | None => match envelope_points().as_deref().and_then(classify_envelope) {
                Some(preset) => preset,
                None => {
                    self.report.warning(
                        "fla_frame_sound",
                        at,
                        "Custom frame-sound envelope is approximated without gain automation; the envelope points remain in the embedded source.",
                    );
                    SoundEffect::None
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
                SoundEffect::None
            }
        };
        let in_trimmed = f.sound_in_point.unwrap_or(0) > 0;
        let out_point = f.sound_out_point.unwrap_or(0);
        let out_trimmed = out_point > 0
            && out_point < 0x3fff_ffff
            && asset.duration_44 > 0
            && out_point.saturating_add(8) < asset.duration_44;
        if in_trimmed || out_trimmed {
            self.report.warning(
                "fla_frame_sound",
                at,
                "Frame-sound in/out trim points are not applied yet; the full sound resource plays and trim offsets remain in the embedded source.",
            );
        }
        Some(FrameSound {
            resource: asset.resource_id,
            sync,
            loops,
            loop_forever,
            effect,
        })
    }
    fn actions(
        &mut self,
        stream: &BinaryTimelineStream,
        actions: Option<&TimelineActions>,
    ) -> Result<Option<Script>, String> {
        let Some(a) = actions else { return Ok(None) };
        match a.presence {
            TimelineActionPresence::Empty => Ok(None),
            TimelineActionPresence::Source if a.complete => Ok(Some(Script {
                language: self.language,
                source: read_string(
                    &stream.original_bytes,
                    a.source.as_ref().ok_or("Missing action source")?,
                )?,
            })),
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
                        format!("stream #{} @0x{:x}", stream.index, a.encoded.start),
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
            _ => Err(format!(
                "Stream #{} @0x{:x}: compiled/historical action records need a dedicated importer",
                stream.index, a.encoded.start
            )),
        }
    }
    fn timeline(&mut self, s: &BinaryTimelineStream) -> Result<Timeline, String> {
        let page = object(s, s.root.ok_or("Timeline has no root")?)?;
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
            let Some(TimelinePayload::Layer(l)) = &o.payload else {
                self.report.warning("fla_layer_skipped",format!("stream #{} object #{id}",s.index),if o.complete {"A timeline child is not a layer record; it remains in the embedded source."} else {"The layer record stops before its settings; it remains in the embedded source."});
                continue;
            };
            let name = optional_text(&s.original_bytes, l.name.as_ref())?;
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
            if !editor_visible && self.include_hidden {
                self.report.warning("fla_publish_hidden_layer",format!("stream #{} layer {name}",s.index),"Layer is visible according to the source publish profile; its original editor eye toggle is retained in provenance.")
            }
            self.provenance.push(serde_json::json!({"stream":s.index,"object":id,"target":layer_id,"editor_visible":editor_visible,"source":o.encoded}));
            let mut layer = Layer {
                id: layer_id,
                name,
                visible: editor_visible || self.include_hidden,
                locked,
                keyframes: vec![],
                properties,
            };
            let mut cursor = 0u32;
            for frame_id in &o.children {
                let frame = object(s, *frame_id)?;
                let Some(TimelinePayload::Frame(f)) = &frame.payload else {
                    self.report.warning(
                        "fla_frame_skipped",
                        format!("stream #{} layer {}", s.index, layer.name),
                        "A layer child is not a frame record; it remains in the embedded source.",
                    );
                    continue;
                };
                if !frame.complete {
                    // Only fully read frames are converted; the stream stop is reported.
                    break;
                }
                if f.duration == Some(0) {
                    // Flash writes empty zero-length keys at the start of some
                    // layers; they occupy no frame, so the next key starts here too.
                    if !frame.children.is_empty()
                        || frame.shape_index.is_some()
                        || f.label.is_some()
                        || f.actions
                            .as_ref()
                            .is_some_and(|a| a.presence != TimelineActionPresence::Empty)
                    {
                        self.report.warning("fla_frame_duration",format!("stream #{} layer {} frame {}",s.index,layer.name,cursor+1),"A zero-length keyframe with content occupies no frame in Flash and is skipped; it remains in the embedded source.");
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
                if f.rotation_mode.unwrap_or(0) != 0 || f.rotation_count.unwrap_or(0) != 0 {
                    self.report.warning("fla_tween_rotation",&at,"Directed tween rotation (clockwise/counter-clockwise turns) is interpolated along the shortest angle.");
                }
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
                let mut elements = Vec::new();
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
                    match shape::convert_shape(
                        &s.shapes[index].record,
                        &format!("fla-{}-{}-shape-{frame_id}", s.index, id),
                        self.report,
                        &format!("stream #{} frame {cursor}", s.index),
                    ) {
                        Ok(paths) => elements.extend(paths),
                        Err(e) => self.report.warning("fla_shape_skipped",&at,format!("Frame artwork was not converted: {e}. It remains in the embedded source.")),
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
                if let Some(previous) = layer.keyframes.last_mut() {
                    for (a, b) in previous.elements.iter().zip(elements.iter_mut()) {
                        if placement_matches(a, b) {
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
                            || previous.elements[0].id != elements[0].id)
                    {
                        previous.tween = None;
                        self.report.warning("fla_tween_dropped",&at,"This classic motion tween does not connect one stable symbol placement, so the frames hold their keyframe poses.");
                    }
                }
                if let Some(script) = self.actions(s, f.actions.as_ref())? {
                    timeline.scripts.push(FrameScript {
                        frame: cursor,
                        script,
                    })
                }
                layer.keyframes.push(Keyframe {
                    frame: cursor,
                    elements,
                    tween,
                    tween_kind,
                    sound,
                    label: optional_text(&s.original_bytes, f.label.as_ref())?,
                    ..Default::default()
                });
                let duration = u32::from(f.duration.ok_or("Frame duration absent")?);
                cursor = cursor
                    .checked_add(duration)
                    .ok_or("Timeline length overflow")?;
            }
            if let Some(last) = layer.keyframes.last_mut()
                && last.tween.is_some()
            {
                // Flash shows an unterminated tween as a held pose.
                last.tween = None;
                last.tween_kind = TweenKind::Classic;
            }
            if layer.keyframes.is_empty() && layer.properties.role != LayerRole::Folder {
                self.report.warning("fla_layer_skipped",format!("stream #{} layer {}",s.index,layer.name),"No complete frame could be read for this layer; it remains in the embedded source.");
                continue;
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
        };
        match &o.payload {
            Some(TimelinePayload::Instance(i)) => {
                if !i.filters.is_empty() {
                    let kinds: Vec<_> = i.filters.iter().map(|f| format!("{:?}", f.kind)).collect();
                    self.report.warning("fla_filters_unsupported",&location,format!("Filters ({}) are not rendered yet; the instance is drawn without them and the decoded filters remain in the embedded source.",kinds.join(", ")));
                }
                if i.blend_mode.is_some_and(|v| v > 1) {
                    self.report.warning("fla_blend_mode",&location,"Blend modes are not rendered yet; the instance is drawn normally and its blend mode remains in the embedded source.");
                }
                if i.is_extensible.is_some_and(|v| v != 0)
                    || !i.animation_frame_settings.is_empty()
                    || i.property_count.is_some_and(|v| v != 0)
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
                if i.matrix_3d.is_some_and(|m| {
                    m != [
                        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                    ]
                }) {
                    self.report.warning("fla_3d_transform",&location,"3D transforms are not rendered; the instance uses its 2D placement matrix.");
                }
                element.name = optional_text(
                    &s.original_bytes,
                    i.instance_name.as_ref().or(i.legacy_name.as_ref()),
                )?;
                element.transform = Transform::from_matrix(shape::matrix(
                    i.matrix_words.ok_or("Missing instance matrix")?,
                ))
                .ok_or("Invalid instance matrix")?;
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
                let symbol_id = self
                    .library
                    .get(&i.library_id.ok_or("Missing library identity")?)
                    .ok_or_else(|| format!("{location}: unresolved library reference"))?
                    .clone();
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
                if let Some(script) = self.actions(s, i.actions.as_ref())? {
                    element.scripts.push(ObjectScript {
                        event: ObjectEvent::Load,
                        script,
                    });
                }
                if i.accessibility.as_ref().is_some_and(|a| a.revision != 0) {
                    self.report.warning("fla_accessibility_metadata",&location,"Source accessibility metadata remains embedded; no inferred role was assigned.");
                }
            }
            Some(TimelinePayload::Text(t)) => {
                element = text::convert_text(s, t, id, "", self.report)?;
            }
            Some(TimelinePayload::Bitmap(b)) => {
                let media = b.media_id.ok_or("Missing bitmap identity")?;
                element.transform = Transform::from_matrix(shape::matrix(
                    b.matrix_words.ok_or("Missing bitmap matrix")?,
                ))
                .ok_or("Invalid bitmap matrix")?;
                if let Some(asset) = self.bitmaps.get(&media) {
                    element.kind = ElementKind::Bitmap {
                        resource_id: asset.resource_id.clone(),
                        width: asset.width as u32,
                        height: asset.height as u32,
                        smoothing: asset.smoothing,
                    };
                } else {
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
                    self.report.warning("fla_source_bitmap_missing",&location,format!("Bitmap #{media} is already absent from the source archive. The placement and unresolved reference are retained; no image was invented."));
                }
            }
            Some(TimelinePayload::Shape) => {
                let mut members = Vec::new();
                for (slot, child) in o.children.iter().enumerate() {
                    members.push(self.element_or_placeholder(
                        s,
                        *child,
                        &format!("{id}-member-{slot}"),
                        depth + 1,
                    ))
                }
                if let Some(index) = o.shape_index {
                    match shape::convert_shape(
                        &s.shapes[index].record,
                        &format!("{id}-paint"),
                        self.report,
                        &location,
                    ) {
                        Ok(paths) => members.extend(paths),
                        Err(e) => self.report.warning("fla_shape_skipped",&location,format!("Group artwork was not converted: {e}. It remains in the embedded source.")),
                    }
                }
                // A one-frame graphic keeps group membership and vector paint order editable.
                let symbol_id = format!("{id}-group");
                self.generated.push(group_symbol(
                    &symbol_id,
                    format!("Source group {object_id}"),
                    members,
                ));
                element.kind = ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::SingleFrame(0),
                };
            }
            _ => {
                return Err(format!(
                    "{location}: unsupported display record {:?}",
                    o.class
                ));
            }
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
            let fixed = match *playback {
                InstancePlayback::SingleFrame(f) if f >= len => {
                    InstancePlayback::SingleFrame(len - 1)
                }
                InstancePlayback::PlayOnceFrom(f) if f >= len => {
                    InstancePlayback::PlayOnceFrom(len - 1)
                }
                InstancePlayback::LoopFrom(f) if f >= len => InstancePlayback::LoopFrom(f % len),
                _ => continue,
            };
            report.warning("fla_graphic_first_frame",&element.id,format!("The graphic's first frame lies beyond the symbol's {len} frames; it is mapped to {fixed:?}."));
            *playback = fixed;
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

    // 2. Shed imported bitmap resources (legacy-bitmap-*, largest first).
    while json.len() > MAX_PROJECT_JSON_BYTES {
        let excess = json.len() - MAX_PROJECT_JSON_BYTES;
        if !shed_bitmap_resources(project, report, excess) {
            break;
        }
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

fn shed_audio_resources(project: &mut Project, report: &mut ImportReport, excess: usize) -> bool {
    let mut candidates: Vec<(usize, u16, String)> = project
        .resources
        .iter()
        .filter_map(|r| {
            let suffix = r.id.strip_prefix("legacy-sound-")?;
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

fn shed_bitmap_resources(project: &mut Project, report: &mut ImportReport, excess: usize) -> bool {
    let mut placement_counts = BTreeMap::<String, usize>::new();
    for element in project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
    {
        if let ElementKind::Bitmap { resource_id, .. } = &element.kind {
            *placement_counts.entry(resource_id.clone()).or_default() += 1;
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
                                _ => ElementKind::Rectangle {
                                    width: 0.0,
                                    height: 0.0,
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
                                ElementKind::Rectangle {
                                    width: 0.0,
                                    height: 0.0,
                                    fill: [0; 4],
                                    stroke: [0; 4],
                                    stroke_width: 0.0,
                                }
                            },
                            scripts: vec![],
                            accessibility: None,
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
