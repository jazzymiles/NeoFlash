use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn classify_script_language(
    lang: ScriptLanguage,
    location: &str,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
    has_avm1: &mut bool,
    has_avm2: &mut bool,
    has_web_scripts: &mut bool,
    report: &mut FlaCompatibilityReport,
) {
    match lang {
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
            *has_avm1 = true;
            let disp = if matches!(profile, FlaTargetProfile::AnimateCC) {
                // Animate CC can still open XFL with AS2 frame/object scripts (with PublishSettings ActionScriptVersion=2)
                FlaFeatureDisposition::Exact
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "script_avm1",
                FlaFeatureCategory::Scripts,
                disp,
                location,
                format!("{:?} script preserved.", lang),
            );
        }
        ScriptLanguage::ActionScript3 => {
            *has_avm2 = true;
            if profile.supports_as3() {
                report.push(
                    "script_avm2",
                    FlaFeatureCategory::Scripts,
                    FlaFeatureDisposition::Exact,
                    location,
                    "ActionScript 3.0 script preserved.".to_owned(),
                );
            } else {
                report.push(
                    "script_avm2_in_avm1_profile",
                    FlaFeatureCategory::Scripts,
                    if options.strip_unsupported_scripts {
                        FlaFeatureDisposition::Omitted
                    } else {
                        FlaFeatureDisposition::Blocking
                    },
                    location,
                    format!(
                        "ActionScript 3.0 is not supported by {} (AVM1 only).",
                        profile.label()
                    ),
                );
            }
        }
        ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => {
            *has_web_scripts = true;
            report.push(
                "script_web_language",
                FlaFeatureCategory::Scripts,
                if options.strip_unsupported_scripts {
                    FlaFeatureDisposition::Omitted
                } else {
                    FlaFeatureDisposition::Blocking
                },
                location,
                format!(
                    "{:?} script is a NeoFlash web script and cannot run natively in Flash/Animate.",
                    lang
                ),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn preflight_layer(
    project: &Project,
    sym: &Symbol,
    layer: &Layer,
    layer_loc: &str,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
    bitmap_ids: &BTreeSet<Id>,
    sound_ids: &BTreeSet<Id>,
    has_avm1: &mut bool,
    has_avm2: &mut bool,
    has_web_scripts: &mut bool,
    report: &mut FlaCompatibilityReport,
) {
    let props = &layer.properties;

    // Mask modes & invert
    if props.role == LayerRole::Mask && (props.mask_mode != MaskMode::Geometry || props.mask_invert)
    {
        report.push(
            "layer_advanced_mask_mode",
            FlaFeatureCategory::LayersAndMasks,
            if options.allow_approximations {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Blocking
            },
            layer_loc,
            format!(
                "Mask mode {:?} (invert={}) is approximated as a classic geometric mask in {}.",
                props.mask_mode,
                props.mask_invert,
                profile.label()
            ),
        );
    }

    // Layer-level filters & blend mode
    preflight_filter_stack(&props.filters, layer_loc, profile, report);
    if !props.filters.is_empty() || !props.blend_mode.is_normal() {
        let disp = if options.bake_unsupported_tweens || options.allow_approximations {
            FlaFeatureDisposition::Converted
        } else {
            FlaFeatureDisposition::Blocking
        };
        report.push(
            "layer_filters_or_blend",
            FlaFeatureCategory::FiltersAndBlends,
            disp,
            layer_loc,
            "Layer-level filters and blend mode are propagated to child display instances for Flash compatibility.".to_owned(),
        );
    }

    // Layer Z-depth & camera attachment
    if props.z_depth != 0.0 || props.attach_to_camera {
        let disp = if options.bake_unsupported_tweens {
            FlaFeatureDisposition::Baked
        } else if options.allow_approximations && profile.supports_modern_layer_effects() {
            FlaFeatureDisposition::Approximated
        } else {
            FlaFeatureDisposition::Blocking
        };
        report.push(
            "layer_z_depth_camera",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            layer_loc,
            "Layer Z-depth / camera attachment inspected for target profile.".to_owned(),
        );
    }

    // Motion guide link
    if props.motion_guide.is_some() {
        let disp = if options.bake_unsupported_tweens && profile.is_legacy_binary() {
            FlaFeatureDisposition::Baked
        } else {
            FlaFeatureDisposition::Exact
        };
        report.push(
            "layer_motion_guide",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            layer_loc,
            "Motion guide layer link inspected.".to_owned(),
        );
    }

    let frame_count = sym.timeline.frame_count.max(1);
    let effective_end = props
        .end_frame
        .unwrap_or(frame_count)
        .min(frame_count)
        .max(1);
    let mut sorted_kfs: Vec<&Keyframe> = layer.keyframes.iter().collect();
    sorted_kfs.sort_by_key(|k| k.frame);
    let bake_video = options.allow_approximations && crate::video_frames::bakeable(project, layer);
    for (idx, kf) in sorted_kfs.iter().enumerate() {
        let kf_loc = format!("{}:frame[{}]", layer_loc, kf.frame);
        if profile.is_legacy_binary() && kf.frame < effective_end {
            let next_start = sorted_kfs
                .get(idx + 1)
                .map(|n| n.frame.min(effective_end))
                .unwrap_or(effective_end);
            let duration = next_start.saturating_sub(kf.frame).max(1);
            if duration > u32::from(u16::MAX) {
                report.push(
                    "legacy_frame_duration_limit",
                    FlaFeatureCategory::AnimationAndRigging,
                    FlaFeatureDisposition::Blocking,
                    &kf_loc,
                    format!(
                        "Keyframe hold duration ({duration} frames) exceeds legacy binary FLA u16 limit (65535)."
                    ),
                );
            }
        }
        preflight_filter_stack(&kf.filters, &kf_loc, profile, report);
        for el in &kf.elements {
            let effective_filter_count = props.filters.len() + kf.filters.len() + el.filters.len();
            if effective_filter_count > neoflash_core::MAX_OBJECT_FILTERS {
                report.push(
                    "effective_filter_stack_too_large",
                    FlaFeatureCategory::FiltersAndBlends,
                    FlaFeatureDisposition::Blocking,
                    format!("{kf_loc}:element:{}", el.id),
                    format!(
                        "Element, keyframe and layer filters combine to {effective_filter_count}; the portable display stack supports at most {}. Export is blocked instead of truncating the stack.",
                        neoflash_core::MAX_OBJECT_FILTERS
                    ),
                );
            }
        }
        preflight_keyframe(
            bake_video,
            project,
            kf,
            &kf_loc,
            profile,
            options,
            bitmap_ids,
            sound_ids,
            has_avm1,
            has_avm2,
            has_web_scripts,
            report,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn preflight_keyframe(
    bake_video: bool,
    project: &Project,
    kf: &Keyframe,
    kf_loc: &str,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
    bitmap_ids: &BTreeSet<Id>,
    sound_ids: &BTreeSet<Id>,
    has_avm1: &mut bool,
    has_avm2: &mut bool,
    has_web_scripts: &mut bool,
    report: &mut FlaCompatibilityReport,
) {
    // Tween & easing inspection
    if let Some(easing) = kf.tween {
        match kf.tween_kind {
            TweenKind::Classic => {
                let ease_disp = match easing {
                    Easing::Linear | Easing::EaseIn | Easing::EaseOut | Easing::Flash { .. } => {
                        FlaFeatureDisposition::Exact
                    }
                    Easing::EaseInOut | Easing::CubicBezier { .. } => {
                        if profile.is_legacy_binary() {
                            if options.bake_unsupported_tweens {
                                FlaFeatureDisposition::Baked
                            } else if options.allow_approximations {
                                FlaFeatureDisposition::Approximated
                            } else {
                                FlaFeatureDisposition::Blocking
                            }
                        } else {
                            FlaFeatureDisposition::Exact
                        }
                    }
                    Easing::Bounce | Easing::Elastic => {
                        if options.bake_unsupported_tweens {
                            FlaFeatureDisposition::Baked
                        } else if options.allow_approximations {
                            FlaFeatureDisposition::Approximated
                        } else {
                            FlaFeatureDisposition::Blocking
                        }
                    }
                };
                report.push(
                    "classic_tween",
                    FlaFeatureCategory::AnimationAndRigging,
                    ease_disp,
                    kf_loc,
                    format!("Classic tween with {:?} easing.", easing),
                );
            }
            TweenKind::Shape => {
                let disp = if profile.is_legacy_binary() && options.bake_unsupported_tweens {
                    FlaFeatureDisposition::Baked
                } else {
                    FlaFeatureDisposition::Exact
                };
                report.push(
                    "shape_tween",
                    FlaFeatureCategory::AnimationAndRigging,
                    disp,
                    kf_loc,
                    "Shape tween span inspected.".to_owned(),
                );
            }
            TweenKind::Motion => {
                let disp = if options.bake_unsupported_tweens {
                    FlaFeatureDisposition::Baked
                } else if options.allow_approximations {
                    FlaFeatureDisposition::Approximated
                } else {
                    FlaFeatureDisposition::Blocking
                };
                report.push(
                    "motion_tween_span",
                    FlaFeatureCategory::AnimationAndRigging,
                    disp,
                    kf_loc,
                    "Property-track Motion tween span is baked into frame-by-frame keyframes for portable round-trip.".to_owned(),
                );
            }
        }
    }

    // Rigging / IK / Asset Warp / Camera / Parenting / Appearances / Vector Effects / Expressions
    if !kf.ik_armatures.is_empty() {
        let disp = if options.bake_unsupported_tweens {
            FlaFeatureDisposition::Baked
        } else {
            FlaFeatureDisposition::Blocking
        };
        report.push(
            "ik_armature",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            kf_loc,
            "IK bone armature poses are baked into keyframe element transforms.".to_owned(),
        );
    }

    if !kf.asset_warps.is_empty() {
        let disp = if options.bake_unsupported_tweens {
            FlaFeatureDisposition::Baked
        } else {
            FlaFeatureDisposition::Blocking
        };
        report.push(
            "asset_warp_rig",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            kf_loc,
            "Asset Warp / Puppet pin deformations are baked into vector contours.".to_owned(),
        );
    }

    if kf.camera.is_some() {
        let disp = if options.bake_unsupported_tweens {
            FlaFeatureDisposition::Baked
        } else if options.allow_approximations && profile.supports_modern_layer_effects() {
            FlaFeatureDisposition::Approximated
        } else {
            FlaFeatureDisposition::Blocking
        };
        report.push(
            "camera_pose",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            kf_loc,
            "Timeline camera pan/zoom/rotation is baked into stage transforms.".to_owned(),
        );
    }

    if !kf.element_parents.is_empty()
        || !kf.appearances.is_empty()
        || !kf.vector_effects.is_empty()
        || !kf.property_expressions.is_empty()
    {
        let disp = if options.bake_unsupported_tweens {
            FlaFeatureDisposition::Baked
        } else if options.allow_approximations {
            FlaFeatureDisposition::Approximated
        } else {
            FlaFeatureDisposition::Blocking
        };
        report.push(
            "advanced_keyframe_modifiers",
            FlaFeatureCategory::AnimationAndRigging,
            disp,
            kf_loc,
            "Element parenting, appearances, vector effects, or expressions are baked into concrete elements.".to_owned(),
        );
    }

    // Frame sound
    if let Some(snd) = &kf.sound {
        if !sound_ids.contains(&snd.resource) {
            report.push(
                "missing_sound_resource",
                FlaFeatureCategory::MediaAndSound,
                FlaFeatureDisposition::Blocking,
                kf_loc,
                format!(
                    "Frame sound references unknown resource {:?}.",
                    snd.resource
                ),
            );
        } else {
            let has_trim_or_env =
                snd.envelope.is_some() || snd.in_point.is_some() || snd.out_point.is_some();
            let disp = if has_trim_or_env && profile.is_legacy_binary() {
                if options.allow_approximations {
                    FlaFeatureDisposition::Approximated
                } else {
                    FlaFeatureDisposition::Blocking
                }
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "frame_sound",
                FlaFeatureCategory::MediaAndSound,
                disp,
                kf_loc,
                format!("Frame sound ({:?}) inspected.", snd.sync),
            );
        }
    }

    if !bake_video && let Some(v) = &kf.video {
        let xfl_bundles_embedded_stream = !profile.is_legacy_binary()
            && v.deployment.is_embedded()
            && v.stream_url.is_none()
            && v.external_path.is_none()
            && v.start_offset_ms == 0
            && v.end_offset_ms.is_none()
            && v.cue_points.is_empty()
            && project.resource(&v.resource).is_some_and(|r| r.is_video());
        if xfl_bundles_embedded_stream {
            report.push(
                "timeline_video_serialized",
                FlaFeatureCategory::MediaAndSound,
                FlaFeatureDisposition::Exact,
                kf_loc,
                "Embedded timeline video stream is bundled as an XFL DOMVideoItem.".to_owned(),
            );
        } else {
            report.push(
                "timeline_video_not_serialized",
                FlaFeatureCategory::MediaAndSound,
                if options.allow_approximations {
                    FlaFeatureDisposition::Approximated
                } else {
                    FlaFeatureDisposition::Blocking
                },
                kf_loc,
                "Timeline video clip-trim, external deployment or cue-point metadata is not serialized by the target FLA/XFL format; approximation keeps the video item and authored viewport."
                    .to_owned(),
            );
        }
    }

    // Elements
    for el in &kf.elements {
        if bake_video && matches!(el.kind, ElementKind::Video { .. }) {
            continue;
        }
        let el_loc = format!("{}/Element({})", kf_loc, el.id);
        preflight_element(
            project,
            el,
            &el_loc,
            profile,
            options,
            bitmap_ids,
            has_avm1,
            has_avm2,
            has_web_scripts,
            report,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn preflight_element(
    project: &Project,
    el: &Element,
    el_loc: &str,
    profile: FlaTargetProfile,
    options: &FlaExportOptions,
    bitmap_ids: &BTreeSet<Id>,
    has_avm1: &mut bool,
    has_avm2: &mut bool,
    has_web_scripts: &mut bool,
    report: &mut FlaCompatibilityReport,
) {
    // Object scripts
    for os in &el.scripts {
        let os_loc = format!("{}:on({:?})", el_loc, os.event);
        classify_script_language(
            os.script.language,
            &os_loc,
            profile,
            options,
            has_avm1,
            has_avm2,
            has_web_scripts,
            report,
        );
        if os.script.source.trim().is_empty() {
            continue;
        }
        if os.script.language == ScriptLanguage::ActionScript3 {
            report.push(
                "object_script_as3_unsupported",
                FlaFeatureCategory::Scripts,
                if options.strip_unsupported_scripts {
                    FlaFeatureDisposition::Omitted
                } else {
                    FlaFeatureDisposition::Blocking
                },
                &os_loc,
                "ActionScript 3.0 does not support instance object scripts (on/onClipEvent); convert to frame scripts or a class.".to_owned(),
            );
        } else if matches!(
            os.script.language,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        ) {
            match &el.kind {
                ElementKind::Instance {
                    symbol_id,
                    playback,
                } => {
                    let target_kind = project
                        .symbols
                        .iter()
                        .find(|s| &s.id == symbol_id)
                        .map(|s| s.kind);
                    if target_kind == Some(SymbolKind::Graphic)
                        || !matches!(playback, InstancePlayback::Independent)
                    {
                        report.push(
                            "object_script_invalid_target",
                            FlaFeatureCategory::Scripts,
                            if options.strip_unsupported_scripts {
                                FlaFeatureDisposition::Omitted
                            } else {
                                FlaFeatureDisposition::Blocking
                            },
                            &os_loc,
                            "Graphic symbol instances cannot carry Flash object scripts; use a MovieClip or Button instance.".to_owned(),
                        );
                    } else if target_kind == Some(SymbolKind::Button)
                        && !matches!(os.event, ObjectEvent::Click)
                        && !script_starts_with_event_wrapper(&os.script.source)
                    {
                        report.push(
                            "object_script_invalid_button_event",
                            FlaFeatureCategory::Scripts,
                            if options.strip_unsupported_scripts {
                                FlaFeatureDisposition::Omitted
                            } else {
                                FlaFeatureDisposition::Blocking
                            },
                            &os_loc,
                            format!(
                                "Button instances only support click/key events in Flash, not {:?}.",
                                os.event
                            ),
                        );
                    }
                }
                _ => {
                    report.push(
                        "object_script_invalid_target",
                        FlaFeatureCategory::Scripts,
                        if options.strip_unsupported_scripts {
                            FlaFeatureDisposition::Omitted
                        } else {
                            FlaFeatureDisposition::Blocking
                        },
                        &os_loc,
                        "Object scripts on non-symbol elements (shapes/bitmaps/text/video) cannot run in Flash; convert the element to a symbol first.".to_owned(),
                    );
                }
            }
        }
    }

    // Filters & Blend modes
    preflight_filter_stack(&el.filters, el_loc, profile, report);
    if !el.filters.is_empty() || !el.blend_mode.is_normal() {
        let is_instance_or_text = matches!(
            el.kind,
            ElementKind::Instance { .. } | ElementKind::Text { .. } | ElementKind::TextBox { .. }
        );
        let disp = if matches!(el.kind, ElementKind::Bitmap { .. }) {
            if profile.is_legacy_binary() || !el.blend_mode.is_normal() {
                if options.allow_approximations {
                    FlaFeatureDisposition::Approximated
                } else {
                    FlaFeatureDisposition::Blocking
                }
            } else {
                FlaFeatureDisposition::Exact
            }
        } else if profile.is_legacy_binary() && !is_instance_or_text {
            if options.allow_approximations {
                FlaFeatureDisposition::Approximated
            } else {
                FlaFeatureDisposition::Blocking
            }
        } else {
            FlaFeatureDisposition::Exact
        };
        report.push(
            "element_filters_or_blend",
            FlaFeatureCategory::FiltersAndBlends,
            disp,
            el_loc,
            "Display filters / blend mode inspected.".to_owned(),
        );
    }

    // Variable-width stroke profile
    if !el.stroke_width_profile().is_default() {
        let disp = if profile.supports_variable_width_strokes() && !profile.is_legacy_binary() {
            FlaFeatureDisposition::Converted
        } else {
            FlaFeatureDisposition::Baked
        };
        report.push(
            "variable_width_stroke",
            FlaFeatureCategory::ShapesAndPaints,
            disp,
            el_loc,
            "Variable-width stroke profile expanded to filled vector contour.".to_owned(),
        );
    }

    match &el.kind {
        ElementKind::Rectangle { .. } | ElementKind::Ellipse { .. } => {
            let disp = if el.has_parametric_options() && profile.is_legacy_binary() {
                FlaFeatureDisposition::Converted
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "primitive_shape",
                FlaFeatureCategory::ShapesAndPaints,
                disp,
                el_loc,
                "Vector primitive shape inspected.".to_owned(),
            );
        }
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            if let ElementKind::PaintedPath {
                fill_gradient,
                stroke_gradient,
                ..
            } = &el.kind
            {
                for grad in [fill_gradient.as_ref(), stroke_gradient.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    if grad.stops.is_empty() || grad.stops.len() > 15 {
                        report.push(
                            "gradient_stop_count_limit",
                            FlaFeatureCategory::ShapesAndPaints,
                            FlaFeatureDisposition::Blocking,
                            el_loc,
                            format!(
                                "Shape gradient has {} stops, outside Flash/XFL supported range (1..=15).",
                                grad.stops.len()
                            ),
                        );
                    } else if profile.is_legacy_binary()
                        && (grad.spread != GradientSpread::Extend || grad.linear_rgb)
                    {
                        report.push(
                            "legacy_gradient_mode_approximated",
                            FlaFeatureCategory::ShapesAndPaints,
                            if options.allow_approximations {
                                FlaFeatureDisposition::Approximated
                            } else {
                                FlaFeatureDisposition::Blocking
                            },
                            el_loc,
                            "Gradient spreadMode/linearRGB is approximated as pad/RGB in legacy binary FLA.".to_owned(),
                        );
                    }
                }
            }
            let has_cubic = commands
                .iter()
                .any(|c| matches!(c, PathCommand::CubicTo { .. }));
            let disp = if has_cubic {
                FlaFeatureDisposition::Converted
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "vector_path",
                FlaFeatureCategory::ShapesAndPaints,
                disp,
                el_loc,
                if has_cubic {
                    "Cubic Bezier segments converted to quadratic curves for Flash/XFL.".to_owned()
                } else {
                    "Quadratic/linear vector path preserved.".to_owned()
                },
            );
        }
        ElementKind::Text { .. } | ElementKind::TextBox { .. } => {
            let disp = if options.outline_unsupported_text
                || options.font_policy == FontExportPolicy::Outlines
            {
                FlaFeatureDisposition::Outlined
            } else {
                FlaFeatureDisposition::Exact
            };
            report.push(
                "text_element",
                FlaFeatureCategory::TextAndFonts,
                disp,
                el_loc,
                if disp == FlaFeatureDisposition::Outlined {
                    "Text element converted to vector glyph outlines.".to_owned()
                } else {
                    "Text element preserved.".to_owned()
                },
            );
        }
        ElementKind::Bitmap { resource_id, .. } => {
            if !bitmap_ids.contains(resource_id) {
                report.push(
                    "missing_bitmap_resource",
                    FlaFeatureCategory::MediaAndSound,
                    FlaFeatureDisposition::Blocking,
                    el_loc,
                    format!(
                        "Bitmap element references missing image resource {:?}.",
                        resource_id
                    ),
                );
            } else {
                let disp = if !el.visible || el.cache_as_bitmap {
                    if options.allow_approximations {
                        FlaFeatureDisposition::Approximated
                    } else {
                        FlaFeatureDisposition::Blocking
                    }
                } else {
                    FlaFeatureDisposition::Exact
                };
                report.push(
                    "bitmap_instance",
                    FlaFeatureCategory::MediaAndSound,
                    disp,
                    el_loc,
                    "Bitmap instance inspected.".to_owned(),
                );
            }
        }
        ElementKind::Video { .. } => {
            report.push(
                "video_instance",
                FlaFeatureCategory::MediaAndSound,
                if options.allow_approximations {
                    FlaFeatureDisposition::Approximated
                } else {
                    FlaFeatureDisposition::Blocking
                },
                el_loc,
                "Video viewport and transform are serialized as a DOMVideoItem/DOMVideoInstance placeholder; embedded FLV/video stream payload is not bundled."
                    .to_owned(),
            );
        }
        ElementKind::Instance { symbol_id, .. } => {
            if !project.symbols.iter().any(|s| &s.id == symbol_id) {
                report.push(
                    "missing_symbol_instance_target",
                    FlaFeatureCategory::SymbolsAndLibrary,
                    FlaFeatureDisposition::Blocking,
                    el_loc,
                    format!("Symbol instance references unknown symbol {:?}.", symbol_id),
                );
            } else {
                report.push(
                    "symbol_instance",
                    FlaFeatureCategory::SymbolsAndLibrary,
                    FlaFeatureDisposition::Exact,
                    el_loc,
                    "Library symbol instance preserved.".to_owned(),
                );
            }
        }
    }
}
