//! Compatibility and portability preflight for Xcode + Swift 6 export (`T-455`).

use crate::{embedded_font_families, has_embedded_font_face, is_font_resource};
use neoflash_core::{
    BlendMode, ElementKind, FilterSpec, LayerRole, MaskMode, Project, VideoDeploymentMode,
    inspect_video_resource, is_avi_bytes, resolve_video_source,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Severity of an Xcode export preflight diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XcodePreflightSeverity {
    Info,
    Warning,
}

/// Actionable located diagnostic emitted before or during Xcode project export.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct XcodePreflightItem {
    pub code: String,
    pub severity: XcodePreflightSeverity,
    pub location: String,
    pub message: String,
}

impl XcodePreflightItem {
    pub(super) fn new(
        code: impl Into<String>,
        severity: XcodePreflightSeverity,
        location: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            location: location.into(),
            message: message.into(),
        }
    }
}

fn is_macos_only_system_font(font_family: &str) -> bool {
    let lower = font_family.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "monaco"
            | "geneva"
            | "lucida grande"
            | "apple chancery"
            | "skia"
            | "herculanum"
            | "charcoal"
            | "chicago"
            | "gadget"
            | "andale mono"
            | "pt mono"
            | "sf mono"
            | "apple braille"
            | "krungthep"
            | "silom"
            | "ayuthaya"
            | "satoshi"
    )
}

fn is_generic_device_font_alias(font_family: &str) -> bool {
    let lower = font_family.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "" | "_sans"
            | "_serif"
            | "_typewriter"
            | "sans-serif"
            | "serif"
            | "monospace"
            | "system-ui"
    )
}

fn check_blend_mode(blend: BlendMode, location: &str, out: &mut Vec<XcodePreflightItem>) {
    match blend {
        BlendMode::Invert => out.push(XcodePreflightItem::new(
            "unsupported_blend_invert",
            XcodePreflightSeverity::Warning,
            location,
            "Blend mode 'Invert' has no exact CoreGraphics CGBlendMode equivalent and will be approximated with '.difference'.",
        )),
        BlendMode::Subtract => out.push(XcodePreflightItem::new(
            "unsupported_blend_subtract",
            XcodePreflightSeverity::Warning,
            location,
            "Blend mode 'Subtract' has no direct CoreGraphics CGBlendMode equivalent and will be approximated with '.difference'.",
        )),
        BlendMode::Erase => out.push(XcodePreflightItem::new(
            "unsupported_blend_erase",
            XcodePreflightSeverity::Warning,
            location,
            "Blend mode 'Erase' clears pixels in the active CoreGraphics buffer ('.destinationOut'); group its parent symbol with 'Layer' blend mode if isolated Flash erasing is expected.",
        )),
        BlendMode::Alpha => out.push(XcodePreflightItem::new(
            "unsupported_blend_alpha",
            XcodePreflightSeverity::Warning,
            location,
            "Blend mode 'Alpha' uses CoreGraphics '.destinationIn' compositing and requires an enclosing 'Layer' transparency group for exact Flash parity.",
        )),
        _ => {}
    }
}

fn check_filters(filters: &[FilterSpec], location: &str, out: &mut Vec<XcodePreflightItem>) {
    for (idx, filter) in filters.iter().enumerate() {
        if !filter.is_enabled() {
            continue;
        }
        let loc = format!("{location} / filter[{idx}]");
        if filter.as_displacement().is_some() {
            out.push(XcodePreflightItem::new(
                "unsupported_displacement_map_filter",
                XcodePreflightSeverity::Warning,
                loc,
                "DisplacementMapFilter is not supported by the zero-shader CoreGraphics vector renderer and will be omitted.",
            ));
            continue;
        }
        if filter.as_shader().is_some() {
            out.push(XcodePreflightItem::new(
                "unsupported_shader_filter",
                XcodePreflightSeverity::Warning,
                loc,
                "Pixel Bender ShaderFilter bytecode is not supported by CoreGraphics and will be omitted.",
            ));
            continue;
        }
        match filter {
            FilterSpec::DropShadow { .. } | FilterSpec::Glow { .. } | FilterSpec::Blur { .. } => {}
            FilterSpec::Convolution { .. } => out.push(XcodePreflightItem::new(
                "unsupported_convolution_filter",
                XcodePreflightSeverity::Warning,
                loc,
                "ConvolutionFilter has no direct CoreGraphics vector equivalent and will be omitted.",
            )),
            FilterSpec::ColorMatrix { .. } => out.push(XcodePreflightItem::new(
                "approximated_color_matrix_filter",
                XcodePreflightSeverity::Warning,
                loc,
                "ColorMatrixFilter is not natively applied to CoreGraphics vector paths; prefer element ColorTransform where possible.",
            )),
            FilterSpec::Bevel { .. }
            | FilterSpec::GradientGlow { .. }
            | FilterSpec::GradientBevel { .. } => out.push(XcodePreflightItem::new(
                "approximated_bevel_gradient_filter",
                XcodePreflightSeverity::Warning,
                loc,
                "Bevel / GradientGlow / GradientBevel filter is approximated as a single-pass CoreGraphics soft shadow.",
            )),
        }
    }
}

/// Inspects `project` for Xcode / macOS / iOS portability issues before export (`T-455`).
///
/// Checks unembedded fonts (especially macOS-only system fonts that do not exist on iOS),
/// unsupported or approximated blend modes (`Erase`, `Invert`, `Subtract`, `Alpha`),
/// unsupported filters (`DisplacementMap`, `Shader`, `Convolution`), non-geometry masks,
/// and external or Apple-unsupported audio/video containers and codecs.
#[must_use]
pub fn preflight_xcode_project(project: &Project) -> Vec<XcodePreflightItem> {
    let mut items = Vec::new();
    let embedded_families = embedded_font_families(project);
    let mut reported_font_locs = BTreeSet::new();

    for res in &project.resources {
        if is_font_resource(res) {
            continue;
        }
        let mime = res.media_type.trim().to_ascii_lowercase();
        if mime == "audio/ogg" || res.path.to_ascii_lowercase().ends_with(".ogg") {
            items.push(XcodePreflightItem::new(
                "unsupported_audio_codec",
                XcodePreflightSeverity::Warning,
                format!("resource '{}'", res.path),
                format!(
                    "Audio resource '{}' uses Ogg Vorbis ('{}'), which Apple AVAudioPlayer does not decode natively on macOS/iOS; convert to AAC (.m4a), MP3, or WAV.",
                    res.path, res.media_type
                ),
            ));
        }
        if res.is_video() {
            let bytes = res.decoded_bytes().unwrap_or_default();
            let path_lower = res.path.to_ascii_lowercase();
            let unsupported_container = mime.contains("webm")
                || mime.contains("flv")
                || mime.contains("ogg")
                || mime.contains("msvideo")
                || path_lower.ends_with(".webm")
                || path_lower.ends_with(".flv")
                || path_lower.ends_with(".avi")
                || (!bytes.is_empty() && is_avi_bytes(&bytes));
            if unsupported_container {
                items.push(XcodePreflightItem::new(
                    "unsupported_video_codec",
                    XcodePreflightSeverity::Warning,
                    format!("resource '{}'", res.path),
                    format!(
                        "Video resource '{}' ('{}') uses a container/codec unsupported by Apple AVFoundation on iOS/macOS; re-encode to H.264 or HEVC (.mp4 / .mov).",
                        res.path, res.media_type
                    ),
                ));
            } else {
                for diag in inspect_video_resource(&res.path, &res.media_type, &bytes, None) {
                    if diag.code.contains("unsupported") || diag.code.contains("flv") {
                        items.push(XcodePreflightItem::new(
                            "unsupported_video_codec",
                            XcodePreflightSeverity::Warning,
                            format!("resource '{}'", res.path),
                            diag.message,
                        ));
                    }
                }
            }
        }
    }

    for symbol in &project.symbols {
        for script in &symbol.timeline.scripts {
            if !script.script.source.trim().is_empty() {
                items.push(XcodePreflightItem::new(
                    "authored_timeline_script",
                    XcodePreflightSeverity::Info,
                    format!("{} / frame {}", symbol.name, script.frame + 1),
                    format!(
                        "Timeline {} script at frame {} requires an XcodeScriptBundle entry to execute natively in Swift 6.",
                        script.script.language.display_label(),
                        script.frame + 1
                    ),
                ));
            }
        }

        for layer in &symbol.timeline.layers {
            if layer.properties.role == LayerRole::Folder {
                continue;
            }
            let layer_loc = format!("{} / {}", symbol.name, layer.name);
            check_blend_mode(layer.properties.blend_mode, &layer_loc, &mut items);
            check_filters(&layer.properties.filters, &layer_loc, &mut items);

            if layer.properties.role == LayerRole::Mask
                && (layer.properties.mask_mode != MaskMode::Geometry
                    || layer.properties.mask_invert)
            {
                let mode_desc = match (layer.properties.mask_mode, layer.properties.mask_invert) {
                    (MaskMode::Geometry, true) => "Inverted Geometry",
                    (MaskMode::Alpha, false) => "Alpha",
                    (MaskMode::Alpha, true) => "Inverted Alpha",
                    (MaskMode::Luminance, false) => "Luminance",
                    (MaskMode::Luminance, true) => "Inverted Luminance",
                    (MaskMode::Geometry, false) => "Geometry",
                };
                items.push(XcodePreflightItem::new(
                    "approximated_mask_mode",
                    XcodePreflightSeverity::Warning,
                    layer_loc.clone(),
                    format!(
                        "Mask layer '{}' uses '{}' masking, which is approximated as non-inverted geometric CGPath clipping in CoreGraphics.",
                        layer.name, mode_desc
                    ),
                ));
            }

            for key in &layer.keyframes {
                let frame_loc =
                    format!("{} / {} / frame {}", symbol.name, layer.name, key.frame + 1);
                check_filters(&key.filters, &frame_loc, &mut items);

                if let Some(video_span) = &key.video
                    && let Some(spec) = resolve_video_source(project, video_span)
                {
                    match spec.mode {
                        VideoDeploymentMode::External => items.push(XcodePreflightItem::new(
                            "external_video_source",
                            XcodePreflightSeverity::Warning,
                            frame_loc.clone(),
                            format!(
                                "Timeline video references external file '{}'; supply XcodeExportOptions.base_dir so it can be copied into Generated/Resources/.",
                                spec.url_or_path
                            ),
                        )),
                        VideoDeploymentMode::Progressive => {
                            let is_http = spec
                                .url_or_path
                                .trim()
                                .to_ascii_lowercase()
                                .starts_with("http://");
                            items.push(XcodePreflightItem::new(
                                if is_http {
                                    "insecure_progressive_video_url"
                                } else {
                                    "progressive_video_url"
                                },
                                if is_http {
                                    XcodePreflightSeverity::Warning
                                } else {
                                    XcodePreflightSeverity::Info
                                },
                                frame_loc.clone(),
                                if is_http {
                                    format!(
                                        "Progressive video URL '{}' uses cleartext HTTP, which Apple App Transport Security (ATS) blocks by default on iOS/macOS; prefer HTTPS.",
                                        spec.url_or_path
                                    )
                                } else {
                                    format!(
                                        "Progressive video URL '{}' streams over the network at runtime.",
                                        spec.url_or_path
                                    )
                                },
                            ));
                        }
                        VideoDeploymentMode::Embedded => {}
                    }
                }

                for element in &key.elements {
                    let elem_label = if element.name.trim().is_empty() {
                        element.id.as_str()
                    } else {
                        element.name.as_str()
                    };
                    let elem_loc = format!("{frame_loc} / {elem_label}");
                    check_blend_mode(element.blend_mode, &elem_loc, &mut items);
                    check_filters(&element.filters, &elem_loc, &mut items);

                    for handler in &element.scripts {
                        if !handler.script.source.trim().is_empty() {
                            items.push(XcodePreflightItem::new(
                                "authored_instance_script",
                                XcodePreflightSeverity::Info,
                                format!("{elem_loc} / {:?}", handler.event),
                                format!(
                                    "Instance {:?} {} script requires an XcodeScriptBundle handler entry to execute natively in Swift 6.",
                                    handler.event,
                                    handler.script.language.display_label()
                                ),
                            ));
                        }
                    }

                    match &element.kind {
                        ElementKind::Text { font_family, .. } => {
                            let fam = font_family.trim();
                            if !is_generic_device_font_alias(fam)
                                && !embedded_families.contains(&fam.to_ascii_lowercase())
                                && reported_font_locs.insert((elem_loc.clone(), fam.to_owned()))
                            {
                                if is_macos_only_system_font(fam) {
                                    items.push(XcodePreflightItem::new(
                                        "unembedded_macos_font",
                                        XcodePreflightSeverity::Warning,
                                        elem_loc,
                                        format!(
                                            "Font '{fam}' is a macOS-only system font and is not embedded; iOS devices do not ship '{fam}' and will substitute a system fallback unless embedded."
                                        ),
                                    ));
                                } else {
                                    items.push(XcodePreflightItem::new(
                                        "unembedded_font",
                                        XcodePreflightSeverity::Warning,
                                        elem_loc,
                                        format!(
                                            "Font '{fam}' is referenced by name without an embedded .ttf/.otf resource; target iOS/macOS devices without '{fam}' installed will use a fallback font."
                                        ),
                                    ));
                                }
                            }
                        }
                        ElementKind::TextBox { format, .. } => {
                            let fam = format.font.trim();
                            if is_generic_device_font_alias(fam) {
                                continue;
                            }
                            let requires_embed =
                                format.field_options().is_some_and(|opts| opts.embed_fonts);
                            if requires_embed
                                && !has_embedded_font_face(project, fam, format.bold, format.italic)
                            {
                                items.push(XcodePreflightItem::new(
                                    "missing_embedded_font_face",
                                    XcodePreflightSeverity::Warning,
                                    elem_loc,
                                    format!(
                                        "TextBox requires embedded font '{fam}' (bold={}, italic={}), but no matching font resource is embedded in the project.",
                                        format.bold, format.italic
                                    ),
                                ));
                            } else if !embedded_families.contains(&fam.to_ascii_lowercase())
                                && reported_font_locs.insert((elem_loc.clone(), fam.to_owned()))
                            {
                                if is_macos_only_system_font(fam) {
                                    items.push(XcodePreflightItem::new(
                                        "unembedded_macos_font",
                                        XcodePreflightSeverity::Warning,
                                        elem_loc,
                                        format!(
                                            "Font '{fam}' is a macOS-only system font and is not embedded; iOS devices do not ship '{fam}' and will substitute a system fallback unless embedded."
                                        ),
                                    ));
                                } else {
                                    items.push(XcodePreflightItem::new(
                                        "unembedded_font",
                                        XcodePreflightSeverity::Warning,
                                        elem_loc,
                                        format!(
                                            "Font '{fam}' is referenced by name without an embedded .ttf/.otf resource; target iOS/macOS devices without '{fam}' installed will use a fallback font."
                                        ),
                                    ));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    items
}
