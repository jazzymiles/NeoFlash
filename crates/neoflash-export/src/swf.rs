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
                        elements: vec![element.clone()],
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
            "SWF header {}x{} px ({}x{} twips) @ {} fps, profile {:?} ({:?}).",
            project.width,
            project.height,
            project.width.saturating_mul(20),
            project.height.saturating_mul(20),
            project.fps,
            options.profile,
            options.compression
        ),
    );

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
                report.push(
                    if options.profile.is_avm2() {
                        "swf_as3_source_doabc"
                    } else {
                        "swf_as3_source_in_avm1"
                    },
                    FlaCompatibilityCategory::Scripts,
                    if options.profile.is_avm2() {
                        FlaCompatibilityStatus::Exact
                    } else if options.scriptless_mode {
                        FlaCompatibilityStatus::Omitted
                    } else {
                        FlaCompatibilityStatus::Blocking
                    },
                    loc,
                    if options.profile.is_avm2() {
                        format!("AS3 source '{}' compiled into DoABC2 tag.", src.path)
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
                report.push(
                    if !options.profile.is_avm2() {
                        "swf_avm1_source"
                    } else {
                        "swf_avm1_source_in_avm2"
                    },
                    FlaCompatibilityCategory::Scripts,
                    if !options.profile.is_avm2() {
                        FlaCompatibilityStatus::Exact
                    } else if options.scriptless_mode {
                        FlaCompatibilityStatus::Omitted
                    } else {
                        FlaCompatibilityStatus::Blocking
                    },
                    loc,
                    if !options.profile.is_avm2() {
                        format!("AVM1 source '{}' compiled into DoInitAction.", src.path)
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
                    report.push(
                        if options.profile.is_avm2() {
                            "swf_avm2_frame_script"
                        } else {
                            "swf_avm2_frame_script_in_avm1"
                        },
                        FlaCompatibilityCategory::Scripts,
                        if options.profile.is_avm2() {
                            FlaCompatibilityStatus::Exact
                        } else if options.scriptless_mode {
                            FlaCompatibilityStatus::Omitted
                        } else {
                            FlaCompatibilityStatus::Blocking
                        },
                        fs_loc,
                        "AS3 frame script compiled into AVM2 addFrameScript method.".to_owned(),
                    );
                }
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
                    has_avm1_scripts = true;
                    report.push(
                        if !options.profile.is_avm2() {
                            "swf_avm1_frame_script"
                        } else {
                            "swf_avm1_frame_script_in_avm2"
                        },
                        FlaCompatibilityCategory::Scripts,
                        if !options.profile.is_avm2() {
                            FlaCompatibilityStatus::Exact
                        } else if options.scriptless_mode {
                            FlaCompatibilityStatus::Omitted
                        } else {
                            FlaCompatibilityStatus::Blocking
                        },
                        fs_loc,
                        "AS1/AS2 frame script compiled into AVM1 DoAction tag.".to_owned(),
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
                                report.push(
                                    if !options.profile.is_avm2() {
                                        "swf_avm1_object_script"
                                    } else {
                                        "swf_avm1_object_script_in_avm2"
                                    },
                                    FlaCompatibilityCategory::Scripts,
                                    if !options.profile.is_avm2() {
                                        FlaCompatibilityStatus::Exact
                                    } else if options.scriptless_mode {
                                        FlaCompatibilityStatus::Omitted
                                    } else {
                                        FlaCompatibilityStatus::Blocking
                                    },
                                    format!("{el_loc}:script"),
                                    "Object script compiled into AVM1 CLIPACTIONS / BUTTONCONDACTION."
                                        .to_owned(),
                                );
                            }
                        }
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
                            report.push(
                                if options.outline_text {
                                    "swf_text_outlined"
                                } else {
                                    "swf_define_edit_text"
                                },
                                FlaCompatibilityCategory::TextAndFonts,
                                if options.outline_text {
                                    FlaCompatibilityStatus::Outlined
                                } else {
                                    FlaCompatibilityStatus::Exact
                                },
                                el_loc,
                                if options.outline_text {
                                    "Text converted to vector glyph contours.".to_owned()
                                } else {
                                    "Text emitted via DefineFont3 + DefineEditText.".to_owned()
                                },
                            );
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
    let declared_len = (8usize.saturating_add(body.len())) as u32;
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
            let mut out = Vec::with_capacity(17 + compressed.len());
            out.extend_from_slice(b"ZWS");
            // SWF spec requires SWF version >= 13 for ZWS container.
            out.push(profile.swf_version().max(13));
            out.extend_from_slice(&declared_len.to_le_bytes());
            out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
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

/// Build a `DefineSound` (tag 14) payload for an `audio/wav` or `audio/mpeg` resource.
fn build_define_sound_payload(sound_id: u16, media_type: &str, raw: &[u8]) -> Option<Vec<u8>> {
    if media_type == "audio/wav"
        && raw.len() > 44
        && &raw[0..4] == b"RIFF"
        && &raw[8..12] == b"WAVE"
    {
        let mut pos = 12usize;
        let mut channels = 1u16;
        let mut sample_rate = 44100u32;
        let mut bits_per_sample = 16u16;
        let mut pcm_data: &[u8] = &[];
        while pos + 8 <= raw.len() {
            let chunk_id = &raw[pos..pos + 4];
            let chunk_len = u32::from_le_bytes(raw[pos + 4..pos + 8].try_into().unwrap()) as usize;
            pos += 8;
            let end = pos.saturating_add(chunk_len).min(raw.len());
            if chunk_id == b"fmt " && chunk_len >= 16 {
                channels = u16::from_le_bytes([raw[pos + 2], raw[pos + 3]]).clamp(1, 2);
                sample_rate = u32::from_le_bytes(raw[pos + 4..pos + 8].try_into().unwrap());
                bits_per_sample = u16::from_le_bytes([raw[pos + 14], raw[pos + 15]]);
            } else if chunk_id == b"data" {
                pcm_data = &raw[pos..end];
            }
            pos = end + (chunk_len & 1);
        }
        if pcm_data.is_empty() {
            return None;
        }
        let rate_code: u8 = if sample_rate >= 33075 {
            3
        } else if sample_rate >= 16537 {
            2
        } else if sample_rate >= 8268 {
            1
        } else {
            0
        };
        let is_16bit = u8::from(bits_per_sample > 8);
        let is_stereo = u8::from(channels >= 2);
        // Codec 3 = uncompressed little-endian PCM
        let flags: u8 = (3 << 4) | (rate_code << 2) | (is_16bit << 1) | is_stereo;
        let bytes_per_frame = usize::from(channels) * if is_16bit != 0 { 2usize } else { 1usize };
        let sample_count = (pcm_data.len() / bytes_per_frame.max(1)).max(1) as u32;
        let mut w = SwfBitWriter::new();
        w.u16(sound_id);
        w.u8(flags);
        w.u32(sample_count);
        w.bytes(pcm_data);
        Some(w.buf)
    } else if media_type == "audio/mpeg" && !raw.is_empty() {
        // Codec 2 = MP3, 44.1 kHz, 16-bit stereo
        let flags: u8 = (2 << 4) | (3 << 2) | (1 << 1) | 1;
        let sample_count = ((raw.len() as u32) * 8).max(576);
        let mut w = SwfBitWriter::new();
        w.u16(sound_id);
        w.u8(flags);
        w.u32(sample_count);
        w.i16(0); // seek_samples = 0
        w.bytes(raw);
        Some(w.buf)
    } else {
        None
    }
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

fn split_script_statements(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        for part in trimmed.split(';') {
            let stmt = part.trim();
            if !stmt.is_empty() && !stmt.starts_with("//") {
                out.push(stmt.to_owned());
            }
        }
    }
    out
}

fn parse_call_stmt(stmt: &str) -> Option<(&str, &str)> {
    let open = stmt.find('(')?;
    let close = stmt.rfind(')')?;
    if close < open {
        return None;
    }
    let name = stmt[..open].trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$')
    {
        return None;
    }
    let args = stmt[open + 1..close].trim();
    Some((name, args))
}

fn unquote_literal(arg: &str) -> Option<String> {
    let s = arg.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
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
            } else {
                out.push(c);
            }
        }
        Some(out)
    } else {
        None
    }
}

fn avm1_push_string(out: &mut Vec<u8>, s: &str) {
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
    let payload_len = (1 + bytes.len() + 1) as u16;
    out.push(0x96); // ActionPush
    out.extend_from_slice(&payload_len.to_le_bytes());
    out.push(0x00); // type 0 = String
    out.extend_from_slice(&bytes);
    out.push(0);
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

fn avm1_compile_expr(out: &mut Vec<u8>, expr: &str) {
    let e = expr.trim();
    if let Some(s) = unquote_literal(e) {
        avm1_push_string(out, &s);
    } else if e == "true" {
        avm1_push_bool(out, true);
    } else if e == "false" {
        avm1_push_bool(out, false);
    } else if let Ok(iv) = e.parse::<i32>() {
        avm1_push_int(out, iv);
    } else if let Ok(fv) = e.parse::<f64>() {
        avm1_push_double(out, fv);
    } else if let Some((lhs, rhs)) = e.split_once('+') {
        avm1_compile_expr(out, lhs);
        avm1_compile_expr(out, rhs);
        out.push(0x47); // ActionAdd2
    } else if let Some((lhs, rhs)) = e.split_once('-')
        && !lhs.trim().is_empty()
    {
        avm1_compile_expr(out, lhs);
        avm1_compile_expr(out, rhs);
        out.push(0x0b); // ActionSubtract
    } else if let Some((lhs, rhs)) = e.split_once('*') {
        avm1_compile_expr(out, lhs);
        avm1_compile_expr(out, rhs);
        out.push(0x0c); // ActionMultiply
    } else if let Some((lhs, rhs)) = e.split_once('/') {
        avm1_compile_expr(out, lhs);
        avm1_compile_expr(out, rhs);
        out.push(0x0d); // ActionDivide
    } else {
        // Variable lookup
        avm1_push_string(out, e);
        out.push(0x1c); // ActionGetVariable
    }
}

/// Compile an AS1/AS2 script source into AVM1 action bytecode (terminated by `0x00`).
pub(crate) fn compile_avm1_actions(source: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for stmt in split_script_statements(source) {
        if let Some((func, args)) = parse_call_stmt(&stmt) {
            let short_fn = func.rsplit('.').next().unwrap_or(func);
            match short_fn {
                "stop" if args.is_empty() => {
                    out.push(0x07); // ActionStop
                }
                "play" if args.is_empty() => {
                    out.push(0x06); // ActionPlay
                }
                "nextFrame" if args.is_empty() => {
                    out.push(0x04); // ActionNextFrame
                }
                "prevFrame" if args.is_empty() => {
                    out.push(0x05); // ActionPrevFrame
                }
                "stopAllSounds" if args.is_empty() => {
                    out.push(0x09); // ActionStopSounds
                }
                "gotoAndStop" | "gotoAndPlay" => {
                    let is_play = short_fn == "gotoAndPlay";
                    let first_arg = args.rsplit(',').next().unwrap_or(args).trim();
                    if let Ok(frame_1based) = first_arg.parse::<u32>() {
                        let frame_0 = frame_1based.saturating_sub(1).min(65535) as u16;
                        out.push(0x81); // ActionGotoFrame
                        out.extend_from_slice(&2u16.to_le_bytes());
                        out.extend_from_slice(&frame_0.to_le_bytes());
                        if is_play {
                            out.push(0x06); // ActionPlay
                        }
                    } else if let Some(lbl) = unquote_literal(first_arg) {
                        if !is_play {
                            let lbl_bytes: Vec<u8> = lbl.bytes().filter(|&b| b != 0).collect();
                            let len = (lbl_bytes.len() + 1) as u16;
                            out.push(0x8c); // ActionGotoLabel
                            out.extend_from_slice(&len.to_le_bytes());
                            out.extend_from_slice(&lbl_bytes);
                            out.push(0);
                        } else {
                            avm1_push_string(&mut out, &lbl);
                            out.push(0x9f); // ActionGotoFrame2
                            out.extend_from_slice(&1u16.to_le_bytes());
                            out.push(0x01); // play = true
                        }
                    } else {
                        avm1_compile_expr(&mut out, first_arg);
                        out.push(0x9f); // ActionGotoFrame2
                        out.extend_from_slice(&1u16.to_le_bytes());
                        out.push(u8::from(is_play));
                    }
                }
                "trace" => {
                    avm1_compile_expr(&mut out, args);
                    out.push(0x26); // ActionTrace
                }
                _ => {
                    let call_args: Vec<&str> = if args.is_empty() {
                        Vec::new()
                    } else {
                        args.split(',').map(str::trim).collect()
                    };
                    for a in call_args.iter().rev() {
                        avm1_compile_expr(&mut out, a);
                    }
                    avm1_push_int(&mut out, call_args.len() as i32);
                    avm1_push_string(&mut out, short_fn);
                    out.push(0x3d); // ActionCallFunction
                    out.push(0x17); // ActionPop
                }
            }
        } else if let Some(rest) = stmt.strip_prefix("var ") {
            if let Some((lhs, rhs)) = rest.split_once('=') {
                let var_name = lhs.split(':').next().unwrap_or(lhs).trim();
                avm1_push_string(&mut out, var_name);
                avm1_compile_expr(&mut out, rhs);
                out.push(0x3c); // ActionDefineLocal
            }
        } else if let Some((lhs, rhs)) = stmt.split_once('=')
            && !lhs.ends_with(['=', '!', '<', '>'])
            && !rhs.starts_with('=')
        {
            let var_name = lhs.trim();
            avm1_push_string(&mut out, var_name);
            avm1_compile_expr(&mut out, rhs);
            out.push(0x1d); // ActionSetVariable
        }
    }
    out.push(0x00); // ActionEndFlag
    out
}

/// Encode `BUTTONCONDACTION` records for a `DefineButton2` (tag 34).
fn encode_button2_cond_actions(scripts: &[ObjectScript]) -> Vec<u8> {
    let mut valid: Vec<(u8, u8, Vec<u8>)> = Vec::new();
    for os in scripts {
        if !matches!(
            os.script.language,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        ) {
            continue;
        }
        let actions = compile_avm1_actions(&os.script.source);
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
            (4 + actions.len()) as u16
        };
        out.extend_from_slice(&cond_size.to_le_bytes());
        out.push(b0);
        out.push(b1);
        out.extend_from_slice(&actions);
    }
    out
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
        let actions = compile_avm1_actions(&os.script.source);
        if actions.len() <= 1 {
            continue;
        }
        let (flag, key): (u32, Option<u8>) = match os.event {
            ObjectEvent::Load => (0x0000_0001, None), // ClipEventLoad
            ObjectEvent::EnterFrame => (0x0000_0002, None), // ClipEventEnterFrame
            ObjectEvent::Click => (0x0000_0400, None), // ClipEventPress
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

struct AbcClassSpec {
    pkg: String,
    class_name: String,
    super_pkg: String,
    super_name: String,
    ctor_statements: Vec<String>,
    frame_scripts: Vec<(u32, String)>,
    extra_methods: Vec<(String, Vec<String>)>,
}

fn split_qualified_class(qname: &str) -> (String, String) {
    match qname.rsplit_once('.') {
        Some((pkg, cls)) => (pkg.to_owned(), cls.to_owned()),
        None => (String::new(), qname.to_owned()),
    }
}

fn parse_as3_source_to_spec(source: &str, fallback_qname: &str) -> AbcClassSpec {
    let mut pkg = String::new();
    let mut class_name = String::new();
    let mut super_pkg = "flash.display".to_owned();
    let mut super_name = "MovieClip".to_owned();
    let mut ctor_statements = Vec::new();
    let mut extra_methods = Vec::new();

    for line in source.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("package") {
            let p = rest.trim_end_matches('{').trim();
            if !p.is_empty() {
                pkg = p.to_owned();
            }
        } else if t.contains("class ") {
            let after_class = t.split("class ").nth(1).unwrap_or("");
            let mut parts = after_class.split_whitespace();
            if let Some(cname) = parts.next() {
                class_name = cname.trim_end_matches('{').to_owned();
            }
            if let Some(ext_pos) = after_class.find("extends ") {
                let after_ext = &after_class[ext_pos + "extends ".len()..];
                if let Some(sup) = after_ext.split_whitespace().next() {
                    let clean_sup = sup.trim_end_matches('{');
                    if clean_sup.contains('.') {
                        let (sp, sc) = split_qualified_class(clean_sup);
                        super_pkg = sp;
                        super_name = sc;
                    } else {
                        super_name = clean_sup.to_owned();
                    }
                }
            }
        } else if t.starts_with("trace(")
            || t.starts_with("stop(")
            || t.starts_with("play(")
            || t.starts_with("gotoAndStop(")
            || t.starts_with("gotoAndPlay(")
            || t.starts_with("nextFrame(")
            || t.starts_with("prevFrame(")
        {
            for stmt in split_script_statements(t) {
                ctor_statements.push(stmt);
            }
        }
    }

    if class_name.is_empty() {
        let (fp, fc) = split_qualified_class(fallback_qname);
        pkg = fp;
        class_name = if fc.is_empty() { "Main".to_owned() } else { fc };
    }

    // Ensure non-pure user classes have a helper method if they have no extra methods so they are
    // never misclassified as auto-generated pure timeline classes.
    if extra_methods.is_empty() {
        extra_methods.push(("init".to_owned(), Vec::new()));
    }

    AbcClassSpec {
        pkg,
        class_name,
        super_pkg,
        super_name,
        ctor_statements,
        frame_scripts: Vec::new(),
        extra_methods,
    }
}

fn compile_avm2_statements(pool: &mut AbcPoolBuilder, stmts: &[String], code: &mut SwfBitWriter) {
    for stmt in stmts {
        if let Some((func, args)) = parse_call_stmt(stmt) {
            let short_fn = func.rsplit('.').next().unwrap_or(func);
            match short_fn {
                "stop" | "play" | "nextFrame" | "prevFrame" | "stopAllSounds"
                    if args.is_empty() =>
                {
                    let fn_mn = pool.add_qname("", short_fn);
                    code.u8(0x5D); // findpropstrict
                    code.u32_v(fn_mn);
                    code.u8(0x4F); // callpropvoid
                    code.u32_v(fn_mn);
                    code.u32_v(0);
                }
                "gotoAndStop" | "gotoAndPlay" => {
                    let fn_mn = pool.add_qname("", short_fn);
                    code.u8(0x5D); // findpropstrict
                    code.u32_v(fn_mn);
                    let first_arg = args.split(',').next().unwrap_or(args).trim();
                    if let Ok(frame_num) = first_arg.parse::<i32>() {
                        code.u8(0x25); // pushshort
                        code.u32_v(frame_num as u32);
                    } else if let Some(lbl) = unquote_literal(first_arg) {
                        let s_idx = pool.add_string(&lbl);
                        code.u8(0x2C); // pushstring
                        code.u32_v(s_idx);
                    } else {
                        let s_idx = pool.add_string(first_arg);
                        code.u8(0x2C);
                        code.u32_v(s_idx);
                    }
                    code.u8(0x4F); // callpropvoid
                    code.u32_v(fn_mn);
                    code.u32_v(1);
                }
                "trace" => {
                    let trace_mn = pool.add_qname("", "trace");
                    code.u8(0x5D); // findpropstrict
                    code.u32_v(trace_mn);
                    if let Some(s) = unquote_literal(args) {
                        let s_idx = pool.add_string(&s);
                        code.u8(0x2C); // pushstring
                        code.u32_v(s_idx);
                    } else if let Ok(iv) = args.parse::<i32>() {
                        code.u8(0x25); // pushshort
                        code.u32_v(iv as u32);
                    } else {
                        let s_idx = pool.add_string(args);
                        code.u8(0x2C); // pushstring
                        code.u32_v(s_idx);
                    }
                    code.u8(0x4F); // callpropvoid
                    code.u32_v(trace_mn);
                    code.u32_v(1);
                }
                _ => {
                    let fn_mn = pool.add_qname("", short_fn);
                    code.u8(0x5D); // findpropstrict
                    code.u32_v(fn_mn);
                    let call_args: Vec<&str> = if args.is_empty() {
                        Vec::new()
                    } else {
                        args.split(',').map(str::trim).collect()
                    };
                    for a in &call_args {
                        if let Some(s) = unquote_literal(a) {
                            let s_idx = pool.add_string(&s);
                            code.u8(0x2C);
                            code.u32_v(s_idx);
                        } else if let Ok(iv) = a.parse::<i32>() {
                            code.u8(0x25);
                            code.u32_v(iv as u32);
                        } else {
                            let prop_mn = pool.add_qname("", a);
                            code.u8(0x60); // getlex
                            code.u32_v(prop_mn);
                        }
                    }
                    code.u8(0x4F); // callpropvoid
                    code.u32_v(fn_mn);
                    code.u32_v(call_args.len() as u32);
                }
            }
        } else if let Some(rest) = stmt.strip_prefix("var ")
            && let Some((lhs, rhs)) = rest.split_once('=')
        {
            let var_name = lhs.split(':').next().unwrap_or(lhs).trim();
            let prop_mn = pool.add_qname("", var_name);
            code.u8(0xD0); // getlocal_0
            if let Some(s) = unquote_literal(rhs) {
                let s_idx = pool.add_string(&s);
                code.u8(0x2C);
                code.u32_v(s_idx);
            } else if let Ok(iv) = rhs.trim().parse::<i32>() {
                code.u8(0x25);
                code.u32_v(iv as u32);
            } else {
                let s_idx = pool.add_string(rhs.trim());
                code.u8(0x2C);
                code.u32_v(s_idx);
            }
            code.u8(0x61); // setproperty
            code.u32_v(prop_mn);
        }
    }
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
    let mut method_bodies: Vec<(u32, u32, u32, u32, u32, Vec<u8>)> = Vec::new();
    let mut compiled_classes = Vec::with_capacity(classes.len());

    for cls in classes {
        let class_mn = pool.add_qname(&cls.pkg, &cls.class_name);
        let super_mn = pool.add_qname(&cls.super_pkg, &cls.super_name);

        // 1. cinit
        let cinit_method = method_count;
        method_count += 1;
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

            let mut m_code = SwfBitWriter::new();
            m_code.u8(0xD0); // getlocal_0
            m_code.u8(0x30); // pushscope
            let stmts = split_script_statements(script_src);
            compile_avm2_statements(&mut pool, &stmts, &mut m_code);
            m_code.u8(0x47); // returnvoid
            method_bodies.push((m_idx, 8, 1, 1, 2, m_code.buf));

            trait_methods.push((fn_mn, m_idx));
            frame_bindings.push((*frame_idx, fn_mn));
        }

        for (m_name, m_stmts) in &cls.extra_methods {
            let fn_mn = pool.add_qname("", m_name);
            let m_idx = method_count;
            method_count += 1;

            let mut m_code = SwfBitWriter::new();
            m_code.u8(0xD0); // getlocal_0
            m_code.u8(0x30); // pushscope
            compile_avm2_statements(&mut pool, m_stmts, &mut m_code);
            m_code.u8(0x47); // returnvoid
            method_bodies.push((m_idx, 8, 1, 1, 2, m_code.buf));
            trait_methods.push((fn_mn, m_idx));
        }

        // 3. iinit (constructor)
        let iinit_method = method_count;
        method_count += 1;
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
                iinit_code.u8(0x25); // pushshort
                iinit_code.u32_v(*frame_idx);
                iinit_code.u8(0x60); // getlex
                iinit_code.u32_v(*fn_mn);
            }
            iinit_code.u8(0x4F); // callpropvoid
            iinit_code.u32_v(add_frame_script_mn);
            iinit_code.u32_v((frame_bindings.len() * 2) as u32);
        }
        compile_avm2_statements(&mut pool, &cls.ctor_statements, &mut iinit_code);
        iinit_code.u8(0x47); // returnvoid
        let max_stack = (2 + frame_bindings.len() * 2).max(8) as u32;
        method_bodies.push((iinit_method, max_stack, 1, 1, 2, iinit_code.buf));

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
    for _ in 0..method_count {
        w.u32_v(0); // param_count = 0
        w.u32_v(0); // return_type = *
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
    let count = filters.len().min(16);
    w.u8(count as u8);
    let to_fixed16 = |v: f64| -> i32 { (v * 65536.0).round() as i32 };
    let to_fixed8 = |v: f64| -> i16 { (v.clamp(0.0, 127.0) * 256.0).round() as i16 };

    for filter in filters.iter().take(count) {
        match filter {
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
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
                    | ((*quality).clamp(1, 15) & 0x1F);
                w.u8(flags);
            }
            FilterSpec::Blur {
                blur_x,
                blur_y,
                quality,
            } => {
                w.u8(1); // BlurFilter
                w.u32(to_fixed16(*blur_x) as u32);
                w.u32(to_fixed16(*blur_y) as u32);
                let flags: u8 = ((*quality).clamp(1, 15) & 0x1F) << 3;
                w.u8(flags);
            }
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
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
                    | ((*quality).clamp(1, 15) & 0x1F);
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
                quality,
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
                    | ((*quality).clamp(1, 15) & 0x0F);
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
                quality,
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
                quality,
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
                    | ((*quality).clamp(1, 15) & 0x0F);
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
    let has_filters = !slot.filters.is_empty();
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
    next_char_id: u16,
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

impl<'a> SwfDictionaryCompiler<'a> {
    fn new(project: &'a Project, options: &'a SwfPublishOptions) -> Self {
        Self {
            project,
            options,
            next_char_id: 1,
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
        let id = self.next_char_id;
        self.next_char_id = self.next_char_id.saturating_add(1);
        id
    }

    fn ensure_media_resources(&mut self) {
        for res in &self.project.resources {
            let Ok(raw) = res.decoded_bytes() else {
                continue;
            };
            if res.media_type == "image/png" {
                let bid = self.alloc_char_id();
                if let Some((payload, _, _)) = build_define_bits_lossless2_payload(bid, &raw) {
                    self.root_tags.write_tag(36, &payload);
                    self.bitmap_ids.insert(res.id.clone(), bid);
                }
            } else if res.media_type.starts_with("audio/") {
                let sid = self.alloc_char_id();
                if let Some(payload) = build_define_sound_payload(sid, &res.media_type, &raw) {
                    self.root_tags.write_tag(14, &payload);
                    self.sound_ids.insert(res.id.clone(), sid);
                }
            }
        }
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
                    tid,
                    fid,
                    text,
                    *width,
                    *height,
                    format,
                    *word_wrap,
                    *multiline,
                    element.transform.alpha,
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
                    element.transform.alpha * (f64::from(color[3]) / 255.0),
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

    fn compile_button_symbol(&mut self, sym: &Symbol) -> u16 {
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
        let mut depth_counter = 1u16;

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
                for el in &kf.elements {
                    let Some(cid) = self.ensure_element_character(el) else {
                        continue;
                    };
                    let depth = depth_counter.saturating_add((l_idx as u16) * 16);
                    depth_counter = depth_counter.saturating_add(1);
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
            self.button_scripts_by_sym
                .get(&sym.id)
                .map(|s| encode_button2_cond_actions(s))
                .unwrap_or_default()
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
            let action_offset = (2 + rec_writer.buf.len()) as u16;
            w.u16(action_offset);
            w.bytes(&rec_writer.buf);
            w.bytes(&cond_bytes);
        }
        self.root_tags.write_tag(34, &w.buf);
        button_id
    }

    fn build_timeline_tags(&mut self, timeline: &Timeline, is_root: bool) -> (Vec<u8>, u16) {
        let total_frames = timeline.frame_count.max(1);
        let layer_count = timeline.layers.len().max(1);

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
                    if !element_char_cache.contains_key(&el.id)
                        && let Some(cid) = self.ensure_element_character(el)
                    {
                        element_char_cache.insert(el.id.clone(), cid);
                    }
                }
            }
        }

        // Map layer IDs for mask `clip_depth` calculation
        let layer_id_to_idx: BTreeMap<String, usize> = timeline
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| (l.id.clone(), i))
            .collect();

        // Depth formula: layer `l_idx` gets base depth `((layer_count - l_idx) * 16) as u16`
        // except when a Mask layer `m_idx` masks layers `[m_idx + 1 ..= last_masked]`:
        // in SWF, a mask at `mask_depth` clips depths in `(mask_depth, clip_depth]`, so we assign
        // depths in `1 + l_idx * 16` order when masks are present, or standard ascending layer depths.
        let base_depth_for_layer =
            |l_idx: usize| -> u16 { ((l_idx as u16) + 1).saturating_mul(16) };

        let mut mask_clip_depths: BTreeMap<usize, u16> = BTreeMap::new();
        for (l_idx, layer) in timeline.layers.iter().enumerate() {
            if layer.properties.role == LayerRole::Mask {
                let mut max_masked_depth = 0u16;
                for (other_idx, other) in timeline.layers.iter().enumerate() {
                    if other.properties.mask.as_deref() == Some(layer.id.as_str())
                        && other_idx > l_idx
                    {
                        let d = base_depth_for_layer(other_idx) + 15;
                        max_masked_depth = max_masked_depth.max(d);
                    }
                }
                if max_masked_depth > base_depth_for_layer(l_idx) {
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

                let base_depth = base_depth_for_layer(l_idx);
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
                for (e_offset, el) in sampled_elements.iter().take(15).enumerate() {
                    let cid_opt = if let Some(&cached) = element_char_cache.get(&el.id) {
                        Some(cached)
                    } else {
                        self.ensure_element_character(el)
                    };
                    let Some(cid) = cid_opt else {
                        continue;
                    };
                    let depth = base_depth + (e_offset as u16);
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

            let _ = &layer_id_to_idx;
            let _ = layer_count;

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
                        let actions = compile_avm1_actions(&fs.script.source);
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
            (Vec::new(), total_frames.min(65535) as u16)
        } else {
            stream_tags.write_tag(0, &[]); // End tag inside DefineSprite
            (stream_tags.buf, total_frames.min(65535) as u16)
        }
    }

    fn compile_symbol_in_order(&mut self, sym_id: &str, visited: &mut BTreeSet<String>) {
        if visited.contains(sym_id) {
            return;
        }
        visited.insert(sym_id.to_owned());
        let Some(sym) = self.project.symbol(sym_id).cloned() else {
            return;
        };

        // Compile child symbol dependencies first so every DefineSprite / DefineButton2 is at root level before use!
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                        self.compile_symbol_in_order(symbol_id, visited);
                    }
                }
            }
        }

        let char_id = if sym.kind == SymbolKind::Button {
            self.compile_button_symbol(&sym)
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
    compiler.ensure_media_resources();

    // Compile all non-scene library symbols in bottom-up topological order
    let mut visited = BTreeSet::new();
    for &sid in &scene_ids {
        visited.insert(sid.to_owned());
    }
    for sym in &project.symbols {
        if !scene_ids.contains(&sym.id.as_str()) {
            compiler.compile_symbol_in_order(&sym.id, &mut visited);
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
                    let actions = compile_avm1_actions(&src.source);
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
                    extra_methods: vec![("init".to_owned(), Vec::new())],
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
                    extra_methods: vec![("init".to_owned(), Vec::new())],
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
