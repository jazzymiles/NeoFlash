use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn write_dom_symbol_item_xml(
    sym: &Symbol,
    lib_name: &str,
    project: &Project,
    symbol_lib_names: &BTreeMap<Id, String>,
    bitmap_names: &BTreeMap<Id, String>,
    sound_names: &BTreeMap<Id, String>,
    sound_durations: &BTreeMap<Id, u64>,
    video_names: &BTreeMap<VideoKey, String>,
) -> String {
    let sym_type = match sym.kind {
        SymbolKind::MovieClip => "movie clip",
        SymbolKind::Graphic => "graphic",
        SymbolKind::Button => "button",
    };
    let mut linkage_attrs = String::new();
    let has_class = sym
        .class_name
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let has_id = sym
        .linkage_id
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    if has_class || has_id {
        linkage_attrs.push_str(" linkageExportForAS=\"true\" linkageExportInFirstFrame=\"true\"");
        if let Some(cls) = sym.class_name.as_deref().filter(|s| !s.trim().is_empty()) {
            linkage_attrs.push_str(&format!(" linkageClassName=\"{}\"", xml_escape(cls.trim())));
        }
        if let Some(lid) = sym.linkage_id.as_deref().filter(|s| !s.trim().is_empty()) {
            linkage_attrs.push_str(&format!(
                " linkageIdentifier=\"{}\"",
                xml_escape(lid.trim())
            ));
        }
    }
    let inner_name = lib_name.rsplit_once('/').map_or(lib_name, |(_, b)| b);
    let timeline_xml = write_dom_timeline_xml(
        &sym.timeline,
        inner_name,
        project,
        symbol_lib_names,
        bitmap_names,
        sound_names,
        sound_durations,
        video_names,
        "    ",
    );
    format!(
        r#"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="{}" symbolType="{}"{}>
  <timeline>
{}  </timeline>
</DOMSymbolItem>
"#,
        xml_escape(lib_name),
        sym_type,
        linkage_attrs,
        timeline_xml
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn write_dom_timeline_xml(
    timeline: &Timeline,
    name: &str,
    project: &Project,
    symbol_lib_names: &BTreeMap<Id, String>,
    bitmap_names: &BTreeMap<Id, String>,
    sound_names: &BTreeMap<Id, String>,
    sound_durations: &BTreeMap<Id, u64>,
    video_names: &BTreeMap<VideoKey, String>,
    indent: &str,
) -> String {
    let mut out = format!(
        "{indent}<DOMTimeline name=\"{}\">\n{indent}  <layers>\n",
        xml_escape(name)
    );

    // XFL lists topmost layers first; NeoFlash stores painter order (bottom-to-top).
    let xfl_layers: Vec<&Layer> = timeline.layers.iter().rev().collect();
    let id_to_xfl_idx: BTreeMap<&str, usize> = xfl_layers
        .iter()
        .enumerate()
        .map(|(i, l)| (l.id.as_str(), i))
        .collect();

    // Map frame scripts by frame number so they can be attached to the first normal layer
    let mut scripts_by_frame: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for fs in &timeline.scripts {
        let marker = match fs.script.language {
            ScriptLanguage::JavaScript => "/* neoflash:language=javascript */\n",
            ScriptLanguage::TypeScript => "/* neoflash:language=typescript */\n",
            ScriptLanguage::ActionScript1 => "/* neoflash:language=actionscript1 */\n",
            ScriptLanguage::ActionScript2 => "/* neoflash:language=actionscript2 */\n",
            ScriptLanguage::ActionScript3 => "/* neoflash:language=actionscript3 */\n",
        };
        scripts_by_frame
            .entry(fs.frame)
            .or_default()
            .push(format!("{marker}{}", fs.script.source));
    }

    let first_normal_idx = xfl_layers
        .iter()
        .position(|l| l.properties.role != LayerRole::Folder);

    if xfl_layers.is_empty() || (first_normal_idx.is_none() && !scripts_by_frame.is_empty()) {
        let synth_layer = Layer {
            id: "__nf_synth_actions_layer__".to_owned(),
            name: if scripts_by_frame.is_empty() {
                "Layer 1".to_owned()
            } else {
                "Actions".to_owned()
            },
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: Vec::new(),
                ..Default::default()
            }],
            properties: neoflash_core::LayerProperties::default(),
        };
        out.push_str(&write_dom_layer_xml(
            &synth_layer,
            timeline.frame_count.max(1),
            &id_to_xfl_idx,
            Some(&scripts_by_frame),
            project,
            symbol_lib_names,
            bitmap_names,
            sound_names,
            sound_durations,
            video_names,
            &format!("{indent}    "),
        ));
    }
    for (xfl_idx, layer) in xfl_layers.iter().copied().enumerate() {
        let attach_scripts = if Some(xfl_idx) == first_normal_idx {
            Some(&scripts_by_frame)
        } else {
            None
        };
        out.push_str(&write_dom_layer_xml(
            layer,
            timeline.frame_count.max(1),
            &id_to_xfl_idx,
            attach_scripts,
            project,
            symbol_lib_names,
            bitmap_names,
            sound_names,
            sound_durations,
            video_names,
            &format!("{indent}    "),
        ));
    }

    out.push_str(&format!("{indent}  </layers>\n{indent}</DOMTimeline>\n"));
    out
}

#[allow(clippy::too_many_arguments)]
pub(super) fn write_dom_layer_xml(
    layer: &Layer,
    timeline_frame_count: u32,
    id_to_xfl_idx: &BTreeMap<&str, usize>,
    frame_scripts: Option<&BTreeMap<u32, Vec<String>>>,
    project: &Project,
    symbol_lib_names: &BTreeMap<Id, String>,
    bitmap_names: &BTreeMap<Id, String>,
    sound_names: &BTreeMap<Id, String>,
    sound_durations: &BTreeMap<Id, u64>,
    video_names: &BTreeMap<VideoKey, String>,
    indent: &str,
) -> String {
    let mut attrs = format!(
        "name=\"{}\"",
        xml_escape(if layer.name.trim().is_empty() {
            "Layer"
        } else {
            &layer.name
        })
    );
    match layer.properties.role {
        LayerRole::Normal => {}
        LayerRole::Guide => attrs.push_str(" layerType=\"guide\""),
        LayerRole::Mask => attrs.push_str(" layerType=\"mask\""),
        LayerRole::Folder => {
            attrs.push_str(" layerType=\"folder\"");
            if layer.properties.collapsed {
                attrs.push_str(" open=\"false\"");
            }
        }
    }
    if !layer.visible {
        attrs.push_str(" visible=\"false\"");
    }
    if layer.locked {
        attrs.push_str(" locked=\"true\"");
    }
    let parent_target = layer
        .properties
        .mask
        .as_deref()
        .or(layer.properties.motion_guide.as_deref())
        .or(layer.properties.parent.as_deref());
    if let Some(pid) = parent_target
        && let Some(&p_idx) = id_to_xfl_idx.get(pid)
    {
        attrs.push_str(&format!(" parentLayerIndex=\"{p_idx}\""));
    }

    if layer.properties.role == LayerRole::Folder {
        return format!("{indent}<DOMLayer {attrs}>\n{indent}  <frames/>\n{indent}</DOMLayer>\n");
    }

    let mut out = format!("{indent}<DOMLayer {attrs}>\n{indent}  <frames>\n");
    let effective_end = layer
        .properties
        .end_frame
        .unwrap_or(timeline_frame_count)
        .min(timeline_frame_count)
        .max(1);

    let mut keyframes = layer.keyframes.clone();
    keyframes.sort_by_key(|k| k.frame);
    if keyframes.is_empty() || keyframes[0].frame > 0 {
        keyframes.insert(
            0,
            Keyframe {
                frame: 0,
                elements: Vec::new(),
                ..Default::default()
            },
        );
    }

    // Ensure keyframes exist for any attached frame scripts
    if let Some(scripts_map) = frame_scripts {
        for &script_frame in scripts_map.keys() {
            if script_frame < effective_end && !keyframes.iter().any(|k| k.frame == script_frame) {
                let held = keyframes
                    .iter()
                    .rev()
                    .find(|k| k.frame < script_frame)
                    .map(|k| (k.elements.clone(), k.bitmap_fills.clone()))
                    .unwrap_or_default();
                keyframes.push(Keyframe {
                    frame: script_frame,
                    elements: held.0,
                    bitmap_fills: held.1,
                    ..Default::default()
                });
                keyframes.sort_by_key(|k| k.frame);
            }
        }
    }

    let mut active_vid_res: Option<Id> = None;
    for (idx, kf) in keyframes.iter().enumerate() {
        if kf.frame >= effective_end {
            break;
        }
        if let Some(span) = &kf.video {
            active_vid_res = project
                .resource(&span.resource)
                .is_some_and(|r| !is_internal_provenance_resource(r) && r.is_video())
                .then(|| span.resource.clone());
        } else if !kf
            .elements
            .iter()
            .any(|e| matches!(e.kind, ElementKind::Video { .. }))
        {
            active_vid_res = None;
        }
        let next_start = keyframes
            .get(idx + 1)
            .map(|n| n.frame.min(effective_end))
            .unwrap_or(effective_end);
        let duration = next_start.saturating_sub(kf.frame).max(1);

        let mut f_attrs = format!("index=\"{}\" duration=\"{duration}\"", kf.frame);
        if !kf.label.trim().is_empty() {
            f_attrs.push_str(&format!(
                " name=\"{}\" labelType=\"name\"",
                xml_escape(&kf.label)
            ));
        }

        let mut custom_ease_bezier: Option<[f64; 4]> = None;
        if let Some(easing) = kf.tween {
            match kf.tween_kind {
                TweenKind::Shape => f_attrs.push_str(" tweenType=\"shape\""),
                TweenKind::Classic | TweenKind::Motion => f_attrs.push_str(" tweenType=\"motion\""),
            }
            match easing {
                Easing::Linear => {}
                Easing::EaseIn => f_attrs.push_str(" acceleration=\"-100\""),
                Easing::EaseOut => f_attrs.push_str(" acceleration=\"100\""),
                Easing::Flash { acceleration } => {
                    let accel = acceleration.round().clamp(-100.0, 100.0) as i32;
                    if accel != 0 {
                        f_attrs.push_str(&format!(" acceleration=\"{accel}\""));
                    }
                }
                Easing::EaseInOut => {
                    f_attrs.push_str(" hasCustomEase=\"true\" useSingleEaseCurve=\"true\"");
                    custom_ease_bezier = Some([0.42, 0.0, 0.58, 1.0]);
                }
                Easing::Bounce | Easing::Elastic => {
                    f_attrs.push_str(" hasCustomEase=\"true\" useSingleEaseCurve=\"true\"");
                    custom_ease_bezier = Some([0.34, 1.56, 0.64, 1.0]);
                }
                Easing::CubicBezier { x1, y1, x2, y2 } => {
                    f_attrs.push_str(" hasCustomEase=\"true\" useSingleEaseCurve=\"true\"");
                    custom_ease_bezier = Some([
                        x1.clamp(0.0, 1.0),
                        y1.clamp(-100.0, 100.0),
                        x2.clamp(0.0, 1.0),
                        y2.clamp(-100.0, 100.0),
                    ]);
                }
            }
            if kf.orient_to_path {
                f_attrs.push_str(" motionTweenOrientToPath=\"true\"");
            }
        }

        let mut sound_envelope_points: Option<Vec<(u64, u32, u32)>> = None;
        if let Some(snd) = &kf.sound
            && let Some(snd_name) = sound_names.get(&snd.resource)
        {
            let sync_str = match snd.sync {
                SoundSync::Event => "event",
                SoundSync::Start => "start",
                SoundSync::Stop => "stop",
                SoundSync::Stream => "stream",
            };
            f_attrs.push_str(&format!(
                " soundName=\"{}\" soundSync=\"{sync_str}\"",
                xml_escape(snd_name)
            ));
            if snd.loop_forever {
                f_attrs.push_str(" soundLoopMode=\"loop\"");
            } else if snd.loops > 1 {
                f_attrs.push_str(&format!(
                    " soundLoopMode=\"repeat\" soundLoop=\"{}\"",
                    snd.loops.min(FrameSound::MAX_LOOPS)
                ));
            }
            if let Some(in_sec) = snd.in_point
                && in_sec.is_finite()
                && in_sec > 0.0
            {
                let in_44 = (in_sec.clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS) * 44_100.0)
                    .round() as u64;
                if in_44 > 0 {
                    f_attrs.push_str(&format!(" inPoint44=\"{in_44}\""));
                }
            }
            if let Some(out_sec) = snd.out_point
                && out_sec.is_finite()
                && out_sec > 0.0
            {
                let out_44 = (out_sec.clamp(0.0, FrameSound::MAX_SOUND_TRIM_SECONDS) * 44_100.0)
                    .round() as u64;
                if out_44 > 0 {
                    f_attrs.push_str(&format!(" outPoint44=\"{out_44}\""));
                }
            }
            let has_custom_env = snd.effect == SoundEffect::Custom
                || snd.envelope.as_ref().is_some_and(|v| !v.is_empty());
            let effect_str = if has_custom_env {
                Some("custom")
            } else {
                match snd.effect {
                    SoundEffect::None => None,
                    SoundEffect::LeftChannel => Some("left channel"),
                    SoundEffect::RightChannel => Some("right channel"),
                    SoundEffect::FadeLeftToRight => Some("fade left to right"),
                    SoundEffect::FadeRightToLeft => Some("fade right to left"),
                    SoundEffect::FadeIn => Some("fade in"),
                    SoundEffect::FadeOut => Some("fade out"),
                    SoundEffect::Custom => Some("custom"),
                }
            };
            if let Some(eff) = effect_str {
                f_attrs.push_str(&format!(" soundEffect=\"{eff}\""));
            }
            if has_custom_env {
                let dur_44 = sound_durations
                    .get(&snd.resource)
                    .copied()
                    .unwrap_or(44_100);
                let pts = encode_sound_envelope_points(snd, dur_44);
                if !pts.is_empty() {
                    sound_envelope_points = Some(pts);
                }
            }
        }

        out.push_str(&format!("{indent}    <DOMFrame {f_attrs}>\n"));

        if let Some([x1, y1, x2, y2]) = custom_ease_bezier {
            out.push_str(&format!(
                "{indent}      <tweens>\n{indent}        <CustomEase target=\"all\">\n{indent}          <Point x=\"0\" y=\"0\"/>\n{indent}          <Point x=\"{}\" y=\"{}\"/>\n{indent}          <Point x=\"{}\" y=\"{}\"/>\n{indent}          <Point x=\"1\" y=\"1\"/>\n{indent}        </CustomEase>\n{indent}      </tweens>\n",
                fmt_num(x1),
                fmt_num(y1),
                fmt_num(x2),
                fmt_num(y2),
            ));
        }

        if let Some(env_pts) = sound_envelope_points {
            out.push_str(&format!("{indent}      <SoundEnvelope>\n"));
            for (mark44, level0, level1) in env_pts {
                out.push_str(&format!(
                    "{indent}        <SoundEnvelopePoint mark44=\"{mark44}\" level0=\"{level0}\" level1=\"{level1}\"/>\n"
                ));
            }
            out.push_str(&format!("{indent}      </SoundEnvelope>\n"));
        }

        if let Some(scripts_map) = frame_scripts
            && let Some(sources) = scripts_map.get(&kf.frame)
            && !sources.is_empty()
        {
            let joined = sources.join("\n");
            out.push_str(&format!(
                "{indent}      <Actionscript>\n{indent}        <script>{}</script>\n{indent}      </Actionscript>\n",
                xml_escape(&joined)
            ));
        }

        if !kf.elements.is_empty() {
            let eff_vid_res = kf
                .video
                .as_ref()
                .filter(|v| {
                    project
                        .resource(&v.resource)
                        .is_some_and(|r| !is_internal_provenance_resource(r) && r.is_video())
                })
                .map(|v| v.resource.clone())
                .or_else(|| active_vid_res.clone());
            out.push_str(&format!("{indent}      <elements>\n"));
            for el in &kf.elements {
                out.push_str(&write_dom_element_xml(
                    el,
                    kf.bitmap_fill(&el.id).and_then(|fill| {
                        bitmap_names
                            .get(&fill.resource_id)
                            .map(|name| (fill, name.as_str()))
                    }),
                    eff_vid_res.as_ref(),
                    project,
                    symbol_lib_names,
                    bitmap_names,
                    video_names,
                    &format!("{indent}        "),
                ));
            }
            out.push_str(&format!("{indent}      </elements>\n"));
        }

        out.push_str(&format!("{indent}    </DOMFrame>\n"));
    }

    out.push_str(&format!("{indent}  </frames>\n{indent}</DOMLayer>\n"));
    out
}

#[allow(clippy::too_many_arguments)]
pub(super) fn write_dom_element_xml(
    el: &Element,
    bitmap_paint: Option<(&AuthoredBitmapFill, &str)>,
    frame_video_resource: Option<&Id>,
    project: &Project,
    symbol_lib_names: &BTreeMap<Id, String>,
    bitmap_names: &BTreeMap<Id, String>,
    video_names: &BTreeMap<VideoKey, String>,
    indent: &str,
) -> String {
    let matrix_xml = write_matrix_xml(&el.transform, &format!("{indent}  "));
    let color_xml = write_color_transform_xml(&el.transform, &format!("{indent}  "));
    let filters_xml = write_filters_xml(&el.filters, &format!("{indent}  "));

    // XFL primitive objects cannot carry a bitmap brush on reimport. Use their exact contour.
    if bitmap_paint.is_some()
        && let ElementKind::Rectangle {
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
        } = &el.kind
        && let Some((commands, _)) = el.to_path_commands()
    {
        return write_dom_shape_xml(
            el,
            &commands,
            *fill,
            None,
            bitmap_paint,
            *stroke,
            None,
            *stroke_width,
            indent,
        );
    }

    match &el.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let mut attrs = format!(
                "objectWidth=\"{}\" objectHeight=\"{}\"",
                fmt_num(*width),
                fmt_num(*height)
            );
            if !el.name.trim().is_empty() && el.name != "DOMRectangleObject" {
                attrs.push_str(&format!(" name=\"{}\"", xml_escape(&el.name)));
            }
            let rect_opts = el.rectangle_options();
            if rect_opts.has_rounded_corners() {
                attrs.push_str(&format!(
                    " topLeftRadius=\"{}\" topRightRadius=\"{}\" bottomLeftRadius=\"{}\" bottomRightRadius=\"{}\"",
                    fmt_num(rect_opts.corner_radii[0]),
                    fmt_num(rect_opts.corner_radii[1]),
                    fmt_num(rect_opts.corner_radii[2]),
                    fmt_num(rect_opts.corner_radii[3]),
                ));
                if !rect_opts.corners_locked {
                    attrs.push_str(" lockFlag=\"false\"");
                }
            }
            let fill_xml = if fill[3] > 0 {
                format!(
                    "{indent}  <fill>\n{indent}    <SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>\n{indent}  </fill>\n",
                    fill[0],
                    fill[1],
                    fill[2],
                    fmt_num(f64::from(fill[3]) / 255.0)
                )
            } else {
                String::new()
            };
            let stroke_xml = if *stroke_width > 0.0 && stroke[3] > 0 {
                format!(
                    "{indent}  <stroke>\n{indent}    <SolidStroke scaleMode=\"normal\" weight=\"{}\">\n{indent}      <fill>\n{indent}        <SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>\n{indent}      </fill>\n{indent}    </SolidStroke>\n{indent}  </stroke>\n",
                    fmt_num(*stroke_width),
                    stroke[0],
                    stroke[1],
                    stroke[2],
                    fmt_num(f64::from(stroke[3]) / 255.0)
                )
            } else {
                String::new()
            };
            format!(
                "{indent}<DOMRectangleObject {attrs}>\n{matrix_xml}{color_xml}{filters_xml}{fill_xml}{stroke_xml}{indent}</DOMRectangleObject>\n"
            )
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let mut attrs = format!(
                "objectWidth=\"{}\" objectHeight=\"{}\"",
                fmt_num(*width),
                fmt_num(*height)
            );
            if !el.name.trim().is_empty() && el.name != "DOMOvalObject" {
                attrs.push_str(&format!(" name=\"{}\"", xml_escape(&el.name)));
            }
            let oval_opts = el.oval_options();
            if !oval_opts.is_default() {
                attrs.push_str(&format!(
                    " startAngle=\"{}\" endAngle=\"{}\" innerRadius=\"{}\" closePath=\"{}\"",
                    fmt_num(oval_opts.start_angle),
                    fmt_num(oval_opts.end_angle),
                    fmt_num(oval_opts.inner_radius),
                    oval_opts.close_path
                ));
            }
            let fill_xml = if fill[3] > 0 {
                format!(
                    "{indent}  <fill>\n{indent}    <SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>\n{indent}  </fill>\n",
                    fill[0],
                    fill[1],
                    fill[2],
                    fmt_num(f64::from(fill[3]) / 255.0)
                )
            } else {
                String::new()
            };
            let stroke_xml = if *stroke_width > 0.0 && stroke[3] > 0 {
                format!(
                    "{indent}  <stroke>\n{indent}    <SolidStroke scaleMode=\"normal\" weight=\"{}\">\n{indent}      <fill>\n{indent}        <SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>\n{indent}      </fill>\n{indent}    </SolidStroke>\n{indent}  </stroke>\n",
                    fmt_num(*stroke_width),
                    stroke[0],
                    stroke[1],
                    stroke[2],
                    fmt_num(f64::from(stroke[3]) / 255.0)
                )
            } else {
                String::new()
            };
            format!(
                "{indent}<DOMOvalObject {attrs}>\n{matrix_xml}{color_xml}{filters_xml}{fill_xml}{stroke_xml}{indent}</DOMOvalObject>\n"
            )
        }
        ElementKind::Path {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        } => write_dom_shape_xml(
            el,
            commands,
            *fill,
            None,
            bitmap_paint,
            *stroke,
            None,
            *stroke_width,
            indent,
        ),
        ElementKind::PaintedPath {
            commands,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => write_dom_shape_xml(
            el,
            commands,
            *fill,
            fill_gradient.as_ref(),
            bitmap_paint,
            *stroke,
            stroke_gradient.as_ref(),
            *stroke_width,
            indent,
        ),
        ElementKind::Instance {
            symbol_id,
            playback,
        } => {
            let Some(lib_name) = symbol_lib_names.get(symbol_id) else {
                return String::new();
            };
            let target_kind = project
                .symbols
                .iter()
                .find(|s| &s.id == symbol_id)
                .map(|s| s.kind)
                .unwrap_or(SymbolKind::MovieClip);
            let mut attrs = format!("libraryItemName=\"{}\"", xml_escape(lib_name));
            if !el.name.trim().is_empty() && el.name != "DOMSymbolInstance" {
                attrs.push_str(&format!(" name=\"{}\"", xml_escape(&el.name)));
            }
            match (target_kind, playback) {
                (SymbolKind::Button, _) => attrs.push_str(" symbolType=\"button\""),
                (_, InstancePlayback::SingleFrame(f)) => {
                    attrs.push_str(&format!(
                        " symbolType=\"graphic\" loop=\"single frame\" firstFrame=\"{f}\""
                    ));
                }
                (_, InstancePlayback::Loop) => {
                    attrs.push_str(" symbolType=\"graphic\" loop=\"loop\"");
                }
                (_, InstancePlayback::LoopFrom(f)) => {
                    attrs.push_str(&format!(
                        " symbolType=\"graphic\" loop=\"loop\" firstFrame=\"{f}\""
                    ));
                }
                (_, InstancePlayback::PlayOnce) => {
                    attrs.push_str(" symbolType=\"graphic\" loop=\"play once\"");
                }
                (_, InstancePlayback::PlayOnceFrom(f)) => {
                    attrs.push_str(&format!(
                        " symbolType=\"graphic\" loop=\"play once\" firstFrame=\"{f}\""
                    ));
                }
                (_, InstancePlayback::Independent) => {
                    if target_kind == SymbolKind::Graphic {
                        attrs.push_str(" symbolType=\"graphic\" loop=\"loop\"");
                    } else {
                        attrs.push_str(" symbolType=\"movie clip\"");
                    }
                }
            }
            if !el.visible {
                attrs.push_str(" isVisible=\"false\"");
            }
            if el.cache_as_bitmap {
                attrs.push_str(" cacheAsBitmap=\"true\"");
            }
            if !el.blend_mode.is_normal() {
                attrs.push_str(&format!(" blendMode=\"{}\"", el.blend_mode.as_str()));
            }
            let scripts_xml = write_object_scripts_xml(&el.scripts, &format!("{indent}  "));
            format!(
                "{indent}<DOMSymbolInstance {attrs}>\n{matrix_xml}{color_xml}{filters_xml}{scripts_xml}{indent}</DOMSymbolInstance>\n"
            )
        }
        ElementKind::Bitmap { resource_id, .. } => {
            let Some(bmp_name) = bitmap_names.get(resource_id) else {
                return String::new();
            };
            format!(
                "{indent}<DOMBitmapInstance libraryItemName=\"{}\">\n{matrix_xml}{color_xml}{filters_xml}{indent}</DOMBitmapInstance>\n",
                xml_escape(bmp_name)
            )
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            let hex = format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]);
            let alpha_attr = if color[3] < 255 {
                format!(" alpha=\"{}\"", fmt_num(f64::from(color[3]) / 255.0))
            } else {
                String::new()
            };
            format!(
                "{indent}<DOMStaticText isSelectable=\"false\">\n{matrix_xml}{color_xml}{filters_xml}{indent}  <textRuns>\n{indent}    <DOMTextRun>\n{indent}      <characters>{}</characters>\n{indent}      <textAttrs>\n{indent}        <DOMTextAttrs face=\"{}\" size=\"{}\" fillColor=\"{hex}\"{alpha_attr}/>\n{indent}      </textAttrs>\n{indent}    </DOMTextRun>\n{indent}  </textRuns>\n{indent}</DOMStaticText>\n",
                xml_escape(text),
                xml_escape(if font_family.trim().is_empty() {
                    "Arial"
                } else {
                    font_family
                }),
                fmt_num(*font_size)
            )
        }
        ElementKind::TextBox {
            text,
            width,
            height,
            format,
            word_wrap,
            multiline,
        } => {
            let tag = if !el.name.trim().is_empty() && el.name != "DOMStaticText" {
                "DOMDynamicText"
            } else {
                "DOMStaticText"
            };
            let line_type = if *multiline {
                if *word_wrap {
                    "multiline"
                } else {
                    "multiline no wrap"
                }
            } else {
                "single line"
            };
            let mut attrs = format!(
                "width=\"{}\" height=\"{}\" lineType=\"{line_type}\"",
                fmt_num(*width),
                fmt_num(*height)
            );
            if !el.name.trim().is_empty()
                && !matches!(el.name.as_str(), "DOMStaticText" | "DOMDynamicText")
            {
                attrs.push_str(&format!(" name=\"{}\"", xml_escape(&el.name)));
            }
            let r = ((format.color >> 16) & 0xFF) as u8;
            let g = ((format.color >> 8) & 0xFF) as u8;
            let b = (format.color & 0xFF) as u8;
            let hex = format!("#{r:02X}{g:02X}{b:02X}");
            let align = match format.align {
                TextAlign::Left => "left",
                TextAlign::Center => "center",
                TextAlign::Right => "right",
            };
            let mut t_attrs = format!(
                "face=\"{}\" size=\"{}\" fillColor=\"{hex}\" alignment=\"{align}\"",
                xml_escape(if format.font.trim().is_empty() {
                    "Arial"
                } else {
                    &format.font
                }),
                fmt_num(format.size)
            );
            if format.bold {
                t_attrs.push_str(" bold=\"true\"");
            }
            if format.italic {
                t_attrs.push_str(" italic=\"true\"");
            }
            if format.underline {
                t_attrs.push_str(" underline=\"true\"");
            }
            if format.letter_spacing != 0.0 {
                t_attrs.push_str(&format!(
                    " letterSpacing=\"{}\"",
                    fmt_num(format.letter_spacing)
                ));
            }
            if format.leading != 0.0 {
                t_attrs.push_str(&format!(" lineSpacing=\"{}\"", fmt_num(format.leading)));
            }
            format!(
                "{indent}<{tag} {attrs}>\n{matrix_xml}{color_xml}{filters_xml}{indent}  <textRuns>\n{indent}    <DOMTextRun>\n{indent}      <characters>{}</characters>\n{indent}      <textAttrs>\n{indent}        <DOMTextAttrs {t_attrs}/>\n{indent}      </textAttrs>\n{indent}    </DOMTextRun>\n{indent}  </textRuns>\n{indent}</{tag}>\n",
                xml_escape(text)
            )
        }
        ElementKind::Video { width, height } => {
            let v_key = VideoKey::from_dims(frame_video_resource.cloned(), *width, *height);
            let Some(v_name) = video_names
                .get(&v_key)
                .or_else(|| video_names.get(&VideoKey::from_dims(None, *width, *height)))
            else {
                return String::new();
            };
            let right_twips = ((*width).max(1.0) * 20.0).round() as i64;
            let bottom_twips = ((*height).max(1.0) * 20.0).round() as i64;
            format!(
                "{indent}<DOMVideoInstance libraryItemName=\"{}\" frameLeft=\"0\" frameTop=\"0\" frameRight=\"{right_twips}\" frameBottom=\"{bottom_twips}\">\n{matrix_xml}{color_xml}{filters_xml}{indent}</DOMVideoInstance>\n",
                xml_escape(v_name)
            )
        }
    }
}
