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
};
use flate2::Compression;
use flate2::write::ZlibEncoder;
use neoflash_core::{
    BlendMode, Color, ColorTransform, Easing, Element, ElementKind, FilterSpec, FrameSound,
    GradientKind, GradientPaint, LayerRole, ObjectEvent, ObjectScript, PathCommand, Project,
    ScriptLanguage, SoundEffect, SoundSync, Symbol, SymbolKind, TextAlign, Timeline, TweenKind,
    flatten_scenes_with_ranges, transform_matrix,
};
pub use neoflash_import::{
    SwfAudioEvent, SwfBytecodePlayer, SwfDisplayEntry, SwfFrameState, SwfInspection,
    SwfPlaybackResult, SwfTagCounts, SwfVmKind, play_swf_bytes, play_swf_path,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum StrokeCap {
    Round,
    Butt,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum StrokeJoin {
    Round,
    Bevel,
    Miter,
}

fn outline_text_element_to_paths(element: &Element) -> Vec<Element> {
    let mut outlined_element = element.clone();
    // A named TextBox is runtime-addressable in the authoring model, but outline_text is an
    // explicit request for static visual conversion. Clear only the temporary authoring name so
    // the shared outline converter does not intentionally preserve it as an editable field.
    outlined_element.name.clear();
    let mut temp_proj = Project {
        format_version: 6,
        name: "outline_tmp".to_owned(),
        width: 550,
        height: 400,
        fps: 24,
        background: [255, 255, 255, 255],
        root: "root".to_owned(),
        symbols: vec![Symbol {
            id: "root".to_owned(),
            name: "Scene 1".to_owned(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![neoflash_core::Layer {
                    id: "l1".to_owned(),
                    name: "Layer 1".to_owned(),
                    visible: true,
                    locked: false,
                    properties: Default::default(),
                    keyframes: vec![neoflash_core::Keyframe {
                        frame: 0,
                        elements: vec![outlined_element],
                        ..Default::default()
                    }],
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        }],
        library_folders: Vec::new(),
        sources: Vec::new(),
        resources: Vec::new(),
        document_class: None,
        scenes: Vec::new(),
    };
    let _ = crate::fonts::convert_project_text_to_outlines(&mut temp_proj);
    temp_proj
        .symbols
        .into_iter()
        .next()
        .and_then(|mut s| s.timeline.layers.pop())
        .and_then(|mut l| l.keyframes.pop())
        .map(|k| k.elements)
        .unwrap_or_default()
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

fn timeline_depth_bands(timeline: &Timeline) -> Option<Vec<(u16, u16)>> {
    let mut next_depth = 1usize;
    let mut bands = Vec::with_capacity(timeline.layers.len());
    for layer in &timeline.layers {
        if !layer.visible || matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide) {
            bands.push((0, 0));
            continue;
        }
        let mut width = layer
            .keyframes
            .iter()
            .map(|keyframe| keyframe.elements.len())
            .max()
            .unwrap_or(0)
            .max(1);
        for (keyframe_index, keyframe) in layer.keyframes.iter().enumerate() {
            if keyframe.tween.is_some()
                && keyframe.tween_kind == TweenKind::Shape
                && let Some(next_keyframe) = layer.keyframes.get(keyframe_index + 1)
                && !can_build_morph_shape(&keyframe.elements, &next_keyframe.elements)
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
        let end_depth = next_depth.checked_add(width)?.checked_sub(1)?;
        if end_depth > usize::from(u16::MAX) {
            return None;
        }
        bands.push((next_depth as u16, end_depth as u16));
        next_depth = end_depth + 1;
    }
    Some(bands)
}

fn used_swf_media_resources(project: &Project) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut bitmaps = BTreeSet::new();
    let mut sounds = BTreeSet::new();
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
        if let Some(sound) = &keyframe.sound {
            sounds.insert(sound.resource.clone());
        }
    }
    (bitmaps, sounds)
}

fn swf_text_outline_issue(element: &Element) -> Option<String> {
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

fn swf_script_compilation_issue(
    language: ScriptLanguage,
    source: &str,
    external_source: bool,
) -> Option<String> {
    match language {
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
            Some(format!("{language:?} is not supported by the SWF compiler"))
        }
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
            compile_avm1_actions(source).err()
        }
        ScriptLanguage::ActionScript3 => try_compile_avm2_source(source, external_source).err(),
    }
}

/// Run a comprehensive compatibility preflight for publishing `project` as a `.swf` movie.
#[must_use]
pub fn preflight_swf_publish(
    project: &Project,
    options: &SwfPublishOptions,
) -> FlaCompatibilityReport {
    let equivalent_fla_profile = match options.profile {
        SwfPublishProfile::FlashPlayer8Avm1 => FlaTargetProfile::LegacyFlash8,
        SwfPublishProfile::FlashPlayer9Avm2 => FlaTargetProfile::LegacyFlashCS4,
        SwfPublishProfile::FlashPlayer10Avm2 => FlaTargetProfile::FlashCS6,
    };
    let mut report = FlaCompatibilityReport::new(equivalent_fla_profile);

    // 1. Document settings & scenes
    report.push(
        "swf_document_header",
        FlaCompatibilityCategory::DocumentSettings,
        FlaCompatibilityStatus::Exact,
        format!("project:{}", project.name),
        format!(
            "SWF header {}x{} px ({}x{} twips), profile {:?} ({:?}).",
            project.width,
            project.height,
            project.width.saturating_mul(20),
            project.height.saturating_mul(20),
            options.profile,
            options.compression
        ),
    );
    if !(1..=255).contains(&project.fps) {
        report.push(
            "swf_frame_rate_limit",
            FlaCompatibilityCategory::DocumentSettings,
            FlaCompatibilityStatus::Blocking,
            format!("project:{}", project.name),
            format!(
                "Project frame rate is {} fps; this SWF writer can represent integer frame rates from 1 through 255 without changing playback speed.",
                project.fps
            ),
        );
    } else {
        report.push(
            "swf_frame_rate_exact",
            FlaCompatibilityCategory::DocumentSettings,
            FlaCompatibilityStatus::Exact,
            "Project.fps",
            format!("Frame rate {} fps is represented exactly.", project.fps),
        );
    }

    if project.scenes.len() > 1 {
        report.push(
            "swf_scenes_flattened",
            FlaCompatibilityCategory::Scenes,
            FlaCompatibilityStatus::Converted,
            "project.scenes",
            format!(
                "{} scenes are flattened into the root SWF timeline and indexed via DefineSceneAndFrameLabelData.",
                project.scenes.len()
            ),
        );
    }

    let (flattened_project, _) = flatten_scenes_with_ranges(project);
    if let Some(root) = flattened_project.symbol(&flattened_project.root) {
        if root.timeline.frame_count > u32::from(u16::MAX) {
            report.push(
                "swf_flattened_scene_frame_limit",
                FlaCompatibilityCategory::Scenes,
                FlaCompatibilityStatus::Blocking,
                "project.scenes",
                format!(
                    "Flattened scenes need {} frames; SWF declares at most {} frames.",
                    root.timeline.frame_count,
                    u16::MAX
                ),
            );
        }
        if timeline_depth_bands(&root.timeline).is_none() {
            report.push(
                "swf_flattened_scene_depth_limit",
                FlaCompatibilityCategory::Scenes,
                FlaCompatibilityStatus::Blocking,
                "project.scenes",
                format!(
                    "Flattened scenes need more than {} simultaneous SWF display depths.",
                    u16::MAX
                ),
            );
        }
    }

    let (used_bitmaps, used_sounds) = used_swf_media_resources(project);
    for resource_id in used_bitmaps {
        let location = format!("resource:{resource_id}");
        let Some(resource) = project.resource(&resource_id) else {
            report.push(
                "swf_bitmap_resource_missing",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!("Placed bitmap references missing resource '{resource_id}'."),
            );
            continue;
        };
        let lossless_override = resource
            .bitmap_export
            .is_some_and(|s| s.compression == neoflash_core::BitmapCompression::LosslessPng);
        if !resource.media_type.eq_ignore_ascii_case("image/png") && !lossless_override {
            report.push(
                "swf_bitmap_codec_unsupported",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!(
                    "Placed bitmap '{}' uses {}; this SWF writer currently embeds only PNG resources. Convert it to PNG or set its Library Bitmap Properties to Lossless PNG before publishing.",
                    resource.path, resource.media_type
                ),
            );
            continue;
        }
        let Ok(raw) = resource.decoded_bytes() else {
            report.push(
                "swf_bitmap_data_invalid",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!("Placed PNG '{}' has invalid resource data.", resource.path),
            );
            continue;
        };
        if build_define_bits_lossless2_payload(1, &raw).is_none() {
            report.push(
                "swf_bitmap_data_unsupported",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!(
                    "Placed PNG '{}' could not be decoded as a non-empty bitmap of at most 8192x8192 pixels.",
                    resource.path
                ),
            );
        } else {
            report.push(
                "swf_bitmap_png",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Exact,
                location,
                format!(
                    "Placed PNG '{}' is embedded as DefineBitsLossless2.",
                    resource.path
                ),
            );
        }
    }
    for resource_id in used_sounds {
        let location = format!("resource:{resource_id}");
        let Some(resource) = project.resource(&resource_id) else {
            report.push(
                "swf_sound_resource_missing",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!("Frame sound references missing resource '{resource_id}'."),
            );
            continue;
        };
        let is_supported_codec = resource.media_type.eq_ignore_ascii_case("audio/wav")
            || resource.media_type.eq_ignore_ascii_case("audio/x-wav")
            || resource.media_type.eq_ignore_ascii_case("audio/wave")
            || resource.media_type.eq_ignore_ascii_case("audio/mpeg")
            || resource.media_type.eq_ignore_ascii_case("audio/mp3");
        if !is_supported_codec {
            report.push(
                "swf_sound_codec_unsupported",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!(
                    "Frame sound '{}' uses {}; this SWF writer currently embeds only proven PCM WAV and MP3 layouts. Convert it to a supported WAV or MP3 first.",
                    resource.path, resource.media_type
                ),
            );
            continue;
        }
        let Ok(raw) = resource.decoded_bytes() else {
            report.push(
                "swf_sound_data_invalid",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!("Frame sound '{}' has invalid resource data.", resource.path),
            );
            continue;
        };
        match analyze_swf_sound_resource(&resource.media_type, &raw) {
            Ok(info) => report.push(
                "swf_sound_embedded",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Exact,
                location,
                format!(
                    "Frame sound '{}' is embedded with {} samples at {} Hz ({}).",
                    resource.path,
                    info.sample_count,
                    info.sample_rate,
                    if info.stereo { "stereo" } else { "mono" }
                ),
            ),
            Err(issue) => report.push(
                "swf_sound_layout_unsupported",
                FlaCompatibilityCategory::MediaAndSound,
                FlaCompatibilityStatus::Blocking,
                location,
                format!(
                    "Frame sound '{}' cannot be embedded without changing or corrupting it: {issue}",
                    resource.path
                ),
            ),
        }
    }

    // Track script languages for mixed-VM detection
    let mut has_avm1_scripts = false;
    let mut has_avm2_scripts = false;

    if let Some(doc_cls) = &project.document_class
        && !doc_cls.trim().is_empty()
    {
        has_avm2_scripts = true;
        if options.profile.is_avm2() {
            report.push(
                "swf_document_class",
                FlaCompatibilityCategory::Scripts,
                FlaCompatibilityStatus::Exact,
                "project.document_class",
                format!("Document class '{doc_cls}' bound to root via SymbolClass (ID 0)."),
            );
        } else {
            report.push(
                "swf_document_class_avm1_unsupported",
                FlaCompatibilityCategory::Scripts,
                if options.scriptless_mode {
                    FlaCompatibilityStatus::Omitted
                } else {
                    FlaCompatibilityStatus::Blocking
                },
                "project.document_class",
                format!(
                    "AS3 document class '{doc_cls}' cannot be bound in FlashPlayer8Avm1 SWF profile."
                ),
            );
        }
    }

    // 2. Inspect external sources
    for (idx, src) in project.sources.iter().enumerate() {
        let loc = format!("sources[{idx}]:{}", src.path);
        match src.language {
            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                report.push(
                    "swf_js_ts_source_unsupported",
                    FlaCompatibilityCategory::Scripts,
                    if options.scriptless_mode {
                        FlaCompatibilityStatus::Omitted
                    } else {
                        FlaCompatibilityStatus::Blocking
                    },
                    loc,
                    format!(
                        "{:?} source '{}' cannot be compiled into SWF AVM bytecode.",
                        src.language, src.path
                    ),
                );
            }
            ScriptLanguage::ActionScript3 => {
                has_avm2_scripts = true;
                let compile_issue = swf_script_compilation_issue(src.language, &src.source, true);
                report.push(
                    if options.profile.is_avm2() && compile_issue.is_none() {
                        "swf_as3_source_doabc"
                    } else if options.profile.is_avm2() {
                        "swf_as3_source_unsupported_syntax"
                    } else {
                        "swf_as3_source_in_avm1"
                    },
                    FlaCompatibilityCategory::Scripts,
                    if options.scriptless_mode {
                        FlaCompatibilityStatus::Omitted
                    } else if options.profile.is_avm2() && compile_issue.is_none() {
                        FlaCompatibilityStatus::Exact
                    } else {
                        FlaCompatibilityStatus::Blocking
                    },
                    loc,
                    if options.profile.is_avm2() && compile_issue.is_none() {
                        format!("AS3 source '{}' compiled into DoABC2 tag.", src.path)
                    } else if let Some(issue) = compile_issue {
                        format!(
                            "AS3 source '{}' cannot be compiled losslessly: {issue}.",
                            src.path
                        )
                    } else {
                        format!(
                            "AS3 source '{}' requires FlashPlayer9Avm2 or FlashPlayer10Avm2.",
                            src.path
                        )
                    },
                );
            }
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                has_avm1_scripts = true;
                let compile_issue = swf_script_compilation_issue(src.language, &src.source, true);
                report.push(
                    if !options.profile.is_avm2() && compile_issue.is_none() {
                        "swf_avm1_source"
                    } else if !options.profile.is_avm2() {
                        "swf_avm1_source_unsupported_syntax"
                    } else {
                        "swf_avm1_source_in_avm2"
                    },
                    FlaCompatibilityCategory::Scripts,
                    if options.scriptless_mode {
                        FlaCompatibilityStatus::Omitted
                    } else if !options.profile.is_avm2() && compile_issue.is_none() {
                        FlaCompatibilityStatus::Exact
                    } else {
                        FlaCompatibilityStatus::Blocking
                    },
                    loc,
                    if !options.profile.is_avm2() && compile_issue.is_none() {
                        format!("AVM1 source '{}' compiled into DoInitAction.", src.path)
                    } else if let Some(issue) = compile_issue {
                        format!(
                            "AS1/AS2 source '{}' cannot be compiled losslessly: {issue}.",
                            src.path
                        )
                    } else {
                        format!(
                            "AS1/AS2 source '{}' cannot run in an AVM2 SWF movie.",
                            src.path
                        )
                    },
                );
            }
        }
    }

    // 3. Inspect symbols, timelines, elements, tweens, and scripts
    for sym in &project.symbols {
        let sym_loc = format!("symbol:{}", sym.name);
        if sym.timeline.frame_count > u32::from(u16::MAX) {
            report.push(
                "swf_timeline_frame_limit",
                FlaCompatibilityCategory::AnimationAndRigging,
                FlaCompatibilityStatus::Blocking,
                sym_loc.clone(),
                format!(
                    "Timeline has {} frames; SWF declares at most {} frames without truncation.",
                    sym.timeline.frame_count,
                    u16::MAX
                ),
            );
        }
        if timeline_depth_bands(&sym.timeline).is_none() {
            report.push(
                "swf_display_depth_limit",
                FlaCompatibilityCategory::LayersAndMasks,
                FlaCompatibilityStatus::Blocking,
                sym_loc.clone(),
                format!(
                    "Timeline needs more than {} simultaneous SWF display depths.",
                    u16::MAX
                ),
            );
        }
        if let Some(cls) = &sym.class_name
            && !cls.is_empty()
        {
            has_avm2_scripts = true;
            if !options.profile.is_avm2() {
                report.push(
                    "swf_symbol_class_in_avm1",
                    FlaCompatibilityCategory::SymbolsAndLibrary,
                    if options.scriptless_mode {
                        FlaCompatibilityStatus::Omitted
                    } else {
                        FlaCompatibilityStatus::Blocking
                    },
                    sym_loc.clone(),
                    format!("Symbol AS3 class '{cls}' requires an AVM2 SWF profile."),
                );
            }
        }

        for fs in &sym.timeline.scripts {
            let fs_loc = format!("{sym_loc}:frame[{}].script", fs.frame);
            match fs.script.language {
                ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                    report.push(
                        "swf_js_ts_frame_script",
                        FlaCompatibilityCategory::Scripts,
                        if options.scriptless_mode {
                            FlaCompatibilityStatus::Omitted
                        } else {
                            FlaCompatibilityStatus::Blocking
                        },
                        fs_loc,
                        format!(
                            "{:?} frame script cannot be compiled to SWF bytecode.",
                            fs.script.language
                        ),
                    );
                }
                ScriptLanguage::ActionScript3 => {
                    has_avm2_scripts = true;
                    let compile_issue =
                        swf_script_compilation_issue(fs.script.language, &fs.script.source, false);
                    report.push(
                        if options.profile.is_avm2() && compile_issue.is_none() {
                            "swf_avm2_frame_script"
                        } else if options.profile.is_avm2() {
                            "swf_avm2_frame_script_unsupported_syntax"
                        } else {
                            "swf_avm2_frame_script_in_avm1"
                        },
                        FlaCompatibilityCategory::Scripts,
                        if options.scriptless_mode {
                            FlaCompatibilityStatus::Omitted
                        } else if options.profile.is_avm2() && compile_issue.is_none() {
                            FlaCompatibilityStatus::Exact
                        } else {
                            FlaCompatibilityStatus::Blocking
                        },
                        fs_loc,
                        compile_issue.map_or_else(
                            || {
                                "AS3 frame script compiled into AVM2 addFrameScript method."
                                    .to_owned()
                            },
                            |issue| {
                                format!("AS3 frame script cannot be compiled losslessly: {issue}.")
                            },
                        ),
                    );
                }
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                    has_avm1_scripts = true;
                    let compile_issue =
                        swf_script_compilation_issue(fs.script.language, &fs.script.source, false);
                    report.push(
                        if !options.profile.is_avm2() && compile_issue.is_none() {
                            "swf_avm1_frame_script"
                        } else if !options.profile.is_avm2() {
                            "swf_avm1_frame_script_unsupported_syntax"
                        } else {
                            "swf_avm1_frame_script_in_avm2"
                        },
                        FlaCompatibilityCategory::Scripts,
                        if options.scriptless_mode {
                            FlaCompatibilityStatus::Omitted
                        } else if !options.profile.is_avm2() && compile_issue.is_none() {
                            FlaCompatibilityStatus::Exact
                        } else {
                            FlaCompatibilityStatus::Blocking
                        },
                        fs_loc,
                        compile_issue.map_or_else(
                            || "AS1/AS2 frame script compiled into AVM1 DoAction tag.".to_owned(),
                            |issue| {
                                format!(
                                    "AS1/AS2 frame script cannot be compiled losslessly: {issue}."
                                )
                            },
                        ),
                    );
                }
            }
        }

        for layer in &sym.timeline.layers {
            let layer_loc = format!("{sym_loc}:layer:{}", layer.name);
            for (k_idx, kf) in layer.keyframes.iter().enumerate() {
                let kf_loc = format!("{layer_loc}:frame[{}]", kf.frame);
                if let Some(easing) = &kf.tween {
                    match kf.tween_kind {
                        TweenKind::Motion => {
                            report.push(
                                "swf_motion_tween_baked",
                                FlaCompatibilityCategory::AnimationAndRigging,
                                if options.bake_unsupported_tweens {
                                    FlaCompatibilityStatus::Baked
                                } else {
                                    FlaCompatibilityStatus::Approximated
                                },
                                kf_loc.clone(),
                                "Motion tween span evaluated into frame-by-frame SWF display-list placements."
                                    .to_owned(),
                            );
                        }
                        TweenKind::Shape => {
                            let can_morph = layer.keyframes.get(k_idx + 1).is_some_and(|next| {
                                can_build_morph_shape(&kf.elements, &next.elements)
                            });
                            report.push(
                                if can_morph {
                                    "swf_define_morph_shape"
                                } else {
                                    "swf_shape_tween_baked"
                                },
                                FlaCompatibilityCategory::AnimationAndRigging,
                                if can_morph {
                                    FlaCompatibilityStatus::Exact
                                } else if options.bake_unsupported_tweens {
                                    FlaCompatibilityStatus::Baked
                                } else {
                                    FlaCompatibilityStatus::Approximated
                                },
                                kf_loc.clone(),
                                if can_morph {
                                    "Shape tween compiled into DefineMorphShape (tag 46) with per-frame ratio."
                                        .to_owned()
                                } else {
                                    "Multi-shape morph span evaluated into frame-by-frame DefineShape tags."
                                        .to_owned()
                                },
                            );
                        }
                        TweenKind::Classic => {
                            let status = if matches!(easing, Easing::CubicBezier { .. })
                                && !options.bake_unsupported_tweens
                            {
                                FlaCompatibilityStatus::Approximated
                            } else {
                                FlaCompatibilityStatus::Exact
                            };
                            report.push(
                                "swf_classic_tween",
                                FlaCompatibilityCategory::AnimationAndRigging,
                                status,
                                kf_loc.clone(),
                                "Classic tween compiled into per-frame PlaceObject2 matrix/cxform updates."
                                    .to_owned(),
                            );
                        }
                    }
                }

                for el in &kf.elements {
                    let el_loc = format!("{kf_loc}:element:{}", el.id);
                    for os in &el.scripts {
                        match os.script.language {
                            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
                                report.push(
                                    "swf_js_ts_object_script",
                                    FlaCompatibilityCategory::Scripts,
                                    if options.scriptless_mode {
                                        FlaCompatibilityStatus::Omitted
                                    } else {
                                        FlaCompatibilityStatus::Blocking
                                    },
                                    format!("{el_loc}:script"),
                                    "JS/TS object script cannot be compiled into SWF.".to_owned(),
                                );
                            }
                            ScriptLanguage::ActionScript3 => {
                                has_avm2_scripts = true;
                                report.push(
                                    "swf_as3_object_script_unsupported",
                                    FlaCompatibilityCategory::Scripts,
                                    if options.scriptless_mode {
                                        FlaCompatibilityStatus::Omitted
                                    } else {
                                        FlaCompatibilityStatus::Blocking
                                    },
                                    format!("{el_loc}:script"),
                                    "AS3 does not allow inline object scripts on symbol/button instances."
                                        .to_owned(),
                                );
                            }
                            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                                has_avm1_scripts = true;
                                let compile_issue = swf_script_compilation_issue(
                                    os.script.language,
                                    &os.script.source,
                                    false,
                                );
                                report.push(
                                    if !options.profile.is_avm2() && compile_issue.is_none() {
                                        "swf_avm1_object_script"
                                    } else if !options.profile.is_avm2() {
                                        "swf_avm1_object_script_unsupported_syntax"
                                    } else {
                                        "swf_avm1_object_script_in_avm2"
                                    },
                                    FlaCompatibilityCategory::Scripts,
                                    if options.scriptless_mode {
                                        FlaCompatibilityStatus::Omitted
                                    } else if !options.profile.is_avm2()
                                        && compile_issue.is_none()
                                    {
                                        FlaCompatibilityStatus::Exact
                                    } else {
                                        FlaCompatibilityStatus::Blocking
                                    },
                                    format!("{el_loc}:script"),
                                    compile_issue.map_or_else(
                                        || {
                                            "Object script compiled into AVM1 CLIPACTIONS / BUTTONCONDACTION."
                                                .to_owned()
                                        },
                                        |issue| {
                                            format!(
                                                "AS1/AS2 object script cannot be compiled losslessly: {issue}."
                                            )
                                        },
                                    ),
                                );
                            }
                        }
                    }
                    if !options.profile.is_avm2()
                        && !options.scriptless_mode
                        && project.symbols.iter().any(|symbol| {
                            matches!(
                                &el.kind,
                                ElementKind::Instance { symbol_id, .. }
                                    if symbol.id == *symbol_id && symbol.kind == SymbolKind::Button
                            )
                        })
                        && let Err(issue) = encode_button2_cond_actions(&el.scripts)
                    {
                        report.push(
                            "swf_avm1_button_action_capacity",
                            FlaCompatibilityCategory::Scripts,
                            FlaCompatibilityStatus::Blocking,
                            format!("{el_loc}:script"),
                            format!("Button actions exceed an SWF record limit: {issue}."),
                        );
                    }

                    match &el.kind {
                        ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => {
                            if !el.stroke_width_profile().is_default() {
                                report.push(
                                    "swf_variable_width_stroke",
                                    FlaCompatibilityCategory::ShapesAndPaints,
                                    if options.expand_variable_width_strokes {
                                        FlaCompatibilityStatus::Baked
                                    } else {
                                        FlaCompatibilityStatus::Approximated
                                    },
                                    el_loc.clone(),
                                    "Variable-width stroke expanded into closed filled contour for SWF."
                                        .to_owned(),
                                );
                            }
                        }
                        ElementKind::TextBox { .. } | ElementKind::Text { .. } => {
                            if !options.outline_text {
                                report.push(
                                    "swf_text_requires_outlines",
                                    FlaCompatibilityCategory::TextAndFonts,
                                    FlaCompatibilityStatus::Blocking,
                                    el_loc,
                                    "The current DefineFont3 writer contains no glyph table and cannot prove that DefineEditText will render or preserve editable/rich TextField semantics. Enable outline_text for supported visual text, or embed text with a future full SWF font/TextField writer."
                                        .to_owned(),
                                );
                            } else if let Some(issue) = swf_text_outline_issue(el) {
                                report.push(
                                    "swf_text_outline_unsupported",
                                    FlaCompatibilityCategory::TextAndFonts,
                                    FlaCompatibilityStatus::Blocking,
                                    el_loc,
                                    format!(
                                        "Text cannot be converted faithfully to static glyph outlines: {issue}. Simplify the field or keep it out of this SWF publish."
                                    ),
                                );
                            } else {
                                report.push(
                                    "swf_text_outlined",
                                    FlaCompatibilityCategory::TextAndFonts,
                                    FlaCompatibilityStatus::Outlined,
                                    el_loc,
                                    "Text is intentionally converted to static vector glyph contours; editing, selection, binding and input behavior are not retained."
                                        .to_owned(),
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if has_avm1_scripts && has_avm2_scripts && !options.scriptless_mode {
        report.push(
            "swf_mixed_avm1_avm2_scripts",
            FlaCompatibilityCategory::Scripts,
            FlaCompatibilityStatus::Blocking,
            "project",
            "Project mixes AVM1 (AS1/AS2) and AVM2 (AS3) scripts; enable scriptless_mode or unify script language before SWF publish."
                .to_owned(),
        );
    }

    report
}

fn can_build_morph_shape(start_els: &[Element], end_els: &[Element]) -> bool {
    if start_els.len() != 1 || end_els.len() != 1 {
        return false;
    }
    let a_cmds = element_vector_commands(&start_els[0]);
    let b_cmds = element_vector_commands(&end_els[0]);
    match (a_cmds, b_cmds) {
        (Some(a), Some(b)) => a.len() == b.len() && !a.is_empty(),
        _ => false,
    }
}

fn element_vector_commands(el: &Element) -> Option<Vec<PathCommand>> {
    match &el.kind {
        ElementKind::Rectangle { width, height, .. } => Some(vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::LineTo { x: *width, y: 0.0 },
            PathCommand::LineTo {
                x: *width,
                y: *height,
            },
            PathCommand::LineTo { x: 0.0, y: *height },
            PathCommand::Close,
        ]),
        ElementKind::Ellipse { width, height, .. } => Some(oval_commands(*width, *height)),
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            Some(commands.clone())
        }
        _ => None,
    }
}

fn oval_commands(width: f64, height: f64) -> Vec<PathCommand> {
    let rx = width * 0.5;
    let ry = height * 0.5;
    let cx = rx;
    let cy = ry;
    let k = std::f64::consts::FRAC_1_SQRT_2;
    let t = (std::f64::consts::PI / 8.0).tan();
    vec![
        PathCommand::MoveTo { x: cx + rx, y: cy },
        PathCommand::QuadraticTo {
            cx: cx + rx,
            cy: cy + ry * t,
            x: cx + rx * k,
            y: cy + ry * k,
        },
        PathCommand::QuadraticTo {
            cx: cx + rx * t,
            cy: cy + ry,
            x: cx,
            y: cy + ry,
        },
        PathCommand::QuadraticTo {
            cx: cx - rx * t,
            cy: cy + ry,
            x: cx - rx * k,
            y: cy + ry * k,
        },
        PathCommand::QuadraticTo {
            cx: cx - rx,
            cy: cy + ry * t,
            x: cx - rx,
            y: cy,
        },
        PathCommand::QuadraticTo {
            cx: cx - rx,
            cy: cy - ry * t,
            x: cx - rx * k,
            y: cy - ry * k,
        },
        PathCommand::QuadraticTo {
            cx: cx - rx * t,
            cy: cy - ry,
            x: cx,
            y: cy - ry,
        },
        PathCommand::QuadraticTo {
            cx: cx + rx * t,
            cy: cy - ry,
            x: cx + rx * k,
            y: cy - ry * k,
        },
        PathCommand::QuadraticTo {
            cx: cx + rx,
            cy: cy - ry * t,
            x: cx + rx,
            y: cy,
        },
        PathCommand::Close,
    ]
}

/// Bit-level and byte-level writer for SWF structures and tags.
#[derive(Default, Clone)]
pub(crate) struct SwfBitWriter {
    pub(crate) buf: Vec<u8>,
    bit_buf: u8,
    bit_count: u8,
}

impl SwfBitWriter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub(crate) fn align(&mut self) {
        if self.bit_count > 0 {
            self.buf.push(self.bit_buf);
            self.bit_buf = 0;
            self.bit_count = 0;
        }
    }

    #[inline]
    pub(crate) fn bit(&mut self, b: bool) {
        if b {
            self.bit_buf |= 1 << (7 - self.bit_count);
        }
        self.bit_count += 1;
        if self.bit_count == 8 {
            self.buf.push(self.bit_buf);
            self.bit_buf = 0;
            self.bit_count = 0;
        }
    }

    pub(crate) fn ub(&mut self, n: usize, val: u32) {
        for i in (0..n).rev() {
            self.bit(((val >> i) & 1) != 0);
        }
    }

    pub(crate) fn sb(&mut self, n: usize, val: i32) {
        for i in (0..n).rev() {
            self.bit(((val >> i) & 1) != 0);
        }
    }

    #[inline]
    pub(crate) fn u8(&mut self, v: u8) {
        self.align();
        self.buf.push(v);
    }

    #[inline]
    pub(crate) fn u16(&mut self, v: u16) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[inline]
    pub(crate) fn i16(&mut self, v: i16) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[inline]
    pub(crate) fn u32(&mut self, v: u32) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[inline]
    pub(crate) fn f32(&mut self, v: f32) {
        self.align();
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    #[inline]
    pub(crate) fn bytes(&mut self, slice: &[u8]) {
        self.align();
        self.buf.extend_from_slice(slice);
    }

    pub(crate) fn c_string(&mut self, s: &str) {
        self.align();
        for &b in s.as_bytes() {
            if b != 0 {
                self.buf.push(b);
            }
        }
        self.buf.push(0);
    }

    /// Write an ABC / SWF variable-length `u32` (1 to 5 bytes, 7 bits per byte).
    pub(crate) fn u32_v(&mut self, mut v: u32) {
        self.align();
        loop {
            let byte = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(byte);
                break;
            } else {
                self.buf.push(byte | 0x80);
            }
        }
    }

    pub(crate) fn rgba(&mut self, c: Color) {
        self.align();
        self.buf.extend_from_slice(&c);
    }

    pub(crate) fn rect_twips(&mut self, bounds: [i32; 4]) {
        self.align();
        let nbits = needed_sb_bits(&bounds).max(1);
        self.ub(5, nbits as u32);
        for v in bounds {
            self.sb(nbits, v);
        }
        self.align();
    }

    pub(crate) fn matrix_px(&mut self, m: [f64; 6]) {
        self.align();
        self.write_matrix_raw(m, 20.0);
        self.align();
    }

    pub(crate) fn write_matrix_raw(&mut self, m: [f64; 6], translate_scale: f64) {
        let [a, b, c, d, tx, ty] = m;
        let has_scale = (a - 1.0).abs() > 1e-5 || (d - 1.0).abs() > 1e-5;
        self.bit(has_scale);
        if has_scale {
            let sx = (a * 65536.0).round() as i32;
            let sy = (d * 65536.0).round() as i32;
            let nbits = needed_sb_bits(&[sx, sy]).max(1);
            self.ub(5, nbits as u32);
            self.sb(nbits, sx);
            self.sb(nbits, sy);
        }
        let has_rotate = b.abs() > 1e-5 || c.abs() > 1e-5;
        self.bit(has_rotate);
        if has_rotate {
            let r0 = (b * 65536.0).round() as i32;
            let r1 = (c * 65536.0).round() as i32;
            let nbits = needed_sb_bits(&[r0, r1]).max(1);
            self.ub(5, nbits as u32);
            self.sb(nbits, r0);
            self.sb(nbits, r1);
        }
        let tx_t = (tx * translate_scale).round() as i32;
        let ty_t = (ty * translate_scale).round() as i32;
        if tx_t == 0 && ty_t == 0 {
            self.ub(5, 0);
        } else {
            let nbits = needed_sb_bits(&[tx_t, ty_t]).max(1);
            self.ub(5, nbits as u32);
            self.sb(nbits, tx_t);
            self.sb(nbits, ty_t);
        }
    }

    pub(crate) fn cxform_with_alpha(&mut self, ct: &ColorTransform, alpha: f64) {
        self.align();
        let eff_alpha_mul = if (ct.multipliers[3] - 1.0).abs() > 1e-5 {
            ct.multipliers[3]
        } else {
            alpha
        };
        let muls = [
            (ct.multipliers[0] * 256.0).round() as i32,
            (ct.multipliers[1] * 256.0).round() as i32,
            (ct.multipliers[2] * 256.0).round() as i32,
            (eff_alpha_mul * 256.0).round() as i32,
        ];
        let adds = [
            ct.offsets[0].round() as i32,
            ct.offsets[1].round() as i32,
            ct.offsets[2].round() as i32,
            ct.offsets[3].round() as i32,
        ];
        let has_add = adds.iter().any(|&v| v != 0);
        let has_mult = muls.iter().any(|&v| v != 256);
        self.bit(has_add);
        self.bit(has_mult);
        let mut all_vals = Vec::new();
        if has_mult {
            all_vals.extend_from_slice(&muls);
        }
        if has_add {
            all_vals.extend_from_slice(&adds);
        }
        let nbits = needed_sb_bits(&all_vals).clamp(1, 15);
        self.ub(4, nbits as u32);
        if has_mult {
            for v in muls {
                self.sb(nbits, v);
            }
        }
        if has_add {
            for v in adds {
                self.sb(nbits, v);
            }
        }
        self.align();
    }

    pub(crate) fn write_tag(&mut self, tag_code: u16, payload: &[u8]) {
        self.align();
        // Tags that Flash requires long header when >= 63 bytes or for certain bitmap/sound tags:
        let force_long = matches!(tag_code, 20 | 36 | 14 | 19 | 35 | 21 | 6);
        if payload.len() < 0x3f && !force_long {
            let hdr = (tag_code << 6) | (payload.len() as u16);
            self.u16(hdr);
        } else {
            let hdr = (tag_code << 6) | 0x3f;
            self.u16(hdr);
            self.u32(payload.len() as u32);
        }
        self.bytes(payload);
    }
}

pub(crate) fn needed_sb_bits(vals: &[i32]) -> usize {
    let mut max_bits = 1usize;
    for &v in vals {
        let bits = if v == 0 {
            1
        } else if v > 0 {
            (32 - v.leading_zeros() + 1) as usize
        } else if v == -1 {
            1
        } else {
            (32 - (!v).leading_zeros() + 1) as usize
        };
        if bits > max_bits {
            max_bits = bits;
        }
    }
    max_bits.min(31)
}

pub(crate) fn needed_ub_bits(val: u32) -> usize {
    if val == 0 {
        0
    } else {
        (32 - val.leading_zeros()) as usize
    }
}

struct LzmaRangeEncoder {
    out: Vec<u8>,
    low: u64,
    range: u32,
    cache_size: u32,
    cache: u8,
}

impl LzmaRangeEncoder {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            low: 0,
            range: 0xFFFF_FFFF,
            cache_size: 1,
            cache: 0,
        }
    }

    fn shift_low(&mut self) {
        let low_hi = (self.low >> 32) as u32;
        if (self.low as u32) < 0xFF00_0000 || low_hi != 0 {
            let mut temp = self.cache;
            loop {
                self.out.push(temp.wrapping_add(low_hi as u8));
                temp = 0xFF;
                self.cache_size -= 1;
                if self.cache_size == 0 {
                    break;
                }
            }
            self.cache = ((self.low as u32) >> 24) as u8;
        }
        self.cache_size += 1;
        self.low = (self.low & 0x00FF_FFFF) << 8;
    }

    fn encode_bit(&mut self, prob: &mut u16, bit: bool) {
        let bound = (self.range >> 11) * u32::from(*prob);
        if !bit {
            self.range = bound;
            *prob += (0x800_u16 - *prob) >> 5;
        } else {
            self.low += u64::from(bound);
            self.range -= bound;
            *prob -= *prob >> 5;
        }
        if self.range < 0x0100_0000 {
            self.range <<= 8;
            self.shift_low();
        }
    }

    fn finish(mut self) -> Vec<u8> {
        for _ in 0..5 {
            self.shift_low();
        }
        self.out
    }
}

/// Encode `uncompressed` bytes as a valid LZMA1 literal stream with properties `lc=3, lp=0, pb=2`
/// (`props = [0x5d, 0x00, 0x00, 0x01, 0x00]`) matching `neoflash-import/src/swf/lzma.rs`.
fn compress_zws_lzma(uncompressed: &[u8]) -> ([u8; 5], Vec<u8>) {
    let props = [0x5d, 0x00, 0x00, 0x01, 0x00]; // lc=3, lp=0, pb=2, dict=65536
    let mut rc = LzmaRangeEncoder::new();
    let mut is_match = [0x400_u16; 192];
    let mut literal_probs = [[0x400_u16; 0x300]; 8]; // 1 << (lc + lp) = 8

    for (pos, &byte) in uncompressed.iter().enumerate() {
        let pos_state = pos & 3; // pb = 2 -> pb_mask = 3
        rc.encode_bit(&mut is_match[pos_state], false);
        let prev_byte = if pos > 0 { uncompressed[pos - 1] } else { 0 };
        let lit_state = usize::from(prev_byte >> 5);
        let probs = &mut literal_probs[lit_state];
        let mut symbol = 1usize;
        for bit_idx in (0..8).rev() {
            let bit = ((byte >> bit_idx) & 1) != 0;
            rc.encode_bit(&mut probs[symbol], bit);
            symbol = (symbol << 1) ^ usize::from(bit);
        }
    }

    (props, rc.finish())
}

fn wrap_swf_container(
    body: &[u8],
    profile: SwfPublishProfile,
    compression: SwfCompression,
) -> Result<Vec<u8>, ExportError> {
    let declared_len = body
        .len()
        .checked_add(8)
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| {
            ExportError::Options(format!(
                "SWF output is {} bytes before compression; the SWF FileLength field supports at most {} bytes",
                body.len().saturating_add(8),
                u32::MAX
            ))
        })?;
    match compression {
        SwfCompression::UncompressedFws => {
            let mut out = Vec::with_capacity(8 + body.len());
            out.extend_from_slice(b"FWS");
            out.push(profile.swf_version());
            out.extend_from_slice(&declared_len.to_le_bytes());
            out.extend_from_slice(body);
            Ok(out)
        }
        SwfCompression::DeflateCws => {
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(body)?;
            let compressed = enc.finish()?;
            let mut out = Vec::with_capacity(8 + compressed.len());
            out.extend_from_slice(b"CWS");
            out.push(profile.swf_version());
            out.extend_from_slice(&declared_len.to_le_bytes());
            out.extend_from_slice(&compressed);
            Ok(out)
        }
        SwfCompression::LzmaZws => {
            let (props, compressed) = compress_zws_lzma(body);
            let compressed_len = u32::try_from(compressed.len()).map_err(|_| {
                ExportError::Options(format!(
                    "Compressed ZWS payload is {} bytes; its SWF length field supports at most {} bytes",
                    compressed.len(),
                    u32::MAX
                ))
            })?;
            let mut out = Vec::with_capacity(17 + compressed.len());
            out.extend_from_slice(b"ZWS");
            // SWF spec requires SWF version >= 13 for ZWS container.
            out.push(profile.swf_version().max(13));
            out.extend_from_slice(&declared_len.to_le_bytes());
            out.extend_from_slice(&compressed_len.to_le_bytes());
            out.extend_from_slice(&props);
            out.extend_from_slice(&compressed);
            Ok(out)
        }
    }
}

/// Build a `DefineBitsLossless2` (tag 36) payload for a PNG bitmap resource.
fn build_define_bits_lossless2_payload(
    bitmap_id: u16,
    png_bytes: &[u8],
) -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(png_bytes).ok()?.to_rgba8();
    let width = img.width();
    let height = img.height();
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return None;
    }
    let rgba = img.into_raw();

    // DefineBitsLossless2 format 5: premultiplied ARGB (`[a, r_prem, g_prem, b_prem]`).
    let mut argb_prem = Vec::with_capacity(rgba.len());
    for px in rgba.as_chunks::<4>().0 {
        let r = u16::from(px[0]);
        let g = u16::from(px[1]);
        let b = u16::from(px[2]);
        let a = u16::from(px[3]);
        let rp = ((r * a + 127) / 255) as u8;
        let gp = ((g * a + 127) / 255) as u8;
        let bp = ((b * a + 127) / 255) as u8;
        argb_prem.extend_from_slice(&[a as u8, rp, gp, bp]);
    }

    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&argb_prem).ok()?;
    let compressed = enc.finish().ok()?;

    let mut w = SwfBitWriter::new();
    w.u16(bitmap_id);
    w.u8(5); // format 5 = 32-bit ARGB
    w.u16(width as u16);
    w.u16(height as u16);
    w.bytes(&compressed);
    Some((w.buf, width, height))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwfSoundInfo {
    sound_format: u8,
    sample_rate: u32,
    rate_code: u8,
    is_16_bit: bool,
    stereo: bool,
    sample_count: u32,
    data_start: usize,
    data_end: usize,
    mp3: bool,
}

fn swf_sound_rate_code(sample_rate: u32) -> Option<u8> {
    match sample_rate {
        5_512 => Some(0),
        11_025 => Some(1),
        22_050 => Some(2),
        44_100 => Some(3),
        _ => None,
    }
}

fn analyze_swf_wav(raw: &[u8]) -> Result<SwfSoundInfo, String> {
    if raw.len() < 12 || &raw[0..4] != b"RIFF" || &raw[8..12] != b"WAVE" {
        return Err("the resource is not a complete RIFF/WAVE file".to_owned());
    }
    let riff_size = u32::from_le_bytes(raw[4..8].try_into().expect("four-byte RIFF size"));
    let riff_end = usize::try_from(riff_size)
        .ok()
        .and_then(|size| size.checked_add(8))
        .filter(|end| *end >= 12 && *end <= raw.len())
        .ok_or_else(|| "the RIFF size exceeds the available resource bytes".to_owned())?;

    let mut position = 12usize;
    let mut format = None;
    let mut data_range = None;
    while position + 8 <= riff_end {
        let chunk_id = &raw[position..position + 4];
        let chunk_len = u32::from_le_bytes(
            raw[position + 4..position + 8]
                .try_into()
                .expect("four-byte WAV chunk size"),
        ) as usize;
        let start = position + 8;
        let end = start
            .checked_add(chunk_len)
            .filter(|end| *end <= riff_end)
            .ok_or_else(|| "a WAV chunk is truncated".to_owned())?;
        if chunk_id == b"fmt " {
            if chunk_len < 16 {
                return Err("the WAV fmt chunk is shorter than 16 bytes".to_owned());
            }
            if format.is_some() {
                return Err("the WAV contains more than one fmt chunk".to_owned());
            }
            format = Some((
                u16::from_le_bytes(raw[start..start + 2].try_into().unwrap()),
                u16::from_le_bytes(raw[start + 2..start + 4].try_into().unwrap()),
                u32::from_le_bytes(raw[start + 4..start + 8].try_into().unwrap()),
                u32::from_le_bytes(raw[start + 8..start + 12].try_into().unwrap()),
                u16::from_le_bytes(raw[start + 12..start + 14].try_into().unwrap()),
                u16::from_le_bytes(raw[start + 14..start + 16].try_into().unwrap()),
            ));
        } else if chunk_id == b"data" {
            if data_range.is_some() {
                return Err("the WAV contains more than one data chunk".to_owned());
            }
            data_range = Some((start, end));
        }
        position = end
            .checked_add(chunk_len & 1)
            .filter(|next| *next <= riff_end)
            .ok_or_else(|| "the WAV chunk padding is truncated".to_owned())?;
    }

    let (encoding, channels, sample_rate, byte_rate, block_align, bits_per_sample) =
        format.ok_or_else(|| "the WAV has no fmt chunk".to_owned())?;
    let (data_start, data_end) =
        data_range.ok_or_else(|| "the WAV has no data chunk".to_owned())?;
    if encoding != 1 {
        return Err(format!(
            "WAV format code {encoding} is not integer PCM; no WAV transcoder runs in SWF publish"
        ));
    }
    if !matches!(channels, 1 | 2) {
        return Err(format!(
            "WAV has {channels} channels; SWF DefineSound supports only mono or stereo"
        ));
    }
    let rate_code = swf_sound_rate_code(sample_rate).ok_or_else(|| {
        format!(
            "WAV sample rate {sample_rate} Hz is not one of 5512, 11025, 22050 or 44100 Hz and is not resampled by SWF publish"
        )
    })?;
    if !matches!(bits_per_sample, 8 | 16) {
        return Err(format!(
            "WAV uses {bits_per_sample}-bit samples; SWF publish supports only 8-bit or 16-bit PCM without transcoding"
        ));
    }
    let expected_align = channels
        .checked_mul(bits_per_sample / 8)
        .ok_or_else(|| "WAV block alignment overflows".to_owned())?;
    if block_align != expected_align {
        return Err(format!(
            "WAV blockAlign is {block_align}, expected {expected_align} for its channel/sample layout"
        ));
    }
    let expected_byte_rate = sample_rate
        .checked_mul(u32::from(block_align))
        .ok_or_else(|| "WAV byte rate overflows".to_owned())?;
    if byte_rate != expected_byte_rate {
        return Err(format!(
            "WAV byteRate is {byte_rate}, expected {expected_byte_rate}"
        ));
    }
    let data_len = data_end - data_start;
    if data_len == 0 || data_len % usize::from(block_align) != 0 {
        return Err("WAV PCM data is empty or ends inside a sample frame".to_owned());
    }
    let sample_count = u32::try_from(data_len / usize::from(block_align))
        .map_err(|_| "WAV sample count exceeds the SWF UI32 field".to_owned())?;
    Ok(SwfSoundInfo {
        sound_format: 3,
        sample_rate,
        rate_code,
        is_16_bit: bits_per_sample == 16,
        stereo: channels == 2,
        sample_count,
        data_start,
        data_end,
        mp3: false,
    })
}

fn analyze_swf_mp3(raw: &[u8]) -> Result<SwfSoundInfo, String> {
    const BITRATES_V1_L3: [u32; 16] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    const BITRATES_V2_L3: [u32; 16] = [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ];

    let mut start = 0usize;
    if raw.starts_with(b"ID3") {
        if raw.len() < 10 || raw[6..10].iter().any(|byte| byte & 0x80 != 0) {
            return Err("the MP3 has an invalid ID3v2 header".to_owned());
        }
        let tag_size = raw[6..10]
            .iter()
            .fold(0usize, |size, byte| (size << 7) | usize::from(*byte));
        let footer_len = if raw[5] & 0x10 != 0 { 10 } else { 0 };
        start = 10usize
            .checked_add(tag_size)
            .and_then(|size| size.checked_add(footer_len))
            .filter(|size| *size <= raw.len())
            .ok_or_else(|| "the MP3 ID3v2 tag is truncated".to_owned())?;
    }
    let end = if raw.len() >= start + 128 && &raw[raw.len() - 128..raw.len() - 125] == b"TAG" {
        raw.len() - 128
    } else {
        raw.len()
    };
    if start >= end {
        return Err("the MP3 contains no MPEG audio frames".to_owned());
    }

    let mut position = start;
    let mut sample_rate = None;
    let mut stereo = None;
    let mut sample_count = 0u32;
    let mut frame_count = 0u32;
    while position < end {
        if end - position < 4 || raw[position] != 0xff || raw[position + 1] & 0xe0 != 0xe0 {
            return Err(format!(
                "MP3 data at byte {position} is not a contiguous MPEG Layer III frame"
            ));
        }
        let b1 = raw[position + 1];
        let b2 = raw[position + 2];
        let b3 = raw[position + 3];
        let version_bits = (b1 >> 3) & 0x03;
        let layer_bits = (b1 >> 1) & 0x03;
        let bitrate_index = usize::from((b2 >> 4) & 0x0f);
        let sample_rate_index = usize::from((b2 >> 2) & 0x03);
        let padding = usize::from((b2 >> 1) & 0x01);
        if version_bits == 1 || layer_bits != 1 || bitrate_index == 0 || bitrate_index == 15 {
            return Err(format!(
                "MP3 frame {frame_count} is not a supported fixed-header MPEG Layer III frame"
            ));
        }
        let rate = match (version_bits, sample_rate_index) {
            (3, 0) => 44_100,
            (3, 1) => 48_000,
            (3, 2) => 32_000,
            (2, 0) => 22_050,
            (2, 1) => 24_000,
            (2, 2) => 16_000,
            (0, 0) => 11_025,
            (0, 1) => 12_000,
            (0, 2) => 8_000,
            _ => {
                return Err(format!(
                    "MP3 frame {frame_count} has a reserved sample-rate field"
                ));
            }
        };
        let rate_code = swf_sound_rate_code(rate).ok_or_else(|| {
            format!(
                "MP3 sample rate {rate} Hz is not one of the rates representable by SWF DefineSound and is not resampled"
            )
        })?;
        let bitrate_kbps = if version_bits == 3 {
            BITRATES_V1_L3[bitrate_index]
        } else {
            BITRATES_V2_L3[bitrate_index]
        };
        let samples_in_frame = if version_bits == 3 { 1152u32 } else { 576u32 };
        let coefficient = if version_bits == 3 { 144usize } else { 72usize };
        let frame_len = coefficient
            .checked_mul(usize::try_from(bitrate_kbps).unwrap())
            .and_then(|value| value.checked_mul(1000))
            .map(|value| value / usize::try_from(rate).unwrap() + padding)
            .filter(|length| *length >= 4)
            .ok_or_else(|| "MP3 frame length overflows".to_owned())?;
        if position
            .checked_add(frame_len)
            .is_none_or(|next| next > end)
        {
            return Err(format!("MP3 frame {frame_count} is truncated"));
        }
        let frame_stereo = b3 >> 6 != 3;
        if sample_rate.is_some_and(|previous| previous != rate)
            || stereo.is_some_and(|previous| previous != frame_stereo)
        {
            return Err(
                "MP3 changes sample rate or channel layout between frames; DefineSound has one global layout"
                    .to_owned(),
            );
        }
        sample_rate = Some(rate);
        stereo = Some(frame_stereo);
        sample_count = sample_count
            .checked_add(samples_in_frame)
            .ok_or_else(|| "MP3 sample count exceeds the SWF UI32 field".to_owned())?;
        frame_count += 1;
        position += frame_len;
        let _ = rate_code;
    }
    let sample_rate = sample_rate.ok_or_else(|| "the MP3 has no audio frames".to_owned())?;
    Ok(SwfSoundInfo {
        sound_format: 2,
        sample_rate,
        rate_code: swf_sound_rate_code(sample_rate).expect("validated MP3 rate"),
        is_16_bit: true,
        stereo: stereo.unwrap_or(false),
        sample_count,
        data_start: start,
        data_end: end,
        mp3: true,
    })
}

fn analyze_swf_sound_resource(media_type: &str, raw: &[u8]) -> Result<SwfSoundInfo, String> {
    if media_type.eq_ignore_ascii_case("audio/wav")
        || media_type.eq_ignore_ascii_case("audio/x-wav")
        || media_type.eq_ignore_ascii_case("audio/wave")
    {
        analyze_swf_wav(raw)
    } else if media_type.eq_ignore_ascii_case("audio/mpeg")
        || media_type.eq_ignore_ascii_case("audio/mp3")
    {
        analyze_swf_mp3(raw)
    } else {
        Err(format!("audio codec '{media_type}' is not supported"))
    }
}

/// Build a `DefineSound` (tag 14) payload after proving the source layout is representable.
fn build_define_sound_payload(
    sound_id: u16,
    media_type: &str,
    raw: &[u8],
) -> Result<Vec<u8>, String> {
    let info = analyze_swf_sound_resource(media_type, raw)?;
    let flags = (info.sound_format << 4)
        | (info.rate_code << 2)
        | (u8::from(info.is_16_bit) << 1)
        | u8::from(info.stereo);
    let mut writer = SwfBitWriter::new();
    writer.u16(sound_id);
    writer.u8(flags);
    writer.u32(info.sample_count);
    if info.mp3 {
        writer.i16(0); // seek_samples = 0; no encoder delay is introduced by this passthrough.
    }
    writer.bytes(&raw[info.data_start..info.data_end]);
    Ok(writer.buf)
}

/// Build a `StartSound` (tag 15) payload for a `FrameSound`.
fn build_start_sound_payload(sound_id: u16, sound: &FrameSound) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u16(sound_id);
    let sync_stop = sound.sync == SoundSync::Stop;
    let sync_no_multiple = sound.sync == SoundSync::Start;
    let loop_count: u16 = if sound.loop_forever {
        0x7fff
    } else {
        sound.loops.clamp(1, 32767) as u16
    };
    let has_loops = !sync_stop && loop_count > 1;
    let env_points: Vec<(u32, u16, u16)> = match sound.effect {
        SoundEffect::None | SoundEffect::Custom => Vec::new(),
        SoundEffect::LeftChannel => vec![(0, 32768, 0)],
        SoundEffect::RightChannel => vec![(0, 0, 32768)],
        SoundEffect::FadeLeftToRight => vec![(0, 32768, 0), (44100, 0, 32768)],
        SoundEffect::FadeRightToLeft => vec![(0, 0, 32768), (44100, 32768, 0)],
        SoundEffect::FadeIn => vec![(0, 0, 0), (44100, 32768, 32768)],
        SoundEffect::FadeOut => vec![(0, 32768, 32768), (44100, 0, 0)],
    };
    let has_envelope = !sync_stop && !env_points.is_empty();
    let flags: u8 = (if sync_stop { 0x20 } else { 0 })
        | (if sync_no_multiple { 0x10 } else { 0 })
        | (if has_envelope { 0x08 } else { 0 })
        | (if has_loops { 0x04 } else { 0 });
    w.u8(flags);
    if has_loops {
        w.u16(loop_count);
    }
    if has_envelope {
        w.u8(env_points.len() as u8);
        for (pos44, left, right) in env_points {
            w.u32(pos44);
            w.u16(left);
            w.u16(right);
        }
    }
    w.buf
}

/// Build a `DefineFont3` (tag 75) payload defining font family, bold, and italic attributes.
fn build_define_font3_payload(font_id: u16, name: &str, bold: bool, italic: bool) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u16(font_id);
    // flags: has_layout(0)=0, shift_jis=0, small_text=0, ansi=0, wide_offsets=0,
    // wide_codes(0x04)=1, italic(0x02), bold(0x01)
    let flags: u8 = 0x04 | (if italic { 0x02 } else { 0 }) | (if bold { 0x01 } else { 0 });
    w.u8(flags);
    w.u8(1); // language_code = 1 (Latin)
    let name_bytes = name.as_bytes();
    let len = name_bytes.len().min(255);
    w.u8(len as u8);
    w.bytes(&name_bytes[..len]);
    w.u16(0); // num_glyphs = 0 (device/system metrics)
    w.buf
}

/// Build a `DefineEditText` (tag 37) payload for an `ElementKind::TextBox`.
#[allow(clippy::too_many_arguments)]
fn build_define_edit_text_payload(
    text_id: u16,
    font_id: u16,
    text: &str,
    width: f64,
    height: f64,
    format: &neoflash_core::TextFormat,
    word_wrap: bool,
    multiline: bool,
    alpha: f64,
) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u16(text_id);
    let w_twips = (width.max(4.0) * 20.0).round() as i32;
    let h_twips = (height.max(4.0) * 20.0).round() as i32;
    w.rect_twips([0, w_twips, 0, h_twips]);

    // flags0: has_text(0x80) | word_wrap(0x40) | multiline(0x20) | read_only(0x08) | has_text_color(0x04) | has_font(0x01)
    let flags0: u8 = 0x80
        | (if word_wrap { 0x40 } else { 0 })
        | (if multiline || text.contains('\n') {
            0x20
        } else {
            0
        })
        | 0x08
        | 0x04
        | 0x01;
    // flags1: has_layout(0x20) | was_static(0x04)
    let flags1: u8 = 0x20 | 0x04;
    w.u8(flags0);
    w.u8(flags1);

    w.u16(font_id);
    let font_height_twips = (format.size.clamp(1.0, 1000.0) * 20.0).round() as u16;
    w.u16(font_height_twips.max(20));

    let r = ((format.color >> 16) & 0xff) as u8;
    let g = ((format.color >> 8) & 0xff) as u8;
    let b = (format.color & 0xff) as u8;
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    w.rgba([r, g, b, a]);

    let align_byte: u8 = match format.align {
        TextAlign::Left => 0,
        TextAlign::Right => 1,
        TextAlign::Center => 2,
    };
    w.u8(align_byte);
    let to_u16_twips = |px: f64| -> u16 { (px.max(0.0) * 20.0).round().clamp(0.0, 65535.0) as u16 };
    let to_i16_twips = |px: f64| -> i16 {
        (px * 20.0)
            .round()
            .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
    };
    w.u16(to_u16_twips(format.left_margin));
    w.u16(to_u16_twips(format.right_margin));
    w.i16(to_i16_twips(format.indent));
    w.i16(to_i16_twips(format.leading));

    w.c_string(""); // variable_name
    w.c_string(text); // initial_text
    w.buf
}

#[derive(Clone, Debug)]
struct SwfShapeFillStyle {
    color: Color,
    gradient: Option<GradientPaint>,
}

#[derive(Clone, Debug)]
struct SwfShapeLineStyle {
    width_twips: u16,
    color: Color,
    gradient: Option<GradientPaint>,
    stroke_cap: StrokeCap,
    stroke_join: StrokeJoin,
    miter_limit: f64,
}

#[derive(Clone, Copy, Debug)]
struct SwfEdge {
    fill_1: u16,
    line: u16,
    start: [i32; 2],
    control: Option<[i32; 2]>,
    end: [i32; 2],
}

fn px_to_twips(v: f64) -> i32 {
    if v.is_finite() {
        (v * 20.0).round().clamp(-1_000_000.0, 1_000_000.0) as i32
    } else {
        0
    }
}

type SwfEdgeSeg = ([i32; 2], Option<[i32; 2]>, [i32; 2]);
type SwfSubpath = (Vec<SwfEdgeSeg>, bool);

fn convert_commands_to_swf_edges(
    commands: &[PathCommand],
    fill_idx: u16,
    line_idx: u16,
    force_close: bool,
    out_edges: &mut Vec<SwfEdge>,
) {
    let mut subpaths: Vec<SwfSubpath> = Vec::new();
    let mut cur = [0.0_f64, 0.0_f64];
    let mut start = [0.0_f64, 0.0_f64];
    let mut has_start = false;
    let mut segs: Vec<SwfEdgeSeg> = Vec::new();

    let push_seg =
        |segs: &mut Vec<SwfEdgeSeg>, p0: [f64; 2], ctrl: Option<[f64; 2]>, p1: [f64; 2]| {
            let s = [px_to_twips(p0[0]), px_to_twips(p0[1])];
            let e = [px_to_twips(p1[0]), px_to_twips(p1[1])];
            let c = ctrl.map(|cp| [px_to_twips(cp[0]), px_to_twips(cp[1])]);
            if s == e && c.is_none() {
                return;
            }
            segs.push((s, c, e));
        };

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                if !segs.is_empty() {
                    subpaths.push((std::mem::take(&mut segs), false));
                }
                cur = [x, y];
                start = [x, y];
                has_start = true;
            }
            PathCommand::LineTo { x, y } => {
                if !has_start {
                    start = cur;
                    has_start = true;
                }
                push_seg(&mut segs, cur, None, [x, y]);
                cur = [x, y];
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if !has_start {
                    start = cur;
                    has_start = true;
                }
                push_seg(&mut segs, cur, Some([cx, cy]), [x, y]);
                cur = [x, y];
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if !has_start {
                    start = cur;
                    has_start = true;
                }
                let p0 = cur;
                let p1 = [cx1, cy1];
                let p2 = [cx2, cy2];
                let p3 = [x, y];
                let q0 = [0.5 * (p0[0] + p1[0]), 0.5 * (p0[1] + p1[1])];
                let q1 = [0.5 * (p1[0] + p2[0]), 0.5 * (p1[1] + p2[1])];
                let q2 = [0.5 * (p2[0] + p3[0]), 0.5 * (p2[1] + p3[1])];
                let r0 = [0.5 * (q0[0] + q1[0]), 0.5 * (q0[1] + q1[1])];
                let r1 = [0.5 * (q1[0] + q2[0]), 0.5 * (q1[1] + q2[1])];
                let mid = [0.5 * (r0[0] + r1[0]), 0.5 * (r0[1] + r1[1])];
                push_seg(&mut segs, p0, Some(q0), mid);
                push_seg(&mut segs, mid, Some(q2), p3);
                cur = p3;
            }
            PathCommand::Close => {
                if has_start {
                    push_seg(&mut segs, cur, None, start);
                    cur = start;
                }
                if !segs.is_empty() {
                    subpaths.push((std::mem::take(&mut segs), true));
                }
            }
        }
    }
    if !segs.is_empty() {
        subpaths.push((segs, false));
    }

    for (mut sp, closed) in subpaths {
        if sp.is_empty() {
            continue;
        }
        let first_pt = sp[0].0;
        let last_pt = sp.last().unwrap().2;
        if (fill_idx != 0 || closed || force_close) && first_pt != last_pt {
            sp.push((last_pt, None, first_pt));
        }
        for (s, c, e) in sp {
            out_edges.push(SwfEdge {
                fill_1: fill_idx,
                line: line_idx,
                start: s,
                control: c,
                end: e,
            });
        }
    }
}

fn write_single_fill_style(w: &mut SwfBitWriter, fill: &SwfShapeFillStyle) {
    if let Some(grad) = &fill.gradient
        && !grad.stops.is_empty()
    {
        let fill_type: u8 = match grad.kind {
            GradientKind::Linear => 0x10,
            GradientKind::Radial => {
                if grad.focal_point_ratio.abs() > 1e-4 {
                    0x13
                } else {
                    0x12
                }
            }
        };
        w.u8(fill_type);
        w.matrix_px(grad.matrix);
        let count = grad.stops.len().min(15);
        // spread(2 bits)=0, interp(2 bits)=0, count(4 bits)
        w.u8(count as u8);
        for stop in grad.stops.iter().take(count) {
            w.u8(stop.ratio);
            w.rgba(stop.color);
        }
        if fill_type == 0x13 {
            let focal_fixed8 = (grad.focal_point_ratio.clamp(-1.0, 1.0) * 256.0).round() as i16;
            w.i16(focal_fixed8);
        }
    } else {
        w.u8(0x00); // Solid fill
        w.rgba(fill.color);
    }
}

fn write_shape_edges_records(
    w: &mut SwfBitWriter,
    edges: &[SwfEdge],
    num_fill_bits: usize,
    num_line_bits: usize,
) {
    w.align();
    w.ub(4, num_fill_bits as u32);
    w.ub(4, num_line_bits as u32);

    let mut cur_x = 0i32;
    let mut cur_y = 0i32;
    let mut cur_f1 = 0u16;
    let mut cur_ls = 0u16;
    let mut first = true;

    for edge in edges {
        let need_move = first || edge.start[0] != cur_x || edge.start[1] != cur_y;
        let need_f1 = first || edge.fill_1 != cur_f1;
        let need_ls = first || edge.line != cur_ls;
        first = false;

        if need_move || need_f1 || need_ls {
            w.bit(false); // StyleChangeRecord
            w.bit(false); // state_new_styles = 0
            w.bit(need_ls);
            w.bit(need_f1);
            w.bit(false); // state_fill_style0 = 0
            w.bit(need_move);

            if need_move {
                let mx = edge.start[0];
                let my = edge.start[1];
                let mbits = needed_sb_bits(&[mx, my]).max(1);
                w.ub(5, mbits as u32);
                w.sb(mbits, mx);
                w.sb(mbits, my);
                cur_x = mx;
                cur_y = my;
            }
            if need_f1 {
                w.ub(num_fill_bits, u32::from(edge.fill_1));
                cur_f1 = edge.fill_1;
            }
            if need_ls {
                w.ub(num_line_bits, u32::from(edge.line));
                cur_ls = edge.line;
            }
        }

        if let Some(ctrl) = edge.control {
            let cdx = ctrl[0] - cur_x;
            let cdy = ctrl[1] - cur_y;
            let adx = edge.end[0] - ctrl[0];
            let ady = edge.end[1] - ctrl[1];
            let nbits = needed_sb_bits(&[cdx, cdy, adx, ady]).clamp(2, 17);
            w.bit(true); // Edge record
            w.bit(false); // Curved edge
            w.ub(4, (nbits - 2) as u32);
            w.sb(nbits, cdx);
            w.sb(nbits, cdy);
            w.sb(nbits, adx);
            w.sb(nbits, ady);
        } else {
            let dx = edge.end[0] - cur_x;
            let dy = edge.end[1] - cur_y;
            let nbits = needed_sb_bits(&[dx, dy]).clamp(2, 17);
            w.bit(true); // Edge record
            w.bit(true); // Straight edge
            w.ub(4, (nbits - 2) as u32);
            w.bit(true); // general line (dx, dy)
            w.sb(nbits, dx);
            w.sb(nbits, dy);
        }
        cur_x = edge.end[0];
        cur_y = edge.end[1];
    }

    // EndShapeRecord (6 zero bits)
    w.ub(6, 0);
    w.align();
}

fn compute_edges_bounds(edges: &[SwfEdge]) -> [i32; 4] {
    if edges.is_empty() {
        return [0, 20, 0, 20];
    }
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    let mut include = |pt: [i32; 2]| {
        min_x = min_x.min(pt[0]);
        max_x = max_x.max(pt[0]);
        min_y = min_y.min(pt[1]);
        max_y = max_y.max(pt[1]);
    };
    for e in edges {
        include(e.start);
        if let Some(c) = e.control {
            include(c);
        }
        include(e.end);
    }
    if max_x <= min_x {
        max_x = min_x + 20;
    }
    if max_y <= min_y {
        max_y = min_y + 20;
    }
    [min_x, max_x, min_y, max_y]
}

/// Build a `DefineShape3` (tag 32) or `DefineShape4` (tag 83) tag code and payload for a vector element.
fn build_define_shape_for_element(
    shape_id: u16,
    element: &Element,
    options: &SwfPublishOptions,
) -> Option<(u16, Vec<u8>)> {
    let mut fills: Vec<SwfShapeFillStyle> = Vec::new();
    let mut lines: Vec<SwfShapeLineStyle> = Vec::new();
    let mut edges: Vec<SwfEdge> = Vec::new();

    let mut add_fill = |color: Color, gradient: Option<GradientPaint>| -> u16 {
        if color[3] == 0 && gradient.is_none() {
            return 0;
        }
        fills.push(SwfShapeFillStyle { color, gradient });
        fills.len() as u16
    };

    #[allow(clippy::too_many_arguments)]
    let mut add_line = |color: Color,
                        width: f64,
                        gradient: Option<GradientPaint>,
                        cap: StrokeCap,
                        join: StrokeJoin,
                        miter: f64|
     -> u16 {
        if width <= 0.0 || (color[3] == 0 && gradient.is_none()) {
            return 0;
        }
        let width_twips = (width * 20.0).round().clamp(1.0, 65535.0) as u16;
        lines.push(SwfShapeLineStyle {
            width_twips,
            color,
            gradient,
            stroke_cap: cap,
            stroke_join: join,
            miter_limit: miter,
        });
        lines.len() as u16
    };

    match &element.kind {
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            let f_idx = add_fill(*fill, None);
            let l_idx = add_line(
                *stroke,
                *stroke_width,
                None,
                StrokeCap::Round,
                StrokeJoin::Round,
                3.0,
            );
            let cmds = element
                .to_path_commands()
                .map(|(c, _)| c)
                .or_else(|| element_vector_commands(element))?;
            convert_commands_to_swf_edges(&cmds, f_idx, l_idx, true, &mut edges);
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let f_idx = add_fill(*fill, None);
            let l_idx = add_line(
                *stroke,
                *stroke_width,
                None,
                StrokeCap::Round,
                StrokeJoin::Round,
                3.0,
            );
            let cmds = element
                .to_path_commands()
                .map(|(c, _)| c)
                .unwrap_or_else(|| oval_commands(*width, *height));
            convert_commands_to_swf_edges(&cmds, f_idx, l_idx, true, &mut edges);
        }
        ElementKind::Path {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            let f_idx = add_fill(*fill, None);
            let profile = element.stroke_width_profile();
            if !profile.is_default() && options.expand_variable_width_strokes {
                if f_idx != 0 {
                    convert_commands_to_swf_edges(commands, f_idx, 0, true, &mut edges);
                }
                let expanded =
                    neoflash_core::expand_variable_width_stroke(commands, *stroke_width, &profile);
                if !expanded.is_empty() {
                    let sf_idx = add_fill(*stroke, None);
                    if sf_idx != 0 {
                        convert_commands_to_swf_edges(&expanded, sf_idx, 0, true, &mut edges);
                    }
                }
            } else {
                let l_idx = add_line(
                    *stroke,
                    *stroke_width,
                    None,
                    StrokeCap::Round,
                    StrokeJoin::Round,
                    3.0,
                );
                convert_commands_to_swf_edges(commands, f_idx, l_idx, false, &mut edges);
            }
        }
        ElementKind::PaintedPath {
            commands,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            let f_idx = add_fill(*fill, fill_gradient.clone());
            let profile = element.stroke_width_profile();
            if !profile.is_default() && options.expand_variable_width_strokes {
                if f_idx != 0 {
                    convert_commands_to_swf_edges(commands, f_idx, 0, true, &mut edges);
                }
                let expanded =
                    neoflash_core::expand_variable_width_stroke(commands, *stroke_width, &profile);
                if !expanded.is_empty() {
                    let sf_idx = add_fill(*stroke, stroke_gradient.clone());
                    if sf_idx != 0 {
                        convert_commands_to_swf_edges(&expanded, sf_idx, 0, true, &mut edges);
                    }
                }
            } else {
                let l_idx = add_line(
                    *stroke,
                    *stroke_width,
                    stroke_gradient.clone(),
                    StrokeCap::Round,
                    StrokeJoin::Round,
                    3.0,
                );
                convert_commands_to_swf_edges(commands, f_idx, l_idx, false, &mut edges);
            }
        }
        ElementKind::TextBox { .. } | ElementKind::Text { .. } if options.outline_text => {
            for outlined in outline_text_element_to_paths(element) {
                match &outlined.kind {
                    ElementKind::Path { commands, fill, .. } => {
                        let f_idx = add_fill(*fill, None);
                        convert_commands_to_swf_edges(commands, f_idx, 0, true, &mut edges);
                    }
                    ElementKind::PaintedPath {
                        commands,
                        fill,
                        fill_gradient,
                        ..
                    } => {
                        let f_idx = add_fill(*fill, fill_gradient.clone());
                        convert_commands_to_swf_edges(commands, f_idx, 0, true, &mut edges);
                    }
                    _ => {}
                }
            }
        }
        _ => return None,
    }

    if edges.is_empty() {
        return None;
    }

    let use_shape4 = options.profile.supports_define_shape4();
    let tag_code: u16 = if use_shape4 { 83 } else { 32 };
    let bounds = compute_edges_bounds(&edges);

    let mut w = SwfBitWriter::new();
    w.u16(shape_id);
    w.rect_twips(bounds);
    if use_shape4 {
        w.rect_twips(bounds); // edge_bounds
        w.u8(0); // shape4 flags
    }

    // FillStyleArray
    if fills.len() >= 0xff {
        w.u8(0xff);
        w.u16(fills.len() as u16);
    } else {
        w.u8(fills.len() as u8);
    }
    for fill in &fills {
        write_single_fill_style(&mut w, fill);
    }

    // LineStyleArray
    if lines.len() >= 0xff {
        w.u8(0xff);
        w.u16(lines.len() as u16);
    } else {
        w.u8(lines.len() as u8);
    }
    for line in &lines {
        w.u16(line.width_twips);
        if use_shape4 {
            let cap_bits: u32 = match line.stroke_cap {
                StrokeCap::Round => 0,
                StrokeCap::Butt => 1,
                StrokeCap::Square => 2,
            };
            let join_bits: u32 = match line.stroke_join {
                StrokeJoin::Round => 0,
                StrokeJoin::Bevel => 1,
                StrokeJoin::Miter => 2,
            };
            let has_fill = line.gradient.is_some();
            w.align();
            w.ub(2, cap_bits);
            w.ub(2, join_bits);
            w.bit(has_fill);
            w.bit(false); // no_hscale
            w.bit(false); // no_vscale
            w.bit(false); // pixel_hinting
            w.ub(5, 0); // reserved
            w.bit(false); // no_close
            w.ub(2, cap_bits); // end_cap
            w.align();
            if join_bits == 2 {
                let miter_fixed8 = (line.miter_limit.clamp(1.0, 255.0) * 256.0).round() as u16;
                w.u16(miter_fixed8);
            }
            if has_fill {
                write_single_fill_style(
                    &mut w,
                    &SwfShapeFillStyle {
                        color: line.color,
                        gradient: line.gradient.clone(),
                    },
                );
            } else {
                w.rgba(line.color);
            }
        } else {
            w.rgba(line.color);
        }
    }

    let num_fill_bits = needed_ub_bits(fills.len() as u32).max(1);
    let num_line_bits = needed_ub_bits(lines.len() as u32).max(1);
    write_shape_edges_records(&mut w, &edges, num_fill_bits, num_line_bits);

    Some((tag_code, w.buf))
}

/// Build a `DefineMorphShape` (tag 46) payload for a compatible start/end shape pair.
fn build_define_morph_shape_payload(
    morph_id: u16,
    start_el: &Element,
    end_el: &Element,
) -> Option<Vec<u8>> {
    let start_cmds = element_vector_commands(start_el)?;
    let end_cmds = element_vector_commands(end_el)?;
    if start_cmds.len() != end_cmds.len() || start_cmds.is_empty() {
        return None;
    }

    let extract_paints = |el: &Element| -> (Color, Color, f64) {
        match &el.kind {
            ElementKind::Rectangle {
                fill,
                stroke,
                stroke_width,
                ..
            }
            | ElementKind::Ellipse {
                fill,
                stroke,
                stroke_width,
                ..
            }
            | ElementKind::Path {
                fill,
                stroke,
                stroke_width,
                ..
            }
            | ElementKind::PaintedPath {
                fill,
                stroke,
                stroke_width,
                ..
            } => (*fill, *stroke, *stroke_width),
            _ => ([0, 0, 0, 255], [0, 0, 0, 0], 0.0),
        }
    };
    let (s_fill, s_stroke, s_width) = extract_paints(start_el);
    let (e_fill, e_stroke, e_width) = extract_paints(end_el);
    let has_fill = s_fill[3] > 0 || e_fill[3] > 0;
    let has_line = (s_width > 0.0 && s_stroke[3] > 0) || (e_width > 0.0 && e_stroke[3] > 0);
    let fill_idx = u16::from(has_fill);
    let line_idx = u16::from(has_line);

    let mut start_edges = Vec::new();
    let mut end_edges = Vec::new();
    convert_commands_to_swf_edges(&start_cmds, fill_idx, line_idx, has_fill, &mut start_edges);
    convert_commands_to_swf_edges(&end_cmds, fill_idx, line_idx, has_fill, &mut end_edges);
    if start_edges.is_empty() || end_edges.is_empty() {
        return None;
    }

    let start_bounds = compute_edges_bounds(&start_edges);
    let end_bounds = compute_edges_bounds(&end_edges);

    let mut styles_and_start = SwfBitWriter::new();
    if has_fill {
        styles_and_start.u8(1); // 1 morph fill style
        styles_and_start.u8(0x00); // solid morph fill
        styles_and_start.rgba(s_fill);
        styles_and_start.rgba(e_fill);
    } else {
        styles_and_start.u8(0);
    }
    if has_line {
        styles_and_start.u8(1); // 1 morph line style
        styles_and_start.u16((s_width * 20.0).round().clamp(1.0, 65535.0) as u16);
        styles_and_start.u16((e_width * 20.0).round().clamp(1.0, 65535.0) as u16);
        styles_and_start.rgba(s_stroke);
        styles_and_start.rgba(e_stroke);
    } else {
        styles_and_start.u8(0);
    }
    write_shape_edges_records(&mut styles_and_start, &start_edges, 1, 1);

    let mut end_edges_writer = SwfBitWriter::new();
    write_shape_edges_records(&mut end_edges_writer, &end_edges, 1, 1);

    let mut w = SwfBitWriter::new();
    w.u16(morph_id);
    w.rect_twips(start_bounds);
    w.rect_twips(end_bounds);
    w.u32(styles_and_start.buf.len() as u32);
    w.bytes(&styles_and_start.buf);
    w.bytes(&end_edges_writer.buf);
    Some(w.buf)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwfUnaryOp {
    Not,
    Neg,
    Pos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwfBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SwfAssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
}

#[derive(Clone, Debug, PartialEq)]
enum SwfLValue {
    Ident(String),
    Member(Box<SwfExpr>, String),
    Index(Box<SwfExpr>, Box<SwfExpr>),
}

#[derive(Clone, Debug, PartialEq)]
enum SwfExpr {
    Null,
    Undefined,
    Bool(bool),
    Int(i32),
    Float(f64),
    Str(String),
    Ident(String),
    ArrayLit(Vec<SwfExpr>),
    Member(Box<SwfExpr>, String),
    Index(Box<SwfExpr>, Box<SwfExpr>),
    Unary {
        op: SwfUnaryOp,
        expr: Box<SwfExpr>,
    },
    Binary {
        op: SwfBinaryOp,
        lhs: Box<SwfExpr>,
        rhs: Box<SwfExpr>,
    },
    Call {
        callee: Box<SwfExpr>,
        args: Vec<SwfExpr>,
    },
    New {
        callee: Box<SwfExpr>,
        args: Vec<SwfExpr>,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum SwfStmt {
    VarDecl {
        name: String,
        init: Option<SwfExpr>,
    },
    Assign {
        target: SwfLValue,
        op: SwfAssignOp,
        value: SwfExpr,
    },
    IncDec {
        target: SwfLValue,
        increment: bool,
    },
    Expr(SwfExpr),
    Block(Vec<SwfStmt>),
    If {
        cond: SwfExpr,
        then_branch: Vec<SwfStmt>,
        else_branch: Option<Vec<SwfStmt>>,
    },
    While {
        cond: SwfExpr,
        body: Vec<SwfStmt>,
    },
    For {
        init: Option<Box<SwfStmt>>,
        cond: Option<SwfExpr>,
        update: Option<Box<SwfStmt>>,
        body: Vec<SwfStmt>,
    },
    FunctionDecl {
        name: String,
        params: Vec<String>,
        body: Vec<SwfStmt>,
    },
    Return(Option<SwfExpr>),
    EventHandler {
        is_clip_event: bool,
        events: Vec<String>,
        body: Vec<SwfStmt>,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum SwfToken {
    Ident(String),
    Int(i32),
    Float(f64),
    Str(String),
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semi,
    Comma,
    Dot,
    Colon,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    PlusPlus,
    MinusMinus,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Not,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
}

fn tokenize_swf_script(source: &str) -> Result<Vec<SwfToken>, String> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < len {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            let mut closed = false;
            while i + 1 < len {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                return Err("unterminated block comment in script".to_owned());
            }
            continue;
        }
        if b == b'"' || b == b'\'' {
            let quote = b;
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < len {
                let ch = source[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '\\' {
                    if i >= len {
                        return Err("unterminated escape sequence in string literal".to_owned());
                    }
                    let esc = source[i..].chars().next().unwrap();
                    i += esc.len_utf8();
                    match esc {
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        other => s.push(other),
                    }
                } else if ch as u32 == u32::from(quote) {
                    closed = true;
                    break;
                } else if ch == '\n' || ch == '\r' {
                    return Err("unterminated string literal in script".to_owned());
                } else {
                    s.push(ch);
                }
            }
            if !closed {
                return Err("unterminated string literal in script".to_owned());
            }
            tokens.push(SwfToken::Str(s));
            continue;
        }
        if b.is_ascii_digit() {
            let start = i;
            if b == b'0' && i + 1 < len && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
                i += 2;
                let hex_start = i;
                while i < len && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
                if i == hex_start {
                    return Err("invalid hex literal in script".to_owned());
                }
                let hex_str = &source[hex_start..i];
                let val = i64::from_str_radix(hex_str, 16)
                    .map_err(|_| format!("invalid hex literal '0x{hex_str}'"))?;
                if let Ok(iv) = i32::try_from(val) {
                    tokens.push(SwfToken::Int(iv));
                } else {
                    tokens.push(SwfToken::Float(val as f64));
                }
                continue;
            }
            let mut is_float = false;
            while i < len && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < len && bytes[i] == b'.' && (i + 1 >= len || bytes[i + 1].is_ascii_digit()) {
                is_float = true;
                i += 1;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
                is_float = true;
                i += 1;
                if i < len && (bytes[i] == b'+' || bytes[i] == b'-') {
                    i += 1;
                }
                let exp_start = i;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i == exp_start {
                    return Err("invalid numeric exponent in script".to_owned());
                }
            }
            let num_str = &source[start..i];
            if !is_float && let Ok(iv) = num_str.parse::<i32>() {
                tokens.push(SwfToken::Int(iv));
            } else if let Ok(fv) = num_str.parse::<f64>() {
                tokens.push(SwfToken::Float(fv));
            } else {
                return Err(format!("invalid numeric literal '{num_str}'"));
            }
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
            let start = i;
            i += 1;
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
            tokens.push(SwfToken::Ident(source[start..i].to_owned()));
            continue;
        }
        let rem = &source[i..];
        if rem.starts_with("===") {
            tokens.push(SwfToken::StrictEq);
            i += 3;
        } else if rem.starts_with("!==") {
            tokens.push(SwfToken::StrictNe);
            i += 3;
        } else if rem.starts_with(">>>") {
            tokens.push(SwfToken::UShr);
            i += 3;
        } else if rem.starts_with("==") {
            tokens.push(SwfToken::Eq);
            i += 2;
        } else if rem.starts_with("!=") {
            tokens.push(SwfToken::Ne);
            i += 2;
        } else if rem.starts_with("<=") {
            tokens.push(SwfToken::Le);
            i += 2;
        } else if rem.starts_with(">=") {
            tokens.push(SwfToken::Ge);
            i += 2;
        } else if rem.starts_with("&&") {
            tokens.push(SwfToken::And);
            i += 2;
        } else if rem.starts_with("||") {
            tokens.push(SwfToken::Or);
            i += 2;
        } else if rem.starts_with("++") {
            tokens.push(SwfToken::PlusPlus);
            i += 2;
        } else if rem.starts_with("--") {
            tokens.push(SwfToken::MinusMinus);
            i += 2;
        } else if rem.starts_with("+=") {
            tokens.push(SwfToken::PlusAssign);
            i += 2;
        } else if rem.starts_with("-=") {
            tokens.push(SwfToken::MinusAssign);
            i += 2;
        } else if rem.starts_with("*=") {
            tokens.push(SwfToken::StarAssign);
            i += 2;
        } else if rem.starts_with("/=") {
            tokens.push(SwfToken::SlashAssign);
            i += 2;
        } else if rem.starts_with("%=") {
            tokens.push(SwfToken::PercentAssign);
            i += 2;
        } else if rem.starts_with("<<") {
            tokens.push(SwfToken::Shl);
            i += 2;
        } else if rem.starts_with(">>") {
            tokens.push(SwfToken::Shr);
            i += 2;
        } else {
            let tok = match b {
                b'(' => SwfToken::LParen,
                b')' => SwfToken::RParen,
                b'{' => SwfToken::LBrace,
                b'}' => SwfToken::RBrace,
                b'[' => SwfToken::LBracket,
                b']' => SwfToken::RBracket,
                b';' => SwfToken::Semi,
                b',' => SwfToken::Comma,
                b'.' => SwfToken::Dot,
                b':' => SwfToken::Colon,
                b'+' => SwfToken::Plus,
                b'-' => SwfToken::Minus,
                b'*' => SwfToken::Star,
                b'/' => SwfToken::Slash,
                b'%' => SwfToken::Percent,
                b'=' => SwfToken::Assign,
                b'<' => SwfToken::Lt,
                b'>' => SwfToken::Gt,
                b'!' => SwfToken::Not,
                b'&' => SwfToken::BitAnd,
                b'|' => SwfToken::BitOr,
                b'^' => SwfToken::BitXor,
                _ => {
                    let ch = source[i..].chars().next().unwrap_or('?');
                    return Err(format!(
                        "unsupported syntax token '{ch}' in bounded SWF compiler"
                    ));
                }
            };
            tokens.push(tok);
            i += 1;
        }
    }
    Ok(tokens)
}

struct SwfParser {
    tokens: Vec<SwfToken>,
    pos: usize,
    is_avm2: bool,
}

impl SwfParser {
    fn new(tokens: Vec<SwfToken>, is_avm2: bool) -> Self {
        Self {
            tokens,
            pos: 0,
            is_avm2,
        }
    }

    fn peek(&self) -> Option<&SwfToken> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<SwfToken> {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    fn eat(&mut self, expected: &SwfToken) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &SwfToken) -> Result<(), String> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(format!(
                "expected {expected:?}, found {:?} in bounded SWF compiler",
                self.peek()
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.next() {
            Some(SwfToken::Ident(s)) => Ok(s),
            other => Err(format!(
                "expected identifier, found {other:?} in bounded SWF compiler"
            )),
        }
    }

    fn skip_type_annotation(&mut self) -> Result<(), String> {
        if !self.eat(&SwfToken::Colon) {
            return Ok(());
        }
        if self.eat(&SwfToken::Star) {
            return Ok(());
        }
        let _ = self.expect_ident()?;
        while self.eat(&SwfToken::Dot) {
            if self.eat(&SwfToken::Lt) {
                let mut depth = 1usize;
                while depth > 0 {
                    match self.next() {
                        Some(SwfToken::Lt) => depth += 1,
                        Some(SwfToken::Gt) => depth -= 1,
                        Some(SwfToken::Shr) => depth = depth.saturating_sub(2),
                        Some(SwfToken::UShr) => depth = depth.saturating_sub(3),
                        Some(_) => {}
                        None => return Err("unterminated generic type annotation".to_owned()),
                    }
                }
                break;
            }
            let _ = self.expect_ident()?;
        }
        Ok(())
    }

    fn parse_stmt_list(&mut self, stop_at_rbrace: bool) -> Result<Vec<SwfStmt>, String> {
        let mut stmts = Vec::new();
        while let Some(tok) = self.peek() {
            if stop_at_rbrace && *tok == SwfToken::RBrace {
                break;
            }
            if *tok == SwfToken::Semi {
                self.pos += 1;
                continue;
            }
            self.parse_stmt_into(&mut stmts)?;
        }
        Ok(stmts)
    }

    fn parse_block_or_single(&mut self) -> Result<Vec<SwfStmt>, String> {
        if self.eat(&SwfToken::LBrace) {
            let stmts = self.parse_stmt_list(true)?;
            self.expect(&SwfToken::RBrace)?;
            Ok(stmts)
        } else {
            let mut stmts = Vec::new();
            self.parse_stmt_into(&mut stmts)?;
            Ok(stmts)
        }
    }

    fn consume_optional_semi(&mut self) {
        while self.eat(&SwfToken::Semi) {}
    }

    fn parse_stmt_into(&mut self, out: &mut Vec<SwfStmt>) -> Result<(), String> {
        while let Some(SwfToken::Ident(m)) = self.peek() {
            if matches!(
                m.as_str(),
                "public" | "private" | "protected" | "internal" | "static" | "override" | "final"
            ) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let Some(tok) = self.peek().cloned() else {
            return Ok(());
        };
        match tok {
            SwfToken::Semi => {
                self.pos += 1;
                Ok(())
            }
            SwfToken::LBrace => {
                self.pos += 1;
                let inner = self.parse_stmt_list(true)?;
                self.expect(&SwfToken::RBrace)?;
                out.push(SwfStmt::Block(inner));
                Ok(())
            }
            SwfToken::Ident(ref kw) => match kw.as_str() {
                "try" | "catch" | "finally" | "switch" | "case" | "default" | "throw" | "with"
                | "do" | "interface" | "break" | "continue" => Err(format!(
                    "ActionScript '{kw}' is not supported by the bounded SWF compiler"
                )),
                "package" | "class" => Err(format!(
                    "ActionScript '{kw}' declaration is not allowed inside a statement block"
                )),
                "import" => {
                    if !self.is_avm2 {
                        return Err("import statements are only supported in AS3".to_owned());
                    }
                    self.pos += 1;
                    let _ = self.expect_ident()?;
                    while self.eat(&SwfToken::Dot) {
                        if !self.eat(&SwfToken::Star) {
                            let _ = self.expect_ident()?;
                        }
                    }
                    self.consume_optional_semi();
                    Ok(())
                }
                "var" | "const" => {
                    self.pos += 1;
                    loop {
                        let name = self.expect_ident()?;
                        self.skip_type_annotation()?;
                        let init = if self.eat(&SwfToken::Assign) {
                            Some(self.parse_expr()?)
                        } else {
                            None
                        };
                        out.push(SwfStmt::VarDecl { name, init });
                        if !self.eat(&SwfToken::Comma) {
                            break;
                        }
                    }
                    self.consume_optional_semi();
                    Ok(())
                }
                "if" => {
                    self.pos += 1;
                    self.expect(&SwfToken::LParen)?;
                    let cond = self.parse_expr()?;
                    self.expect(&SwfToken::RParen)?;
                    let then_branch = self.parse_block_or_single()?;
                    let else_branch = if self.peek() == Some(&SwfToken::Ident("else".to_owned())) {
                        self.pos += 1;
                        Some(self.parse_block_or_single()?)
                    } else {
                        None
                    };
                    out.push(SwfStmt::If {
                        cond,
                        then_branch,
                        else_branch,
                    });
                    Ok(())
                }
                "while" => {
                    self.pos += 1;
                    self.expect(&SwfToken::LParen)?;
                    let cond = self.parse_expr()?;
                    self.expect(&SwfToken::RParen)?;
                    let body = self.parse_block_or_single()?;
                    out.push(SwfStmt::While { cond, body });
                    Ok(())
                }
                "for" => {
                    self.pos += 1;
                    if self.peek() == Some(&SwfToken::Ident("each".to_owned())) {
                        return Err(
                            "for each..in loops are not supported by the bounded SWF compiler"
                                .to_owned(),
                        );
                    }
                    self.expect(&SwfToken::LParen)?;
                    let init = if self.eat(&SwfToken::Semi) {
                        None
                    } else {
                        let mut init_stmts = Vec::new();
                        if self.peek() == Some(&SwfToken::Ident("var".to_owned()))
                            || self.peek() == Some(&SwfToken::Ident("const".to_owned()))
                        {
                            self.pos += 1;
                            let name = self.expect_ident()?;
                            self.skip_type_annotation()?;
                            if self.peek() == Some(&SwfToken::Ident("in".to_owned())) {
                                return Err(
                                    "for..in loops are not supported by the bounded SWF compiler"
                                        .to_owned(),
                                );
                            }
                            let init_val = if self.eat(&SwfToken::Assign) {
                                Some(self.parse_expr()?)
                            } else {
                                None
                            };
                            init_stmts.push(SwfStmt::VarDecl {
                                name,
                                init: init_val,
                            });
                        } else {
                            let s = self.parse_simple_assign_or_expr_stmt()?;
                            if self.peek() == Some(&SwfToken::Ident("in".to_owned())) {
                                return Err(
                                    "for..in loops are not supported by the bounded SWF compiler"
                                        .to_owned(),
                                );
                            }
                            init_stmts.push(s);
                        }
                        self.expect(&SwfToken::Semi)?;
                        init_stmts.into_iter().next().map(Box::new)
                    };
                    let cond = if self.eat(&SwfToken::Semi) {
                        None
                    } else {
                        let c = self.parse_expr()?;
                        self.expect(&SwfToken::Semi)?;
                        Some(c)
                    };
                    let update = if self.eat(&SwfToken::RParen) {
                        None
                    } else {
                        let u = self.parse_simple_assign_or_expr_stmt()?;
                        self.expect(&SwfToken::RParen)?;
                        Some(Box::new(u))
                    };
                    let body = self.parse_block_or_single()?;
                    out.push(SwfStmt::For {
                        init,
                        cond,
                        update,
                        body,
                    });
                    Ok(())
                }
                "function" => {
                    self.pos += 1;
                    let name = self.expect_ident()?;
                    self.expect(&SwfToken::LParen)?;
                    let mut params = Vec::new();
                    if !self.eat(&SwfToken::RParen) {
                        loop {
                            let pname = self.expect_ident()?;
                            self.skip_type_annotation()?;
                            if self.eat(&SwfToken::Assign) {
                                let _ = self.parse_expr()?;
                            }
                            params.push(pname);
                            if self.eat(&SwfToken::Comma) {
                                continue;
                            }
                            self.expect(&SwfToken::RParen)?;
                            break;
                        }
                    }
                    self.skip_type_annotation()?;
                    self.expect(&SwfToken::LBrace)?;
                    let body = self.parse_stmt_list(true)?;
                    self.expect(&SwfToken::RBrace)?;
                    out.push(SwfStmt::FunctionDecl { name, params, body });
                    Ok(())
                }
                "return" => {
                    self.pos += 1;
                    if self.eat(&SwfToken::Semi)
                        || self.peek() == Some(&SwfToken::RBrace)
                        || self.peek().is_none()
                    {
                        out.push(SwfStmt::Return(None));
                    } else {
                        let expr = self.parse_expr()?;
                        self.consume_optional_semi();
                        out.push(SwfStmt::Return(Some(expr)));
                    }
                    Ok(())
                }
                "on" | "onClipEvent" => {
                    if self.is_avm2 {
                        return Err(format!(
                            "AVM1 '{kw}(...)' event handler syntax is not supported in ActionScript 3"
                        ));
                    }
                    let is_clip_event = kw == "onClipEvent";
                    self.pos += 1;
                    self.expect(&SwfToken::LParen)?;
                    let mut events = Vec::new();
                    loop {
                        let ev = self.expect_ident()?;
                        events.push(ev);
                        if !self.eat(&SwfToken::Comma) {
                            break;
                        }
                    }
                    self.expect(&SwfToken::RParen)?;
                    self.expect(&SwfToken::LBrace)?;
                    let body = self.parse_stmt_list(true)?;
                    self.expect(&SwfToken::RBrace)?;
                    out.push(SwfStmt::EventHandler {
                        is_clip_event,
                        events,
                        body,
                    });
                    Ok(())
                }
                _ => {
                    let stmt = self.parse_simple_assign_or_expr_stmt()?;
                    self.consume_optional_semi();
                    out.push(stmt);
                    Ok(())
                }
            },
            SwfToken::PlusPlus | SwfToken::MinusMinus => {
                let stmt = self.parse_simple_assign_or_expr_stmt()?;
                self.consume_optional_semi();
                out.push(stmt);
                Ok(())
            }
            other => Err(format!(
                "unexpected token {other:?} at start of statement in bounded SWF compiler"
            )),
        }
    }

    fn parse_simple_assign_or_expr_stmt(&mut self) -> Result<SwfStmt, String> {
        if self.eat(&SwfToken::PlusPlus) {
            let expr = self.parse_postfix()?;
            let target = expr_to_lvalue(expr)?;
            return Ok(SwfStmt::IncDec {
                target,
                increment: true,
            });
        }
        if self.eat(&SwfToken::MinusMinus) {
            let expr = self.parse_postfix()?;
            let target = expr_to_lvalue(expr)?;
            return Ok(SwfStmt::IncDec {
                target,
                increment: false,
            });
        }
        let lhs = self.parse_expr()?;
        if let Some(op) = match self.peek() {
            Some(SwfToken::Assign) => Some(SwfAssignOp::Assign),
            Some(SwfToken::PlusAssign) => Some(SwfAssignOp::AddAssign),
            Some(SwfToken::MinusAssign) => Some(SwfAssignOp::SubAssign),
            Some(SwfToken::StarAssign) => Some(SwfAssignOp::MulAssign),
            Some(SwfToken::SlashAssign) => Some(SwfAssignOp::DivAssign),
            Some(SwfToken::PercentAssign) => Some(SwfAssignOp::ModAssign),
            _ => None,
        } {
            self.pos += 1;
            let target = expr_to_lvalue(lhs)?;
            let value = self.parse_expr()?;
            return Ok(SwfStmt::Assign { target, op, value });
        }
        if self.eat(&SwfToken::PlusPlus) {
            let target = expr_to_lvalue(lhs)?;
            return Ok(SwfStmt::IncDec {
                target,
                increment: true,
            });
        }
        if self.eat(&SwfToken::MinusMinus) {
            let target = expr_to_lvalue(lhs)?;
            return Ok(SwfStmt::IncDec {
                target,
                increment: false,
            });
        }
        if matches!(lhs, SwfExpr::Call { .. } | SwfExpr::New { .. }) {
            Ok(SwfStmt::Expr(lhs))
        } else {
            Err(
                "standalone expression is not a function call or assignment in bounded SWF compiler"
                    .to_owned(),
            )
        }
    }

    fn parse_expr(&mut self) -> Result<SwfExpr, String> {
        self.parse_logical_or()
    }

    fn parse_logical_or(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_logical_and()?;
        while self.eat(&SwfToken::Or) {
            let rhs = self.parse_logical_and()?;
            lhs = SwfExpr::Binary {
                op: SwfBinaryOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_logical_and(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_bit_or()?;
        while self.eat(&SwfToken::And) {
            let rhs = self.parse_bit_or()?;
            lhs = SwfExpr::Binary {
                op: SwfBinaryOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_bit_or(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_bit_xor()?;
        while self.eat(&SwfToken::BitOr) {
            let rhs = self.parse_bit_xor()?;
            lhs = SwfExpr::Binary {
                op: SwfBinaryOp::BitOr,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_bit_xor(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_bit_and()?;
        while self.eat(&SwfToken::BitXor) {
            let rhs = self.parse_bit_and()?;
            lhs = SwfExpr::Binary {
                op: SwfBinaryOp::BitXor,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_bit_and(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_equality()?;
        while self.eat(&SwfToken::BitAnd) {
            let rhs = self.parse_equality()?;
            lhs = SwfExpr::Binary {
                op: SwfBinaryOp::BitAnd,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_relational()?;
        while let Some(op) = match self.peek() {
            Some(SwfToken::Eq) => Some(SwfBinaryOp::Eq),
            Some(SwfToken::Ne) => Some(SwfBinaryOp::Ne),
            Some(SwfToken::StrictEq) => Some(SwfBinaryOp::StrictEq),
            Some(SwfToken::StrictNe) => Some(SwfBinaryOp::StrictNe),
            _ => None,
        } {
            self.pos += 1;
            let rhs = self.parse_relational()?;
            lhs = SwfExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_relational(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_shift()?;
        while let Some(op) = match self.peek() {
            Some(SwfToken::Lt) => Some(SwfBinaryOp::Lt),
            Some(SwfToken::Le) => Some(SwfBinaryOp::Le),
            Some(SwfToken::Gt) => Some(SwfBinaryOp::Gt),
            Some(SwfToken::Ge) => Some(SwfBinaryOp::Ge),
            _ => None,
        } {
            self.pos += 1;
            let rhs = self.parse_shift()?;
            lhs = SwfExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_additive()?;
        while let Some(op) = match self.peek() {
            Some(SwfToken::Shl) => Some(SwfBinaryOp::Shl),
            Some(SwfToken::Shr) => Some(SwfBinaryOp::Shr),
            Some(SwfToken::UShr) => Some(SwfBinaryOp::UShr),
            _ => None,
        } {
            self.pos += 1;
            let rhs = self.parse_additive()?;
            lhs = SwfExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_multiplicative()?;
        while let Some(op) = match self.peek() {
            Some(SwfToken::Plus) => Some(SwfBinaryOp::Add),
            Some(SwfToken::Minus) => Some(SwfBinaryOp::Sub),
            _ => None,
        } {
            self.pos += 1;
            let rhs = self.parse_multiplicative()?;
            lhs = SwfExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<SwfExpr, String> {
        let mut lhs = self.parse_unary()?;
        while let Some(op) = match self.peek() {
            Some(SwfToken::Star) => Some(SwfBinaryOp::Mul),
            Some(SwfToken::Slash) => Some(SwfBinaryOp::Div),
            Some(SwfToken::Percent) => Some(SwfBinaryOp::Mod),
            _ => None,
        } {
            self.pos += 1;
            let rhs = self.parse_unary()?;
            lhs = SwfExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<SwfExpr, String> {
        if self.eat(&SwfToken::Not) {
            let expr = self.parse_unary()?;
            return Ok(SwfExpr::Unary {
                op: SwfUnaryOp::Not,
                expr: Box::new(expr),
            });
        }
        if self.eat(&SwfToken::Minus) {
            let expr = self.parse_unary()?;
            return Ok(match expr {
                SwfExpr::Int(v) if v != i32::MIN => SwfExpr::Int(-v),
                SwfExpr::Float(2_147_483_648.0) => SwfExpr::Int(i32::MIN),
                SwfExpr::Float(v) => SwfExpr::Float(-v),
                other => SwfExpr::Unary {
                    op: SwfUnaryOp::Neg,
                    expr: Box::new(other),
                },
            });
        }
        if self.eat(&SwfToken::Plus) {
            let expr = self.parse_unary()?;
            return Ok(match expr {
                SwfExpr::Int(v) => SwfExpr::Int(v),
                SwfExpr::Float(v) => SwfExpr::Float(v),
                other => SwfExpr::Unary {
                    op: SwfUnaryOp::Pos,
                    expr: Box::new(other),
                },
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<SwfExpr, String> {
        let is_new = self.peek() == Some(&SwfToken::Ident("new".to_owned()));
        if is_new {
            self.pos += 1;
        }
        let mut expr = self.parse_primary()?;
        loop {
            if self.eat(&SwfToken::Dot) {
                let prop = self.expect_ident()?;
                expr = SwfExpr::Member(Box::new(expr), prop);
            } else if self.eat(&SwfToken::LBracket) {
                let idx = self.parse_expr()?;
                self.expect(&SwfToken::RBracket)?;
                expr = SwfExpr::Index(Box::new(expr), Box::new(idx));
            } else if self.eat(&SwfToken::LParen) {
                let mut args = Vec::new();
                if !self.eat(&SwfToken::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.eat(&SwfToken::Comma) {
                            continue;
                        }
                        self.expect(&SwfToken::RParen)?;
                        break;
                    }
                }
                if is_new {
                    expr = SwfExpr::New {
                        callee: Box::new(expr),
                        args,
                    };
                    return Ok(expr);
                }
                expr = SwfExpr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else {
                break;
            }
        }
        if is_new {
            return Ok(SwfExpr::New {
                callee: Box::new(expr),
                args: Vec::new(),
            });
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<SwfExpr, String> {
        match self.next() {
            Some(SwfToken::Int(v)) => Ok(SwfExpr::Int(v)),
            Some(SwfToken::Float(v)) => Ok(SwfExpr::Float(v)),
            Some(SwfToken::Str(s)) => Ok(SwfExpr::Str(s)),
            Some(SwfToken::Ident(s)) => match s.as_str() {
                "true" => Ok(SwfExpr::Bool(true)),
                "false" => Ok(SwfExpr::Bool(false)),
                "null" => Ok(SwfExpr::Null),
                "undefined" => Ok(SwfExpr::Undefined),
                _ => Ok(SwfExpr::Ident(s)),
            },
            Some(SwfToken::LParen) => {
                let e = self.parse_expr()?;
                self.expect(&SwfToken::RParen)?;
                Ok(e)
            }
            Some(SwfToken::LBracket) => {
                let mut elems = Vec::new();
                if !self.eat(&SwfToken::RBracket) {
                    loop {
                        elems.push(self.parse_expr()?);
                        if self.eat(&SwfToken::Comma) {
                            if self.eat(&SwfToken::RBracket) {
                                break;
                            }
                            continue;
                        }
                        self.expect(&SwfToken::RBracket)?;
                        break;
                    }
                }
                Ok(SwfExpr::ArrayLit(elems))
            }
            other => Err(format!(
                "unexpected token {other:?} in expression in bounded SWF compiler"
            )),
        }
    }
}

fn expr_to_lvalue(expr: SwfExpr) -> Result<SwfLValue, String> {
    match expr {
        SwfExpr::Ident(name) => Ok(SwfLValue::Ident(name)),
        SwfExpr::Member(obj, prop) => Ok(SwfLValue::Member(obj, prop)),
        SwfExpr::Index(obj, idx) => Ok(SwfLValue::Index(obj, idx)),
        _ => Err("invalid assignment target in bounded SWF compiler".to_owned()),
    }
}

fn parse_swf_statements_strict(source: &str, is_avm2: bool) -> Result<Vec<SwfStmt>, String> {
    let tokens = tokenize_swf_script(source)?;
    let mut parser = SwfParser::new(tokens, is_avm2);
    let stmts = parser.parse_stmt_list(false)?;
    if parser.pos < parser.tokens.len() {
        return Err(format!(
            "unexpected trailing token {:?} in bounded SWF compiler",
            parser.peek()
        ));
    }
    Ok(stmts)
}

#[cfg(test)]
fn unquote_literal(arg: &str) -> Option<String> {
    let s = arg.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        let delimiter = s.chars().next()?;
        let inner = &s[1..s.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(next) = chars.next() {
                    match next {
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        other => out.push(other),
                    }
                }
            } else if c == delimiter {
                return None;
            } else {
                out.push(c);
            }
        }
        Some(out)
    } else {
        None
    }
}

fn avm1_push_string(out: &mut Vec<u8>, s: &str) -> Result<(), String> {
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
    let payload_len = bytes
        .len()
        .checked_add(2)
        .and_then(|len| u16::try_from(len).ok())
        .ok_or_else(|| {
            format!(
                "AVM1 string value encodes {} bytes; ActionPush supports at most {} bytes",
                bytes.len(),
                usize::from(u16::MAX) - 2
            )
        })?;
    out.push(0x96); // ActionPush
    out.extend_from_slice(&payload_len.to_le_bytes());
    out.push(0x00); // type 0 = String
    out.extend_from_slice(&bytes);
    out.push(0);
    Ok(())
}

fn avm1_push_int(out: &mut Vec<u8>, v: i32) {
    out.push(0x96); // ActionPush
    out.extend_from_slice(&5u16.to_le_bytes());
    out.push(0x07); // type 7 = Integer (i32)
    out.extend_from_slice(&v.to_le_bytes());
}

fn avm1_push_double(out: &mut Vec<u8>, v: f64) {
    out.push(0x96); // ActionPush
    out.extend_from_slice(&9u16.to_le_bytes());
    out.push(0x06); // type 6 = Double (SWF word-swapped f64: high 4 bytes LE, then low 4 bytes LE)
    let bits = v.to_bits();
    let hi = ((bits >> 32) as u32).to_le_bytes();
    let lo = (bits as u32).to_le_bytes();
    out.extend_from_slice(&hi);
    out.extend_from_slice(&lo);
}

fn avm1_push_bool(out: &mut Vec<u8>, v: bool) {
    out.push(0x96); // ActionPush
    out.extend_from_slice(&2u16.to_le_bytes());
    out.push(0x05); // type 5 = Boolean
    out.push(u8::from(v));
}

fn is_avm1_mc_builtin_prop(name: &str) -> bool {
    matches!(
        name,
        "_x" | "_y" | "_xscale" | "_yscale" | "_alpha" | "_visible" | "_rotation" | "_name"
    )
}

fn avm1_compile_expr(out: &mut Vec<u8>, expr: &SwfExpr) -> Result<(), String> {
    match expr {
        SwfExpr::Null => {
            out.push(0x96);
            out.extend_from_slice(&1u16.to_le_bytes());
            out.push(0x02); // Null
        }
        SwfExpr::Undefined => {
            out.push(0x96);
            out.extend_from_slice(&1u16.to_le_bytes());
            out.push(0x03); // Undefined
        }
        SwfExpr::Bool(b) => avm1_push_bool(out, *b),
        SwfExpr::Int(iv) => avm1_push_int(out, *iv),
        SwfExpr::Float(fv) => avm1_push_double(out, *fv),
        SwfExpr::Str(s) => avm1_push_string(out, s)?,
        SwfExpr::Ident(name) => {
            avm1_push_string(out, name)?;
            out.push(0x1c); // ActionGetVariable
        }
        SwfExpr::ArrayLit(elems) => {
            for el in elems.iter().rev() {
                avm1_compile_expr(out, el)?;
            }
            avm1_push_int(out, elems.len() as i32);
            out.push(0x42); // ActionInitArray
        }
        SwfExpr::Member(obj, prop) => {
            if matches!(obj.as_ref(), SwfExpr::Ident(id) if id == "this")
                && is_avm1_mc_builtin_prop(prop)
            {
                avm1_push_string(out, prop)?;
                out.push(0x1c); // ActionGetVariable
            } else {
                avm1_compile_expr(out, obj)?;
                avm1_push_string(out, prop)?;
                out.push(0x4e); // ActionGetMember
            }
        }
        SwfExpr::Index(obj, idx) => {
            avm1_compile_expr(out, obj)?;
            avm1_compile_expr(out, idx)?;
            out.push(0x4e); // ActionGetMember
        }
        SwfExpr::Unary { op, expr } => match op {
            SwfUnaryOp::Not => {
                avm1_compile_expr(out, expr)?;
                out.push(0x12); // ActionNot
            }
            SwfUnaryOp::Neg => {
                avm1_push_int(out, 0);
                avm1_compile_expr(out, expr)?;
                out.push(0x0b); // ActionSubtract
            }
            SwfUnaryOp::Pos => {
                avm1_compile_expr(out, expr)?;
                out.push(0x4a); // ActionToNumber
            }
        },
        SwfExpr::Binary { op, lhs, rhs } => {
            if matches!(op, SwfBinaryOp::And | SwfBinaryOp::Or) {
                avm1_compile_expr(out, lhs)?;
                out.push(0x12); // ActionNot
                out.push(0x12); // ActionNot: normalize lhs to Boolean
                out.push(0x4c); // ActionPushDuplicate
                if *op == SwfBinaryOp::And {
                    out.push(0x12); // ActionNot: branch when lhs is false
                }
                let mut rhs_bytes = Vec::new();
                avm1_compile_expr(&mut rhs_bytes, rhs)?;
                rhs_bytes.push(0x12); // ActionNot
                rhs_bytes.push(0x12); // ActionNot: normalize rhs to Boolean
                let skip_rhs = rhs_bytes
                    .len()
                    .checked_add(1)
                    .and_then(|len| i16::try_from(len).ok())
                    .ok_or_else(|| "AVM1 logical RHS exceeds i16 branch range".to_owned())?;
                out.push(0x9d); // ActionIf
                out.extend_from_slice(&2u16.to_le_bytes());
                out.extend_from_slice(&skip_rhs.to_le_bytes());
                out.push(0x17); // ActionPop the non-short-circuit lhs
                out.extend_from_slice(&rhs_bytes);
                return Ok(());
            }
            avm1_compile_expr(out, lhs)?;
            avm1_compile_expr(out, rhs)?;
            match op {
                SwfBinaryOp::Add => out.push(0x47), // ActionAdd2
                SwfBinaryOp::Sub => out.push(0x0b), // ActionSubtract
                SwfBinaryOp::Mul => out.push(0x0c), // ActionMultiply
                SwfBinaryOp::Div => out.push(0x0d), // ActionDivide
                SwfBinaryOp::Mod => out.push(0x53), // ActionModulo
                SwfBinaryOp::Eq => out.push(0x49),  // ActionEquals2
                SwfBinaryOp::Ne => {
                    out.push(0x49); // ActionEquals2
                    out.push(0x12); // ActionNot
                }
                SwfBinaryOp::StrictEq => out.push(0x66), // ActionStrictEquals
                SwfBinaryOp::StrictNe => {
                    out.push(0x66); // ActionStrictEquals
                    out.push(0x12); // ActionNot
                }
                SwfBinaryOp::Lt => out.push(0x48), // ActionLess2
                SwfBinaryOp::Ge => {
                    out.push(0x48); // ActionLess2
                    out.push(0x12); // ActionNot
                }
                SwfBinaryOp::Gt => out.push(0x67), // ActionGreater
                SwfBinaryOp::Le => {
                    out.push(0x67); // ActionGreater
                    out.push(0x12); // ActionNot
                }
                SwfBinaryOp::And | SwfBinaryOp::Or => unreachable!("handled above"),
                SwfBinaryOp::BitAnd => out.push(0x60), // ActionBitAnd
                SwfBinaryOp::Shl => out.push(0x61),    // ActionBitLShift
                SwfBinaryOp::BitOr => out.push(0x62),  // ActionBitOr
                SwfBinaryOp::Shr => out.push(0x63),    // ActionBitRShift
                SwfBinaryOp::UShr => out.push(0x64),   // ActionBitURShift
                SwfBinaryOp::BitXor => out.push(0x65), // ActionBitXor
            }
        }
        SwfExpr::Call { callee, args } => {
            avm1_compile_call(out, callee, args, false)?;
        }
        SwfExpr::New { callee, args } => {
            for a in args.iter().rev() {
                avm1_compile_expr(out, a)?;
            }
            avm1_push_int(out, args.len() as i32);
            match callee.as_ref() {
                SwfExpr::Ident(name) => avm1_push_string(out, name)?,
                SwfExpr::Member(_, prop) => avm1_push_string(out, prop)?,
                _ => return Err("unsupported 'new' constructor expression in AVM1".to_owned()),
            }
            out.push(0x40); // ActionNewObject
        }
    }
    Ok(())
}

fn avm1_compile_call(
    out: &mut Vec<u8>,
    callee: &SwfExpr,
    args: &[SwfExpr],
    discard_result: bool,
) -> Result<(), String> {
    let direct_fn = match callee {
        SwfExpr::Ident(name) => Some(name.as_str()),
        SwfExpr::Member(recv, prop) if matches!(recv.as_ref(), SwfExpr::Ident(id) if id == "this") => {
            Some(prop.as_str())
        }
        _ => None,
    };

    if let Some(short_fn) = direct_fn {
        match short_fn {
            "stop" if args.is_empty() && discard_result => {
                out.push(0x07); // ActionStop
                return Ok(());
            }
            "play" if args.is_empty() && discard_result => {
                out.push(0x06); // ActionPlay
                return Ok(());
            }
            "nextFrame" if args.is_empty() && discard_result => {
                out.push(0x04); // ActionNextFrame
                return Ok(());
            }
            "prevFrame" if args.is_empty() && discard_result => {
                out.push(0x05); // ActionPrevFrame
                return Ok(());
            }
            "stopAllSounds" if args.is_empty() && discard_result => {
                out.push(0x09); // ActionStopSounds
                return Ok(());
            }
            "gotoAndStop" | "gotoAndPlay" if !args.is_empty() && discard_result => {
                let is_play = short_fn == "gotoAndPlay";
                let target_arg = args.last().unwrap();
                match target_arg {
                    SwfExpr::Int(frame_i32) => {
                        let frame_1based = u32::try_from(*frame_i32).map_err(|_| {
                            format!(
                                "AVM1 numeric {short_fn} target {frame_i32} must be a one-based frame"
                            )
                        })?;
                        if frame_1based == 0 {
                            return Err(format!(
                                "AVM1 numeric {short_fn} target 0 must be a one-based frame"
                            ));
                        }
                        let frame_0 =
                            u16::try_from(frame_1based - 1).map_err(|_| {
                                format!(
                                    "AVM1 numeric {short_fn} target {frame_1based} exceeds the ActionGotoFrame limit of 65536 one-based frames"
                                )
                            })?;
                        out.push(0x81); // ActionGotoFrame
                        out.extend_from_slice(&2u16.to_le_bytes());
                        out.extend_from_slice(&frame_0.to_le_bytes());
                        if is_play {
                            out.push(0x06); // ActionPlay
                        }
                    }
                    SwfExpr::Float(fv) if fv.fract() == 0.0 => {
                        if !(1.0..=65536.0).contains(fv) {
                            return Err(format!(
                                "AVM1 numeric {short_fn} target {fv} must be within 1..=65536 one-based frames"
                            ));
                        }
                        let frame_0 = ((*fv as u32) - 1) as u16;
                        out.push(0x81); // ActionGotoFrame
                        out.extend_from_slice(&2u16.to_le_bytes());
                        out.extend_from_slice(&frame_0.to_le_bytes());
                        if is_play {
                            out.push(0x06); // ActionPlay
                        }
                    }
                    SwfExpr::Str(lbl) => {
                        if !is_play {
                            let lbl_bytes: Vec<u8> = lbl.bytes().filter(|&b| b != 0).collect();
                            let len = lbl_bytes
                                .len()
                                .checked_add(1)
                                .and_then(|len| u16::try_from(len).ok())
                                .ok_or_else(|| {
                                    format!(
                                        "AVM1 frame label encodes {} bytes; ActionGotoLabel supports at most {} bytes",
                                        lbl_bytes.len(),
                                        usize::from(u16::MAX) - 1
                                    )
                                })?;
                            out.push(0x8c); // ActionGotoLabel
                            out.extend_from_slice(&len.to_le_bytes());
                            out.extend_from_slice(&lbl_bytes);
                            out.push(0);
                        } else {
                            avm1_push_string(out, lbl)?;
                            out.push(0x9f); // ActionGotoFrame2
                            out.extend_from_slice(&1u16.to_le_bytes());
                            out.push(0x01); // play = true
                        }
                    }
                    SwfExpr::Unary {
                        op: SwfUnaryOp::Neg,
                        expr,
                    } if matches!(expr.as_ref(), SwfExpr::Int(_) | SwfExpr::Float(_)) => {
                        return Err(format!(
                            "AVM1 numeric {short_fn} target must be a positive one-based frame"
                        ));
                    }
                    other => {
                        avm1_compile_expr(out, other)?;
                        out.push(0x9f); // ActionGotoFrame2
                        out.extend_from_slice(&1u16.to_le_bytes());
                        out.push(u8::from(is_play));
                    }
                }
                return Ok(());
            }
            "trace" if discard_result => {
                if let Some(first) = args.first() {
                    avm1_compile_expr(out, first)?;
                } else {
                    avm1_push_string(out, "")?;
                }
                out.push(0x26); // ActionTrace
                return Ok(());
            }
            _ => {
                for a in args.iter().rev() {
                    avm1_compile_expr(out, a)?;
                }
                avm1_push_int(out, args.len() as i32);
                avm1_push_string(out, short_fn)?;
                out.push(0x3d); // ActionCallFunction
                if discard_result {
                    out.push(0x17); // ActionPop
                }
                return Ok(());
            }
        }
    }

    if let SwfExpr::Member(recv, method_name) = callee {
        for a in args.iter().rev() {
            avm1_compile_expr(out, a)?;
        }
        avm1_push_int(out, args.len() as i32);
        avm1_compile_expr(out, recv)?;
        avm1_push_string(out, method_name)?;
        out.push(0x52); // ActionCallMethod
        if discard_result {
            out.push(0x17); // ActionPop
        }
        return Ok(());
    }

    Err("unsupported function call target in AVM1 compiler".to_owned())
}

fn avm1_load_lvalue(out: &mut Vec<u8>, target: &SwfLValue) -> Result<(), String> {
    match target {
        SwfLValue::Ident(name) => {
            avm1_push_string(out, name)?;
            out.push(0x1c); // ActionGetVariable
        }
        SwfLValue::Member(obj, prop) => {
            if matches!(obj.as_ref(), SwfExpr::Ident(id) if id == "this")
                && is_avm1_mc_builtin_prop(prop)
            {
                avm1_push_string(out, prop)?;
                out.push(0x1c); // ActionGetVariable
            } else {
                avm1_compile_expr(out, obj)?;
                avm1_push_string(out, prop)?;
                out.push(0x4e); // ActionGetMember
            }
        }
        SwfLValue::Index(obj, idx) => {
            avm1_compile_expr(out, obj)?;
            avm1_compile_expr(out, idx)?;
            out.push(0x4e); // ActionGetMember
        }
    }
    Ok(())
}

fn avm1_store_lvalue_with_rhs<F>(
    out: &mut Vec<u8>,
    target: &SwfLValue,
    emit_rhs: F,
) -> Result<(), String>
where
    F: FnOnce(&mut Vec<u8>) -> Result<(), String>,
{
    match target {
        SwfLValue::Ident(name) => {
            avm1_push_string(out, name)?;
            emit_rhs(out)?;
            out.push(0x1d); // ActionSetVariable
            if is_avm1_mc_builtin_prop(name) {
                avm1_push_string(out, "this")?;
                out.push(0x1c); // ActionGetVariable
                avm1_push_string(out, name)?;
                avm1_push_string(out, name)?;
                out.push(0x1c); // ActionGetVariable
                out.push(0x4f); // ActionSetMember
            }
        }
        SwfLValue::Member(obj, prop) => {
            if matches!(obj.as_ref(), SwfExpr::Ident(id) if id == "this")
                && is_avm1_mc_builtin_prop(prop)
            {
                avm1_push_string(out, prop)?;
                emit_rhs(out)?;
                out.push(0x1d); // ActionSetVariable
                avm1_push_string(out, "this")?;
                out.push(0x1c); // ActionGetVariable
                avm1_push_string(out, prop)?;
                avm1_push_string(out, prop)?;
                out.push(0x1c); // ActionGetVariable
                out.push(0x4f); // ActionSetMember
            } else {
                avm1_compile_expr(out, obj)?;
                avm1_push_string(out, prop)?;
                emit_rhs(out)?;
                out.push(0x4f); // ActionSetMember
            }
        }
        SwfLValue::Index(obj, idx) => {
            avm1_compile_expr(out, obj)?;
            avm1_compile_expr(out, idx)?;
            emit_rhs(out)?;
            out.push(0x4f); // ActionSetMember
        }
    }
    Ok(())
}

fn avm1_compile_stmt_list(
    out: &mut Vec<u8>,
    stmts: &[SwfStmt],
    in_function: bool,
) -> Result<(), String> {
    for stmt in stmts {
        avm1_compile_stmt(out, stmt, in_function)?;
    }
    Ok(())
}

fn avm1_compile_stmt(out: &mut Vec<u8>, stmt: &SwfStmt, in_function: bool) -> Result<(), String> {
    match stmt {
        SwfStmt::VarDecl { name, init } => {
            if let Some(rhs) = init {
                avm1_push_string(out, name)?;
                avm1_compile_expr(out, rhs)?;
                out.push(if in_function { 0x3c } else { 0x1d });
            } else if in_function {
                avm1_push_string(out, name)?;
                out.push(0x41); // ActionDefineLocal2
            } else {
                avm1_push_string(out, name)?;
                out.push(0x96);
                out.extend_from_slice(&1u16.to_le_bytes());
                out.push(0x03); // Undefined
                out.push(0x1d); // ActionSetVariable
            }
        }
        SwfStmt::Assign { target, op, value } => {
            avm1_store_lvalue_with_rhs(out, target, |buf| {
                if *op == SwfAssignOp::Assign {
                    avm1_compile_expr(buf, value)
                } else {
                    avm1_load_lvalue(buf, target)?;
                    avm1_compile_expr(buf, value)?;
                    buf.push(match op {
                        SwfAssignOp::AddAssign => 0x47,
                        SwfAssignOp::SubAssign => 0x0b,
                        SwfAssignOp::MulAssign => 0x0c,
                        SwfAssignOp::DivAssign => 0x0d,
                        SwfAssignOp::ModAssign => 0x53,
                        SwfAssignOp::Assign => unreachable!(),
                    });
                    Ok(())
                }
            })?;
        }
        SwfStmt::IncDec { target, increment } => {
            avm1_store_lvalue_with_rhs(out, target, |buf| {
                avm1_load_lvalue(buf, target)?;
                buf.push(if *increment { 0x50 } else { 0x51 }); // ActionIncrement / ActionDecrement
                Ok(())
            })?;
        }
        SwfStmt::Expr(SwfExpr::Call { callee, args }) => {
            avm1_compile_call(out, callee, args, true)?;
        }
        SwfStmt::Expr(expr) => {
            avm1_compile_expr(out, expr)?;
            out.push(0x17); // ActionPop
        }
        SwfStmt::Block(inner) => {
            avm1_compile_stmt_list(out, inner, in_function)?;
        }
        SwfStmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            avm1_compile_expr(out, cond)?;
            out.push(0x12); // ActionNot: jump if cond is false
            let mut then_bytes = Vec::new();
            avm1_compile_stmt_list(&mut then_bytes, then_branch, in_function)?;
            if let Some(else_stmts) = else_branch
                && !else_stmts.is_empty()
            {
                let mut else_bytes = Vec::new();
                avm1_compile_stmt_list(&mut else_bytes, else_stmts, in_function)?;
                let else_off = i16::try_from(else_bytes.len())
                    .map_err(|_| "AVM1 else branch exceeds i16 jump offset limit".to_owned())?;
                then_bytes.push(0x99); // ActionJump
                then_bytes.extend_from_slice(&2u16.to_le_bytes());
                then_bytes.extend_from_slice(&else_off.to_le_bytes());

                let then_off = i16::try_from(then_bytes.len())
                    .map_err(|_| "AVM1 then branch exceeds i16 jump offset limit".to_owned())?;
                out.push(0x9d); // ActionIf
                out.extend_from_slice(&2u16.to_le_bytes());
                out.extend_from_slice(&then_off.to_le_bytes());
                out.extend_from_slice(&then_bytes);
                out.extend_from_slice(&else_bytes);
            } else {
                let then_off = i16::try_from(then_bytes.len())
                    .map_err(|_| "AVM1 then branch exceeds i16 jump offset limit".to_owned())?;
                out.push(0x9d); // ActionIf
                out.extend_from_slice(&2u16.to_le_bytes());
                out.extend_from_slice(&then_off.to_le_bytes());
                out.extend_from_slice(&then_bytes);
            }
        }
        SwfStmt::While { cond, body } => {
            let loop_start = out.len();
            avm1_compile_expr(out, cond)?;
            out.push(0x12); // ActionNot
            let mut body_bytes = Vec::new();
            avm1_compile_stmt_list(&mut body_bytes, body, in_function)?;
            let skip_off = i16::try_from(body_bytes.len() + 5)
                .map_err(|_| "AVM1 while loop body exceeds i16 jump offset limit".to_owned())?;
            out.push(0x9d); // ActionIf
            out.extend_from_slice(&2u16.to_le_bytes());
            out.extend_from_slice(&skip_off.to_le_bytes());
            out.extend_from_slice(&body_bytes);
            let back_off = i16::try_from((loop_start as isize) - ((out.len() + 5) as isize))
                .map_err(|_| "AVM1 while loop backward jump exceeds i16 offset limit".to_owned())?;
            out.push(0x99); // ActionJump
            out.extend_from_slice(&2u16.to_le_bytes());
            out.extend_from_slice(&back_off.to_le_bytes());
        }
        SwfStmt::For {
            init,
            cond,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                avm1_compile_stmt(out, init_stmt, in_function)?;
            }
            let loop_cond = cond.clone().unwrap_or(SwfExpr::Bool(true));
            let mut combined_body = body.clone();
            if let Some(upd_stmt) = update {
                combined_body.push((**upd_stmt).clone());
            }
            let while_stmt = SwfStmt::While {
                cond: loop_cond,
                body: combined_body,
            };
            avm1_compile_stmt(out, &while_stmt, in_function)?;
        }
        SwfStmt::FunctionDecl { name, params, body } => {
            let mut body_bytes = Vec::new();
            avm1_compile_stmt_list(&mut body_bytes, body, true)?;
            let code_size = u16::try_from(body_bytes.len())
                .map_err(|_| "AVM1 function body exceeds 65535 bytes".to_owned())?;
            let mut header = Vec::new();
            header.extend(name.bytes().filter(|&b| b != 0));
            header.push(0);
            let num_params = u16::try_from(params.len())
                .map_err(|_| "AVM1 function has too many parameters".to_owned())?;
            header.extend_from_slice(&num_params.to_le_bytes());
            for p in params {
                header.extend(p.bytes().filter(|&b| b != 0));
                header.push(0);
            }
            header.extend_from_slice(&code_size.to_le_bytes());
            let header_len = u16::try_from(header.len())
                .map_err(|_| "AVM1 function header exceeds 65535 bytes".to_owned())?;
            out.push(0x9b); // ActionDefineFunction
            out.extend_from_slice(&header_len.to_le_bytes());
            out.extend_from_slice(&header);
            out.extend_from_slice(&body_bytes);
        }
        SwfStmt::Return(expr_opt) => {
            if let Some(expr) = expr_opt {
                avm1_compile_expr(out, expr)?;
            } else {
                out.push(0x96);
                out.extend_from_slice(&1u16.to_le_bytes());
                out.push(0x03); // Undefined
            }
            out.push(0x3e); // ActionReturn
        }
        SwfStmt::EventHandler { body, .. } => {
            avm1_compile_stmt_list(out, body, in_function)?;
        }
    }
    Ok(())
}

/// Compile an AS1/AS2 script source into AVM1 action bytecode (terminated by `0x00`).
pub(crate) fn compile_avm1_actions(source: &str) -> Result<Vec<u8>, String> {
    let stmts = parse_swf_statements_strict(source, false)?;
    let mut out = Vec::new();
    avm1_compile_stmt_list(&mut out, &stmts, false)?;
    out.push(0x00); // ActionEndFlag
    Ok(out)
}

/// Encode `BUTTONCONDACTION` records for a `DefineButton2` (tag 34).
fn encode_button2_cond_actions(scripts: &[ObjectScript]) -> Result<Vec<u8>, String> {
    let mut valid: Vec<(u8, u8, Vec<u8>)> = Vec::new();
    for os in scripts {
        if !matches!(
            os.script.language,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        ) {
            continue;
        }
        let actions = compile_avm1_actions(&os.script.source)?;
        if actions.len() <= 1 {
            continue;
        }
        // OverDownToOverUp (release) = 0x08 in cond_b0
        let (b0, b1): (u8, u8) = match os.event {
            ObjectEvent::Click => (0x08, 0x00),
            ObjectEvent::EnterFrame | ObjectEvent::Load => (0x04, 0x00),
        };
        valid.push((b0, b1, actions));
    }
    let mut out = Vec::new();
    let count = valid.len();
    for (idx, (b0, b1, actions)) in valid.into_iter().enumerate() {
        let cond_size: u16 = if idx + 1 == count {
            0
        } else {
            actions
                .len()
                .checked_add(4)
                .and_then(|len| u16::try_from(len).ok())
                .ok_or_else(|| {
                    format!(
                        "AVM1 button conditional action encodes {} bytes; non-final BUTTONCONDACTION records support at most {} action bytes",
                        actions.len(),
                        usize::from(u16::MAX) - 4
                    )
                })?
        };
        out.extend_from_slice(&cond_size.to_le_bytes());
        out.push(b0);
        out.push(b1);
        out.extend_from_slice(&actions);
    }
    Ok(out)
}

/// Encode `CLIPACTIONS` for a `PlaceObject2` (tag 26) or `PlaceObject3` (tag 70) in SWF 6+.
fn encode_clip_actions(scripts: &[ObjectScript]) -> Option<Vec<u8>> {
    let mut records: Vec<(u32, Option<u8>, Vec<u8>)> = Vec::new();
    let mut all_flags = 0u32;
    for os in scripts {
        if !matches!(
            os.script.language,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        ) {
            continue;
        }
        let actions = compile_avm1_actions(&os.script.source)
            .expect("AVM1 action record sizes are checked by SWF preflight");
        if actions.len() <= 1 {
            continue;
        }
        let (flag, key): (u32, Option<u8>) = match os.event {
            ObjectEvent::Load => (0x0000_0080, None), // ClipEventLoad
            ObjectEvent::EnterFrame => (0x0000_0040, None), // ClipEventEnterFrame
            // NeoFlash's Click event is dispatched after a completed pointer activation, which
            // corresponds to ClipEventRelease rather than Press/RollOver in SWF CLIPACTIONS.
            ObjectEvent::Click => (0x0000_1000, None), // ClipEventRelease
        };
        all_flags |= flag;
        records.push((flag, key, actions));
    }
    if records.is_empty() {
        return None;
    }
    let mut w = SwfBitWriter::new();
    w.u16(0); // reserved
    w.u32(all_flags);
    for (flag, key, actions) in records {
        w.u32(flag);
        let record_size = actions.len() + usize::from(key.is_some());
        w.u32(record_size as u32);
        if let Some(k) = key {
            w.u8(k);
        }
        w.bytes(&actions);
    }
    w.u32(0); // end of clip action records
    Some(w.buf)
}

#[derive(Default)]
struct AbcPoolBuilder {
    ints: Vec<i32>,
    int_map: HashMap<i32, u32>,
    uints: Vec<u32>,
    doubles: Vec<f64>,
    strings: Vec<String>,
    string_map: HashMap<String, u32>,
    namespaces: Vec<(u8, u32)>,
    ns_map: HashMap<(u8, u32), u32>,
    multinames: Vec<(u8, u32, u32)>,
    mn_map: HashMap<(u8, u32, u32), u32>,
}

impl AbcPoolBuilder {
    fn add_int(&mut self, value: i32) -> u32 {
        if let Some(&index) = self.int_map.get(&value) {
            return index;
        }
        let index = (self.ints.len() + 1) as u32;
        self.ints.push(value);
        self.int_map.insert(value, index);
        index
    }

    fn add_double(&mut self, value: f64) -> u32 {
        let bits = value.to_bits();
        if let Some(pos) = self.doubles.iter().position(|d| d.to_bits() == bits) {
            return (pos + 1) as u32;
        }
        let index = (self.doubles.len() + 1) as u32;
        self.doubles.push(value);
        index
    }

    fn add_string(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.string_map.get(s) {
            return idx;
        }
        let idx = (self.strings.len() + 1) as u32;
        self.strings.push(s.to_owned());
        self.string_map.insert(s.to_owned(), idx);
        idx
    }

    fn add_namespace(&mut self, kind: u8, name: &str) -> u32 {
        let s_idx = self.add_string(name);
        if let Some(&idx) = self.ns_map.get(&(kind, s_idx)) {
            return idx;
        }
        let idx = (self.namespaces.len() + 1) as u32;
        self.namespaces.push((kind, s_idx));
        self.ns_map.insert((kind, s_idx), idx);
        idx
    }

    fn add_qname(&mut self, pkg: &str, name: &str) -> u32 {
        let ns_idx = self.add_namespace(0x16, pkg); // 0x16 = PackageNamespace
        let name_idx = self.add_string(name);
        let key = (0x07, ns_idx, name_idx); // 0x07 = QName
        if let Some(&idx) = self.mn_map.get(&key) {
            return idx;
        }
        let idx = (self.multinames.len() + 1) as u32;
        self.multinames.push(key);
        self.mn_map.insert(key, idx);
        idx
    }
}

fn emit_avm2_int(pool: &mut AbcPoolBuilder, code: &mut SwfBitWriter, value: i32) {
    if let Ok(short) = i16::try_from(value) {
        code.u8(0x25); // pushshort; the operand is interpreted through its low signed 16 bits
        code.u32_v(u32::from(short as u16));
    } else {
        code.u8(0x2D); // pushint
        code.u32_v(pool.add_int(value));
    }
}

fn emit_avm2_s24(code: &mut SwfBitWriter, offset: i32) -> Result<(), String> {
    if !(-0x80_0000..=0x7F_FFFF).contains(&offset) {
        return Err("AVM2 branch offset exceeds 24-bit signed range".to_owned());
    }
    let bytes = offset.to_le_bytes();
    code.u8(bytes[0]);
    code.u8(bytes[1]);
    code.u8(bytes[2]);
    Ok(())
}

struct AbcClassSpec {
    pkg: String,
    class_name: String,
    super_pkg: String,
    super_name: String,
    ctor_statements: Vec<SwfStmt>,
    frame_scripts: Vec<(u32, String)>,
    extra_methods: Vec<(String, Vec<String>, Vec<SwfStmt>)>,
}

fn split_qualified_class(qname: &str) -> (String, String) {
    match qname.rsplit_once('.') {
        Some((pkg, cls)) => (pkg.to_owned(), cls.to_owned()),
        None => (String::new(), qname.to_owned()),
    }
}

fn parse_dotted_name(parser: &mut SwfParser) -> Result<String, String> {
    let mut name = parser.expect_ident()?;
    while parser.eat(&SwfToken::Dot) {
        let next = parser.expect_ident()?;
        name.push('.');
        name.push_str(&next);
    }
    Ok(name)
}

fn parse_as3_source_to_spec_strict(
    source: &str,
    fallback_qname: &str,
    external_source: bool,
) -> Result<AbcClassSpec, String> {
    let tokens = tokenize_swf_script(source)?;
    let mut parser = SwfParser::new(tokens, true);

    let starts_with_package = parser.peek() == Some(&SwfToken::Ident("package".to_owned()));
    let has_class_tok = parser
        .tokens
        .iter()
        .any(|t| *t == SwfToken::Ident("class".to_owned()));

    if !starts_with_package && !has_class_tok {
        if external_source {
            return Err(
                "external ActionScript 3 source file must declare a package and class".to_owned(),
            );
        }
        let stmts = parser.parse_stmt_list(false)?;
        if parser.pos < parser.tokens.len() {
            return Err(format!(
                "unexpected trailing token {:?} in ActionScript 3 script",
                parser.peek()
            ));
        }
        let (pkg, fc) = split_qualified_class(fallback_qname);
        return Ok(AbcClassSpec {
            pkg,
            class_name: if fc.is_empty() { "Main".to_owned() } else { fc },
            super_pkg: "flash.display".to_owned(),
            super_name: "MovieClip".to_owned(),
            ctor_statements: stmts,
            frame_scripts: Vec::new(),
            extra_methods: Vec::new(),
        });
    }

    let mut pkg = String::new();
    let mut has_package_brace = false;
    if starts_with_package {
        parser.pos += 1;
        if parser.peek() != Some(&SwfToken::LBrace) {
            pkg = parse_dotted_name(&mut parser)?;
        }
        parser.expect(&SwfToken::LBrace)?;
        has_package_brace = true;
    }

    // Consume package-level imports and [SWF(...)] metadata
    loop {
        if parser.eat(&SwfToken::Semi) {
            continue;
        }
        if parser.eat(&SwfToken::LBracket) {
            let mut depth = 1usize;
            while depth > 0 {
                match parser.next() {
                    Some(SwfToken::LBracket) => depth += 1,
                    Some(SwfToken::RBracket) => depth -= 1,
                    Some(_) => {}
                    None => return Err("unterminated metadata tag in AS3 source".to_owned()),
                }
            }
            continue;
        }
        if parser.peek() == Some(&SwfToken::Ident("import".to_owned())) {
            parser.pos += 1;
            let _ = parser.expect_ident()?;
            while parser.eat(&SwfToken::Dot) {
                if !parser.eat(&SwfToken::Star) {
                    let _ = parser.expect_ident()?;
                }
            }
            parser.consume_optional_semi();
            continue;
        }
        break;
    }

    // Consume class modifiers
    while let Some(SwfToken::Ident(m)) = parser.peek() {
        if matches!(m.as_str(), "public" | "internal" | "final" | "dynamic") {
            parser.pos += 1;
        } else {
            break;
        }
    }

    if parser.next() != Some(SwfToken::Ident("class".to_owned())) {
        return Err("expected 'class' declaration in ActionScript 3 source".to_owned());
    }
    let class_name = parser.expect_ident()?;
    let mut super_pkg = "flash.display".to_owned();
    let mut super_name = "MovieClip".to_owned();

    if parser.peek() == Some(&SwfToken::Ident("extends".to_owned())) {
        parser.pos += 1;
        let sup = parse_dotted_name(&mut parser)?;
        if sup.contains('.') {
            let (sp, sc) = split_qualified_class(&sup);
            super_pkg = sp;
            super_name = sc;
        } else {
            super_name = sup;
        }
    }

    if parser.peek() == Some(&SwfToken::Ident("implements".to_owned())) {
        parser.pos += 1;
        loop {
            let _ = parse_dotted_name(&mut parser)?;
            if !parser.eat(&SwfToken::Comma) {
                break;
            }
        }
    }

    parser.expect(&SwfToken::LBrace)?;

    let mut ctor_statements = Vec::new();
    let mut extra_methods = Vec::new();

    while parser.peek().is_some() && parser.peek() != Some(&SwfToken::RBrace) {
        if parser.eat(&SwfToken::Semi) {
            continue;
        }
        while let Some(SwfToken::Ident(m)) = parser.peek() {
            if matches!(
                m.as_str(),
                "public" | "private" | "protected" | "internal" | "static" | "override" | "final"
            ) {
                parser.pos += 1;
            } else {
                break;
            }
        }
        if parser.peek() == Some(&SwfToken::Ident("function".to_owned())) {
            parser.pos += 1;
            let fn_name = parser.expect_ident()?;
            parser.expect(&SwfToken::LParen)?;
            let mut params = Vec::new();
            if !parser.eat(&SwfToken::RParen) {
                loop {
                    let pname = parser.expect_ident()?;
                    parser.skip_type_annotation()?;
                    if parser.eat(&SwfToken::Assign) {
                        let _ = parser.parse_expr()?;
                    }
                    params.push(pname);
                    if parser.eat(&SwfToken::Comma) {
                        continue;
                    }
                    parser.expect(&SwfToken::RParen)?;
                    break;
                }
            }
            parser.skip_type_annotation()?;
            parser.expect(&SwfToken::LBrace)?;
            let body = parser.parse_stmt_list(true)?;
            parser.expect(&SwfToken::RBrace)?;
            if fn_name == class_name {
                for s in body {
                    if let SwfStmt::Expr(SwfExpr::Call { callee, .. }) = &s
                        && matches!(callee.as_ref(), SwfExpr::Ident(id) if id == "super")
                    {
                        continue;
                    }
                    ctor_statements.push(s);
                }
            } else {
                extra_methods.push((fn_name, params, body));
            }
        } else {
            parser.parse_stmt_into(&mut ctor_statements)?;
        }
    }

    parser.expect(&SwfToken::RBrace)?;
    if has_package_brace {
        while parser.eat(&SwfToken::Semi) {}
        parser.expect(&SwfToken::RBrace)?;
    }
    while parser.eat(&SwfToken::Semi) {}
    if parser.pos < parser.tokens.len() {
        return Err(format!(
            "unexpected trailing token {:?} after ActionScript 3 class",
            parser.peek()
        ));
    }

    if extra_methods.is_empty() {
        extra_methods.push(("init".to_owned(), Vec::new(), Vec::new()));
    }

    Ok(AbcClassSpec {
        pkg,
        class_name,
        super_pkg,
        super_name,
        ctor_statements,
        frame_scripts: Vec::new(),
        extra_methods,
    })
}

fn parse_as3_source_to_spec(source: &str, fallback_qname: &str) -> AbcClassSpec {
    parse_as3_source_to_spec_strict(source, fallback_qname, true).unwrap_or_else(|_| {
        let (pkg, fc) = split_qualified_class(fallback_qname);
        AbcClassSpec {
            pkg,
            class_name: if fc.is_empty() { "Main".to_owned() } else { fc },
            super_pkg: "flash.display".to_owned(),
            super_name: "MovieClip".to_owned(),
            ctor_statements: Vec::new(),
            frame_scripts: Vec::new(),
            extra_methods: vec![("init".to_owned(), Vec::new(), Vec::new())],
        }
    })
}

fn try_compile_avm2_source(source: &str, external_source: bool) -> Result<AbcClassSpec, String> {
    let spec = parse_as3_source_to_spec_strict(source, "Main", external_source)?;
    let mut pool = AbcPoolBuilder::default();
    let mut code = SwfBitWriter::new();
    let mut method_count = 1u32;
    let mut method_params = vec![0u32];
    let mut method_bodies = Vec::new();
    compile_avm2_stmt_list(
        &mut pool,
        &spec.ctor_statements,
        &mut code,
        &mut method_count,
        &mut method_params,
        &mut method_bodies,
    )?;
    for (_, _, m_stmts) in &spec.extra_methods {
        let mut m_code = SwfBitWriter::new();
        compile_avm2_stmt_list(
            &mut pool,
            m_stmts,
            &mut m_code,
            &mut method_count,
            &mut method_params,
            &mut method_bodies,
        )?;
    }
    Ok(spec)
}

fn is_avm2_mc_display_prop(name: &str) -> bool {
    matches!(
        name,
        "x" | "y" | "scaleX" | "scaleY" | "alpha" | "visible" | "rotation"
    )
}

type AbcMethodBodyRecord = (u32, u32, u32, u32, u32, Vec<u8>);

#[allow(clippy::too_many_arguments)]
fn compile_avm2_expr(
    pool: &mut AbcPoolBuilder,
    expr: &SwfExpr,
    code: &mut SwfBitWriter,
    method_count: &mut u32,
    method_params: &mut Vec<u32>,
    method_bodies: &mut Vec<AbcMethodBodyRecord>,
) -> Result<(), String> {
    match expr {
        SwfExpr::Null => code.u8(0x20),        // pushnull
        SwfExpr::Undefined => code.u8(0x21),   // pushundefined
        SwfExpr::Bool(true) => code.u8(0x26),  // pushtrue
        SwfExpr::Bool(false) => code.u8(0x27), // pushfalse
        SwfExpr::Int(iv) => emit_avm2_int(pool, code, *iv),
        SwfExpr::Float(fv) => {
            let d_idx = pool.add_double(*fv);
            code.u8(0x2F); // pushdouble
            code.u32_v(d_idx);
        }
        SwfExpr::Str(s) => {
            let s_idx = pool.add_string(s);
            code.u8(0x2C); // pushstring
            code.u32_v(s_idx);
        }
        SwfExpr::Ident(name) => {
            if name == "this" || name == "_root" {
                code.u8(0xD0); // getlocal_0
            } else {
                let mn = pool.add_qname("", name);
                code.u8(0x60); // getlex
                code.u32_v(mn);
            }
        }
        SwfExpr::ArrayLit(elems) => {
            for el in elems {
                compile_avm2_expr(pool, el, code, method_count, method_params, method_bodies)?;
            }
            code.u8(0x56); // newarray
            code.u32_v(elems.len() as u32);
        }
        SwfExpr::Member(obj, prop) => {
            compile_avm2_expr(pool, obj, code, method_count, method_params, method_bodies)?;
            let mn = if let SwfExpr::Ident(id) = obj.as_ref()
                && id != "this"
                && id != "_root"
                && is_avm2_mc_display_prop(prop)
            {
                pool.add_qname("", &format!("__nf_mc_{id}_{prop}"))
            } else {
                pool.add_qname("", prop)
            };
            code.u8(0x66); // getproperty
            code.u32_v(mn);
        }
        SwfExpr::Index(obj, idx) => {
            if let SwfExpr::Int(iv) = idx.as_ref() {
                compile_avm2_expr(pool, obj, code, method_count, method_params, method_bodies)?;
                let mn = pool.add_qname("", &iv.to_string());
                code.u8(0x66); // getproperty
                code.u32_v(mn);
            } else if let SwfExpr::Str(s) = idx.as_ref() {
                compile_avm2_expr(pool, obj, code, method_count, method_params, method_bodies)?;
                let mn = pool.add_qname("", s);
                code.u8(0x66); // getproperty
                code.u32_v(mn);
            } else {
                return Err(
                    "dynamic non-literal bracket indexing in AS3 requires MultinameL not in bounded SWF subset"
                        .to_owned(),
                );
            }
        }
        SwfExpr::Unary { op, expr } => {
            compile_avm2_expr(pool, expr, code, method_count, method_params, method_bodies)?;
            match op {
                SwfUnaryOp::Not => code.u8(0x96), // not
                SwfUnaryOp::Neg => code.u8(0x90), // negate
                SwfUnaryOp::Pos => code.u8(0x75), // convert_d
            }
        }
        SwfExpr::Binary { op, lhs, rhs } => {
            if matches!(op, SwfBinaryOp::And | SwfBinaryOp::Or) {
                compile_avm2_expr(pool, lhs, code, method_count, method_params, method_bodies)?;
                code.u8(0x76); // convert_b
                code.u8(0x2A); // dup: branch consumes one value and leaves the result
                let mut rhs_code = SwfBitWriter::new();
                compile_avm2_expr(
                    pool,
                    rhs,
                    &mut rhs_code,
                    method_count,
                    method_params,
                    method_bodies,
                )?;
                rhs_code.u8(0x76); // convert_b
                let skip_rhs = i32::try_from(rhs_code.buf.len().saturating_add(1))
                    .map_err(|_| "AVM2 logical RHS is too large".to_owned())?;
                code.u8(if *op == SwfBinaryOp::And {
                    0x12 // iffalse
                } else {
                    0x11 // iftrue
                });
                emit_avm2_s24(code, skip_rhs)?;
                code.u8(0x29); // pop the non-short-circuit LHS
                code.bytes(&rhs_code.buf);
                return Ok(());
            }
            compile_avm2_expr(pool, lhs, code, method_count, method_params, method_bodies)?;
            compile_avm2_expr(pool, rhs, code, method_count, method_params, method_bodies)?;
            match op {
                SwfBinaryOp::Add => code.u8(0xA0),    // add
                SwfBinaryOp::Sub => code.u8(0xA1),    // subtract
                SwfBinaryOp::Mul => code.u8(0xA2),    // multiply
                SwfBinaryOp::Div => code.u8(0xA3),    // divide
                SwfBinaryOp::Mod => code.u8(0xA4),    // modulo
                SwfBinaryOp::Shl => code.u8(0xA5),    // lshift
                SwfBinaryOp::Shr => code.u8(0xA6),    // rshift
                SwfBinaryOp::UShr => code.u8(0xA7),   // urshift
                SwfBinaryOp::BitAnd => code.u8(0xA8), // bitand
                SwfBinaryOp::BitOr => code.u8(0xA9),  // bitor
                SwfBinaryOp::BitXor => code.u8(0xAA), // bitxor
                SwfBinaryOp::Eq => code.u8(0xAB),     // equals
                SwfBinaryOp::Ne => {
                    code.u8(0xAB); // equals
                    code.u8(0x96); // not
                }
                SwfBinaryOp::StrictEq => code.u8(0xAC), // strictequals
                SwfBinaryOp::StrictNe => {
                    code.u8(0xAC); // strictequals
                    code.u8(0x96); // not
                }
                SwfBinaryOp::Lt => code.u8(0xAD), // lessthan
                SwfBinaryOp::Le => code.u8(0xAE), // lessequals
                SwfBinaryOp::Gt => code.u8(0xAF), // greaterthan
                SwfBinaryOp::Ge => code.u8(0xB0), // greaterequals
                SwfBinaryOp::And | SwfBinaryOp::Or => unreachable!("handled above"),
            }
        }
        SwfExpr::Call { callee, args } => {
            compile_avm2_call(
                pool,
                callee,
                args,
                false,
                code,
                method_count,
                method_params,
                method_bodies,
            )?;
        }
        SwfExpr::New { callee, args } => {
            let cls_name = match callee.as_ref() {
                SwfExpr::Ident(n) => n.as_str(),
                SwfExpr::Member(_, prop) => prop.as_str(),
                _ => return Err("unsupported 'new' constructor target in AVM2".to_owned()),
            };
            let mn = pool.add_qname("", cls_name);
            code.u8(0x5D); // findpropstrict
            code.u32_v(mn);
            for a in args {
                compile_avm2_expr(pool, a, code, method_count, method_params, method_bodies)?;
            }
            code.u8(0x4A); // constructprop
            code.u32_v(mn);
            code.u32_v(args.len() as u32);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn compile_avm2_call(
    pool: &mut AbcPoolBuilder,
    callee: &SwfExpr,
    args: &[SwfExpr],
    discard_result: bool,
    code: &mut SwfBitWriter,
    method_count: &mut u32,
    method_params: &mut Vec<u32>,
    method_bodies: &mut Vec<AbcMethodBodyRecord>,
) -> Result<(), String> {
    let direct_fn = match callee {
        SwfExpr::Ident(name) => Some(name.as_str()),
        SwfExpr::Member(recv, prop) if matches!(recv.as_ref(), SwfExpr::Ident(id) if id == "this" || id == "_root") => {
            Some(prop.as_str())
        }
        _ => None,
    };

    if let Some(short_fn) = direct_fn {
        let is_builtin = matches!(
            short_fn,
            "stop"
                | "play"
                | "nextFrame"
                | "prevFrame"
                | "stopAllSounds"
                | "gotoAndStop"
                | "gotoAndPlay"
                | "trace"
                | "int"
                | "uint"
                | "Number"
                | "String"
                | "Boolean"
                | "parseInt"
                | "parseFloat"
        );
        if is_builtin {
            let fn_mn = pool.add_qname("", short_fn);
            code.u8(0x5D); // findpropstrict
            code.u32_v(fn_mn);
            let effective_args =
                if matches!(short_fn, "gotoAndStop" | "gotoAndPlay") && !args.is_empty() {
                    &args[..1]
                } else {
                    args
                };
            for a in effective_args {
                compile_avm2_expr(pool, a, code, method_count, method_params, method_bodies)?;
            }
            code.u8(if discard_result { 0x4F } else { 0x46 }); // callpropvoid / callproperty
            code.u32_v(fn_mn);
            code.u32_v(effective_args.len() as u32);
            return Ok(());
        }

        // User-defined function or method on timeline/class:
        let fn_mn = pool.add_qname("", short_fn);
        code.u8(0x60); // getlex
        code.u32_v(fn_mn);
        code.u8(0xD0); // getlocal_0 (receiver)
        for a in args {
            compile_avm2_expr(pool, a, code, method_count, method_params, method_bodies)?;
        }
        code.u8(0x41); // call
        code.u32_v(args.len() as u32);
        if discard_result {
            code.u8(0x29); // pop
        }
        return Ok(());
    }

    if let SwfExpr::Member(recv, method_name) = callee {
        compile_avm2_expr(pool, recv, code, method_count, method_params, method_bodies)?;
        for a in args {
            compile_avm2_expr(pool, a, code, method_count, method_params, method_bodies)?;
        }
        let mn = pool.add_qname("", method_name);
        code.u8(if discard_result { 0x4F } else { 0x46 }); // callpropvoid / callproperty
        code.u32_v(mn);
        code.u32_v(args.len() as u32);
        return Ok(());
    }

    Err("unsupported function call target in AVM2 compiler".to_owned())
}

#[allow(clippy::too_many_arguments)]
fn compile_avm2_load_lvalue(
    pool: &mut AbcPoolBuilder,
    target: &SwfLValue,
    code: &mut SwfBitWriter,
    method_count: &mut u32,
    method_params: &mut Vec<u32>,
    method_bodies: &mut Vec<AbcMethodBodyRecord>,
) -> Result<(), String> {
    match target {
        SwfLValue::Ident(name) => {
            let mn = pool.add_qname("", name);
            code.u8(0x60); // getlex
            code.u32_v(mn);
        }
        SwfLValue::Member(obj, prop) => {
            compile_avm2_expr(pool, obj, code, method_count, method_params, method_bodies)?;
            let mn = if let SwfExpr::Ident(id) = obj.as_ref()
                && id != "this"
                && id != "_root"
                && is_avm2_mc_display_prop(prop)
            {
                pool.add_qname("", &format!("__nf_mc_{id}_{prop}"))
            } else {
                pool.add_qname("", prop)
            };
            code.u8(0x66); // getproperty
            code.u32_v(mn);
        }
        SwfLValue::Index(obj, idx) => {
            let expr = SwfExpr::Index(obj.clone(), idx.clone());
            compile_avm2_expr(
                pool,
                &expr,
                code,
                method_count,
                method_params,
                method_bodies,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn compile_avm2_store_lvalue_with_rhs<F>(
    pool: &mut AbcPoolBuilder,
    target: &SwfLValue,
    code: &mut SwfBitWriter,
    method_count: &mut u32,
    method_params: &mut Vec<u32>,
    method_bodies: &mut Vec<AbcMethodBodyRecord>,
    emit_rhs: F,
) -> Result<(), String>
where
    F: FnOnce(
        &mut AbcPoolBuilder,
        &mut SwfBitWriter,
        &mut u32,
        &mut Vec<u32>,
        &mut Vec<AbcMethodBodyRecord>,
    ) -> Result<(), String>,
{
    match target {
        SwfLValue::Ident(name) => {
            let prop_mn = pool.add_qname("", name);
            code.u8(0xD0); // getlocal_0
            emit_rhs(pool, code, method_count, method_params, method_bodies)?;
            code.u8(0x61); // setproperty
            code.u32_v(prop_mn);
        }
        SwfLValue::Member(obj, prop) => {
            let prop_mn = pool.add_qname("", prop);
            compile_avm2_expr(pool, obj, code, method_count, method_params, method_bodies)?;
            emit_rhs(pool, code, method_count, method_params, method_bodies)?;
            if let SwfExpr::Ident(id) = obj.as_ref()
                && id != "this"
                && id != "_root"
                && is_avm2_mc_display_prop(prop)
            {
                let mirror_mn = pool.add_qname("", &format!("__nf_mc_{id}_{prop}"));
                code.u8(0x63); // setlocal 63
                code.u32_v(63);
                code.u8(0x62); // getlocal 63
                code.u32_v(63);
                code.u8(0x61); // setproperty on obj
                code.u32_v(prop_mn);
                code.u8(0xD0); // getlocal_0
                code.u8(0x62); // getlocal 63
                code.u32_v(63);
                code.u8(0x61); // setproperty on _root for get_avm2_property lookup
                code.u32_v(mirror_mn);
            } else {
                code.u8(0x61); // setproperty
                code.u32_v(prop_mn);
            }
        }
        SwfLValue::Index(obj, idx) => {
            let prop_name = match idx.as_ref() {
                SwfExpr::Int(iv) => iv.to_string(),
                SwfExpr::Str(s) => s.clone(),
                _ => {
                    return Err(
                        "dynamic non-literal bracket indexing in AS3 requires MultinameL not in bounded SWF subset"
                            .to_owned(),
                    )
                }
            };
            let prop_mn = pool.add_qname("", &prop_name);
            compile_avm2_expr(pool, obj, code, method_count, method_params, method_bodies)?;
            emit_rhs(pool, code, method_count, method_params, method_bodies)?;
            code.u8(0x61); // setproperty
            code.u32_v(prop_mn);
        }
    }
    Ok(())
}

fn compile_avm2_stmt_list(
    pool: &mut AbcPoolBuilder,
    stmts: &[SwfStmt],
    code: &mut SwfBitWriter,
    method_count: &mut u32,
    method_params: &mut Vec<u32>,
    method_bodies: &mut Vec<AbcMethodBodyRecord>,
) -> Result<(), String> {
    for stmt in stmts {
        compile_avm2_stmt(pool, stmt, code, method_count, method_params, method_bodies)?;
    }
    Ok(())
}

fn compile_avm2_stmt(
    pool: &mut AbcPoolBuilder,
    stmt: &SwfStmt,
    code: &mut SwfBitWriter,
    method_count: &mut u32,
    method_params: &mut Vec<u32>,
    method_bodies: &mut Vec<AbcMethodBodyRecord>,
) -> Result<(), String> {
    match stmt {
        SwfStmt::VarDecl { name, init } => {
            let prop_mn = pool.add_qname("", name);
            code.u8(0xD0); // getlocal_0
            if let Some(rhs) = init {
                compile_avm2_expr(pool, rhs, code, method_count, method_params, method_bodies)?;
            } else {
                code.u8(0x21); // pushundefined
            }
            code.u8(0x61); // setproperty
            code.u32_v(prop_mn);
        }
        SwfStmt::Assign { target, op, value } => {
            compile_avm2_store_lvalue_with_rhs(
                pool,
                target,
                code,
                method_count,
                method_params,
                method_bodies,
                |p, c, mc, mp, mb| {
                    if *op == SwfAssignOp::Assign {
                        compile_avm2_expr(p, value, c, mc, mp, mb)
                    } else {
                        compile_avm2_load_lvalue(p, target, c, mc, mp, mb)?;
                        compile_avm2_expr(p, value, c, mc, mp, mb)?;
                        c.u8(match op {
                            SwfAssignOp::AddAssign => 0xA0, // add
                            SwfAssignOp::SubAssign => 0xA1, // subtract
                            SwfAssignOp::MulAssign => 0xA2, // multiply
                            SwfAssignOp::DivAssign => 0xA3, // divide
                            SwfAssignOp::ModAssign => 0xA4, // modulo
                            SwfAssignOp::Assign => unreachable!(),
                        });
                        Ok(())
                    }
                },
            )?;
        }
        SwfStmt::IncDec { target, increment } => {
            compile_avm2_store_lvalue_with_rhs(
                pool,
                target,
                code,
                method_count,
                method_params,
                method_bodies,
                |p, c, mc, mp, mb| {
                    compile_avm2_load_lvalue(p, target, c, mc, mp, mb)?;
                    c.u8(if *increment { 0x91 } else { 0x93 }); // increment / decrement
                    Ok(())
                },
            )?;
        }
        SwfStmt::Expr(SwfExpr::Call { callee, args }) => {
            compile_avm2_call(
                pool,
                callee,
                args,
                true,
                code,
                method_count,
                method_params,
                method_bodies,
            )?;
        }
        SwfStmt::Expr(expr) => {
            compile_avm2_expr(pool, expr, code, method_count, method_params, method_bodies)?;
            code.u8(0x29); // pop
        }
        SwfStmt::Block(inner) => {
            compile_avm2_stmt_list(
                pool,
                inner,
                code,
                method_count,
                method_params,
                method_bodies,
            )?;
        }
        SwfStmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            compile_avm2_expr(pool, cond, code, method_count, method_params, method_bodies)?;
            let mut then_writer = SwfBitWriter::new();
            compile_avm2_stmt_list(
                pool,
                then_branch,
                &mut then_writer,
                method_count,
                method_params,
                method_bodies,
            )?;
            if let Some(else_stmts) = else_branch
                && !else_stmts.is_empty()
            {
                let mut else_writer = SwfBitWriter::new();
                compile_avm2_stmt_list(
                    pool,
                    else_stmts,
                    &mut else_writer,
                    method_count,
                    method_params,
                    method_bodies,
                )?;
                let else_len = i32::try_from(else_writer.buf.len())
                    .map_err(|_| "AVM2 else branch is too large".to_owned())?;
                then_writer.u8(0x10); // jump
                emit_avm2_s24(&mut then_writer, else_len)?;

                let then_len = i32::try_from(then_writer.buf.len())
                    .map_err(|_| "AVM2 then branch is too large".to_owned())?;
                code.u8(0x12); // iffalse
                emit_avm2_s24(code, then_len)?;
                code.bytes(&then_writer.buf);
                code.bytes(&else_writer.buf);
            } else {
                let then_len = i32::try_from(then_writer.buf.len())
                    .map_err(|_| "AVM2 then branch is too large".to_owned())?;
                code.u8(0x12); // iffalse
                emit_avm2_s24(code, then_len)?;
                code.bytes(&then_writer.buf);
            }
        }
        SwfStmt::While { cond, body } => {
            // Standard ASC while loop layout: `jump +body_len`, `L_body: ...`, `L_cond: ... iftrue L_body`
            let mut body_writer = SwfBitWriter::new();
            compile_avm2_stmt_list(
                pool,
                body,
                &mut body_writer,
                method_count,
                method_params,
                method_bodies,
            )?;
            let mut cond_writer = SwfBitWriter::new();
            compile_avm2_expr(
                pool,
                cond,
                &mut cond_writer,
                method_count,
                method_params,
                method_bodies,
            )?;
            let body_len = i32::try_from(body_writer.buf.len())
                .map_err(|_| "AVM2 while loop body is too large".to_owned())?;
            let cond_len = i32::try_from(cond_writer.buf.len())
                .map_err(|_| "AVM2 while loop condition is too large".to_owned())?;
            code.u8(0x10); // jump to L_cond
            emit_avm2_s24(code, body_len)?;
            code.bytes(&body_writer.buf);
            code.bytes(&cond_writer.buf);
            code.u8(0x11); // iftrue back to L_body
            emit_avm2_s24(code, -(body_len + cond_len + 4))?;
        }
        SwfStmt::For {
            init,
            cond,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                compile_avm2_stmt(
                    pool,
                    init_stmt,
                    code,
                    method_count,
                    method_params,
                    method_bodies,
                )?;
            }
            let loop_cond = cond.clone().unwrap_or(SwfExpr::Bool(true));
            let mut combined_body = body.clone();
            if let Some(upd_stmt) = update {
                combined_body.push((**upd_stmt).clone());
            }
            let while_stmt = SwfStmt::While {
                cond: loop_cond,
                body: combined_body,
            };
            compile_avm2_stmt(
                pool,
                &while_stmt,
                code,
                method_count,
                method_params,
                method_bodies,
            )?;
        }
        SwfStmt::FunctionDecl { name, params, body } => {
            let m_idx = *method_count;
            *method_count += 1;
            method_params.push(params.len() as u32);
            let mut fn_code = SwfBitWriter::new();
            fn_code.u8(0xD0); // getlocal_0
            fn_code.u8(0x30); // pushscope
            for (idx, param_name) in params.iter().enumerate() {
                let param_mn = pool.add_qname("", param_name);
                fn_code.u8(0xD0); // getlocal_0
                let reg = (idx + 1) as u32;
                match reg {
                    1 => fn_code.u8(0xD1), // getlocal_1
                    2 => fn_code.u8(0xD2), // getlocal_2
                    3 => fn_code.u8(0xD3), // getlocal_3
                    r => {
                        fn_code.u8(0x62); // getlocal
                        fn_code.u32_v(r);
                    }
                }
                fn_code.u8(0x61); // setproperty
                fn_code.u32_v(param_mn);
            }
            compile_avm2_stmt_list(
                pool,
                body,
                &mut fn_code,
                method_count,
                method_params,
                method_bodies,
            )?;
            fn_code.u8(0x47); // returnvoid
            let local_count = (params.len() + 2).max(64) as u32;
            method_bodies.push((m_idx, 16, local_count, 1, 2, fn_code.buf));

            let fn_mn = pool.add_qname("", name);
            code.u8(0xD0); // getlocal_0
            code.u8(0x40); // newfunction
            code.u32_v(m_idx);
            code.u8(0x61); // setproperty
            code.u32_v(fn_mn);
        }
        SwfStmt::Return(expr_opt) => {
            if let Some(expr) = expr_opt {
                compile_avm2_expr(pool, expr, code, method_count, method_params, method_bodies)?;
                code.u8(0x48); // returnvalue
            } else {
                code.u8(0x47); // returnvoid
            }
        }
        SwfStmt::EventHandler { body, .. } => {
            compile_avm2_stmt_list(pool, body, code, method_count, method_params, method_bodies)?;
        }
    }
    Ok(())
}

fn build_abc_bytes(classes: &[AbcClassSpec]) -> Vec<u8> {
    let mut pool = AbcPoolBuilder::default();
    let object_mn = pool.add_qname("", "Object");
    let add_frame_script_mn = pool.add_qname("", "addFrameScript");

    struct CompiledClass {
        class_mn: u32,
        super_mn: u32,
        cinit_method: u32,
        iinit_method: u32,
        trait_methods: Vec<(u32, u32)>,
    }

    let mut method_count = 0u32;
    let mut method_params: Vec<u32> = Vec::new();
    let mut method_bodies: Vec<(u32, u32, u32, u32, u32, Vec<u8>)> = Vec::new();
    let mut compiled_classes = Vec::with_capacity(classes.len());

    for cls in classes {
        let class_mn = pool.add_qname(&cls.pkg, &cls.class_name);
        let super_mn = pool.add_qname(&cls.super_pkg, &cls.super_name);

        // 1. cinit
        let cinit_method = method_count;
        method_count += 1;
        method_params.push(0);
        let mut cinit_code = SwfBitWriter::new();
        cinit_code.u8(0xD0); // getlocal_0
        cinit_code.u8(0x30); // pushscope
        cinit_code.u8(0x47); // returnvoid
        method_bodies.push((cinit_method, 1, 1, 1, 2, cinit_code.buf));

        // 2. Instance methods (frame scripts + extra methods)
        let mut trait_methods = Vec::new();
        let mut frame_bindings: Vec<(u32, u32)> = Vec::new();

        for (frame_idx, script_src) in &cls.frame_scripts {
            let fn_name = format!("frame{}", frame_idx + 1);
            let fn_mn = pool.add_qname("", &fn_name);
            let m_idx = method_count;
            method_count += 1;
            method_params.push(0);

            let mut m_code = SwfBitWriter::new();
            m_code.u8(0xD0); // getlocal_0
            m_code.u8(0x30); // pushscope
            let stmts = parse_swf_statements_strict(script_src, true)
                .expect("AVM2 frame scripts are validated by SWF preflight");
            compile_avm2_stmt_list(
                &mut pool,
                &stmts,
                &mut m_code,
                &mut method_count,
                &mut method_params,
                &mut method_bodies,
            )
            .expect("AVM2 frame scripts are validated by SWF preflight");
            m_code.u8(0x47); // returnvoid
            method_bodies.push((m_idx, 16, 64, 1, 2, m_code.buf));

            trait_methods.push((fn_mn, m_idx));
            frame_bindings.push((*frame_idx, fn_mn));
        }

        for (m_name, m_params, m_stmts) in &cls.extra_methods {
            let fn_mn = pool.add_qname("", m_name);
            let m_idx = method_count;
            method_count += 1;
            method_params.push(m_params.len() as u32);

            let mut m_code = SwfBitWriter::new();
            m_code.u8(0xD0); // getlocal_0
            m_code.u8(0x30); // pushscope
            for (idx, param_name) in m_params.iter().enumerate() {
                let param_mn = pool.add_qname("", param_name);
                m_code.u8(0xD0); // getlocal_0
                let reg = (idx + 1) as u32;
                match reg {
                    1 => m_code.u8(0xD1),
                    2 => m_code.u8(0xD2),
                    3 => m_code.u8(0xD3),
                    r => {
                        m_code.u8(0x62);
                        m_code.u32_v(r);
                    }
                }
                m_code.u8(0x61); // setproperty
                m_code.u32_v(param_mn);
            }
            compile_avm2_stmt_list(
                &mut pool,
                m_stmts,
                &mut m_code,
                &mut method_count,
                &mut method_params,
                &mut method_bodies,
            )
            .expect("AVM2 class methods are validated by SWF preflight");
            m_code.u8(0x47); // returnvoid
            let local_count = (m_params.len() + 2).max(64) as u32;
            method_bodies.push((m_idx, 16, local_count, 1, 2, m_code.buf));
            trait_methods.push((fn_mn, m_idx));
        }

        // 3. iinit (constructor)
        let iinit_method = method_count;
        method_count += 1;
        method_params.push(0);
        let mut iinit_code = SwfBitWriter::new();
        iinit_code.u8(0xD0); // getlocal_0
        iinit_code.u8(0x30); // pushscope
        iinit_code.u8(0xD0); // getlocal_0
        iinit_code.u8(0x49); // constructsuper
        iinit_code.u32_v(0);
        if !frame_bindings.is_empty() {
            iinit_code.u8(0x5D); // findpropstrict
            iinit_code.u32_v(add_frame_script_mn);
            for (frame_idx, fn_mn) in &frame_bindings {
                emit_avm2_int(
                    &mut pool,
                    &mut iinit_code,
                    i32::try_from(*frame_idx).expect("SWF preflight bounds AVM2 frame indexes"),
                );
                iinit_code.u8(0x60); // getlex
                iinit_code.u32_v(*fn_mn);
            }
            iinit_code.u8(0x4F); // callpropvoid
            iinit_code.u32_v(add_frame_script_mn);
            iinit_code.u32_v((frame_bindings.len() * 2) as u32);
        }
        compile_avm2_stmt_list(
            &mut pool,
            &cls.ctor_statements,
            &mut iinit_code,
            &mut method_count,
            &mut method_params,
            &mut method_bodies,
        )
        .expect("AVM2 constructor statements are validated by SWF preflight");
        iinit_code.u8(0x47); // returnvoid
        let max_stack = (2 + frame_bindings.len() * 2).max(16) as u32;
        method_bodies.push((iinit_method, max_stack, 64, 1, 2, iinit_code.buf));

        compiled_classes.push(CompiledClass {
            class_mn,
            super_mn,
            cinit_method,
            iinit_method,
            trait_methods,
        });
    }

    // Script init method
    let sinit_method = method_count;
    method_count += 1;
    method_params.push(0);
    let mut sinit_code = SwfBitWriter::new();
    sinit_code.u8(0xD0); // getlocal_0
    sinit_code.u8(0x30); // pushscope
    for (c_idx, cls) in compiled_classes.iter().enumerate() {
        sinit_code.u8(0x65); // getscopeobject 0
        sinit_code.u8(0x00);
        sinit_code.u8(0x60); // getlex Object
        sinit_code.u32_v(object_mn);
        sinit_code.u8(0x30); // pushscope
        sinit_code.u8(0x60); // getlex super
        sinit_code.u32_v(cls.super_mn);
        sinit_code.u8(0x30); // pushscope
        sinit_code.u8(0x60); // getlex super
        sinit_code.u32_v(cls.super_mn);
        sinit_code.u8(0x58); // newclass
        sinit_code.u32_v(c_idx as u32);
        sinit_code.u8(0x1D); // popscope
        sinit_code.u8(0x1D); // popscope
        sinit_code.u8(0x68); // initproperty
        sinit_code.u32_v(cls.class_mn);
    }
    sinit_code.u8(0x47); // returnvoid
    method_bodies.push((sinit_method, 4, 1, 1, 4, sinit_code.buf));

    // Serialize ABC 46.16 stream
    let mut w = SwfBitWriter::new();
    w.u16(16); // minor_version = 16
    w.u16(46); // major_version = 46

    // Constant pool
    w.u32_v(if pool.ints.is_empty() {
        0
    } else {
        (pool.ints.len() + 1) as u32
    });
    for &v in &pool.ints {
        w.u32_v(v as u32);
    }
    w.u32_v(if pool.uints.is_empty() {
        0
    } else {
        (pool.uints.len() + 1) as u32
    });
    for &v in &pool.uints {
        w.u32_v(v);
    }
    w.u32_v(if pool.doubles.is_empty() {
        0
    } else {
        (pool.doubles.len() + 1) as u32
    });
    for &v in &pool.doubles {
        w.bytes(&v.to_le_bytes());
    }
    w.u32_v((pool.strings.len() + 1) as u32);
    for s in &pool.strings {
        w.u32_v(s.len() as u32);
        w.bytes(s.as_bytes());
    }
    w.u32_v((pool.namespaces.len() + 1) as u32);
    for &(kind, s_idx) in &pool.namespaces {
        w.u8(kind);
        w.u32_v(s_idx);
    }
    w.u32_v(0); // ns_set_count = 0
    w.u32_v((pool.multinames.len() + 1) as u32);
    for &(kind, ns_idx, name_idx) in &pool.multinames {
        w.u8(kind);
        w.u32_v(ns_idx);
        w.u32_v(name_idx);
    }

    // Methods
    w.u32_v(method_count);
    for m_idx in 0..method_count {
        let p_count = method_params.get(m_idx as usize).copied().unwrap_or(0);
        w.u32_v(p_count);
        w.u32_v(0); // return_type = *
        for _ in 0..p_count {
            w.u32_v(0); // param_type = *
        }
        w.u32_v(0); // name = ""
        w.u8(0); // flags = 0
    }

    // Metadata
    w.u32_v(0);

    // Instances
    w.u32_v(compiled_classes.len() as u32);
    for cls in &compiled_classes {
        w.u32_v(cls.class_mn);
        w.u32_v(cls.super_mn);
        w.u8(0x00); // flags = dynamic (0)
        w.u32_v(0); // intrf_count = 0
        w.u32_v(cls.iinit_method);
        w.u32_v(cls.trait_methods.len() as u32);
        for &(m_mn, m_idx) in &cls.trait_methods {
            w.u32_v(m_mn);
            w.u8(0x01); // Trait_Method
            w.u32_v(0); // disp_id = 0
            w.u32_v(m_idx);
        }
    }

    // Classes
    for cls in &compiled_classes {
        w.u32_v(cls.cinit_method);
        w.u32_v(0); // static trait_count = 0
    }

    // Scripts (1 script containing all classes)
    w.u32_v(1);
    w.u32_v(sinit_method);
    w.u32_v(compiled_classes.len() as u32);
    for (c_idx, cls) in compiled_classes.iter().enumerate() {
        w.u32_v(cls.class_mn);
        w.u8(0x04); // Trait_Class
        w.u32_v((c_idx + 1) as u32); // slot_id
        w.u32_v(c_idx as u32); // class_idx
    }

    // Method bodies
    w.u32_v(method_bodies.len() as u32);
    for (m_idx, max_stack, local_count, init_scope, max_scope, code) in method_bodies {
        w.u32_v(m_idx);
        w.u32_v(max_stack);
        w.u32_v(local_count);
        w.u32_v(init_scope);
        w.u32_v(max_scope);
        w.u32_v(code.len() as u32);
        w.bytes(&code);
        w.u32_v(0); // exception_count = 0
        w.u32_v(0); // trait_count = 0
    }

    w.buf
}

fn build_do_abc2_payload(name: &str, abc_bytes: &[u8]) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u32(1); // lazyInitializeFlag = 1
    w.c_string(name);
    w.bytes(abc_bytes);
    w.buf
}

fn build_symbol_class_payload(bindings: &[(u16, String)]) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u16(bindings.len() as u16);
    for (cid, cls_name) in bindings {
        w.u16(*cid);
        w.c_string(cls_name);
    }
    w.buf
}

fn build_export_assets_payload(exports: &[(u16, String)]) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u16(exports.len() as u16);
    for (cid, name) in exports {
        w.u16(*cid);
        w.c_string(name);
    }
    w.buf
}

fn build_define_scene_and_frame_label_data_payload(scenes: &[(u32, String)]) -> Vec<u8> {
    let mut w = SwfBitWriter::new();
    w.u32_v(scenes.len() as u32);
    for (offset, name) in scenes {
        w.u32_v(*offset);
        w.c_string(name);
    }
    w.u32_v(0); // frame label count = 0 (emitted via FrameLabel tag 43)
    w.buf
}

fn rgb_alpha_to_rgba(rgb: u32, alpha: f64) -> Color {
    let r = ((rgb >> 16) & 0xff) as u8;
    let g = ((rgb >> 8) & 0xff) as u8;
    let b = (rgb & 0xff) as u8;
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    [r, g, b, a]
}

fn write_swf_filter_list(w: &mut SwfBitWriter, filters: &[FilterSpec]) {
    let active_filters: Vec<&FilterSpec> = filters
        .iter()
        .filter(|filter| filter.is_enabled())
        .take(16)
        .collect();
    let count = active_filters.len();
    w.u8(count as u8);
    let to_fixed16 = |v: f64| -> i32 { (v * 65536.0).round() as i32 };
    let to_fixed8 = |v: f64| -> i16 { (v.clamp(0.0, 127.0) * 256.0).round() as i16 };

    for filter in active_filters {
        let quality_level = filter.quality_level().clamp(1, 15);
        match filter {
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                inner,
                knockout,
                hide_object,
            } => {
                w.u8(0); // DropShadowFilter
                w.rgba(rgb_alpha_to_rgba(*color, *alpha));
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                w.u32(to_fixed16(angle.to_radians()) as u32);
                w.u32(to_fixed16(*distance) as u32);
                w.i16(to_fixed8(*strength));
                let composite_source = !*hide_object;
                let flags: u8 = (if *inner { 0x80 } else { 0 })
                    | (if *knockout { 0x40 } else { 0 })
                    | (if composite_source { 0x20 } else { 0 })
                    | (quality_level & 0x1F);
                w.u8(flags);
            }
            FilterSpec::Blur {
                blur_x,
                blur_y,
                quality: _,
            } => {
                w.u8(1); // BlurFilter
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                let flags: u8 = (quality_level & 0x1F) << 3;
                w.u8(flags);
            }
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                inner,
                knockout,
            } => {
                w.u8(2); // GlowFilter
                w.rgba(rgb_alpha_to_rgba(*color, *alpha));
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                w.i16(to_fixed8(*strength));
                let flags: u8 = (if *inner { 0x80 } else { 0 })
                    | (if *knockout { 0x40 } else { 0 })
                    | 0x20
                    | (quality_level & 0x1F);
                w.u8(flags);
            }
            FilterSpec::Bevel {
                distance,
                angle,
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                placement,
                knockout,
            } => {
                w.u8(3); // BevelFilter
                w.rgba(rgb_alpha_to_rgba(*shadow_color, *shadow_alpha));
                w.rgba(rgb_alpha_to_rgba(*highlight_color, *highlight_alpha));
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                w.u32(to_fixed16(angle.to_radians()) as u32);
                w.u32(to_fixed16(*distance) as u32);
                w.i16(to_fixed8(*strength));
                let is_inner = placement == "inner";
                let on_top = placement == "full";
                let flags: u8 = (if is_inner { 0x80 } else { 0 })
                    | (if *knockout { 0x40 } else { 0 })
                    | 0x20
                    | (if on_top { 0x10 } else { 0 })
                    | (quality_level & 0x0F);
                w.u8(flags);
            }
            FilterSpec::GradientGlow {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality: _,
                placement,
                knockout,
            }
            | FilterSpec::GradientBevel {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality: _,
                placement,
                knockout,
            } => {
                let is_grad_bevel = matches!(filter, FilterSpec::GradientBevel { .. });
                w.u8(if is_grad_bevel { 7 } else { 4 });
                let n = colors
                    .len()
                    .min(alphas.len())
                    .min(ratios.len())
                    .clamp(1, 16);
                w.u8(n as u8);
                for i in 0..n {
                    w.rgba(rgb_alpha_to_rgba(colors[i], alphas[i]));
                }
                for &ratio in ratios.iter().take(n) {
                    w.u8(ratio);
                }
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                w.u32(to_fixed16(angle.to_radians()) as u32);
                w.u32(to_fixed16(*distance) as u32);
                w.i16(to_fixed8(*strength));
                let is_inner = placement == "inner";
                let on_top = placement == "full";
                let flags: u8 = (if is_inner { 0x80 } else { 0 })
                    | (if *knockout { 0x40 } else { 0 })
                    | 0x20
                    | (if on_top { 0x10 } else { 0 })
                    | (quality_level & 0x0F);
                w.u8(flags);
            }
            FilterSpec::Convolution {
                matrix_x,
                matrix_y,
                matrix,
                divisor,
                bias,
                preserve_alpha,
                clamp,
                color,
                alpha,
            } => {
                w.u8(5); // ConvolutionFilter
                let mx = (*matrix_x).clamp(1, 15);
                let my = (*matrix_y).clamp(1, 15);
                w.u8(mx);
                w.u8(my);
                w.f32(*divisor as f32);
                w.f32(*bias as f32);
                let total = usize::from(mx) * usize::from(my);
                for i in 0..total {
                    w.f32(matrix.get(i).copied().unwrap_or(0.0) as f32);
                }
                w.rgba(rgb_alpha_to_rgba(*color, *alpha));
                let flags: u8 =
                    (if *clamp { 0x02 } else { 0 }) | (if *preserve_alpha { 0x01 } else { 0 });
                w.u8(flags);
            }
            FilterSpec::ColorMatrix { matrix } => {
                w.u8(6); // ColorMatrixFilter
                for &v in matrix {
                    w.f32(v as f32);
                }
            }
        }
    }
}

fn encode_swf_blend_mode(blend: BlendMode) -> u8 {
    match blend {
        BlendMode::Normal => 1,
        BlendMode::Layer => 2,
        BlendMode::Multiply => 3,
        BlendMode::Screen => 4,
        BlendMode::Lighten => 5,
        BlendMode::Darken => 6,
        BlendMode::Difference => 7,
        BlendMode::Add => 8,
        BlendMode::Subtract => 9,
        BlendMode::Invert => 10,
        BlendMode::Alpha => 11,
        BlendMode::Erase => 12,
        BlendMode::Overlay => 13,
        BlendMode::Hardlight => 14,
    }
}

#[derive(Clone, Debug)]
struct SlotPlacement {
    character_id: u16,
    matrix: [f64; 6],
    color_transform: ColorTransform,
    alpha: f64,
    ratio: Option<u16>,
    name: String,
    clip_depth: Option<u16>,
    visible: bool,
    cache_as_bitmap: bool,
    blend_mode: BlendMode,
    filters: Vec<FilterSpec>,
    clip_actions: Option<Vec<u8>>,
}

impl SlotPlacement {
    fn same_state(&self, other: &Self) -> bool {
        self.character_id == other.character_id
            && self.matrix == other.matrix
            && self.color_transform == other.color_transform
            && (self.alpha - other.alpha).abs() <= 1e-6
            && self.ratio == other.ratio
            && self.name == other.name
            && self.clip_depth == other.clip_depth
            && self.visible == other.visible
            && self.cache_as_bitmap == other.cache_as_bitmap
            && self.blend_mode == other.blend_mode
            && self.filters == other.filters
            && self.clip_actions == other.clip_actions
    }
}

fn write_place_object_tag(
    out_tags: &mut SwfBitWriter,
    depth: u16,
    slot: &SlotPlacement,
    is_move: bool,
) {
    let has_cxform = !slot.color_transform.is_identity() || (slot.alpha - 1.0).abs() > 1e-5;
    let has_ratio = slot.ratio.is_some();
    let has_name = !slot.name.is_empty() && !is_move;
    let has_clip_depth = slot.clip_depth.is_some() && !is_move;
    let has_clip_actions = slot.clip_actions.is_some() && !is_move;
    let has_filters = slot.filters.iter().any(FilterSpec::is_enabled);
    let has_blend = slot.blend_mode != BlendMode::Normal;
    let has_cache = slot.cache_as_bitmap;
    let has_vis = !slot.visible;

    let use_place3 = has_filters || has_blend || has_cache || has_vis;
    let tag_code: u16 = if use_place3 { 70 } else { 26 };

    let flags1: u8 = (if has_clip_actions { 0x80 } else { 0 })
        | (if has_clip_depth { 0x40 } else { 0 })
        | (if has_name { 0x20 } else { 0 })
        | (if has_ratio { 0x10 } else { 0 })
        | (if has_cxform { 0x08 } else { 0 })
        | 0x04 // has_matrix
        | (if !is_move { 0x02 } else { 0 }) // has_character
        | (if is_move { 0x01 } else { 0 });

    let flags2: u8 = (if has_vis { 0x20 } else { 0 })
        | (if has_cache { 0x04 } else { 0 })
        | (if has_blend { 0x02 } else { 0 })
        | (if has_filters { 0x01 } else { 0 });

    let mut w = SwfBitWriter::new();
    w.u8(flags1);
    if use_place3 {
        w.u8(flags2);
    }
    w.u16(depth);
    if !is_move {
        w.u16(slot.character_id);
    }
    w.matrix_px(slot.matrix);
    if has_cxform {
        w.cxform_with_alpha(&slot.color_transform, slot.alpha);
    }
    if let Some(rat) = slot.ratio {
        w.u16(rat);
    }
    if has_name {
        w.c_string(&slot.name);
    }
    if let Some(cd) = slot.clip_depth
        && has_clip_depth
    {
        w.u16(cd);
    }
    if use_place3 {
        if has_filters {
            write_swf_filter_list(&mut w, &slot.filters);
        }
        if has_blend {
            w.u8(encode_swf_blend_mode(slot.blend_mode));
        }
        if has_cache {
            w.u8(1);
        }
        if has_vis {
            w.u8(u8::from(slot.visible));
        }
    }
    if let Some(clip_bytes) = &slot.clip_actions
        && has_clip_actions
    {
        w.bytes(clip_bytes);
    }
    out_tags.write_tag(tag_code, &w.buf);
}

struct SwfDictionaryCompiler<'a> {
    project: &'a Project,
    options: &'a SwfPublishOptions,
    next_char_id: u32,
    dictionary_overflow: bool,
    root_tags: SwfBitWriter,
    bitmap_ids: BTreeMap<String, u16>,
    sound_ids: BTreeMap<String, u16>,
    font_ids: BTreeMap<(String, bool, bool), u16>,
    symbol_char_ids: BTreeMap<String, u16>,
    button_scripts_by_sym: BTreeMap<String, Vec<ObjectScript>>,
    avm1_exports: Vec<(u16, String)>,
    avm2_symbol_classes: Vec<(u16, String)>,
    avm2_classes: Vec<AbcClassSpec>,
}

fn element_character_key(element: &Element) -> String {
    serde_json::to_string(&(&element.kind, &element.transform.primitive))
        .unwrap_or_else(|_| format!("{:?}", element.kind))
}

impl<'a> SwfDictionaryCompiler<'a> {
    fn new(project: &'a Project, options: &'a SwfPublishOptions) -> Self {
        Self {
            project,
            options,
            next_char_id: 1,
            dictionary_overflow: false,
            root_tags: SwfBitWriter::new(),
            bitmap_ids: BTreeMap::new(),
            sound_ids: BTreeMap::new(),
            font_ids: BTreeMap::new(),
            symbol_char_ids: BTreeMap::new(),
            button_scripts_by_sym: BTreeMap::new(),
            avm1_exports: Vec::new(),
            avm2_symbol_classes: Vec::new(),
            avm2_classes: Vec::new(),
        }
    }

    fn alloc_char_id(&mut self) -> u16 {
        if self.next_char_id > u32::from(u16::MAX) {
            self.dictionary_overflow = true;
            return 0;
        }
        let id = self.next_char_id as u16;
        self.next_char_id += 1;
        id
    }

    fn ensure_media_resources(&mut self) -> Result<(), ExportError> {
        let (used_bitmaps, used_sounds) = used_swf_media_resources(self.project);
        for res in &self.project.resources {
            if !used_bitmaps.contains(&res.id) && !used_sounds.contains(&res.id) {
                continue;
            }
            let Ok(raw) = res.decoded_bytes() else {
                continue;
            };
            if used_bitmaps.contains(&res.id)
                && (res.media_type.eq_ignore_ascii_case("image/png")
                    || res.bitmap_export.is_some_and(|s| {
                        s.compression == neoflash_core::BitmapCompression::LosslessPng
                    }))
            {
                let bid = self.alloc_char_id();
                if let Some((payload, _, _)) = build_define_bits_lossless2_payload(bid, &raw) {
                    self.root_tags.write_tag(36, &payload);
                    self.bitmap_ids.insert(res.id.clone(), bid);
                } else {
                    return Err(ExportError::Options(format!(
                        "Placed PNG '{}' could not be encoded as DefineBitsLossless2",
                        res.path
                    )));
                }
            }
            if used_sounds.contains(&res.id) {
                let sid = self.alloc_char_id();
                let payload =
                    build_define_sound_payload(sid, &res.media_type, &raw).map_err(|issue| {
                        ExportError::Options(format!(
                            "Frame sound '{}' cannot be encoded as DefineSound: {issue}",
                            res.path
                        ))
                    })?;
                self.root_tags.write_tag(14, &payload);
                self.sound_ids.insert(res.id.clone(), sid);
            }
        }
        Ok(())
    }

    fn ensure_font(&mut self, name: &str, bold: bool, italic: bool) -> u16 {
        let key = (name.to_owned(), bold, italic);
        if let Some(&fid) = self.font_ids.get(&key) {
            return fid;
        }
        let fid = self.alloc_char_id();
        let payload = build_define_font3_payload(fid, name, bold, italic);
        self.root_tags.write_tag(75, &payload);
        self.font_ids.insert(key, fid);
        fid
    }

    fn ensure_element_character(&mut self, element: &Element) -> Option<u16> {
        match &element.kind {
            ElementKind::Instance { symbol_id, .. } => self.symbol_char_ids.get(symbol_id).copied(),
            ElementKind::Bitmap { resource_id, .. } => self.bitmap_ids.get(resource_id).copied(),
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                word_wrap,
                multiline,
            } if !self.options.outline_text => {
                let fid = self.ensure_font(&format.font, format.bold, format.italic);
                let tid = self.alloc_char_id();
                let payload = build_define_edit_text_payload(
                    tid, fid, text, *width, *height, format, *word_wrap, *multiline, 1.0,
                );
                self.root_tags.write_tag(37, &payload);
                Some(tid)
            }
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } if !self.options.outline_text => {
                let fid = self.ensure_font(font_family, false, false);
                let tid = self.alloc_char_id();
                let fmt = neoflash_core::TextFormat {
                    font: font_family.clone(),
                    size: *font_size,
                    color: (u32::from(color[0]) << 16)
                        | (u32::from(color[1]) << 8)
                        | u32::from(color[2]),
                    ..Default::default()
                };
                let w = ((text.chars().count().max(1) as f64) * (*font_size) * 0.65).max(40.0);
                let h = ((*font_size) * 1.4).max(20.0);
                let payload = build_define_edit_text_payload(
                    tid,
                    fid,
                    text,
                    w,
                    h,
                    &fmt,
                    false,
                    text.contains('\n'),
                    f64::from(color[3]) / 255.0,
                );
                self.root_tags.write_tag(37, &payload);
                Some(tid)
            }
            _ => {
                let sid = self.alloc_char_id();
                let (tag_code, payload) =
                    build_define_shape_for_element(sid, element, self.options)?;
                self.root_tags.write_tag(tag_code, &payload);
                Some(sid)
            }
        }
    }

    fn collect_button_scripts(&mut self) {
        for sym in &self.project.symbols {
            for layer in &sym.timeline.layers {
                for kf in &layer.keyframes {
                    for el in &kf.elements {
                        if let ElementKind::Instance { symbol_id, .. } = &el.kind
                            && !el.scripts.is_empty()
                        {
                            self.button_scripts_by_sym
                                .entry(symbol_id.clone())
                                .or_default()
                                .extend(el.scripts.clone());
                        }
                    }
                }
            }
        }
    }

    fn compile_button_symbol(&mut self, sym: &Symbol) -> Result<u16, ExportError> {
        type ButtonStateRecord = (
            u8,
            u16,
            u16,
            [f64; 6],
            ColorTransform,
            f64,
            BlendMode,
            Vec<FilterSpec>,
        );
        let mut state_records: Vec<ButtonStateRecord> = Vec::new();
        let depth_bands = timeline_depth_bands(&sym.timeline)
            .expect("SWF button depth capacity is checked by preflight before compilation");
        let mut element_char_cache = BTreeMap::<String, u16>::new();

        for (l_idx, layer) in sym.timeline.layers.iter().enumerate() {
            if !layer.visible
                || matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide)
            {
                continue;
            }
            for state_frame in 0..4u32 {
                let state_bit = 1u8 << state_frame;
                let Some(kf) = layer.keyframes.iter().rfind(|k| k.frame <= state_frame) else {
                    continue;
                };
                for (element_offset, el) in kf.elements.iter().enumerate() {
                    let character_key = element_character_key(el);
                    let cid = if let Some(&cached) = element_char_cache.get(&character_key) {
                        cached
                    } else if let Some(cid) = self.ensure_element_character(el) {
                        element_char_cache.insert(character_key, cid);
                        cid
                    } else {
                        continue;
                    };
                    // Button states may use different characters at the same depth as long as
                    // their state masks are disjoint. Reuse the timeline's checked layer bands
                    // instead of the former `layer * 16` arithmetic, which collided for wide
                    // layers and overflowed for large but valid button timelines.
                    let depth = depth_bands[l_idx]
                        .0
                        .checked_add(
                            u16::try_from(element_offset)
                                .expect("button element offset is checked by depth preflight"),
                        )
                        .expect("button depth is checked by preflight");
                    state_records.push((
                        state_bit,
                        cid,
                        depth,
                        transform_matrix(el.transform),
                        el.transform.color_transform,
                        el.transform.alpha,
                        el.blend_mode,
                        el.filters.clone(),
                    ));
                }
            }
        }

        // Ensure a valid DefineButton2 has at least one state record
        if state_records.is_empty() {
            let dummy_el = Element {
                id: "btn-fallback".to_owned(),
                kind: ElementKind::Rectangle {
                    width: 20.0,
                    height: 20.0,
                    fill: [0, 0, 0, 0],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            };
            if let Some(cid) = self.ensure_element_character(&dummy_el) {
                state_records.push((
                    0x0F,
                    cid,
                    1,
                    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    ColorTransform::default(),
                    1.0,
                    BlendMode::Normal,
                    Vec::new(),
                ));
            }
        }
        state_records.sort_by_key(|(state_mask, _, depth, ..)| (*depth, *state_mask));

        let button_id = self.alloc_char_id();
        let mut rec_writer = SwfBitWriter::new();
        for (state_mask, cid, depth, mat, ct, alpha, blend, filters) in state_records {
            let has_filters = !filters.is_empty();
            let has_blend = blend != BlendMode::Normal;
            let flags: u8 = (state_mask & 0x0F)
                | (if has_filters { 0x10 } else { 0 })
                | (if has_blend { 0x20 } else { 0 });
            rec_writer.u8(flags);
            rec_writer.u16(cid);
            rec_writer.u16(depth);
            rec_writer.matrix_px(mat);
            rec_writer.cxform_with_alpha(&ct, alpha);
            if has_filters {
                write_swf_filter_list(&mut rec_writer, &filters);
            }
            if has_blend {
                rec_writer.u8(encode_swf_blend_mode(blend));
            }
        }
        rec_writer.u8(0); // End of button records

        let cond_bytes = if !self.options.profile.is_avm2() && !self.options.scriptless_mode {
            match self.button_scripts_by_sym.get(&sym.id) {
                Some(scripts) => encode_button2_cond_actions(scripts).map_err(|error| {
                    ExportError::Options(format!(
                        "Button '{}' cannot encode its AVM1 actions: {error}",
                        sym.name
                    ))
                })?,
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let mut w = SwfBitWriter::new();
        w.u16(button_id);
        w.u8(0); // flags (trackAsMenu = 0)
        if cond_bytes.is_empty() {
            w.u16(0); // action_offset = 0
            w.bytes(&rec_writer.buf);
        } else {
            let action_offset = rec_writer
                .buf
                .len()
                .checked_add(2)
                .and_then(|len| u16::try_from(len).ok())
                .ok_or_else(|| {
                    ExportError::Options(format!(
                        "Button '{}' state records occupy {} bytes; DefineButton2 with AVM1 actions supports at most {} bytes before its action block",
                        sym.name,
                        rec_writer.buf.len(),
                        usize::from(u16::MAX) - 2
                    ))
                })?;
            w.u16(action_offset);
            w.bytes(&rec_writer.buf);
            w.bytes(&cond_bytes);
        }
        self.root_tags.write_tag(34, &w.buf);
        Ok(button_id)
    }

    fn build_timeline_tags(&mut self, timeline: &Timeline, is_root: bool) -> (Vec<u8>, u16) {
        let total_frames = timeline.frame_count.max(1);
        let depth_bands = timeline_depth_bands(timeline)
            .expect("SWF depth capacity is checked by preflight before compilation");

        // Pre-allocate morph shapes for compatible shape tween spans
        let mut morph_spans: BTreeMap<(usize, usize), (u16, u32, u32, Easing)> = BTreeMap::new();
        let mut element_char_cache: BTreeMap<String, u16> = BTreeMap::new();

        for (l_idx, layer) in timeline.layers.iter().enumerate() {
            if !layer.visible
                || matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide)
            {
                continue;
            }
            for (k_idx, kf) in layer.keyframes.iter().enumerate() {
                if let Some(easing) = kf.tween
                    && kf.tween_kind == TweenKind::Shape
                    && let Some(next_kf) = layer.keyframes.get(k_idx + 1)
                    && can_build_morph_shape(&kf.elements, &next_kf.elements)
                {
                    let morph_id = self.alloc_char_id();
                    if let Some(payload) = build_define_morph_shape_payload(
                        morph_id,
                        &kf.elements[0],
                        &next_kf.elements[0],
                    ) {
                        self.root_tags.write_tag(46, &payload);
                        morph_spans
                            .insert((l_idx, k_idx), (morph_id, kf.frame, next_kf.frame, easing));
                        continue;
                    }
                }
                for el in &kf.elements {
                    let character_key = element_character_key(el);
                    if !element_char_cache.contains_key(&character_key)
                        && let Some(cid) = self.ensure_element_character(el)
                    {
                        element_char_cache.insert(character_key, cid);
                    }
                }
            }
        }

        let mut mask_clip_depths: BTreeMap<usize, u16> = BTreeMap::new();
        for (l_idx, layer) in timeline.layers.iter().enumerate() {
            if layer.properties.role == LayerRole::Mask {
                let mut max_masked_depth = 0u16;
                for (other_idx, other) in timeline.layers.iter().enumerate() {
                    if other.properties.mask.as_deref() == Some(layer.id.as_str())
                        && other_idx > l_idx
                    {
                        let d = depth_bands[other_idx].1;
                        max_masked_depth = max_masked_depth.max(d);
                    }
                }
                if max_masked_depth > depth_bands[l_idx].0 {
                    mask_clip_depths.insert(l_idx, max_masked_depth);
                }
            }
        }

        let mut stream_tags = SwfBitWriter::new();
        let mut active_slots: BTreeMap<u16, SlotPlacement> = BTreeMap::new();

        for frame in 0..total_frames {
            let mut next_slots: BTreeMap<u16, SlotPlacement> = BTreeMap::new();
            let mut frame_labels: Vec<&str> = Vec::new();
            let mut frame_sounds: Vec<&FrameSound> = Vec::new();

            for (l_idx, layer) in timeline.layers.iter().enumerate() {
                for kf in &layer.keyframes {
                    if kf.frame == frame {
                        if !kf.label.is_empty() {
                            frame_labels.push(&kf.label);
                        }
                        if let Some(snd) = &kf.sound {
                            frame_sounds.push(snd);
                        }
                    }
                }

                if !layer.visible
                    || matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide)
                {
                    continue;
                }

                let base_depth = depth_bands[l_idx].0;
                let clip_depth = mask_clip_depths.get(&l_idx).copied();

                // Check if frame falls inside a DefineMorphShape span
                let active_k_idx = layer.keyframes.iter().rposition(|k| k.frame <= frame);
                if let Some(k_idx) = active_k_idx
                    && let Some(&(morph_id, s_frame, e_frame, ease)) =
                        morph_spans.get(&(l_idx, k_idx))
                    && frame >= s_frame
                    && frame <= e_frame
                {
                    let span = (e_frame - s_frame).max(1) as f64;
                    let raw_t = f64::from(frame - s_frame) / span;
                    let eased_t = neoflash_core::easing(raw_t, ease).clamp(0.0, 1.0);
                    let ratio = (eased_t * 65535.0).round() as u16;
                    let start_el = &layer.keyframes[k_idx].elements[0];
                    next_slots.insert(
                        base_depth,
                        SlotPlacement {
                            character_id: morph_id,
                            matrix: transform_matrix(start_el.transform),
                            color_transform: start_el.transform.color_transform,
                            alpha: start_el.transform.alpha,
                            ratio: Some(ratio),
                            name: String::new(),
                            clip_depth,
                            visible: start_el.visible,
                            cache_as_bitmap: start_el.cache_as_bitmap,
                            blend_mode: start_el.blend_mode,
                            filters: start_el.filters.clone(),
                            clip_actions: None,
                        },
                    );
                    continue;
                }

                // Evaluate layer at `frame`
                let sampled_elements = neoflash_core::sample_layer_in(timeline, layer, frame);
                for (e_offset, el) in sampled_elements.iter().enumerate() {
                    let character_key = element_character_key(el);
                    let cid_opt = if let Some(&cached) = element_char_cache.get(&character_key) {
                        Some(cached)
                    } else if let Some(cid) = self.ensure_element_character(el) {
                        element_char_cache.insert(character_key, cid);
                        Some(cid)
                    } else {
                        None
                    };
                    let Some(cid) = cid_opt else {
                        continue;
                    };
                    let depth = base_depth
                        .checked_add(
                            u16::try_from(e_offset)
                                .expect("element offset is checked by SWF depth preflight"),
                        )
                        .expect("display depth is checked by SWF preflight");
                    let is_button_inst = match &el.kind {
                        ElementKind::Instance { symbol_id, .. } => self
                            .project
                            .symbol(symbol_id)
                            .is_some_and(|s| s.kind == SymbolKind::Button),
                        _ => false,
                    };
                    let clip_actions = if !self.options.profile.is_avm2()
                        && !self.options.scriptless_mode
                        && !is_button_inst
                    {
                        encode_clip_actions(&el.scripts)
                    } else {
                        None
                    };
                    next_slots.insert(
                        depth,
                        SlotPlacement {
                            character_id: cid,
                            matrix: transform_matrix(el.transform),
                            color_transform: el.transform.color_transform,
                            alpha: el.transform.alpha,
                            ratio: None,
                            name: el.name.clone(),
                            clip_depth: if e_offset == 0 { clip_depth } else { None },
                            visible: el.visible,
                            cache_as_bitmap: el.cache_as_bitmap,
                            blend_mode: el.blend_mode,
                            filters: el.filters.clone(),
                            clip_actions,
                        },
                    );
                }
            }

            // Emit RemoveObject2 for depths no longer present
            let removed_depths: Vec<u16> = active_slots
                .keys()
                .copied()
                .filter(|d| !next_slots.contains_key(d))
                .collect();
            for d in removed_depths {
                active_slots.remove(&d);
                stream_tags.write_tag(28, &d.to_le_bytes());
            }

            // Emit PlaceObject2 / PlaceObject3 for new or changed slots
            for (&depth, slot) in &next_slots {
                match active_slots.get(&depth) {
                    None => {
                        write_place_object_tag(&mut stream_tags, depth, slot, false);
                    }
                    Some(prev) if !prev.same_state(slot) => {
                        let is_move = prev.character_id == slot.character_id
                            && prev.name == slot.name
                            && prev.clip_depth == slot.clip_depth
                            && prev.clip_actions == slot.clip_actions;
                        if is_move {
                            write_place_object_tag(&mut stream_tags, depth, slot, true);
                        } else {
                            stream_tags.write_tag(28, &depth.to_le_bytes());
                            write_place_object_tag(&mut stream_tags, depth, slot, false);
                        }
                    }
                    Some(_) => {}
                }
            }
            active_slots = next_slots;

            // FrameLabel (tag 43)
            if let Some(lbl) = frame_labels.first() {
                let mut lw = SwfBitWriter::new();
                lw.c_string(lbl);
                if is_root {
                    self.root_tags.write_tag(43, &lw.buf);
                } else {
                    stream_tags.write_tag(43, &lw.buf);
                }
            }

            // StartSound (tag 15)
            for snd in frame_sounds {
                if let Some(&sid) = self.sound_ids.get(&snd.resource) {
                    let payload = build_start_sound_payload(sid, snd);
                    if is_root {
                        self.root_tags.write_tag(15, &payload);
                    } else {
                        stream_tags.write_tag(15, &payload);
                    }
                }
            }

            // AVM1 DoAction (tag 12)
            if !self.options.profile.is_avm2() && !self.options.scriptless_mode {
                for fs in &timeline.scripts {
                    if fs.frame == frame
                        && matches!(
                            fs.script.language,
                            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                        )
                    {
                        let actions = compile_avm1_actions(&fs.script.source)
                            .expect("AVM1 action record sizes are checked by SWF preflight");
                        if actions.len() > 1 {
                            if is_root {
                                self.root_tags.write_tag(12, &actions);
                            } else {
                                stream_tags.write_tag(12, &actions);
                            }
                        }
                    }
                }
            }

            // Flush any newly defined root dictionary tags before ShowFrame
            if is_root {
                self.root_tags.bytes(&stream_tags.buf);
                stream_tags.buf.clear();
                self.root_tags.write_tag(1, &[]); // ShowFrame
            } else {
                stream_tags.write_tag(1, &[]); // ShowFrame
            }
        }

        if is_root {
            (
                Vec::new(),
                u16::try_from(total_frames)
                    .expect("SWF frame capacity is checked by preflight before compilation"),
            )
        } else {
            stream_tags.write_tag(0, &[]); // End tag inside DefineSprite
            (
                stream_tags.buf,
                u16::try_from(total_frames)
                    .expect("SWF frame capacity is checked by preflight before compilation"),
            )
        }
    }

    fn compile_symbol_in_order(
        &mut self,
        sym_id: &str,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), ExportError> {
        if visited.contains(sym_id) {
            return Ok(());
        }
        visited.insert(sym_id.to_owned());
        let Some(sym) = self.project.symbol(sym_id).cloned() else {
            return Ok(());
        };

        // Compile child symbol dependencies first so every DefineSprite / DefineButton2 is at root level before use!
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                        self.compile_symbol_in_order(symbol_id, visited)?;
                    }
                }
            }
        }

        let char_id = if sym.kind == SymbolKind::Button {
            self.compile_button_symbol(&sym)?
        } else {
            let (sprite_tags, frame_count) = self.build_timeline_tags(&sym.timeline, false);
            let sid = self.alloc_char_id();
            let mut w = SwfBitWriter::new();
            w.u16(sid);
            w.u16(frame_count);
            w.bytes(&sprite_tags);
            self.root_tags.write_tag(39, &w.buf); // DefineSprite
            sid
        };

        self.symbol_char_ids.insert(sym.id.clone(), char_id);

        if !self.options.profile.is_avm2() {
            if let Some(linkage) = &sym.linkage_id
                && !linkage.is_empty()
            {
                self.avm1_exports.push((char_id, linkage.clone()));
            }
        } else if !self.options.scriptless_mode {
            let as3_scripts: Vec<(u32, String)> = sym
                .timeline
                .scripts
                .iter()
                .filter(|fs| fs.script.language == ScriptLanguage::ActionScript3)
                .map(|fs| (fs.frame, fs.script.source.clone()))
                .collect();
            if !as3_scripts.is_empty() {
                let tl_class_name = format!("Timeline_{char_id}");
                self.avm2_classes.push(AbcClassSpec {
                    pkg: String::new(),
                    class_name: tl_class_name.clone(),
                    super_pkg: "flash.display".to_owned(),
                    super_name: "MovieClip".to_owned(),
                    ctor_statements: Vec::new(),
                    frame_scripts: as3_scripts,
                    extra_methods: Vec::new(),
                });
                self.avm2_symbol_classes.push((char_id, tl_class_name));
            }
            if let Some(cls_name) = &sym.class_name
                && !cls_name.is_empty()
            {
                self.avm2_symbol_classes.push((char_id, cls_name.clone()));
            }
        }
        Ok(())
    }
}

/// Compile `project` into SWF movie bytes (`FWS`, `CWS`, or `ZWS`) along with a compatibility preflight report.
///
/// Returns `ExportError::CompatibilityBlocked` if `preflight_swf_publish` reports any `Blocking`
/// items (for example, JS/TS scripts or mixed AVM1/AVM2 scripts when `scriptless_mode` is `false`).
pub fn publish_swf_bytes(
    project: &Project,
    options: &SwfPublishOptions,
) -> Result<(Vec<u8>, FlaCompatibilityReport), ExportError> {
    if project.symbol(&project.root).is_none() {
        return Err(ExportError::MissingRootSymbol(project.root.clone()));
    }

    let report = preflight_swf_publish(project, options);
    if !report.can_proceed() {
        return Err(ExportError::CompatibilityBlocked {
            profile: match options.profile {
                SwfPublishProfile::FlashPlayer8Avm1 => "FlashPlayer8Avm1 (SWF 8)",
                SwfPublishProfile::FlashPlayer9Avm2 => "FlashPlayer9Avm2 (SWF 9)",
                SwfPublishProfile::FlashPlayer10Avm2 => "FlashPlayer10Avm2 (SWF 10)",
            },
            blocking_count: report.blocking_count(),
        });
    }

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
    compiler.collect_button_scripts();
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

    // Emit ExportAssets (tag 56) & DoInitAction (tag 59) for AVM1
    if !options.profile.is_avm2() {
        if !compiler.avm1_exports.is_empty() {
            let payload = build_export_assets_payload(&compiler.avm1_exports);
            compiler.root_tags.write_tag(56, &payload);
        }
        if !options.scriptless_mode {
            for (idx, src) in project.sources.iter().enumerate() {
                if matches!(
                    src.language,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) {
                    let actions = compile_avm1_actions(&src.source)
                        .expect("AVM1 action record sizes are checked by SWF preflight");
                    if actions.len() > 1 {
                        let init_sprite_id = compiler
                            .symbol_char_ids
                            .values()
                            .copied()
                            .next()
                            .unwrap_or((idx + 1) as u16);
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
        let root_as3_scripts: Vec<(u32, String)> = root_timeline
            .scripts
            .iter()
            .filter(|fs| fs.script.language == ScriptLanguage::ActionScript3)
            .map(|fs| (fs.frame, fs.script.source.clone()))
            .collect();
        if !root_as3_scripts.is_empty() {
            compiler.avm2_classes.push(AbcClassSpec {
                pkg: String::new(),
                class_name: "MainTimeline".to_owned(),
                super_pkg: "flash.display".to_owned(),
                super_name: "MovieClip".to_owned(),
                ctor_statements: Vec::new(),
                frame_scripts: root_as3_scripts,
                extra_methods: Vec::new(),
            });
            compiler
                .avm2_symbol_classes
                .push((0, "MainTimeline".to_owned()));
        }

        let mut defined_qnames: BTreeSet<String> = compiler
            .avm2_classes
            .iter()
            .map(|c| {
                if c.pkg.is_empty() {
                    c.class_name.clone()
                } else {
                    format!("{}.{}", c.pkg, c.class_name)
                }
            })
            .collect();

        for src in &project.sources {
            if src.language == ScriptLanguage::ActionScript3 {
                let fallback = src
                    .path
                    .trim_end_matches(".as")
                    .rsplit('/')
                    .next()
                    .unwrap_or("Main");
                let spec = parse_as3_source_to_spec(&src.source, fallback);
                let qname = if spec.pkg.is_empty() {
                    spec.class_name.clone()
                } else {
                    format!("{}.{}", spec.pkg, spec.class_name)
                };
                if defined_qnames.insert(qname) {
                    compiler.avm2_classes.push(spec);
                }
            }
        }

        if let Some(doc_cls) = &project.document_class
            && !doc_cls.trim().is_empty()
        {
            let clean = doc_cls.trim().to_owned();
            if defined_qnames.insert(clean.clone()) {
                let (pkg, cls_name) = split_qualified_class(&clean);
                compiler.avm2_classes.push(AbcClassSpec {
                    pkg,
                    class_name: cls_name,
                    super_pkg: "flash.display".to_owned(),
                    super_name: "MovieClip".to_owned(),
                    ctor_statements: Vec::new(),
                    frame_scripts: Vec::new(),
                    extra_methods: vec![("init".to_owned(), Vec::new(), Vec::new())],
                });
            }
            compiler.avm2_symbol_classes.push((0, clean));
        }

        for (_, cls_name) in &compiler.avm2_symbol_classes {
            if defined_qnames.insert(cls_name.clone()) {
                let (pkg, short) = split_qualified_class(cls_name);
                compiler.avm2_classes.push(AbcClassSpec {
                    pkg,
                    class_name: short,
                    super_pkg: "flash.display".to_owned(),
                    super_name: "MovieClip".to_owned(),
                    ctor_statements: Vec::new(),
                    frame_scripts: Vec::new(),
                    extra_methods: vec![("init".to_owned(), Vec::new(), Vec::new())],
                });
            }
        }

        if !compiler.avm2_classes.is_empty() {
            let abc_bytes = build_abc_bytes(&compiler.avm2_classes);
            let do_abc_payload = build_do_abc2_payload("frame1", &abc_bytes);
            compiler.root_tags.write_tag(82, &do_abc_payload);
        }
        if !compiler.avm2_symbol_classes.is_empty() {
            let sym_cls_payload = build_symbol_class_payload(&compiler.avm2_symbol_classes);
            compiler.root_tags.write_tag(76, &sym_cls_payload);
        }
    }

    // Build root timeline display list + ShowFrame tags
    let (_, root_frame_count) = compiler.build_timeline_tags(&root_timeline, true);
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
    Ok((swf_bytes, report))
}

/// Publish `project` as a `.swf` movie at `dest`, verifying the compiled movie via
/// `import_swf_bytes` before atomically replacing `dest`.
pub fn publish_swf(
    project: &Project,
    dest: impl AsRef<Path>,
    options: &SwfPublishOptions,
) -> Result<FlaCompatibilityReport, ExportError> {
    let dest = dest.as_ref();
    let (bytes, report) = publish_swf_bytes(project, options)?;
    let _verified = import_swf_bytes(&bytes)?;

    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let parent_dir = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("movie.swf");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent_dir.join(format!(
        ".{stem}.neoflash-swf-tmp-{}-{nanos}",
        std::process::id()
    ));
    std::fs::write(&tmp_path, &bytes)?;
    std::fs::rename(&tmp_path, dest)?;
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
    std::fs::create_dir_all(&tmp_dir)?;
    let tmp_file = tmp_dir.join("verify.swf");
    std::fs::write(&tmp_file, bytes)?;
    let res = neoflash_import::import_project(&tmp_file);
    let _ = std::fs::remove_dir_all(&tmp_dir);
    res.map_err(|e| ExportError::RoundTripVerification(e.to_string()))
}

#[cfg(test)]
mod dictionary_capacity_tests {
    use super::*;

    fn root_keyframe_mut(project: &mut Project) -> &mut neoflash_core::Keyframe {
        let root = project.root.clone();
        &mut project
            .symbol_mut(&root)
            .expect("blank project root")
            .timeline
            .layers[0]
            .keyframes[0]
    }

    fn tiny_png() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    fn wav_bytes(encoding: u16, channels: u16, sample_rate: u32, bits: u16) -> Vec<u8> {
        let bytes_per_sample = usize::from(bits.div_ceil(8));
        let block_align = channels.saturating_mul(bits.div_ceil(8));
        let byte_rate = sample_rate.saturating_mul(u32::from(block_align));
        let data_len = usize::from(channels)
            .saturating_mul(bytes_per_sample)
            .max(1)
            * 4;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + u32::try_from(data_len).unwrap()).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&encoding.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&bits.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&u32::try_from(data_len).unwrap().to_le_bytes());
        wav.resize(wav.len() + data_len, 0);
        wav
    }

    fn one_frame_mp3() -> Vec<u8> {
        // MPEG-1 Layer III, 128 kbps, 44.1 kHz, stereo: floor(144*128000/44100)=417 bytes.
        let mut frame = vec![0u8; 417];
        frame[0..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
        frame
    }

    fn project_with_sound(media_type: &str, extension: &str, bytes: &[u8]) -> Project {
        let mut project = Project::blank();
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes(
                "sound",
                format!("sound.{extension}"),
                media_type,
                bytes,
            )
            .unwrap(),
        );
        root_keyframe_mut(&mut project).sound = Some(FrameSound::new("sound"));
        project
    }

    #[test]
    fn character_allocator_never_reuses_the_last_swf_dictionary_id() {
        let project = Project::blank();
        let options = SwfPublishOptions::default();
        let mut compiler = SwfDictionaryCompiler::new(&project, &options);
        compiler.next_char_id = u32::from(u16::MAX);

        assert_eq!(compiler.alloc_char_id(), u16::MAX);
        assert!(!compiler.dictionary_overflow);
        assert_eq!(compiler.alloc_char_id(), 0);
        assert!(compiler.dictionary_overflow);
        assert_eq!(compiler.alloc_char_id(), 0);
    }

    #[test]
    fn preflight_refuses_frame_rates_that_the_swf_header_would_clamp() {
        let mut project = Project::blank();
        project.fps = 256;
        let options = SwfPublishOptions::default();
        let report = preflight_swf_publish(&project, &options);
        assert!(report.items.iter().any(|item| {
            item.code == "swf_frame_rate_limit"
                && item.disposition == FlaCompatibilityStatus::Blocking
        }));
        assert!(
            !report
                .items
                .iter()
                .any(|item| item.code == "swf_frame_rate_exact")
        );
        assert!(matches!(
            publish_swf_bytes(&project, &options),
            Err(ExportError::CompatibilityBlocked { .. })
        ));

        project.fps = 255;
        let exact = preflight_swf_publish(&project, &options);
        assert!(exact.items.iter().any(|item| {
            item.code == "swf_frame_rate_exact" && item.disposition == FlaCompatibilityStatus::Exact
        }));
    }

    #[test]
    fn preflight_checks_only_used_media_and_refuses_unsupported_codecs() {
        let jpeg = neoflash_core::ResourceUnit::from_bytes(
            "photo",
            "photo.jpg",
            "image/jpeg",
            &[0xff, 0xd8, 0xff, 0xd9],
        )
        .unwrap();
        let ogg = neoflash_core::ResourceUnit::from_bytes(
            "music",
            "music.ogg",
            "audio/ogg",
            b"OggS\0unsupported",
        )
        .unwrap();

        let mut unused = Project::blank();
        unused.resources.extend([jpeg.clone(), ogg.clone()]);
        let unused_report = preflight_swf_publish(&unused, &SwfPublishOptions::default());
        assert!(unused_report.can_proceed(), "{:#?}", unused_report.items);

        let mut placed_jpeg = Project::blank();
        placed_jpeg.resources.push(jpeg);
        root_keyframe_mut(&mut placed_jpeg).elements.push(Element {
            id: "photo-placement".to_owned(),
            kind: ElementKind::Bitmap {
                resource_id: "photo".to_owned(),
                width: 1,
                height: 1,
                smoothing: true,
            },
            ..Element::default()
        });
        let jpeg_report = preflight_swf_publish(&placed_jpeg, &SwfPublishOptions::default());
        assert!(
            jpeg_report
                .items
                .iter()
                .any(|item| item.code == "swf_bitmap_codec_unsupported")
        );
        assert!(!jpeg_report.can_proceed());

        let mut used_ogg = Project::blank();
        used_ogg.resources.push(ogg);
        root_keyframe_mut(&mut used_ogg).sound = Some(FrameSound::new("music"));
        let ogg_report = preflight_swf_publish(&used_ogg, &SwfPublishOptions::default());
        assert!(
            ogg_report
                .items
                .iter()
                .any(|item| item.code == "swf_sound_codec_unsupported")
        );
        assert!(!ogg_report.can_proceed());
    }

    #[test]
    fn swf_sound_layouts_are_proven_instead_of_clamped_or_guessed() {
        let wav = wav_bytes(1, 1, 44_100, 16);
        let wav_info = analyze_swf_sound_resource("audio/wav", &wav).unwrap();
        assert_eq!(wav_info.sample_count, 4);
        assert_eq!(wav_info.sample_rate, 44_100);
        assert!(!wav_info.stereo);
        assert!(
            preflight_swf_publish(
                &project_with_sound("audio/wav", "wav", &wav),
                &SwfPublishOptions::default()
            )
            .can_proceed()
        );

        let mp3 = one_frame_mp3();
        let mp3_info = analyze_swf_sound_resource("audio/mpeg", &mp3).unwrap();
        assert_eq!(mp3_info.sample_count, 1152);
        assert_eq!(mp3_info.sample_rate, 44_100);
        assert!(mp3_info.stereo);
        let mp3_payload = build_define_sound_payload(7, "audio/mpeg", &mp3).unwrap();
        assert_eq!(
            u32::from_le_bytes(mp3_payload[3..7].try_into().unwrap()),
            1152
        );
        assert!(
            preflight_swf_publish(
                &project_with_sound("audio/mpeg", "mp3", &mp3),
                &SwfPublishOptions::default()
            )
            .can_proceed()
        );

        for (label, invalid) in [
            ("IEEE-float", wav_bytes(3, 1, 44_100, 32)),
            ("three-channel", wav_bytes(1, 3, 44_100, 16)),
            ("48-kHz", wav_bytes(1, 2, 48_000, 16)),
            ("24-bit", wav_bytes(1, 2, 44_100, 24)),
        ] {
            let report = preflight_swf_publish(
                &project_with_sound("audio/wav", "wav", &invalid),
                &SwfPublishOptions::default(),
            );
            assert!(
                report
                    .items
                    .iter()
                    .any(|item| item.code == "swf_sound_layout_unsupported"),
                "{label}: {:#?}",
                report.items
            );
            assert!(!report.can_proceed(), "{label}");
        }
    }

    #[test]
    fn valid_png_and_intentional_basic_text_outlines_remain_publishable() {
        let mut project = Project::blank();
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes("image", "image.png", "image/png", &tiny_png())
                .unwrap(),
        );
        root_keyframe_mut(&mut project).elements.extend([
            Element {
                id: "image-placement".to_owned(),
                kind: ElementKind::Bitmap {
                    resource_id: "image".to_owned(),
                    width: 2,
                    height: 2,
                    smoothing: false,
                },
                ..Element::default()
            },
            Element {
                id: "static-text".to_owned(),
                kind: ElementKind::Text {
                    text: "SWF".to_owned(),
                    font_family: "Arial".to_owned(),
                    font_size: 20.0,
                    color: [10, 20, 30, 255],
                },
                ..Element::default()
            },
        ]);

        let default_report = preflight_swf_publish(&project, &SwfPublishOptions::default());
        assert!(
            default_report
                .items
                .iter()
                .any(|item| item.code == "swf_text_requires_outlines")
        );
        assert!(!default_report.can_proceed());

        let options = SwfPublishOptions {
            outline_text: true,
            compression: SwfCompression::UncompressedFws,
            ..SwfPublishOptions::default()
        };
        let report = preflight_swf_publish(&project, &options);
        assert!(report.can_proceed(), "{:#?}", report.items);
        assert!(
            report
                .items
                .iter()
                .any(|item| item.code == "swf_text_outlined")
        );
        let (bytes, _) = publish_swf_bytes(&project, &options).unwrap();
        assert!(bytes.starts_with(b"FWS"));
    }

    #[test]
    fn text_outline_preflight_converts_named_fields_but_blocks_unpreserved_rich_visuals() {
        let mut project = Project::blank();
        root_keyframe_mut(&mut project).elements.push(Element {
            id: "editable-text".to_owned(),
            name: "scoreField".to_owned(),
            kind: ElementKind::TextBox {
                text: "42".to_owned(),
                width: 100.0,
                height: 30.0,
                format: neoflash_core::TextFormat::default(),
                word_wrap: false,
                multiline: false,
            },
            ..Element::default()
        });
        let outlined = SwfPublishOptions {
            outline_text: true,
            ..SwfPublishOptions::default()
        };
        let report = preflight_swf_publish(&project, &outlined);
        assert!(report.can_proceed(), "{:#?}", report.items);
        assert!(
            report
                .items
                .iter()
                .any(|item| item.code == "swf_text_outlined")
        );

        let element = &mut root_keyframe_mut(&mut project).elements[0];
        let ElementKind::TextBox { format, .. } = &mut element.kind else {
            unreachable!()
        };
        let field_options = neoflash_core::AuthoredTextFieldOptions {
            html: true,
            ..neoflash_core::AuthoredTextFieldOptions::default()
        };
        format.set_field_options(Some(field_options));
        let rich_report = preflight_swf_publish(&project, &outlined);
        assert!(rich_report.items.iter().any(|item| {
            item.code == "swf_text_outline_unsupported" && item.message.contains("HTML")
        }));
    }

    #[test]
    fn baked_shape_tween_depth_band_reserves_both_endpoint_sets() {
        let element = Element::default();
        let timeline = Timeline {
            frame_count: 3,
            layers: vec![
                neoflash_core::Layer {
                    id: "shape".to_owned(),
                    name: "Shape".to_owned(),
                    visible: true,
                    locked: false,
                    properties: Default::default(),
                    keyframes: vec![
                        neoflash_core::Keyframe {
                            frame: 0,
                            elements: vec![element.clone(), element.clone()],
                            tween: Some(Easing::Linear),
                            tween_kind: TweenKind::Shape,
                            ..Default::default()
                        },
                        neoflash_core::Keyframe {
                            frame: 2,
                            elements: vec![element.clone(), element.clone(), element.clone()],
                            ..Default::default()
                        },
                    ],
                },
                neoflash_core::Layer {
                    id: "following".to_owned(),
                    name: "Following".to_owned(),
                    visible: true,
                    locked: false,
                    properties: Default::default(),
                    keyframes: vec![neoflash_core::Keyframe {
                        frame: 0,
                        elements: vec![element],
                        ..Default::default()
                    }],
                },
            ],
            scripts: Vec::new(),
            guides: Vec::new(),
        };

        assert_eq!(timeline_depth_bands(&timeline), Some(vec![(1, 5), (6, 6)]));
    }

    #[test]
    fn button_depths_support_more_than_4096_single_element_layers_without_overflow() {
        let mut project = Project::blank();
        let element = Element {
            id: "button-art".to_owned(),
            kind: ElementKind::Rectangle {
                width: 8.0,
                height: 8.0,
                fill: [20, 90, 180, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        let layers = (0..4097)
            .map(|index| {
                let mut layer_element = element.clone();
                layer_element.id = format!("button-art-{index}");
                neoflash_core::Layer {
                    id: format!("button-layer-{index}"),
                    name: format!("Button layer {index}"),
                    visible: true,
                    locked: false,
                    properties: Default::default(),
                    keyframes: vec![neoflash_core::Keyframe {
                        frame: 0,
                        elements: vec![layer_element],
                        ..Default::default()
                    }],
                }
            })
            .collect();
        project.symbols.push(Symbol {
            id: "large-button".to_owned(),
            name: "Large button".to_owned(),
            folder: String::new(),
            kind: SymbolKind::Button,
            timeline: Timeline {
                frame_count: 4,
                layers,
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        project.validate().unwrap();

        let options = SwfPublishOptions {
            compression: SwfCompression::UncompressedFws,
            ..SwfPublishOptions::default()
        };
        let (bytes, report) = publish_swf_bytes(&project, &options).unwrap();
        assert!(report.can_proceed());
        assert!(bytes.starts_with(b"FWS"));

        project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "large-button-instance".to_owned(),
                kind: ElementKind::Instance {
                    symbol_id: "large-button".to_owned(),
                    playback: neoflash_core::InstancePlayback::Independent,
                },
                scripts: vec![ObjectScript {
                    event: ObjectEvent::Click,
                    script: neoflash_core::Script {
                        language: ScriptLanguage::ActionScript2,
                        source: "stop();".to_owned(),
                    },
                }],
                ..Element::default()
            });
        project.validate().unwrap();
        let avm1_options = SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer8Avm1,
            compression: SwfCompression::UncompressedFws,
            ..SwfPublishOptions::default()
        };
        let error = publish_swf_bytes(&project, &avm1_options).unwrap_err();
        assert!(
            error.to_string().contains("state records occupy"),
            "got: {error}"
        );
    }

    #[test]
    fn clip_actions_use_spec_event_flag_bits() {
        let scripts = [
            ObjectScript {
                event: ObjectEvent::Load,
                script: neoflash_core::Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "trace('load');".to_owned(),
                },
            },
            ObjectScript {
                event: ObjectEvent::EnterFrame,
                script: neoflash_core::Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "trace('frame');".to_owned(),
                },
            },
            ObjectScript {
                event: ObjectEvent::Click,
                script: neoflash_core::Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "trace('click');".to_owned(),
                },
            },
        ];
        let encoded = encode_clip_actions(&scripts).unwrap();
        assert_eq!(
            u32::from_le_bytes(encoded[2..6].try_into().unwrap()),
            0x10C0
        );
        let mut offset = 6;
        for expected in [0x80_u32, 0x40, 0x1000] {
            assert_eq!(
                u32::from_le_bytes(encoded[offset..offset + 4].try_into().unwrap()),
                expected
            );
            let size =
                u32::from_le_bytes(encoded[offset + 4..offset + 8].try_into().unwrap()) as usize;
            offset += 8 + size;
        }
        assert_eq!(
            u32::from_le_bytes(encoded[offset..offset + 4].try_into().unwrap()),
            0
        );
    }

    #[test]
    fn avm1_record_lengths_refuse_oversized_strings_labels_and_conditional_blocks() {
        let oversized_string = "x".repeat(usize::from(u16::MAX) - 1);
        let trace_source = format!("trace(\"{oversized_string}\");");
        let issue =
            swf_script_compilation_issue(ScriptLanguage::ActionScript2, &trace_source, false)
                .expect("oversized ActionPush must fail preflight");
        assert!(issue.contains("ActionPush"), "got: {issue}");
        assert!(compile_avm1_actions(&trace_source).is_err());

        let oversized_label = "l".repeat(usize::from(u16::MAX));
        let goto_source = format!("gotoAndStop(\"{oversized_label}\");");
        let issue =
            swf_script_compilation_issue(ScriptLanguage::ActionScript1, &goto_source, false)
                .expect("oversized ActionGotoLabel must fail preflight");
        assert!(issue.contains("ActionGotoLabel"), "got: {issue}");

        let many_actions = "stop();".repeat(usize::from(u16::MAX));
        let scripts = vec![
            ObjectScript {
                event: ObjectEvent::Click,
                script: neoflash_core::Script {
                    language: ScriptLanguage::ActionScript2,
                    source: many_actions,
                },
            },
            ObjectScript {
                event: ObjectEvent::Load,
                script: neoflash_core::Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "play();".to_owned(),
                },
            },
        ];
        let error = encode_button2_cond_actions(&scripts).unwrap_err();
        assert!(error.contains("BUTTONCONDACTION"), "got: {error}");

        let mut project = Project::blank();
        let mut button = project.symbols[0].clone();
        button.id = "button".to_owned();
        button.name = "Button".to_owned();
        button.kind = SymbolKind::Button;
        project.symbols.push(button);
        root_keyframe_mut(&mut project).elements.push(Element {
            id: "button-instance".to_owned(),
            kind: ElementKind::Instance {
                symbol_id: "button".to_owned(),
                playback: neoflash_core::InstancePlayback::Independent,
            },
            scripts,
            ..Element::default()
        });
        let report = preflight_swf_publish(
            &project,
            &SwfPublishOptions::for_profile(SwfPublishProfile::FlashPlayer8Avm1),
        );
        assert!(
            report
                .items
                .iter()
                .any(|item| item.code == "swf_avm1_button_action_capacity")
        );
    }

    #[test]
    fn bounded_script_preflight_rejects_trailing_as3_and_keeps_avm1_associativity() {
        let valid_external = "package app {\nimport flash.display.MovieClip;\npublic class Main extends MovieClip {\ntrace(-1);\n}\n}";
        assert_eq!(
            swf_script_compilation_issue(ScriptLanguage::ActionScript3, valid_external, true),
            None
        );
        assert_eq!(
            swf_script_compilation_issue(
                ScriptLanguage::ActionScript3,
                "package app { public class Main { trace(1.5); } }",
                true,
            ),
            None
        );
        assert!(
            swf_script_compilation_issue(
                ScriptLanguage::ActionScript3,
                "package app { public class Main { try { trace(1); } catch (e:Error) {} } }",
                true,
            )
            .is_some()
        );
        assert!(
            swf_script_compilation_issue(
                ScriptLanguage::ActionScript3,
                "import flash.display.MovieClip; trace(1);",
                true,
            )
            .is_some()
        );
        assert!(
            swf_script_compilation_issue(
                ScriptLanguage::ActionScript3,
                "evil public class Main extends MovieClip {",
                true,
            )
            .is_some()
        );

        let stmts = parse_swf_statements_strict("trace(8 - 4 - 2); trace(8 / 4 / 2);", false)
            .expect("parse left-associative binary ops");
        assert_eq!(stmts.len(), 2);
        assert_eq!(unquote_literal("\"a\" + \"b\""), None);

        assert!(compile_avm1_actions("gotoAndStop(65536);").is_ok());
        assert!(compile_avm1_actions("gotoAndStop(3.0);").is_ok());
        assert!(compile_avm1_actions("gotoAndStop(0);").is_err());
        assert!(compile_avm1_actions("gotoAndStop(-1);").is_err());
        let issue = swf_script_compilation_issue(
            ScriptLanguage::ActionScript2,
            "gotoAndStop(65537);",
            false,
        )
        .expect("out-of-range numeric AVM1 frame must fail preflight");
        assert!(issue.contains("65536 one-based frames"), "got: {issue}");

        let local_goto = compile_avm1_actions("this.gotoAndStop(3);").unwrap();
        assert!(
            local_goto.contains(&0x81),
            "this.gotoAndStop uses ActionGotoFrame"
        );
        let root_goto = compile_avm1_actions("_root.gotoAndStop(3);").unwrap();
        assert!(
            root_goto.contains(&0x52),
            "_root.gotoAndStop must call the root MovieClip method"
        );
        assert!(
            !root_goto.contains(&0x81),
            "_root.gotoAndStop must not retarget the current nested timeline"
        );
        let root_x = compile_avm1_actions("_root._x = 25;").unwrap();
        assert!(root_x.contains(&0x4f), "_root._x uses ActionSetMember");
        assert!(!root_x.contains(&0x1d), "_root._x is not a local variable");
    }

    #[test]
    fn avm2_integer_emission_preserves_negative_and_large_i32_values() {
        let mut pool = AbcPoolBuilder::default();
        let mut code = SwfBitWriter::new();
        emit_avm2_int(&mut pool, &mut code, -1);
        assert_eq!(code.buf.first(), Some(&0x25));
        assert!(pool.ints.is_empty());

        let mut large_code = SwfBitWriter::new();
        emit_avm2_int(&mut pool, &mut large_code, i32::MAX);
        assert_eq!(large_code.buf, vec![0x2D, 0x01]);
        assert_eq!(pool.ints, vec![i32::MAX]);
        assert_eq!(pool.add_int(i32::MAX), 1);
        assert!(
            swf_script_compilation_issue(
                ScriptLanguage::ActionScript3,
                "trace(2147483647);",
                false
            )
            .is_none()
        );
    }

    #[test]
    fn avm1_and_avm2_logical_operators_short_circuit_rhs_evaluation() {
        let mut avm1_and = Vec::new();
        avm1_compile_expr(
            &mut avm1_and,
            &SwfExpr::Binary {
                op: SwfBinaryOp::And,
                lhs: Box::new(SwfExpr::Bool(false)),
                rhs: Box::new(SwfExpr::Int(7)),
            },
        )
        .unwrap();
        assert!(avm1_and.contains(&0x9d), "AVM1 && uses ActionIf");
        assert!(!avm1_and.contains(&0x10), "AVM1 && is not eager ActionAnd");

        let mut avm1_or = Vec::new();
        avm1_compile_expr(
            &mut avm1_or,
            &SwfExpr::Binary {
                op: SwfBinaryOp::Or,
                lhs: Box::new(SwfExpr::Bool(true)),
                rhs: Box::new(SwfExpr::Int(7)),
            },
        )
        .unwrap();
        assert!(avm1_or.contains(&0x9d), "AVM1 || uses ActionIf");
        assert!(!avm1_or.contains(&0x11), "AVM1 || is not eager ActionOr");

        fn compile(op: SwfBinaryOp, lhs: bool) -> Vec<u8> {
            let mut pool = AbcPoolBuilder::default();
            let mut code = SwfBitWriter::new();
            let mut method_count = 0;
            let mut method_params = Vec::new();
            let mut method_bodies = Vec::new();
            compile_avm2_expr(
                &mut pool,
                &SwfExpr::Binary {
                    op,
                    lhs: Box::new(SwfExpr::Bool(lhs)),
                    rhs: Box::new(SwfExpr::Int(7)),
                },
                &mut code,
                &mut method_count,
                &mut method_params,
                &mut method_bodies,
            )
            .unwrap();
            code.buf
        }

        let and_code = compile(SwfBinaryOp::And, false);
        assert_eq!(&and_code[..4], &[0x27, 0x76, 0x2A, 0x12]);
        assert!(and_code.contains(&0x29), "non-short-circuit path drops lhs");
        assert!(!and_code.contains(&0xA8), "logical && is not eager bitand");

        let or_code = compile(SwfBinaryOp::Or, true);
        assert_eq!(&or_code[..4], &[0x26, 0x76, 0x2A, 0x11]);
        assert!(or_code.contains(&0x29), "non-short-circuit path drops lhs");
        assert!(!or_code.contains(&0xA9), "logical || is not eager bitor");
    }
}
