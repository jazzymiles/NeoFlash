use super::*;

// ============================================================================
// 3. Self-Building Adobe After Effects ExtendScript (`.jsx`) Constructor
// ============================================================================

fn js_str(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap_or_else(|_| "\"\"".into())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

pub(super) fn render_extendscript_jsx(model: &AeProjectModel, aep_path: &Path) -> String {
    let target_aep_name = aep_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project.aep");

    let mut js = String::with_capacity(32_768);
    js.push_str("/* NeoFlash -> Adobe After Effects Interchange Builder (T-246)\n");
    js.push_str(" * Run in After Effects via: File > Scripts > Run Script File...\n");
    js.push_str(
        " * Reconstructs native editable Bezier Shape Layers, Gradients, Strokes, Pre-Comps,\n",
    );
    js.push_str(
        " * Time Remapping, Track Mattes, Filters, Text, Footage trims, and AE JS Expressions.\n",
    );
    js.push_str(" */\n");
    js.push_str("(function buildNeoFlashAfterEffectsProject() {\n");
    js.push_str("  if (typeof app === 'undefined' || !app.project) { return; }\n");
    js.push_str("  app.beginUndoGroup('Import NeoFlash Project');\n");
    js.push_str("  try {\n");
    js.push_str("    if (app.project.expressionEngine !== undefined) { try { app.project.expressionEngine = 'javascript-1.0'; } catch (_) {} }\n");
    js.push_str("    var scriptFile = new File($.fileName);\n");
    js.push_str("    var baseFolder = scriptFile.parent;\n");
    js.push_str(
        "    function hasOwn(m, k) { return Object.prototype.hasOwnProperty.call(m, k); }\n",
    );
    js.push_str("    var folders = {};\n");
    js.push_str("    function ensureFolder(path) {\n");
    js.push_str("      if (!path) return app.project.rootFolder;\n");
    js.push_str("      if (hasOwn(folders, path)) return folders[path];\n");
    js.push_str("      var parts = path.split('/');\n");
    js.push_str("      var cur = app.project.rootFolder;\n");
    js.push_str("      var acc = '';\n");
    js.push_str("      for (var i = 0; i < parts.length; i++) {\n");
    js.push_str("        if (!parts[i]) continue;\n");
    js.push_str("        acc = acc ? (acc + '/' + parts[i]) : parts[i];\n");
    js.push_str("        if (!hasOwn(folders, acc)) {\n");
    js.push_str("          var f = app.project.items.addFolder(parts[i]);\n");
    js.push_str("          f.parentFolder = cur;\n");
    js.push_str("          folders[acc] = f;\n");
    js.push_str("        }\n");
    js.push_str("        cur = folders[acc];\n");
    js.push_str("      }\n");
    js.push_str("      return cur;\n");
    js.push_str("    }\n");
    js.push_str("    function setKeyed1D(prop, keys, fps) {\n");
    js.push_str("      if (!keys || keys.length === 0) return;\n");
    js.push_str("      if (keys.length === 1) { prop.setValue(keys[0][1]); return; }\n");
    js.push_str("      for (var i = 0; i < keys.length; i++) {\n");
    js.push_str("        var k = keys[i];\n");
    js.push_str("        prop.setValueAtTime(k[0] / fps, k[1]);\n");
    js.push_str("      }\n");
    js.push_str("      for (var j = 0; j < keys.length; j++) {\n");
    js.push_str("        var ke = keys[j];\n");
    js.push_str("        var idx = j + 1;\n");
    js.push_str("        if (ke[4]) {\n");
    js.push_str("          prop.setInterpolationTypeAtKey(idx, KeyframeInterpolationType.LINEAR, KeyframeInterpolationType.HOLD);\n");
    js.push_str("        } else {\n");
    js.push_str(
        "          var easeIn = new KeyframeEase(0, Math.max(0.1, Math.min(100, ke[2])));\n",
    );
    js.push_str(
        "          var easeOut = new KeyframeEase(0, Math.max(0.1, Math.min(100, ke[3])));\n",
    );
    js.push_str(
        "          try { prop.setTemporalEaseAtKey(idx, [easeIn], [easeOut]); } catch (_) {}\n",
    );
    js.push_str("        }\n");
    js.push_str("      }\n");
    js.push_str("    }\n");
    js.push_str("    function setKeyed2D(prop, keys, fps, is3D, zVal) {\n");
    js.push_str("      if (!keys || keys.length === 0) return;\n");
    js.push_str(
        "      function vec(v) { return is3D ? [v[0], v[1], zVal || 0] : [v[0], v[1]]; }\n",
    );
    js.push_str("      if (keys.length === 1) { prop.setValue(vec(keys[0][1])); return; }\n");
    js.push_str("      for (var i = 0; i < keys.length; i++) {\n");
    js.push_str("        prop.setValueAtTime(keys[i][0] / fps, vec(keys[i][1]));\n");
    js.push_str("      }\n");
    js.push_str("    }\n");
    js.push_str("    function makeShape(verts, ins, outs, closed) {\n");
    js.push_str("      var s = new Shape();\n");
    js.push_str("      s.vertices = verts;\n");
    js.push_str("      s.inTangents = ins;\n");
    js.push_str("      s.outTangents = outs;\n");
    js.push_str("      s.closed = closed;\n");
    js.push_str("      return s;\n");
    js.push_str("    }\n");

    // 1. Import Footage items
    js.push_str("    var footageMap = {};\n");
    if !model.footage.is_empty() {
        js.push_str("    var footageFolder = ensureFolder('Footage');\n");
        for ft in &model.footage {
            let _ = writeln!(
                js,
                "    (function() {{ var f = new File(baseFolder.fsName + '/' + {}); if (f.exists) {{ var io = new ImportOptions(f); var it = app.project.importFile(io); it.parentFolder = footageFolder; footageMap[{}] = it; }} }})();",
                js_str(&ft.relative_path),
                js_str(&ft.id)
            );
        }
    }

    // 2. Create Compositions first so nested PreComps can resolve any symbol comp
    js.push_str("    var compMap = {};\n");
    for comp in &model.compositions {
        let dur_sec = (f64::from(comp.duration_frames.max(1)) / comp.fps).max(1.0 / comp.fps);
        let _ = writeln!(
            js,
            "    (function() {{ var c = app.project.items.addComp({}, {}, {}, 1.0, {:.6}, {:.4}); c.bgColor = [{:.4}, {:.4}, {:.4}]; c.parentFolder = ensureFolder({}); compMap[{}] = c; }})();",
            js_str(&comp.name),
            comp.width,
            comp.height,
            dur_sec,
            comp.fps,
            comp.background[0],
            comp.background[1],
            comp.background[2],
            js_str(&comp.folder),
            js_str(&comp.id),
        );
    }

    // 3. Populate each Composition (bottom-to-top, since `comp.layers.add*()` inserts at top index 1!)
    for comp in &model.compositions {
        let _ = writeln!(js, "    (function(comp) {{");
        let _ = writeln!(js, "      if (!comp) return;");
        let _ = writeln!(js, "      var fps = {:.4};", comp.fps);
        let _ = writeln!(js, "      var layerMap = {{}};");

        if let Some([ws, we]) = comp.work_area {
            let _ = writeln!(
                js,
                "      try {{ comp.workAreaStart = {:.6}; comp.workAreaDuration = {:.6}; }} catch (_) {{}}",
                f64::from(ws) / comp.fps,
                f64::from(we.saturating_sub(ws).max(1)) / comp.fps
            );
        }
        for g in &comp.guides {
            let ori = if g.horizontal {
                " Horizontal "
            } else {
                " Vertical "
            };
            let _ = ori;
            let _ = writeln!(
                js,
                "      try {{ comp.addGuide({}, {:.3}); }} catch (_) {{}}",
                if g.horizontal { 0 } else { 1 },
                g.position
            );
        }
        for m in &comp.markers {
            let _ = writeln!(
                js,
                "      try {{ var mv = new MarkerValue({}); mv.chapter = {}; mv.duration = {:.6}; comp.markerProperty.setValueAtTime({:.6}, mv); }} catch (_) {{}}",
                js_str(&m.comment),
                js_str(&m.label),
                f64::from(m.duration_frames) / comp.fps,
                f64::from(m.frame) / comp.fps,
            );
        }

        for layer in &comp.layers {
            js.push_str("      (function() {\n");
            js.push_str("        var lyr = null;\n");
            match &layer.kind {
                AeLayerKind::Shape { groups } => {
                    js.push_str("        lyr = comp.layers.addShape();\n");
                    js.push_str("        var rootVec = lyr.property('ADBE Root Vectors Group');\n");
                    for grp in groups {
                        let _ = writeln!(
                            js,
                            "        var vg = rootVec.addProperty('ADBE Vector Group'); vg.name = {};",
                            js_str(&grp.name)
                        );
                        js.push_str("        var vc = vg.property('ADBE Vectors Group');\n");
                        for (ci, track) in grp.contour_tracks.iter().enumerate() {
                            let _ = writeln!(
                                js,
                                "        var sp_{ci} = vc.addProperty('ADBE Vector Shape - Group'); var pathProp_{ci} = sp_{ci}.property('ADBE Vector Shape');"
                            );
                            if track.len() == 1 {
                                let c = &track[0].value;
                                let _ = writeln!(
                                    js,
                                    "        pathProp_{ci}.setValue(makeShape({}, {}, {}, {}));",
                                    serde_json::to_string(&c.vertices).unwrap_or_default(),
                                    serde_json::to_string(&c.in_tangents).unwrap_or_default(),
                                    serde_json::to_string(&c.out_tangents).unwrap_or_default(),
                                    c.closed
                                );
                            } else {
                                for key in track {
                                    let c = &key.value;
                                    let _ = writeln!(
                                        js,
                                        "        pathProp_{ci}.setValueAtTime({} / fps, makeShape({}, {}, {}, {}));",
                                        key.frame,
                                        serde_json::to_string(&c.vertices).unwrap_or_default(),
                                        serde_json::to_string(&c.in_tangents).unwrap_or_default(),
                                        serde_json::to_string(&c.out_tangents).unwrap_or_default(),
                                        c.closed
                                    );
                                }
                            }
                        }

                        // Strokes
                        for st in &grp.strokes {
                            let _ = writeln!(
                                js,
                                "        (function() {{ var st = vc.addProperty('ADBE Vector Graphic - Stroke'); st.property('ADBE Vector Stroke Color').setValue([{:.4}, {:.4}, {:.4}, 1]); st.property('ADBE Vector Stroke Opacity').setValue({:.3}); st.property('ADBE Vector Stroke Width').setValue({:.3}); try {{ st.property('ADBE Vector Stroke Line Cap').setValue(2); st.property('ADBE Vector Stroke Line Join').setValue(2); }} catch (_) {{}}",
                                st.color[0], st.color[1], st.color[2], st.opacity, st.width
                            );
                            if let Some([dash, gap]) = st.dashes {
                                let _ = writeln!(
                                    js,
                                    "          try {{ var ds = st.property('ADBE Vector Stroke Dashes'); var d1 = ds.addProperty('ADBE Vector Stroke Dash 1'); d1.setValue({dash:.3}); var g1 = ds.addProperty('ADBE Vector Stroke Gap 1'); g1.setValue({gap:.3}); }} catch (_) {{}}"
                                );
                            }
                            js.push_str("        })();\n");
                        }

                        // Fills
                        for fl in &grp.fills {
                            match fl {
                                AePaintFill::None => {}
                                AePaintFill::Solid {
                                    color,
                                    opacity,
                                    even_odd,
                                } => {
                                    let _ = writeln!(
                                        js,
                                        "        (function() {{ var fl = vc.addProperty('ADBE Vector Graphic - Fill'); fl.property('ADBE Vector Fill Color').setValue([{:.4}, {:.4}, {:.4}, 1]); fl.property('ADBE Vector Fill Opacity').setValue({:.3}); try {{ fl.property('ADBE Vector Fill Rule').setValue({}); }} catch (_) {{}} }})();",
                                        color[0],
                                        color[1],
                                        color[2],
                                        opacity,
                                        if *even_odd { 2 } else { 1 }
                                    );
                                }
                                AePaintFill::Gradient {
                                    radial,
                                    start,
                                    end,
                                    highlight_length,
                                    stops,
                                    opacity,
                                    even_odd,
                                } => {
                                    let fallback_rgb = stops
                                        .first()
                                        .map(|(_, c)| [c[0], c[1], c[2]])
                                        .unwrap_or([0.5, 0.5, 0.5]);
                                    let _ = writeln!(
                                        js,
                                        "        (function() {{ try {{ var gf = vc.addProperty('ADBE Vector Graphic - G-Fill'); gf.property('ADBE Vector Grad Type').setValue({}); gf.property('ADBE Vector Grad Start Pt').setValue([{:.3}, {:.3}]); gf.property('ADBE Vector Grad End Pt').setValue([{:.3}, {:.3}]); if ({}) {{ gf.property('ADBE Vector Grad HiLite Length').setValue({:.3}); }} gf.property('ADBE Vector Fill Opacity').setValue({:.3}); gf.property('ADBE Vector Fill Rule').setValue({}); }} catch (_) {{ var sf = vc.addProperty('ADBE Vector Graphic - Fill'); sf.property('ADBE Vector Fill Color').setValue([{:.4}, {:.4}, {:.4}, 1]); }} }})();",
                                        if *radial { 2 } else { 1 },
                                        start[0],
                                        start[1],
                                        end[0],
                                        end[1],
                                        radial,
                                        highlight_length,
                                        opacity,
                                        if *even_odd { 2 } else { 1 },
                                        fallback_rgb[0],
                                        fallback_rgb[1],
                                        fallback_rgb[2],
                                    );
                                }
                            }
                        }

                        // Vector Modifiers (`Repeater`, `Offset Paths`, `Round Corners`)
                        for m in &grp.modifiers {
                            match *m {
                                AeVectorModifier::Repeater {
                                    copies,
                                    offset_x,
                                    offset_y,
                                    scale_pct,
                                    rotation,
                                } => {
                                    let _ = writeln!(
                                        js,
                                        "        try {{ var rp = vc.addProperty('ADBE Vector Filter - Repeater'); rp.property('ADBE Vector Repeater Copies').setValue({copies}); var rpt = rp.property('ADBE Vector Repeater Transform'); rpt.property('ADBE Vector Repeater Position').setValue([{offset_x:.3}, {offset_y:.3}]); rpt.property('ADBE Vector Repeater Scale').setValue([{scale_pct:.3}, {scale_pct:.3}]); rpt.property('ADBE Vector Repeater Rotation').setValue({rotation:.3}); }} catch (_) {{}}"
                                    );
                                }
                                AeVectorModifier::OffsetPaths { amount } => {
                                    let _ = writeln!(
                                        js,
                                        "        try {{ var op = vc.addProperty('ADBE Vector Filter - Offset'); op.property('ADBE Vector Offset Amount').setValue({amount:.3}); }} catch (_) {{}}"
                                    );
                                }
                                AeVectorModifier::RoundCorners { radius } => {
                                    let _ = writeln!(
                                        js,
                                        "        try {{ var rc = vc.addProperty('ADBE Vector Filter - RC'); rc.property('ADBE Vector RoundCorner Radius').setValue({radius:.3}); }} catch (_) {{}}"
                                    );
                                }
                            }
                        }
                    }
                }
                AeLayerKind::Text {
                    text,
                    font,
                    font_size,
                    color,
                    bold,
                    italic,
                    align,
                    leading,
                    tracking,
                    box_size,
                } => {
                    if let Some([bw, bh]) = box_size {
                        let _ = writeln!(
                            js,
                            "        lyr = comp.layers.addBoxText([{bw:.2}, {bh:.2}]);"
                        );
                    } else {
                        let _ =
                            writeln!(js, "        lyr = comp.layers.addText({});", js_str(text));
                    }
                    let just = match align {
                        TextAlign::Left => "ParagraphJustification.LEFT_JUSTIFY",
                        TextAlign::Center => "ParagraphJustification.CENTER_JUSTIFY",
                        TextAlign::Right => "ParagraphJustification.RIGHT_JUSTIFY",
                    };
                    let _ = writeln!(
                        js,
                        "        (function() {{ var tp = lyr.property('ADBE Text Properties').property('ADBE Text Document'); var td = tp.value; td.text = {}; td.fontSize = {:.2}; td.fillColor = [{:.4}, {:.4}, {:.4}]; td.applyFill = true; try {{ td.font = {}; td.fauxBold = {}; td.fauxItalic = {}; td.leading = {:.2}; td.tracking = {:.2}; td.justification = {just}; }} catch (_) {{}} tp.setValue(td); }})();",
                        js_str(text),
                        font_size,
                        color[0],
                        color[1],
                        color[2],
                        js_str(font),
                        bold,
                        italic,
                        leading,
                        tracking
                    );
                }
                AeLayerKind::PreComp {
                    comp_id,
                    time_remap_keys,
                    ..
                } => {
                    let _ = writeln!(
                        js,
                        "        var subComp = hasOwn(compMap, {}) ? compMap[{}] : null; if (!subComp) return; lyr = comp.layers.add(subComp);",
                        js_str(comp_id),
                        js_str(comp_id)
                    );
                    if !time_remap_keys.is_empty() || layer.expressions.time_remap.is_some() {
                        js.push_str("        try { lyr.timeRemapEnabled = true; } catch (_) {}\n");
                        if !time_remap_keys.is_empty() {
                            let keys_arr = format_keys_1d(time_remap_keys);
                            let _ = writeln!(
                                js,
                                "        try {{ setKeyed1D(lyr.property('ADBE Time Remapping'), {keys_arr}, fps); }} catch (_) {{}}"
                            );
                        }
                    }
                }
                AeLayerKind::Footage {
                    footage_id,
                    is_audio_only,
                    start_offset_sec,
                    end_offset_sec,
                    loop_count,
                    muted,
                    audio_levels_db,
                    ..
                } => {
                    let _ = writeln!(
                        js,
                        "        var ft = hasOwn(footageMap, {}) ? footageMap[{}] : null; if (!ft) return; if ({loop_count} > 1) {{ try {{ ft.mainSource.loop = {loop_count}; }} catch (_) {{}} }} lyr = comp.layers.add(ft);",
                        js_str(footage_id),
                        js_str(footage_id)
                    );
                    if *is_audio_only {
                        js.push_str("        try { lyr.audioEnabled = true; } catch (_) {}\n");
                    } else if *muted {
                        js.push_str("        try { lyr.audioEnabled = false; } catch (_) {}\n");
                    }
                    if *start_offset_sec > 0.0 {
                        let _ = writeln!(
                            js,
                            "        lyr.startTime = ({} / fps) - {:.6};",
                            layer.in_frame, start_offset_sec
                        );
                    }
                    if let Some(end_sec) = end_offset_sec {
                        let trim_dur = (*end_sec - *start_offset_sec).max(1.0 / comp.fps);
                        let _ = writeln!(
                            js,
                            "        lyr.outPoint = Math.min(lyr.outPoint, ({} / fps) + {:.6});",
                            layer.in_frame, trim_dur
                        );
                    }
                    if !audio_levels_db.is_empty() {
                        let lv_arr = format_keys_2d(audio_levels_db);
                        let _ = writeln!(
                            js,
                            "        try {{ setKeyed2D(lyr.property('ADBE Audio Group').property('ADBE Audio Levels'), {lv_arr}, fps, false, 0); }} catch (_) {{}}"
                        );
                    }
                }
                AeLayerKind::Null => {
                    js.push_str("        lyr = comp.layers.addNull();\n");
                }
                AeLayerKind::Camera {
                    focal_length,
                    zoom_keys,
                } => {
                    let _ = writeln!(
                        js,
                        "        lyr = comp.layers.addCamera({}, [{:.2}, {:.2}]);",
                        js_str(&layer.name),
                        f64::from(comp.width) * 0.5,
                        f64::from(comp.height) * 0.5
                    );
                    let _ = focal_length;
                    if !zoom_keys.is_empty() {
                        let z_arr = format_keys_1d(zoom_keys);
                        let _ = writeln!(
                            js,
                            "        try {{ setKeyed1D(lyr.property('ADBE Camera Options Group').property('ADBE Camera Zoom'), {z_arr}, fps); }} catch (_) {{}}"
                        );
                    }
                }
            }

            // Common Layer setup: name, in/out points, 3D, blend mode, track matte, transforms, effects, expressions, markers
            let _ = writeln!(
                js,
                "        if (!lyr) return; lyr.name = {}; layerMap[{}] = lyr;",
                js_str(&layer.name),
                js_str(&layer.id)
            );
            if !matches!(layer.kind, AeLayerKind::Footage { start_offset_sec, .. } if start_offset_sec > 0.0)
            {
                let _ = writeln!(js, "        lyr.startTime = {} / fps;", layer.in_frame);
            }
            let _ = writeln!(
                js,
                "        lyr.inPoint = {} / fps; lyr.outPoint = Math.max(({} + 1) / fps, {} / fps);",
                layer.in_frame, layer.in_frame, layer.out_frame
            );
            if layer.three_d {
                js.push_str("        try { lyr.threeDLayer = true; } catch (_) {}\n");
            }
            if layer.auto_orient_along_path {
                js.push_str(
                    "        try { lyr.autoOrient = AutoOrientType.ALONG_PATH; } catch (_) {}\n",
                );
            }
            if layer.blend_mode != "NORMAL" {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.blendingMode = BlendingMode.{}; }} catch (_) {{}}",
                    layer.blend_mode
                );
            }
            if layer.track_matte != AeTrackMatte::None {
                let tm = match layer.track_matte {
                    AeTrackMatte::None => "NO_TRACK_MATTE",
                    AeTrackMatte::Alpha => "ALPHA",
                    AeTrackMatte::AlphaInverted => "ALPHA_INVERTED",
                    AeTrackMatte::Luma => "LUMA",
                    AeTrackMatte::LumaInverted => "LUMA_INVERTED",
                };
                let _ = writeln!(
                    js,
                    "        try {{ lyr.trackMatteType = TrackMatteType.{tm}; }} catch (_) {{}}"
                );
            }

            // Transforms
            if !matches!(
                layer.kind,
                AeLayerKind::Footage {
                    is_audio_only: true,
                    ..
                }
            ) {
                js.push_str("        var xf = lyr.property('ADBE Transform Group');\n");
                let _ = writeln!(
                    js,
                    "        if (xf) {{ try {{ xf.property('ADBE Anchor Point').setValue({}); }} catch (_) {{}}",
                    if layer.three_d {
                        format!(
                            "[{:.3}, {:.3}, 0]",
                            layer.anchor_point[0], layer.anchor_point[1]
                        )
                    } else {
                        format!(
                            "[{:.3}, {:.3}]",
                            layer.anchor_point[0], layer.anchor_point[1]
                        )
                    }
                );
                if !layer.position_keys.is_empty() {
                    let _ = writeln!(
                        js,
                        "          setKeyed2D(xf.property('ADBE Position'), {}, fps, {}, {:.3});",
                        format_keys_2d(&layer.position_keys),
                        layer.three_d,
                        layer.z_depth
                    );
                }
                if !layer.scale_keys.is_empty() {
                    let _ = writeln!(
                        js,
                        "          setKeyed2D(xf.property('ADBE Scale'), {}, fps, {}, 100);",
                        format_keys_2d(&layer.scale_keys),
                        layer.three_d
                    );
                }
                if !layer.rotation_keys.is_empty() {
                    let _ = writeln!(
                        js,
                        "          setKeyed1D(xf.property('ADBE Rotate Z'), {}, fps);",
                        format_keys_1d(&layer.rotation_keys)
                    );
                }
                if !layer.opacity_keys.is_empty() {
                    let _ = writeln!(
                        js,
                        "          setKeyed1D(xf.property('ADBE Opacity'), {}, fps);",
                        format_keys_1d(&layer.opacity_keys)
                    );
                }
                js.push_str("        }\n");
            }

            // Skew via `ADBE Geometry2` Transform effect when needed
            if !layer.skew_keys.is_empty() || layer.expressions.skew.is_some() {
                js.push_str("        try { var fxSkew = lyr.property('ADBE Effect Parade').addProperty('ADBE Geometry2'); var skProp = fxSkew.property('ADBE Geometry2-0006');\n");
                if !layer.skew_keys.is_empty() {
                    let _ = writeln!(
                        js,
                        "          setKeyed1D(skProp, {}, fps);",
                        format_keys_1d(&layer.skew_keys)
                    );
                }
                if let Some(sk_expr) = &layer.expressions.skew {
                    let _ = writeln!(js, "          skProp.expression = {};", js_str(sk_expr));
                }
                js.push_str("        } catch (_) {}\n");
            }

            // Native AE Effects (`Drop Shadow`, `Glow`, `Gaussian Blur`, `Bevel Alpha`, `Tint`)
            if !layer.effects.is_empty() {
                js.push_str("        var fxParade = lyr.property('ADBE Effect Parade');\n");
                for fx in &layer.effects {
                    match fx {
                        AeEffect::DropShadow {
                            color,
                            opacity_pct,
                            direction_deg,
                            distance,
                            softness,
                        } => {
                            let _ = writeln!(
                                js,
                                "        try {{ var ds = fxParade.addProperty('ADBE Drop Shadow'); ds.property('ADBE Drop Shadow-0001').setValue([{:.4}, {:.4}, {:.4}, 1]); ds.property('ADBE Drop Shadow-0002').setValue({:.2}); ds.property('ADBE Drop Shadow-0003').setValue({:.2}); ds.property('ADBE Drop Shadow-0004').setValue({:.2}); ds.property('ADBE Drop Shadow-0005').setValue({:.2}); }} catch (_) {{}}",
                                color[0],
                                color[1],
                                color[2],
                                opacity_pct * 2.55,
                                direction_deg,
                                distance,
                                softness
                            );
                        }
                        AeEffect::Glow {
                            color,
                            radius,
                            intensity,
                        } => {
                            let _ = writeln!(
                                js,
                                "        try {{ var gl = fxParade.addProperty('ADBE Glo2'); gl.property('ADBE Glo2-0003').setValue({radius:.2}); gl.property('ADBE Glo2-0004').setValue({intensity:.2}); gl.property('ADBE Glo2-0008').setValue([{:.4}, {:.4}, {:.4}, 1]); }} catch (_) {{}}",
                                color[0], color[1], color[2]
                            );
                        }
                        AeEffect::GaussianBlur {
                            blurriness,
                            dimensions,
                        } => {
                            let _ = writeln!(
                                js,
                                "        try {{ var gb = fxParade.addProperty('ADBE Gaussian Blur 2'); gb.property('ADBE Gaussian Blur 2-0001').setValue({blurriness:.2}); gb.property('ADBE Gaussian Blur 2-0002').setValue({dimensions}); }} catch (_) {{}}"
                            );
                        }
                        AeEffect::BevelAlpha {
                            edge_thickness,
                            light_angle,
                            light_intensity,
                        } => {
                            let _ = writeln!(
                                js,
                                "        try {{ var bv = fxParade.addProperty('ADBE Bevel Alpha'); bv.property('ADBE Bevel Alpha-0001').setValue({edge_thickness:.2}); bv.property('ADBE Bevel Alpha-0002').setValue({light_angle:.2}); bv.property('ADBE Bevel Alpha-0004').setValue({light_intensity:.2}); }} catch (_) {{}}"
                            );
                        }
                        AeEffect::Tint { color, amount_pct } => {
                            let _ = writeln!(
                                js,
                                "        try {{ var tn = fxParade.addProperty('ADBE Tint'); tn.property('ADBE Tint-0001').setValue([{:.4}, {:.4}, {:.4}, 1]); tn.property('ADBE Tint-0002').setValue([{:.4}, {:.4}, {:.4}, 1]); tn.property('ADBE Tint-0003').setValue({amount_pct:.2}); }} catch (_) {{}}",
                                color[0], color[1], color[2], color[0], color[1], color[2]
                            );
                        }
                    }
                }
            }

            // Apply After Effects JavaScript Expressions!
            if let Some(pe) = &layer.expressions.position {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.property('ADBE Transform Group').property('ADBE Position').expression = {}; }} catch (_) {{}}",
                    js_str(pe)
                );
            }
            if let Some(se) = &layer.expressions.scale {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.property('ADBE Transform Group').property('ADBE Scale').expression = {}; }} catch (_) {{}}",
                    js_str(se)
                );
            }
            if let Some(re) = &layer.expressions.rotation {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.property('ADBE Transform Group').property('ADBE Rotate Z').expression = {}; }} catch (_) {{}}",
                    js_str(re)
                );
            }
            if let Some(oe) = &layer.expressions.opacity {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.property('ADBE Transform Group').property('ADBE Opacity').expression = {}; }} catch (_) {{}}",
                    js_str(oe)
                );
            }
            if let Some(tre) = &layer.expressions.time_remap {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.timeRemapEnabled = true; lyr.property('ADBE Time Remapping').expression = {}; }} catch (_) {{}}",
                    js_str(tre)
                );
            }
            if let Some(ste) = &layer.expressions.source_text {
                let _ = writeln!(
                    js,
                    "        try {{ lyr.property('ADBE Text Properties').property('ADBE Text Document').expression = {}; }} catch (_) {{}}",
                    js_str(ste)
                );
            }

            // Layer Markers (preserving scripts & cue notes)
            for m in &layer.markers {
                let _ = writeln!(
                    js,
                    "        try {{ var lmv = new MarkerValue({}); lmv.chapter = {}; lyr.property('ADBE Marker').setValueAtTime({:.6}, lmv); }} catch (_) {{}}",
                    js_str(&m.comment),
                    js_str(&m.label),
                    f64::from(m.frame) / comp.fps
                );
            }

            let _ = writeln!(js, "        lyr.enabled = {};", layer.visible);
            if layer.locked {
                js.push_str("        lyr.locked = true;\n");
            }
            js.push_str("      })();\n");
        }

        // Wire Layer Parenting after all layers in this composition exist
        for layer in &comp.layers {
            if let Some(pid) = &layer.parent_layer_id {
                let _ = writeln!(
                    js,
                    "      if (hasOwn(layerMap, {}) && hasOwn(layerMap, {})) {{ try {{ layerMap[{}].parent = layerMap[{}]; }} catch (_) {{}} }}",
                    js_str(&layer.id),
                    js_str(pid),
                    js_str(&layer.id),
                    js_str(pid)
                );
            }
        }

        let _ = writeln!(
            js,
            "    }})(hasOwn(compMap, {}) ? compMap[{}] : null);",
            js_str(&comp.id),
            js_str(&comp.id)
        );
    }

    let _ = writeln!(
        js,
        "    if (hasOwn(compMap, {})) {{ try {{ compMap[{}].openInViewer(); }} catch (_) {{}} }}",
        js_str(&model.main_comp_id),
        js_str(&model.main_comp_id)
    );
    let _ = writeln!(
        js,
        "    try {{ app.project.save(new File(baseFolder.fsName + '/' + {})); }} catch (_) {{}}",
        js_str(target_aep_name)
    );
    js.push_str("  } finally {\n");
    js.push_str("    app.endUndoGroup();\n");
    js.push_str("  }\n");
    js.push_str("})();\n");
    js
}

fn format_keys_1d(keys: &[AeKey<f64>]) -> String {
    let mut out = String::from("[");
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "[{},{:.4},{:.2},{:.2},{}]",
            k.frame,
            finite_or(k.value, 0.0),
            finite_or(k.ease.in_influence, 16.67),
            finite_or(k.ease.out_influence, 16.67),
            k.ease.hold
        );
    }
    out.push(']');
    out
}

fn format_keys_2d(keys: &[AeKey<[f64; 2]>]) -> String {
    let mut out = String::from("[");
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "[{},[{:.4},{:.4}],{:.2},{:.2},{}]",
            k.frame,
            finite_or(k.value[0], 0.0),
            finite_or(k.value[1], 0.0),
            finite_or(k.ease.in_influence, 16.67),
            finite_or(k.ease.out_influence, 16.67),
            k.ease.hold
        );
    }
    out.push(']');
    out
}
