//! Versioned SWF movie compiler (`T-192`) with AVM1/AVM2 preflight, character dictionary
//! generation (`DefineShape3`/`DefineShape4`, `DefineMorphShape`, `DefineBitsLossless2`,
//! `DefineFont3`, `DefineEditText`, `DefineSound`, `DefineButton2`, `DefineSprite`),
//! display-list tag emission (`PlaceObject2`/`PlaceObject3`, `RemoveObject2`, `FrameLabel`,
//! `StartSound`, `ShowFrame`), phased AVM1 (`DoAction`, `DoInitAction`, clip/button actions)
//! and AVM2 (`DoABC2`, `SymbolClass`) bytecode emission, and `FWS`/`CWS`/`ZWS` container
//! compression.

use crate::ExportError;
use crate::fla_preflight::{
    FlaCompatibilityCategory, FlaCompatibilityReport, FlaCompatibilityStatus, FlaTargetProfile,
    encode_sound_envelope_points, is_actionscript_resource, is_valid_as3_qname,
};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use neoflash_core::{
    BlendMode, Color, ColorTransform, Easing, Element, ElementKind, FillRule, FilterSpec,
    FrameLabelType, FrameSound, GradientKind, GradientPaint, GradientSpread, LayerRole,
    ObjectEvent, ObjectScript, PathCommand, Project, ScriptLanguage, SoundEffect, SoundSync,
    Symbol, SymbolKind, TextAlign, Timeline, TweenKind, flatten_scenes_with_ranges,
    transform_matrix,
};
pub use neoflash_import::{
    SwfAudioEvent, SwfBytecodePlayer, SwfDisplayEntry, SwfFrameState, SwfInspection,
    SwfPlaybackResult, SwfTagCounts, SwfVmKind, play_swf_bytes, play_swf_path,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum StrokeCap {
    Round,
    Butt,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum StrokeJoin {
    Round,
    Bevel,
    Miter,
}

#[derive(Default)]
pub(super) struct SwfOutlineFonts {
    pub(super) system: Option<fontdb::Database>,
    // Recently successful faces avoid searching and rereading the font collection for each label.
    pub(super) recent: Vec<(String, bool, bool, Vec<u8>)>,
    pub(super) substitutions: BTreeSet<(String, String)>,
}

impl SwfOutlineFonts {
    pub(super) fn system_outline(
        &mut self,
        family: &str,
        bold: bool,
        italic: bool,
        text: &str,
        extract: impl Fn(&[u8]) -> Result<Vec<PathCommand>, Vec<String>>,
    ) -> Option<Vec<PathCommand>> {
        use ab_glyph::{Font, FontRef};
        let covers_face = |bytes: &[u8], face_idx: u32| {
            neoflash_core::TextFormat::validate_text_outlines_for_font_face(
                "SwfText", family, text, bytes, face_idx,
            )
            .is_empty()
                && FontRef::try_from_slice_and_index(bytes, face_idx).is_ok_and(|font| {
                    text.chars()
                        .all(|ch| ch.is_whitespace() || font.glyph_id(ch).0 != 0)
                })
        };
        for (name, b, i, bytes) in &self.recent {
            if name == family
                && *b == bold
                && *i == italic
                && covers_face(bytes, 0)
                && let Ok(commands) = extract(bytes)
            {
                return Some(commands);
            }
        }
        let db = self.system.get_or_insert_with(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            db
        });
        let weight = if bold {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        };
        let mut faces: Vec<_> = db.faces().collect();
        faces.sort_by_key(|face| {
            (
                !face
                    .families
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(family)),
                (face.style != fontdb::Style::Normal) != italic,
                face.weight.0.abs_diff(weight.0),
                face.index,
                face.post_script_name.as_str(),
            )
        });
        for face in faces {
            let face_idx = face.index;
            if let Some(Some((commands, cached))) = db.with_face_data(face.id, |bytes, _| {
                if !covers_face(bytes, face_idx) {
                    return None;
                }
                let standalone_sfnt = if face_idx > 0 || bytes.starts_with(b"ttcf") {
                    Some(neoflash_core::extract_ttc_face_to_sfnt(bytes, face_idx)?)
                } else {
                    None
                };
                let slice = standalone_sfnt.as_deref().unwrap_or(bytes);
                let commands = extract(slice).ok()?;
                // Bound resident font bytes for the duration of this one publication.
                let cached = if let Some(sfnt) = standalone_sfnt {
                    (sfnt.len() <= 32 * 1024 * 1024).then_some(sfnt)
                } else {
                    (bytes.len() <= 32 * 1024 * 1024).then(|| bytes.to_vec())
                };
                Some((commands, cached))
            }) {
                self.substitutions
                    .insert((family.to_owned(), face.post_script_name.clone()));
                if let Some(bytes) = cached {
                    if self.recent.len() == 4 {
                        self.recent.remove(0);
                    }
                    self.recent.push((family.to_owned(), bold, italic, bytes));
                }
                return Some(commands);
            }
        }
        None
    }
}

pub(super) fn swf_text_outline(
    project: &Project,
    element: &Element,
    fonts: &mut SwfOutlineFonts,
) -> Result<(Vec<PathCommand>, Color), ExportError> {
    let field = neoflash_core::TextFieldState::from_kind(&element.kind).ok_or_else(|| {
        ExportError::Options(format!("No text field for SWF outline on {}", element.id))
    })?;
    let format = &field.format;
    let font =
        crate::fonts::resolve_project_font_bytes(project, &format.font, format.bold, format.italic)
            .map(|raw| {
                if neoflash_core::sfnt_face_count(&raw) > 1 {
                    let idx = neoflash_core::select_best_sfnt_face_index(
                        &raw,
                        &format.font,
                        format.bold,
                        format.italic,
                    );
                    neoflash_core::extract_ttc_face_to_sfnt(&raw, idx).unwrap_or(raw)
                } else {
                    raw
                }
            });
    let extract = |bytes: Option<&[u8]>| match &element.kind {
        ElementKind::Text {
            text,
            font_family,
            font_size,
            ..
        } => crate::fonts::extract_text_outline_commands_with_diagnostics(
            &element.id,
            text,
            font_family,
            *font_size,
            bytes,
        ),
        _ => crate::fonts::extract_text_field_outline_commands_with_diagnostics(
            &element.id,
            &field,
            bytes,
        ),
    };
    let commands = extract(font.as_deref()).or_else(|errors| {
        fonts
            .system_outline(
                &format.font,
                format.bold,
                format.italic,
                &field.text,
                |bytes| extract(Some(bytes)),
            )
            .ok_or_else(|| ExportError::Options(errors.join("\n")))
    })?;
    let rgb = format.color.to_be_bytes();
    Ok((commands, [rgb[1], rgb[2], rgb[3], field.alpha]))
}

/// Target SWF player & virtual machine profile for SWF movie publishing (`T-192`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SwfPublishProfile {
    /// Flash Player 8 (SWF version 8, ActionScript 1.0 / 2.0 AVM1 bytecode).
    FlashPlayer8Avm1,
    /// Flash Player 9 (SWF version 9, ActionScript 3.0 AVM2 `DoABC2` bytecode, `DefineShape3`).
    FlashPlayer9Avm2,
    /// Flash Player 10+ (SWF version 10, ActionScript 3.0 AVM2 `DoABC2` bytecode, `DefineShape4`).
    FlashPlayer10Avm2,
}

impl SwfPublishProfile {
    /// Base SWF version number for the profile.
    #[must_use]
    pub const fn swf_version(self) -> u8 {
        match self {
            Self::FlashPlayer8Avm1 => 8,
            Self::FlashPlayer9Avm2 => 9,
            Self::FlashPlayer10Avm2 => 10,
        }
    }

    /// Whether this profile targets AVM2 (`UseABC` flag in `FileAttributes`).
    #[must_use]
    pub const fn is_avm2(self) -> bool {
        matches!(self, Self::FlashPlayer9Avm2 | Self::FlashPlayer10Avm2)
    }

    /// Whether this profile supports `DefineShape4` (tag 83) stroke caps/joins/miter.
    #[must_use]
    pub const fn supports_define_shape4(self) -> bool {
        matches!(self, Self::FlashPlayer10Avm2)
    }
}

/// Container compression mode for published SWF files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SwfCompression {
    /// Uncompressed `FWS` container.
    UncompressedFws,
    /// Zlib-compressed `CWS` container (SWF 6+).
    #[default]
    DeflateCws,
    /// LZMA-compressed `ZWS` container (bumps header version byte to at least 13).
    LzmaZws,
}

/// Options controlling SWF movie compilation and preflight behavior.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwfPublishOptions {
    pub profile: SwfPublishProfile,
    pub compression: SwfCompression,
    /// Bake Motion, IK, Puppet, 3D, or incompatible Shape tween spans into frame-by-frame placements.
    pub bake_unsupported_tweens: bool,
    /// Expand variable-width strokes into closed filled contours.
    pub expand_variable_width_strokes: bool,
    /// Convert text boxes into outlined vector glyph paths instead of `DefineEditText`.
    pub outline_text: bool,
    /// Allow publishing visual-only SWF movies by omitting incompatible or JS/TS scripts instead of blocking.
    pub scriptless_mode: bool,
}

impl Default for SwfPublishOptions {
    fn default() -> Self {
        Self {
            profile: SwfPublishProfile::FlashPlayer10Avm2,
            compression: SwfCompression::DeflateCws,
            bake_unsupported_tweens: true,
            expand_variable_width_strokes: true,
            outline_text: false,
            scriptless_mode: false,
        }
    }
}

impl SwfPublishOptions {
    /// Convenience constructor for a specific SWF publish profile.
    #[must_use]
    pub fn for_profile(profile: SwfPublishProfile) -> Self {
        Self {
            profile,
            ..Self::default()
        }
    }
}

/// Extended SWF export options combining `SwfPublishOptions` with optional `strict` preflight enforcement.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwfExportOptions {
    pub publish: SwfPublishOptions,
    /// When `true`, refuse lossy SWF approximations (such as resampling `>15` gradient stops).
    pub strict: bool,
}

impl SwfExportOptions {
    /// Convenience constructor for strict SWF export on a given publish profile.
    #[must_use]
    pub fn strict_for_profile(profile: SwfPublishProfile) -> Self {
        Self {
            publish: SwfPublishOptions::for_profile(profile),
            strict: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SwfLayoutIssue {
    DepthOverflow,
    InvalidMaskTarget(String),
    NestedMask(String),
    AdvancedMaskMode(String),
    MultiElementMask(String),
    NonContiguousMask(String),
}

pub(super) struct SwfTimelineLayout {
    pub(super) depth_bands: Vec<(u16, u16)>,
    pub(super) mask_clip_depths: BTreeMap<usize, u16>,
}

pub(super) fn layer_swf_depth_width(
    layer: &neoflash_core::Layer,
    total_frames: u32,
) -> Option<usize> {
    let effective_end = layer
        .properties
        .end_frame
        .unwrap_or(total_frames)
        .min(total_frames)
        .max(1);
    let mut width = layer
        .keyframes
        .iter()
        .filter(|kf| kf.frame < effective_end)
        .map(|keyframe| keyframe.elements.len())
        .max()
        .unwrap_or(0)
        .max(1);
    for (keyframe_index, keyframe) in layer.keyframes.iter().enumerate() {
        if keyframe.frame >= effective_end {
            break;
        }
        if keyframe.tween.is_some()
            && keyframe.tween_kind == TweenKind::Shape
            && let Some(next_keyframe) = layer.keyframes.get(keyframe_index + 1)
            && next_keyframe.frame < effective_end
            && !can_morph_keys(keyframe, next_keyframe)
        {
            // Baked shape-tween sampling can retain unmatched shapes from both endpoints.
            // Reserve the conservative union so it cannot overlap the following layer's band.
            width = width.max(
                keyframe
                    .elements
                    .len()
                    .checked_add(next_keyframe.elements.len())?,
            );
        }
    }
    Some(width)
}

pub(super) fn layer_has_active_elements_on_frame(
    timeline: &Timeline,
    layer: &neoflash_core::Layer,
    frame: u32,
    total_frames: u32,
) -> bool {
    if !layer.visible {
        return false;
    }
    let effective_end = layer
        .properties
        .end_frame
        .unwrap_or(total_frames)
        .min(total_frames);
    if frame >= effective_end {
        return false;
    }
    let Some(k_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame) else {
        return false;
    };
    let kf = &layer.keyframes[k_idx];
    !kf.elements.is_empty() || !neoflash_core::sample_layer_in(timeline, layer, frame).is_empty()
}

fn layers_have_overlapping_active_frames(
    timeline: &Timeline,
    a_idx: usize,
    b_idx: usize,
    total_frames: u32,
) -> bool {
    let a = &timeline.layers[a_idx];
    let b = &timeline.layers[b_idx];
    (0..total_frames).any(|frame| {
        layer_has_active_elements_on_frame(timeline, a, frame, total_frames)
            && layer_has_active_elements_on_frame(timeline, b, frame, total_frames)
    })
}

pub(super) fn compute_swf_timeline_layout(
    timeline: &Timeline,
) -> Result<SwfTimelineLayout, SwfLayoutIssue> {
    let total_frames = timeline.frame_count.max(1);
    let mut depth_bands = vec![(0u16, 0u16); timeline.layers.len()];
    let mut mask_clip_depths = BTreeMap::new();

    let mut rendered: Vec<usize> = Vec::new();
    let mut rendered_pos_by_layer: BTreeMap<usize, usize> = BTreeMap::new();
    let mut mask_id_to_idx: BTreeMap<&str, usize> = BTreeMap::new();
    for (idx, layer) in timeline.layers.iter().enumerate() {
        if layer.visible && layer.properties.role == LayerRole::Mask {
            mask_id_to_idx.insert(layer.id.as_str(), idx);
        }
        if layer.visible && !matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide) {
            rendered_pos_by_layer.insert(idx, rendered.len());
            rendered.push(idx);
        }
    }

    let mut widths = vec![0usize; timeline.layers.len()];
    for &idx in &rendered {
        let layer = &timeline.layers[idx];
        widths[idx] =
            layer_swf_depth_width(layer, total_frames).ok_or(SwfLayoutIssue::DepthOverflow)?;
        if layer.properties.role == LayerRole::Mask {
            if !layer.properties.mask_mode.is_geometry() || layer.properties.mask_invert {
                return Err(SwfLayoutIssue::AdvancedMaskMode(layer.name.clone()));
            }
            if layer.properties.mask.is_some() {
                return Err(SwfLayoutIssue::NestedMask(layer.name.clone()));
            }
            if widths[idx] > 1 {
                return Err(SwfLayoutIssue::MultiElementMask(layer.name.clone()));
            }
        }
    }

    // Group masked layers with their target mask and ensure each active mask band is contiguous
    // in visible render order so SWF clip_depth can strictly precede its masked layers without
    // accidentally clipping interleaved unmasked layers.
    let mut mask_to_masked: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &idx in &rendered {
        let layer = &timeline.layers[idx];
        if let Some(mask_id) = &layer.properties.mask {
            if layer.properties.role != LayerRole::Normal {
                return Err(SwfLayoutIssue::NestedMask(layer.name.clone()));
            }
            let Some(&mask_idx) = mask_id_to_idx.get(mask_id.as_str()) else {
                return Err(SwfLayoutIssue::InvalidMaskTarget(layer.name.clone()));
            };
            mask_to_masked.entry(mask_idx).or_default().push(idx);
        }
    }

    let mut claimed_mask_span_positions: BTreeSet<usize> = BTreeSet::new();
    let mut block_at_start: BTreeMap<usize, (usize, Vec<usize>, usize)> = BTreeMap::new();
    for (mask_idx, masked_indices) in &mask_to_masked {
        let Some(&mask_rpos) = rendered_pos_by_layer.get(mask_idx) else {
            return Err(SwfLayoutIssue::InvalidMaskTarget(
                timeline.layers[*mask_idx].name.clone(),
            ));
        };
        let mut all_rpos = Vec::with_capacity(masked_indices.len() + 1);
        all_rpos.push(mask_rpos);
        for m_idx in masked_indices {
            if let Some(&rpos) = rendered_pos_by_layer.get(m_idx) {
                all_rpos.push(rpos);
            }
        }
        let min_pos = *all_rpos.iter().min().unwrap_or(&mask_rpos);
        let max_pos = *all_rpos.iter().max().unwrap_or(&mask_rpos);
        if mask_rpos != max_pos && mask_rpos != min_pos {
            return Err(SwfLayoutIssue::NonContiguousMask(
                timeline.layers[*mask_idx].name.clone(),
            ));
        }
        let mut ordered_members = Vec::with_capacity(max_pos - min_pos);
        for (rpos, &candidate_idx) in rendered.iter().enumerate().take(max_pos + 1).skip(min_pos) {
            if !claimed_mask_span_positions.insert(rpos) {
                return Err(SwfLayoutIssue::NonContiguousMask(
                    timeline.layers[*mask_idx].name.clone(),
                ));
            }
            if candidate_idx == *mask_idx {
                continue;
            }
            if !masked_indices.contains(&candidate_idx) {
                let candidate_layer = &timeline.layers[candidate_idx];
                if candidate_layer.properties.role == LayerRole::Mask
                    || candidate_layer.properties.mask.is_some()
                    || layers_have_overlapping_active_frames(
                        timeline,
                        *mask_idx,
                        candidate_idx,
                        total_frames,
                    )
                {
                    return Err(SwfLayoutIssue::NonContiguousMask(
                        timeline.layers[*mask_idx].name.clone(),
                    ));
                }
            }
            ordered_members.push(candidate_idx);
        }
        block_at_start.insert(min_pos, (*mask_idx, ordered_members, max_pos));
    }

    let mut next_depth = 1usize;
    let mut alloc_band = |width: usize| -> Result<(u16, u16), SwfLayoutIssue> {
        let end_depth = next_depth
            .checked_add(width)
            .and_then(|v| v.checked_sub(1))
            .ok_or(SwfLayoutIssue::DepthOverflow)?;
        if end_depth > usize::from(u16::MAX) {
            return Err(SwfLayoutIssue::DepthOverflow);
        }
        let band = (next_depth as u16, end_depth as u16);
        next_depth = end_depth + 1;
        Ok(band)
    };

    let mut pos = 0usize;
    while pos < rendered.len() {
        if let Some((mask_idx, masked_indices, end_pos)) = block_at_start.get(&pos) {
            let mask_band = alloc_band(widths[*mask_idx])?;
            depth_bands[*mask_idx] = mask_band;
            let mut max_masked_depth = mask_band.1;
            for &m_idx in masked_indices {
                let band = alloc_band(widths[m_idx])?;
                depth_bands[m_idx] = band;
                max_masked_depth = max_masked_depth.max(band.1);
            }
            if max_masked_depth > mask_band.0 {
                mask_clip_depths.insert(*mask_idx, max_masked_depth);
            }
            pos = end_pos + 1;
        } else {
            let l_idx = rendered[pos];
            depth_bands[l_idx] = alloc_band(widths[l_idx])?;
            pos += 1;
        }
    }

    Ok(SwfTimelineLayout {
        depth_bands,
        mask_clip_depths,
    })
}

#[cfg(test)]
pub(super) fn timeline_depth_bands(timeline: &Timeline) -> Option<Vec<(u16, u16)>> {
    compute_swf_timeline_layout(timeline)
        .ok()
        .map(|layout| layout.depth_bands)
}

#[derive(Default)]
struct SwfSoundUsage {
    stream: bool,
    event: bool,
}

fn used_swf_media_resources(
    project: &Project,
) -> (BTreeSet<String>, BTreeMap<String, SwfSoundUsage>) {
    let mut bitmaps = BTreeSet::new();
    let mut sounds = BTreeMap::<String, SwfSoundUsage>::new();
    for keyframe in project
        .symbols
        .iter()
        .flat_map(|symbol| &symbol.timeline.layers)
        .flat_map(|layer| &layer.keyframes)
    {
        for element in &keyframe.elements {
            if let ElementKind::Bitmap { resource_id, .. } = &element.kind {
                bitmaps.insert(resource_id.clone());
            }
        }
        bitmaps.extend(
            keyframe
                .bitmap_fills
                .iter()
                .map(|fill| fill.resource_id.clone()),
        );
        if let Some(sound) = &keyframe.sound {
            let usage = sounds.entry(sound.resource.clone()).or_default();
            if sound.sync == SoundSync::Stream {
                usage.stream = true;
            } else {
                usage.event = true;
            }
        }
    }
    (bitmaps, sounds)
}

pub(super) fn swf_text_outline_issue(element: &Element) -> Option<String> {
    let ElementKind::TextBox { format, .. } = &element.kind else {
        return None;
    };
    if format.underline || format.bullet {
        return Some(
            "underline/bullet decoration is not emitted by the current glyph-outline converter"
                .to_owned(),
        );
    }
    let options = format.field_options()?;
    if options.html || !options.format_runs.is_empty() {
        return Some(
            "HTML or per-run rich text cannot be represented faithfully by the current uniform glyph-outline converter"
                .to_owned(),
        );
    }
    if options.border
        || options.background
        || options.highlight
        || options.strikethrough
        || options.superscript
        || options.subscript
    {
        return Some(
            "TextField decoration/background styling is not emitted by the current glyph-outline converter"
                .to_owned(),
        );
    }
    None
}

pub(super) fn strip_as_extension_ci(name: &str) -> &str {
    if name.len() >= 3 && name[name.len() - 3..].eq_ignore_ascii_case(".as") {
        &name[..name.len() - 3]
    } else {
        name
    }
}

pub(super) fn swf_normalize_include_path(parent: &str, requested: &str) -> Result<String, String> {
    let requested = requested.trim();
    if requested.is_empty() || requested.starts_with('/') || requested.contains(['\\', ':', '\0']) {
        return Err("#include requires a relative project resource path using '/'".to_owned());
    }
    let mut parts: Vec<&str> = parent
        .rsplit_once('/')
        .map_or("", |(base, _)| base)
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    for part in requested.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("#include path escapes the project resource root".to_owned());
                }
            }
            value => parts.push(value),
        }
    }
    let result = parts.join("/");
    if strip_as_extension_ci(&result).len() == result.len() {
        return Err("#include expects an .as project resource".to_owned());
    }
    Ok(result)
}

pub(super) fn swf_scan_comments(line: &str, block: &mut bool) {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut quote = None;
    while i < bytes.len() {
        if *block {
            if bytes[i..].starts_with(b"*/") {
                *block = false;
                i += 2;
            } else {
                i += 1;
            }
        } else if let Some(end) = quote {
            if bytes[i] == b'\\' {
                i = (i + 2).min(bytes.len());
            } else {
                if bytes[i] == end {
                    quote = None;
                }
                i += 1;
            }
        } else if bytes[i..].starts_with(b"//") {
            break;
        } else if bytes[i..].starts_with(b"/*") {
            *block = true;
            i += 2;
        } else {
            if matches!(bytes[i], b'\'' | b'"') {
                quote = Some(bytes[i]);
            }
            i += 1;
        }
    }
}

fn strip_leading_inline_comments(mut line: &str) -> Option<&str> {
    loop {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("/*") {
            let close = rest.find("*/")?;
            line = &rest[close + 2..];
        } else {
            return Some(trimmed);
        }
    }
}

fn strip_trailing_directive_comments(raw_arg: &str) -> String {
    let bytes = raw_arg.as_bytes();
    let mut out = String::with_capacity(raw_arg.len());
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        if let Some(q) = quote {
            let ch = raw_arg[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            if ch == '\\' && i < bytes.len() {
                let esc = raw_arg[i..].chars().next().unwrap();
                out.push(esc);
                i += esc.len_utf8();
            } else if ch as u32 == u32::from(q) {
                quote = None;
            }
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            break;
        }
        if bytes[i..].starts_with(b"/*") {
            if let Some(close) = raw_arg[i + 2..].find("*/") {
                i += 2 + close + 2;
                continue;
            }
            break;
        }
        let ch = raw_arg[i..].chars().next().unwrap();
        if matches!(bytes[i], b'"' | b'\'') {
            quote = Some(bytes[i]);
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out.trim().to_owned()
}

pub(super) fn swf_parse_as3_include_arg(trimmed: &str) -> Option<&str> {
    if let Some(rest) = trimmed.strip_prefix("#include")
        && (rest.is_empty()
            || rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\''))
    {
        return Some(rest.trim());
    }
    if let Some(rest) = trimmed.strip_prefix("include")
        && rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\'')
    {
        return Some(rest.trim());
    }
    None
}

pub(super) fn swf_collect_include_map(
    project: &Project,
) -> BTreeMap<String, Result<String, String>> {
    let mut map: BTreeMap<String, Result<String, String>> = BTreeMap::new();
    // Fallback lookup in `project.sources` (`SourceUnit`s), matched case-insensitively on `.as`.
    for src in &project.sources {
        let norm = src
            .path
            .trim()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_owned();
        if strip_as_extension_ci(&norm).len() < norm.len() {
            let entry = if src.source.len() <= 1_048_576 {
                let clean = src
                    .source
                    .strip_prefix('\u{feff}')
                    .map_or_else(|| src.source.clone(), str::to_owned);
                Ok(clean)
            } else {
                Err(format!(
                    "Included source '{}' exceeds 1 MiB limit",
                    src.path
                ))
            };
            map.insert(norm.to_ascii_lowercase(), entry);
        }
    }
    // Primary lookup in `project.resources` (`ResourceUnit`s), overriding `sources` on exact normalized path match.
    for res in &project.resources {
        let norm = res
            .path
            .trim()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_owned();
        let has_as_ext = strip_as_extension_ci(&norm).len() < norm.len();
        if !is_actionscript_resource(res) && !has_as_ext {
            continue;
        }
        let entry = match res.decoded_bytes() {
            Ok(bytes) if bytes.len() <= 1_048_576 => String::from_utf8(bytes)
                .map(|raw| {
                    raw.strip_prefix('\u{feff}')
                        .map_or(raw.clone(), str::to_owned)
                })
                .map_err(|_| format!("#include resource '{}' must contain UTF-8 source", res.path)),
            Ok(_) => Err(format!(
                "Included source '{}' exceeds 1 MiB limit",
                res.path
            )),
            Err(err) => Err(format!("Invalid #include resource '{}': {err}", res.path)),
        };
        map.insert(norm.to_ascii_lowercase(), entry);
    }
    map
}

fn swf_resolve_include_target(
    base_path: &str,
    requested: &str,
) -> Result<(String, String), String> {
    let rel = swf_normalize_include_path(base_path, requested)?;
    let rel_lower = rel.to_ascii_lowercase();
    Ok((rel, rel_lower))
}

fn swf_parse_include_line_requested(
    raw_line: &str,
    is_as3: bool,
) -> Option<Result<String, String>> {
    let clean_start = raw_line.trim_start_matches('\u{feff}');
    let trimmed = strip_leading_inline_comments(clean_start)?.trim_end();
    let raw_arg = if is_as3 {
        swf_parse_as3_include_arg(trimmed)?
    } else {
        let rest = trimmed.strip_prefix("#include")?;
        if !rest.is_empty()
            && !rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        {
            return None;
        }
        rest.trim()
    };
    let no_comment = strip_trailing_directive_comments(raw_arg);
    let argument = if is_as3 {
        no_comment
            .strip_suffix(';')
            .map_or(no_comment.as_str(), str::trim)
    } else {
        no_comment.as_str()
    };
    let requested = argument
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| {
            argument
                .strip_prefix('\'')
                .and_then(|v| v.strip_suffix('\''))
        })
        .filter(|v| !v.contains(['"', '\'', '\\']))
        .map(str::to_owned)
        .ok_or_else(|| {
            if is_as3 {
                "Use include \"path.as\"; on its own line".to_owned()
            } else {
                "Use #include \"path.as\" on its own line without a semicolon".to_owned()
            }
        });
    Some(requested)
}

pub(super) fn expand_swf_includes_recursive(
    source: &str,
    language: ScriptLanguage,
    base_path: &str,
    files: &BTreeMap<String, Result<String, String>>,
    stack: &mut Vec<String>,
    out: &mut String,
) -> Result<(), String> {
    let is_as3 = language == ScriptLanguage::ActionScript3;
    let mut block = false;
    let clean_source = source.strip_prefix('\u{feff}').unwrap_or(source);
    for raw_line in clean_source.lines() {
        let directive_req = if !block {
            swf_parse_include_line_requested(raw_line, is_as3)
        } else {
            None
        };
        if let Some(req_res) = directive_req {
            let requested = req_res?;
            let (included, included_lower) = swf_resolve_include_target(base_path, &requested)?;
            if stack.len() >= 32 || stack.contains(&included_lower) {
                return Err(format!(
                    "Cyclic or over-deep #include: {included} (maximum 32 levels)"
                ));
            }
            let body = if let Some(entry) = files.get(&included_lower) {
                entry.clone()?
            } else {
                return Err(format!(
                    "Missing #include source '{included}'; add the .as file to Project resources or sources"
                ));
            };
            stack.push(included_lower);
            expand_swf_includes_recursive(&body, language, &included, files, stack, out)?;
            stack.pop();
        } else {
            out.push_str(raw_line);
            out.push('\n');
            swf_scan_comments(raw_line, &mut block);
        }
        if out.len() > 1_048_576 {
            return Err("Expanded ActionScript source is limited to 1 MiB".to_owned());
        }
    }
    Ok(())
}

pub(super) fn expand_swf_includes(
    project: &Project,
    language: ScriptLanguage,
    base_path: &str,
    source: &str,
) -> Result<String, String> {
    let clean_source = source.strip_prefix('\u{feff}').unwrap_or(source);
    if !clean_source
        .as_bytes()
        .windows(7)
        .any(|w| w.eq_ignore_ascii_case(b"include"))
    {
        return Ok(clean_source.to_owned());
    }
    let files = swf_collect_include_map(project);
    let mut out = String::with_capacity(clean_source.len());
    let norm_base = base_path
        .trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_owned();
    let mut stack = if norm_base.is_empty() {
        Vec::new()
    } else {
        vec![norm_base.to_ascii_lowercase()]
    };
    expand_swf_includes_recursive(
        clean_source,
        language,
        &norm_base,
        &files,
        &mut stack,
        &mut out,
    )?;
    Ok(out)
}

fn collect_included_paths_from_script(
    source: &str,
    language: ScriptLanguage,
    base_path: &str,
    files: &BTreeMap<String, Result<String, String>>,
    stack: &mut Vec<String>,
    included_out: &mut BTreeSet<String>,
) {
    let clean_source = source.strip_prefix('\u{feff}').unwrap_or(source);
    if !clean_source
        .as_bytes()
        .windows(7)
        .any(|w| w.eq_ignore_ascii_case(b"include"))
    {
        return;
    }
    let is_as3 = language == ScriptLanguage::ActionScript3;
    let mut block = false;
    for raw_line in clean_source.lines() {
        if !block
            && let Some(Ok(requested)) = swf_parse_include_line_requested(raw_line, is_as3)
            && let Ok((included, included_lower)) =
                swf_resolve_include_target(base_path, &requested)
            && stack.len() < 32
            && !stack.contains(&included_lower)
        {
            included_out.insert(included_lower.clone());
            if let Some(Ok(body)) = files.get(&included_lower) {
                stack.push(included_lower);
                collect_included_paths_from_script(
                    body,
                    language,
                    &included,
                    files,
                    stack,
                    included_out,
                );
                stack.pop();
            }
            continue;
        }
        swf_scan_comments(raw_line, &mut block);
    }
}

pub(super) fn swf_collect_included_source_paths(project: &Project) -> BTreeSet<String> {
    let files = swf_collect_include_map(project);
    let mut out = BTreeSet::new();
    for src in &project.sources {
        if matches!(
            src.language,
            ScriptLanguage::ActionScript1
                | ScriptLanguage::ActionScript2
                | ScriptLanguage::ActionScript3
        ) {
            let norm_base = src
                .path
                .trim()
                .replace('\\', "/")
                .trim_start_matches('/')
                .to_owned();
            let mut stack = if norm_base.is_empty() {
                Vec::new()
            } else {
                vec![norm_base.to_ascii_lowercase()]
            };
            collect_included_paths_from_script(
                &src.source,
                src.language,
                &norm_base,
                &files,
                &mut stack,
                &mut out,
            );
        }
    }
    for sym in &project.symbols {
        for fs in &sym.timeline.scripts {
            let mut stack = Vec::new();
            collect_included_paths_from_script(
                &fs.script.source,
                fs.script.language,
                "",
                &files,
                &mut stack,
                &mut out,
            );
        }
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    for os in &el.scripts {
                        let mut stack = Vec::new();
                        collect_included_paths_from_script(
                            &os.script.source,
                            os.script.language,
                            "",
                            &files,
                            &mut stack,
                            &mut out,
                        );
                    }
                }
            }
        }
    }
    out
}

pub(super) fn as3_source_declares_package_or_class(source: &str) -> bool {
    let Ok(tokens) = tokenize_swf_script(source) else {
        return false;
    };
    tokens.first() == Some(&SwfToken::Ident("package".to_owned()))
        || tokens
            .iter()
            .any(|t| matches!(t, SwfToken::Ident(kw) if kw == "class" || kw == "interface"))
}

pub(super) fn swf_script_compilation_issue(
    project: &Project,
    language: ScriptLanguage,
    base_path: &str,
    source: &str,
    external_source: bool,
    is_object_script: bool,
) -> Option<String> {
    match language {
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
            Some(format!("{language:?} is not supported by the SWF compiler"))
        }
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
            let expanded = match expand_swf_includes(project, language, base_path, source) {
                Ok(s) => s,
                Err(err) => return Some(err),
            };
            compile_avm1_actions_mode(&expanded, is_object_script).err()
        }
        ScriptLanguage::ActionScript3 => {
            if is_object_script {
                return Some(
                    "AS3 does not allow inline object scripts on symbol/button instances"
                        .to_owned(),
                );
            }
            let expanded = match expand_swf_includes(project, language, base_path, source) {
                Ok(s) => s,
                Err(err) => return Some(err),
            };
            try_compile_avm2_source(&expanded, external_source).err()
        }
    }
}

pub(super) fn push_swf_layout_issue(
    report: &mut FlaCompatibilityReport,
    location: String,
    flattened: bool,
    issue: SwfLayoutIssue,
) {
    let (code, category, msg) = match issue {
        SwfLayoutIssue::DepthOverflow => (
            if flattened {
                "swf_flattened_scene_depth_limit"
            } else {
                "swf_display_depth_limit"
            },
            if flattened {
                FlaCompatibilityCategory::Scenes
            } else {
                FlaCompatibilityCategory::LayersAndMasks
            },
            format!(
                "Timeline needs more than {} simultaneous SWF display depths.",
                u16::MAX
            ),
        ),
        SwfLayoutIssue::InvalidMaskTarget(layer_name) => (
            "swf_invalid_mask_target",
            FlaCompatibilityCategory::LayersAndMasks,
            format!(
                "Masked layer '{layer_name}' references a missing, non-Mask, or hidden mask layer."
            ),
        ),
        SwfLayoutIssue::NestedMask(layer_name) => (
            "swf_nested_mask",
            FlaCompatibilityCategory::LayersAndMasks,
            format!("Mask layer '{layer_name}' cannot itself be masked in SWF clip layers."),
        ),
        SwfLayoutIssue::AdvancedMaskMode(layer_name) => (
            "swf_advanced_mask_unsupported",
            FlaCompatibilityCategory::LayersAndMasks,
            format!(
                "Mask layer '{layer_name}' uses inverted or non-geometry mask mode, which SWF PlaceObject clip_depth cannot represent."
            ),
        ),
        SwfLayoutIssue::MultiElementMask(layer_name) => (
            "swf_multi_element_mask_unsupported",
            FlaCompatibilityCategory::LayersAndMasks,
            format!(
                "Mask layer '{layer_name}' places multiple simultaneous elements on one frame; combine mask shapes into one shape or symbol for SWF clip_depth."
            ),
        ),
        SwfLayoutIssue::NonContiguousMask(layer_name) => (
            "swf_noncontiguous_mask",
            FlaCompatibilityCategory::LayersAndMasks,
            format!(
                "Mask layer '{layer_name}' and its masked layers are interleaved with unrelated visible layers in z-order and cannot be encoded with SWF clip_depth without changing layer stacking."
            ),
        ),
    };
    report.push(
        code,
        category,
        FlaCompatibilityStatus::Blocking,
        location,
        msg,
    );
}

mod abc;
mod avm1;
mod avm1_events;
mod avm2;
mod avm2_emit;
mod dictionary;
#[cfg(test)]
#[path = "swf/tests.rs"]
mod dictionary_capacity_tests;
mod font_table;
mod parser;
mod preflight;
mod shapes;
mod stream_sound;
mod tags;
#[cfg(test)]
#[path = "swf/tests_t414.rs"]
mod tests_t414;

pub(crate) use avm1::compile_avm1_actions;
/// Preflight the layers that will appear in the published SWF.
pub fn preflight_swf_publish(
    project: &Project,
    options: &SwfPublishOptions,
) -> FlaCompatibilityReport {
    preflight_swf_with_strict(project, options, false)
}

/// Preflight SWF export with explicit `SwfExportOptions` (honoring `options.strict`).
#[must_use]
pub fn preflight_swf_export(
    project: &Project,
    options: &SwfExportOptions,
) -> FlaCompatibilityReport {
    preflight_swf_with_strict(project, &options.publish, options.strict)
}

fn preflight_swf_with_strict(
    project: &Project,
    options: &SwfPublishOptions,
    strict: bool,
) -> FlaCompatibilityReport {
    if project.uses_format_15() {
        let mut published = project.clone();
        published.apply_publish_layer_visibility();
        preflight::preflight_swf_publish_inner(&published, options, strict)
    } else {
        preflight::preflight_swf_publish_inner(project, options, strict)
    }
}
pub(crate) use tags::SwfBitWriter;
use {
    abc::*, avm1::*, avm1_events::*, avm2::*, avm2_emit::*, dictionary::*, font_table::*,
    parser::*, shapes::*, stream_sound::*, tags::*,
};

/// Compile `project` into SWF movie bytes (`FWS`, `CWS`, or `ZWS`) along with a compatibility preflight report.
///
/// Returns `ExportError::CompatibilityBlocked` if `preflight_swf_publish` reports any `Blocking`
/// items (for example, JS/TS scripts or mixed AVM1/AVM2 scripts when `scriptless_mode` is `false`).
pub fn publish_swf_bytes(
    project: &Project,
    options: &SwfPublishOptions,
) -> Result<(Vec<u8>, FlaCompatibilityReport), ExportError> {
    compile_swf_document(project, options, false).map(|(bytes, report, _)| (bytes, report))
}

/// Compile `project` into SWF movie bytes with explicit `SwfExportOptions` (honoring `options.strict`).
pub fn export_swf_bytes(
    project: &Project,
    options: &SwfExportOptions,
) -> Result<(Vec<u8>, FlaCompatibilityReport), ExportError> {
    compile_swf_document(project, &options.publish, options.strict)
        .map(|(bytes, report, _)| (bytes, report))
}

fn compile_swf_document(
    project: &Project,
    options: &SwfPublishOptions,
    strict: bool,
) -> Result<(Vec<u8>, FlaCompatibilityReport, usize), ExportError> {
    if project.symbol(&project.root).is_none() {
        return Err(ExportError::MissingRootSymbol(project.root.clone()));
    }

    let mut report = preflight_swf_with_strict(project, options, strict);
    if let Some(first) = report
        .items
        .iter()
        .find(|item| item.disposition.is_blocking())
    {
        return Err(ExportError::CompatibilityBlocked {
            profile: match options.profile {
                SwfPublishProfile::FlashPlayer8Avm1 => "FlashPlayer8Avm1 (SWF 8)",
                SwfPublishProfile::FlashPlayer9Avm2 => "FlashPlayer9Avm2 (SWF 9)",
                SwfPublishProfile::FlashPlayer10Avm2 => "FlashPlayer10Avm2 (SWF 10)",
            },
            blocking_count: report.blocking_count(),
            first_issue: format!("{} [{}] {}", first.location, first.code, first.message),
        });
    }

    let mut project_cow = crate::strip_skipped_foreign_scripts(project);
    if project_cow.uses_format_15() {
        project_cow.to_mut().apply_publish_layer_visibility();
    }
    if crate::video_frames::has_layers(&project_cow) {
        crate::video_frames::prepare(project_cow.to_mut(), None)?;
    }
    let project = project_cow.as_ref();

    let scene_ids: Vec<&str> = if project.scenes.is_empty() {
        vec![project.root.as_str()]
    } else {
        project.scenes.iter().map(String::as_str).collect()
    };

    let (flattened_proj, scene_ranges) = flatten_scenes_with_ranges(project);
    let root_timeline = flattened_proj
        .symbol(&flattened_proj.root)
        .unwrap_or_else(|| project.symbol(&project.root).unwrap())
        .timeline
        .clone();

    let mut compiler = SwfDictionaryCompiler::new(project, options);
    compiler.ensure_media_resources()?;

    // Compile all non-scene library symbols in bottom-up topological order
    let mut visited = BTreeSet::new();
    for &sid in &scene_ids {
        visited.insert(sid.to_owned());
    }
    for sym in &project.symbols {
        if !scene_ids.contains(&sym.id.as_str()) {
            compiler.compile_symbol_in_order(&sym.id, &mut visited)?;
        }
    }

    let included_source_paths = swf_collect_included_source_paths(project);

    // Emit ExportAssets (tag 56) & DoInitAction (tag 59) for AVM1
    if !options.profile.is_avm2() {
        if !compiler.avm1_exports.is_empty() {
            let payload = build_export_assets_payload(&compiler.avm1_exports);
            compiler.root_tags.write_tag(56, &payload);
        }
        if !options.scriptless_mode {
            for src in &project.sources {
                if matches!(
                    src.language,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) {
                    let norm_src_lower = src
                        .path
                        .trim()
                        .replace('\\', "/")
                        .trim_start_matches('/')
                        .to_ascii_lowercase();
                    if included_source_paths.contains(&norm_src_lower) {
                        continue;
                    }
                    let expanded =
                        expand_swf_includes(project, src.language, &src.path, &src.source)
                            .map_err(|err| {
                                ExportError::Options(format!(
                                    "AVM1 source '{}' include failed: {err}",
                                    src.path
                                ))
                            })?;
                    let actions = compile_avm1_actions(&expanded).map_err(|err| {
                        ExportError::Options(format!(
                            "AVM1 source '{}' failed to compile: {err}",
                            src.path
                        ))
                    })?;
                    if actions.len() > 1 {
                        // Emit a dedicated empty DefineSprite so each external AVM1 source has
                        // its own valid init target sprite ID.
                        let init_sprite_id = compiler.alloc_char_id();
                        let mut sw = SwfBitWriter::new();
                        sw.u16(init_sprite_id);
                        sw.u16(1);
                        sw.write_tag(1, &[]); // ShowFrame
                        sw.write_tag(0, &[]); // End
                        compiler.root_tags.write_tag(39, &sw.buf);

                        let mut w = SwfBitWriter::new();
                        w.u16(init_sprite_id);
                        w.bytes(&actions);
                        compiler.root_tags.write_tag(59, &w.buf);
                    }
                }
            }
        }
    }

    // Emit DefineSceneAndFrameLabelData (tag 86) for multi-scene movies
    if scene_ranges.len() > 1 {
        let scenes_meta: Vec<(u32, String)> = scene_ranges
            .iter()
            .map(|r| (r.start, r.name.clone()))
            .collect();
        let payload = build_define_scene_and_frame_label_data_payload(&scenes_meta);
        compiler.root_tags.write_tag(86, &payload);
    }

    // Prepare AVM2 DoABC2 (tag 82) + SymbolClass (tag 76) before the first ShowFrame so frame 0 scripts execute!
    if options.profile.is_avm2() && !options.scriptless_mode {
        let root_as3_scripts = collect_expanded_as3_frame_scripts(project, &root_timeline)?;

        let mut defined_qnames: BTreeSet<String> = BTreeSet::new();
        for src in &project.sources {
            if src.language == ScriptLanguage::ActionScript3 {
                let norm_src_lower = src
                    .path
                    .trim()
                    .replace('\\', "/")
                    .trim_start_matches('/')
                    .to_ascii_lowercase();
                let expanded = expand_swf_includes(project, src.language, &src.path, &src.source)
                    .map_err(|err| {
                    ExportError::Options(format!("AS3 source '{}' include failed: {err}", src.path))
                })?;
                if included_source_paths.contains(&norm_src_lower)
                    && !as3_source_declares_package_or_class(&expanded)
                {
                    continue;
                }
                let fallback = strip_as_extension_ci(src.path.rsplit('/').next().unwrap_or("Main"));
                let spec =
                    parse_as3_source_to_spec_strict(&expanded, fallback, true).map_err(|err| {
                        ExportError::Options(format!(
                            "AS3 source '{}' failed to parse: {err}",
                            src.path
                        ))
                    })?;
                let qname = spec_qname(&spec);
                if defined_qnames.insert(qname.clone()) {
                    compiler.avm2_classes.push(spec);
                } else {
                    return Err(ExportError::Options(format!(
                        "Duplicate AS3 class definition '{qname}' in project sources"
                    )));
                }
            }
        }

        // Bind root timeline & document class without creating an orphan MainTimeline.
        let clean_doc_class = project
            .document_class
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(doc_cls) = clean_doc_class {
            if let Some(existing) = compiler
                .avm2_classes
                .iter_mut()
                .find(|c| spec_qname(c) == doc_cls)
            {
                if !root_as3_scripts.is_empty() {
                    existing.frame_scripts = root_as3_scripts;
                }
            } else {
                defined_qnames.insert(doc_cls.to_owned());
                let (pkg, cls_name) = split_qualified_class(doc_cls);
                compiler.avm2_classes.push(AbcClassSpec {
                    pkg,
                    class_name: cls_name,
                    super_pkg: "flash.display".to_owned(),
                    super_name: "MovieClip".to_owned(),
                    frame_scripts: root_as3_scripts,
                    ..Default::default()
                });
            }
            compiler.avm2_symbol_classes.push((0, doc_cls.to_owned()));
        } else if !root_as3_scripts.is_empty() {
            let mut main_qname = "MainTimeline".to_owned();
            if defined_qnames.contains(&main_qname) {
                let mut suffix = 1u32;
                while defined_qnames.contains(&format!("_NeoFlashMainTimeline_{suffix}")) {
                    suffix += 1;
                }
                main_qname = format!("_NeoFlashMainTimeline_{suffix}");
            }
            defined_qnames.insert(main_qname.clone());
            compiler.avm2_classes.push(AbcClassSpec {
                pkg: String::new(),
                class_name: main_qname.clone(),
                super_pkg: "flash.display".to_owned(),
                super_name: "MovieClip".to_owned(),
                frame_scripts: root_as3_scripts,
                ..Default::default()
            });
            compiler.avm2_symbol_classes.push((0, main_qname));
        }

        // Attach frame scripts for un-named symbols using non-colliding generated classes.
        let anon_scripts = std::mem::take(&mut compiler.avm2_anon_symbol_scripts);
        for (char_id, as3_scripts) in anon_scripts {
            let mut tl_class_name = format!("Timeline_{char_id}");
            if defined_qnames.contains(&tl_class_name) {
                let mut suffix = 1u32;
                while defined_qnames.contains(&format!("Timeline_{char_id}_{suffix}")) {
                    suffix += 1;
                }
                tl_class_name = format!("Timeline_{char_id}_{suffix}");
            }
            defined_qnames.insert(tl_class_name.clone());
            compiler.avm2_classes.push(AbcClassSpec {
                pkg: String::new(),
                class_name: tl_class_name.clone(),
                super_pkg: "flash.display".to_owned(),
                super_name: "MovieClip".to_owned(),
                frame_scripts: as3_scripts,
                ..Default::default()
            });
            compiler.avm2_symbol_classes.push((char_id, tl_class_name));
        }

        // Attach frame scripts onto existing external classes or generate stubs for bound symbols.
        let mut sym_frame_scripts = std::mem::take(&mut compiler.avm2_symbol_frame_scripts);
        for spec in &mut compiler.avm2_classes {
            let qname = spec_qname(spec);
            if let Some(scripts) = sym_frame_scripts.remove(&qname) {
                spec.frame_scripts = scripts;
            }
        }

        for (_, cls_name) in &compiler.avm2_symbol_classes {
            if defined_qnames.insert(cls_name.clone()) {
                let (pkg, short) = split_qualified_class(cls_name);
                let super_name =
                    if compiler.avm2_symbol_kinds.get(cls_name) == Some(&SymbolKind::Button) {
                        "SimpleButton"
                    } else {
                        "MovieClip"
                    };
                let frame_scripts = sym_frame_scripts.remove(cls_name).unwrap_or_default();
                compiler.avm2_classes.push(AbcClassSpec {
                    pkg,
                    class_name: short,
                    super_pkg: "flash.display".to_owned(),
                    super_name: super_name.to_owned(),
                    frame_scripts,
                    ..Default::default()
                });
            }
        }

        if !compiler.avm2_classes.is_empty() {
            let abc_bytes = build_abc_bytes(&compiler.avm2_classes)?;
            let do_abc_payload = build_do_abc2_payload("frame1", &abc_bytes);
            compiler.root_tags.write_tag(82, &do_abc_payload);
        }
        if !compiler.avm2_symbol_classes.is_empty() {
            let sym_cls_payload = build_symbol_class_payload(&compiler.avm2_symbol_classes);
            compiler.root_tags.write_tag(76, &sym_cls_payload);
        }
    }

    // Build root timeline display list + ShowFrame tags
    let (_, root_frame_count) = compiler.build_timeline_tags(&root_timeline, true)?;
    if compiler.dictionary_overflow {
        return Err(ExportError::Options(format!(
            "SWF dictionary exceeds the {} available character IDs; simplify or split the movie",
            u16::MAX
        )));
    }
    compiler.root_tags.write_tag(0, &[]); // End tag (0)

    // Assemble uncompressed SWF body (RECT stage bounds + frame_rate + frame_count + FileAttributes + SetBackgroundColor + root_tags)
    let mut body = SwfBitWriter::new();
    let w_twips = (project.width.max(1) as i32).saturating_mul(20);
    let h_twips = (project.height.max(1) as i32).saturating_mul(20);
    body.rect_twips([0, w_twips, 0, h_twips]);
    let fps_fixed8 = (project.fps.clamp(1, 255) as u16) << 8;
    body.u16(fps_fixed8);
    body.u16(root_frame_count.max(1));

    // FileAttributes (tag 69): UseABC (0x08) for AVM2
    let file_attr_flags: u32 = if options.profile.is_avm2() {
        0x08
    } else {
        0x00
    };
    body.write_tag(69, &file_attr_flags.to_le_bytes());

    // SetBackgroundColor (tag 9)
    body.write_tag(
        9,
        &[
            project.background[0],
            project.background[1],
            project.background[2],
        ],
    );

    body.bytes(&compiler.root_tags.buf);

    let swf_bytes = wrap_swf_container(&body.buf, options.profile, options.compression)?;
    for (family, face) in compiler.outline_fonts.substitutions {
        report.push(
            "swf_outline_font_substituted",
            FlaCompatibilityCategory::TextAndFonts,
            FlaCompatibilityStatus::Approximated,
            format!("font:{family}"),
            format!(
                "Outlined text using installed face '{face}' for glyphs missing from '{family}'."
            ),
        );
    }
    let decoded_bitmap_bytes = compiler
        .bitmap_meta
        .values()
        .fold(0usize, |bytes, (w, h, _)| {
            bytes.saturating_add((*w as usize).saturating_mul(*h as usize).saturating_mul(4))
        });
    Ok((swf_bytes, report, decoded_bitmap_bytes))
}

/// Publish a SWF after recovering and validating its compiled timeline, shapes and resources.
pub fn publish_swf(
    project: &Project,
    dest: impl AsRef<Path>,
    options: &SwfPublishOptions,
) -> Result<FlaCompatibilityReport, ExportError> {
    export_swf(
        project,
        dest,
        &SwfExportOptions {
            publish: options.clone(),
            strict: false,
        },
    )
}

/// Export a SWF with explicit `SwfExportOptions` (honoring `options.strict`) after round-trip validation.
pub fn export_swf(
    project: &Project,
    dest: impl AsRef<Path>,
    options: &SwfExportOptions,
) -> Result<FlaCompatibilityReport, ExportError> {
    let dest = dest.as_ref();
    let (bytes, mut report, bitmap_bytes) =
        compile_swf_document(project, &options.publish, options.strict)?;
    let editable_bytes = neoflash_import::verify_swf_publication(&bytes, bitmap_bytes)
        .map_err(|err| ExportError::RoundTripVerification(err.to_string()))?;
    if editable_bytes > neoflash_core::MAX_PROJECT_JSON_BYTES {
        report.push("swf_editable_size_limit", FlaCompatibilityCategory::DocumentSettings,
            FlaCompatibilityStatus::Approximated, "document",
            format!("The validated SWF movie can be published. Reopening the compiled vectors as editable JSON requires {editable_bytes} bytes, above the 128 MiB .nflash file limit. Keep the original editable project."));
    }
    crate::atomic_write(dest, &bytes)?;
    Ok(report)
}

/// Alias for `publish_swf`.
pub fn publish_swf_to_path(
    project: &Project,
    dest: impl AsRef<Path>,
    options: &SwfPublishOptions,
) -> Result<FlaCompatibilityReport, ExportError> {
    publish_swf(project, dest, options)
}

/// Re-import compiled `.swf` bytes via `neoflash_import` for round-trip verification.
pub fn import_swf_bytes(bytes: &[u8]) -> Result<Project, ExportError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir().join(format!(
        "neoflash-swf-verify-{}-{seq}-{}",
        std::process::id(),
        bytes.len()
    ));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    let _temp_guard = crate::TemporaryPathGuard::new(tmp_dir.clone());
    std::fs::create_dir_all(&tmp_dir)?;
    let tmp_file = tmp_dir.join("verify.swf");
    std::fs::write(&tmp_file, bytes)?;
    let res = neoflash_import::import_project(&tmp_file);
    res.map_err(|e| ExportError::RoundTripVerification(e.to_string()))
}
