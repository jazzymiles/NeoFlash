use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// 2. `.figma.json` REST API & Plugin JSON Parser
// ─────────────────────────────────────────────────────────────────────────────

fn json_color_to_rgba(c: &Value, extra_opacity: f64) -> Color {
    let r = (c
        .get("r")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let g = (c
        .get("g")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let b = (c
        .get("b")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let a = ((c.get("a").and_then(Value::as_f64).unwrap_or(1.0) * extra_opacity).clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    [r, g, b, a]
}

fn parse_json_paints(arr: &[Value], w: f64, h: f64) -> Vec<FigmaPaint> {
    let mut out = Vec::new();
    for p in arr {
        if !p.get("visible").and_then(Value::as_bool).unwrap_or(true) {
            continue;
        }
        let opacity = p.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
        let ptype = p.get("type").and_then(Value::as_str).unwrap_or("SOLID");
        match ptype {
            "SOLID" => {
                if let Some(c) = p.get("color") {
                    out.push(FigmaPaint::Solid {
                        color: json_color_to_rgba(c, opacity),
                    });
                }
            }
            "GRADIENT_LINEAR" | "GRADIENT_RADIAL" => {
                let kind = if ptype == "GRADIENT_RADIAL" {
                    GradientKind::Radial
                } else {
                    GradientKind::Linear
                };
                let mut stops = Vec::new();
                if let Some(sarr) = p.get("gradientStops").and_then(Value::as_array) {
                    for s in sarr {
                        if let Some(c) = s.get("color") {
                            let pos = s
                                .get("position")
                                .and_then(Value::as_f64)
                                .unwrap_or(0.0)
                                .clamp(0.0, 1.0);
                            stops.push(GradientStop {
                                ratio: (pos * 255.0).round().clamp(0.0, 255.0) as u8,
                                color: json_color_to_rgba(c, opacity),
                            });
                        }
                    }
                }
                stops.sort_by_key(|s| s.ratio);
                let (mut start, mut end) = ([0.0, h * 0.5], [w.max(1.0), h * 0.5]);
                if let Some(handles) = p.get("gradientHandlePositions").and_then(Value::as_array)
                    && handles.len() >= 2
                {
                    let h0x = handles[0].get("x").and_then(Value::as_f64).unwrap_or(0.0);
                    let h0y = handles[0].get("y").and_then(Value::as_f64).unwrap_or(0.5);
                    let h1x = handles[1].get("x").and_then(Value::as_f64).unwrap_or(1.0);
                    let h1y = handles[1].get("y").and_then(Value::as_f64).unwrap_or(0.5);
                    start = [h0x * w.max(1.0), h0y * h.max(1.0)];
                    end = [h1x * w.max(1.0), h1y * h.max(1.0)];
                }
                if stops.len() >= 2 {
                    out.push(FigmaPaint::Gradient {
                        kind,
                        stops,
                        start,
                        end,
                    });
                }
            }
            "IMAGE" => {
                if let Some(iref) = p
                    .get("imageRef")
                    .or_else(|| p.get("imageHash"))
                    .and_then(Value::as_str)
                {
                    out.push(FigmaPaint::Image {
                        image_ref: iref.to_owned(),
                        alpha: opacity.clamp(0.0, 1.0),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn decode_vector_network_json(vn: &Value) -> Vec<(Vec<PathCommand>, FillRule)> {
    let Some(verts) = vn.get("vertices").and_then(Value::as_array) else {
        return Vec::new();
    };
    let Some(segs) = vn.get("segments").and_then(Value::as_array) else {
        return Vec::new();
    };
    let vcoords: Vec<[f64; 2]> = verts
        .iter()
        .map(|v| {
            [
                v.get("x").and_then(Value::as_f64).unwrap_or(0.0),
                v.get("y").and_then(Value::as_f64).unwrap_or(0.0),
            ]
        })
        .collect();

    let append_seg = |cmds: &mut Vec<PathCommand>, seg_idx: usize, forward: bool| -> Option<()> {
        let seg = segs.get(seg_idx)?;
        let s_i = seg.get("start")?.as_u64()? as usize;
        let e_i = seg.get("end")?.as_u64()? as usize;
        let (p0, p1) = if forward {
            (*vcoords.get(s_i)?, *vcoords.get(e_i)?)
        } else {
            (*vcoords.get(e_i)?, *vcoords.get(s_i)?)
        };
        let ts = seg.get("tangentStart");
        let te = seg.get("tangentEnd");
        let t0 = [
            ts.and_then(|t| t.get("x"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            ts.and_then(|t| t.get("y"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        ];
        let t1 = [
            te.and_then(|t| t.get("x"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            te.and_then(|t| t.get("y"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        ];
        let (tan_a, tan_b) = if forward { (t0, t1) } else { (t1, t0) };
        if tan_a[0].abs() < 1e-9
            && tan_a[1].abs() < 1e-9
            && tan_b[0].abs() < 1e-9
            && tan_b[1].abs() < 1e-9
        {
            cmds.push(PathCommand::LineTo { x: p1[0], y: p1[1] });
        } else {
            cmds.push(PathCommand::CubicTo {
                cx1: p0[0] + tan_a[0],
                cy1: p0[1] + tan_a[1],
                cx2: p1[0] + tan_b[0],
                cy2: p1[1] + tan_b[1],
                x: p1[0],
                y: p1[1],
            });
        }
        Some(())
    };

    if let Some(regions) = vn.get("regions").and_then(Value::as_array)
        && !regions.is_empty()
    {
        let mut out = Vec::new();
        for reg in regions {
            let rule = match reg.get("windingRule").and_then(Value::as_str) {
                Some("EVENODD") => FillRule::EvenOdd,
                _ => FillRule::NonZero,
            };
            let mut cmds = Vec::new();
            if let Some(loops) = reg.get("loops").and_then(Value::as_array) {
                for lp in loops {
                    let Some(sidxs) = lp.as_array() else { continue };
                    let mut cur_v: Option<usize> = None;
                    for (k, sv) in sidxs.iter().enumerate() {
                        let Some(si) = sv.as_u64().map(|u| u as usize) else {
                            continue;
                        };
                        let Some(seg) = segs.get(si) else { continue };
                        let s_v = seg.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let e_v = seg.get("end").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let forward = match cur_v {
                            Some(cv) => cv == s_v,
                            None => true,
                        };
                        if k == 0 {
                            let start_pt = if forward {
                                vcoords.get(s_v)
                            } else {
                                vcoords.get(e_v)
                            };
                            if let Some(&[sx, sy]) = start_pt {
                                cmds.push(PathCommand::MoveTo { x: sx, y: sy });
                            }
                        }
                        append_seg(&mut cmds, si, forward);
                        cur_v = Some(if forward { e_v } else { s_v });
                    }
                    if !cmds.is_empty() {
                        cmds.push(PathCommand::Close);
                    }
                }
            }
            if !cmds.is_empty() {
                out.push((cmds, rule));
            }
        }
        return out;
    }

    // Open stroke-only vector network
    let mut cmds = Vec::new();
    let mut last_v: Option<usize> = None;
    for i in 0..segs.len() {
        let Some(seg) = segs.get(i) else { continue };
        let s_v = seg.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
        let e_v = seg.get("end").and_then(Value::as_u64).unwrap_or(0) as usize;
        if last_v != Some(s_v)
            && let Some(&[sx, sy]) = vcoords.get(s_v)
        {
            cmds.push(PathCommand::MoveTo { x: sx, y: sy });
        }
        append_seg(&mut cmds, i, true);
        last_v = Some(e_v);
    }
    if cmds.is_empty() {
        Vec::new()
    } else {
        vec![(cmds, FillRule::NonZero)]
    }
}

fn parse_figma_json_node(
    v: &Value,
    parent_abs: Option<(f64, f64)>,
    node_count: &mut usize,
    depth: usize,
) -> Result<FigmaNode, ImportError> {
    if depth > 64 {
        return Err(ImportError::Container(
            "Figma JSON node hierarchy exceeds depth 64".into(),
        ));
    }
    *node_count += 1;
    if *node_count > MAX_FIGMA_NODES {
        return Err(ImportError::Container(format!(
            "Figma JSON exceeds {MAX_FIGMA_NODES} node budget"
        )));
    }

    let node_type = v
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("GROUP")
        .to_owned();
    let id = v
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("0:0")
        .to_owned();
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&node_type)
        .to_owned();
    let visible = v.get("visible").and_then(Value::as_bool).unwrap_or(true);
    let locked = v.get("locked").and_then(Value::as_bool).unwrap_or(false);
    let opacity = v.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    let blend_mode = v
        .get("blendMode")
        .and_then(Value::as_str)
        .unwrap_or("PASS_THROUGH")
        .to_owned();
    let is_mask = v.get("isMask").and_then(Value::as_bool).unwrap_or(false);
    let mask_type = v
        .get("maskType")
        .and_then(Value::as_str)
        .unwrap_or("ALPHA")
        .to_owned();
    let clips_content = v
        .get("clipsContent")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let bbox = v
        .get("absoluteBoundingBox")
        .or_else(|| v.get("boundingBox"));
    let abs_x = bbox.and_then(|b| b.get("x")).and_then(Value::as_f64);
    let abs_y = bbox.and_then(|b| b.get("y")).and_then(Value::as_f64);
    let width = v
        .get("size")
        .and_then(|s| s.get("x"))
        .and_then(Value::as_f64)
        .or_else(|| bbox.and_then(|b| b.get("width")).and_then(Value::as_f64))
        .unwrap_or(100.0)
        .max(0.0);
    let height = v
        .get("size")
        .and_then(|s| s.get("y"))
        .and_then(Value::as_f64)
        .or_else(|| bbox.and_then(|b| b.get("height")).and_then(Value::as_f64))
        .unwrap_or(100.0)
        .max(0.0);

    let mut relative_transform = None;
    if let Some(rows) = v.get("relativeTransform").and_then(Value::as_array)
        && rows.len() == 2
        && let (Some(r0), Some(r1)) = (rows[0].as_array(), rows[1].as_array())
        && r0.len() >= 3
        && r1.len() >= 3
    {
        let m00 = r0[0].as_f64().unwrap_or(1.0);
        let m01 = r0[1].as_f64().unwrap_or(0.0);
        let m02 = r0[2].as_f64().unwrap_or(0.0);
        let m10 = r1[0].as_f64().unwrap_or(0.0);
        let m11 = r1[1].as_f64().unwrap_or(1.0);
        let m12 = r1[2].as_f64().unwrap_or(0.0);
        relative_transform = Some([m00, m10, m01, m11, m02, m12]);
    }

    let (x, y) = if let Some(rt) = relative_transform {
        (rt[4], rt[5])
    } else if let (Some(ax), Some(ay)) = (abs_x, abs_y) {
        if let Some((px, py)) = parent_abs {
            (ax - px, ay - py)
        } else {
            (ax, ay)
        }
    } else {
        (
            v.get("x").and_then(Value::as_f64).unwrap_or(0.0),
            v.get("y").and_then(Value::as_f64).unwrap_or(0.0),
        )
    };

    let rotation_deg = v
        .get("rotation")
        .and_then(Value::as_f64)
        .map(|r| -r) // Figma REST rotation is counter-clockwise degrees when relativeTransform is absent
        .unwrap_or(0.0);

    let r_uni = v.get("cornerRadius").and_then(Value::as_f64).unwrap_or(0.0);
    let corner_radii = if let Some(arr) = v.get("rectangleCornerRadii").and_then(Value::as_array)
        && arr.len() >= 4
    {
        [
            arr[0].as_f64().unwrap_or(r_uni),
            arr[1].as_f64().unwrap_or(r_uni),
            arr[2].as_f64().unwrap_or(r_uni),
            arr[3].as_f64().unwrap_or(r_uni),
        ]
    } else {
        [r_uni; 4]
    };

    let arc_data = v.get("arcData").and_then(|ad| {
        let s = ad.get("startingAngle")?.as_f64()?;
        let e = ad.get("endingAngle")?.as_f64()?;
        let inner = ad.get("innerRadius").and_then(Value::as_f64).unwrap_or(0.0);
        Some((s, e, inner))
    });

    let fills = v
        .get("fills")
        .and_then(Value::as_array)
        .map(|a| parse_json_paints(a, width, height))
        .unwrap_or_default();
    let strokes = v
        .get("strokes")
        .and_then(Value::as_array)
        .map(|a| parse_json_paints(a, width, height))
        .unwrap_or_default();
    let stroke_weight = v.get("strokeWeight").and_then(Value::as_f64).unwrap_or(0.0);

    let mut fill_paths = Vec::new();
    if let Some(fg) = v.get("fillGeometry").and_then(Value::as_array) {
        for g in fg {
            if let Some(d) = g.get("path").and_then(Value::as_str) {
                let cmds = parse_svg_path_data(d);
                if !cmds.is_empty() {
                    let rule = match g.get("windingRule").and_then(Value::as_str) {
                        Some("EVENODD") => FillRule::EvenOdd,
                        _ => FillRule::NonZero,
                    };
                    fill_paths.push((cmds, rule));
                }
            }
        }
    }
    let mut stroke_paths = Vec::new();
    if let Some(sg) = v.get("strokeGeometry").and_then(Value::as_array) {
        for g in sg {
            if let Some(d) = g.get("path").and_then(Value::as_str) {
                let cmds = parse_svg_path_data(d);
                if !cmds.is_empty() {
                    stroke_paths.push(cmds);
                }
            }
        }
    }
    if fill_paths.is_empty()
        && stroke_paths.is_empty()
        && let Some(vn) = v.get("vectorNetwork")
    {
        let decoded = decode_vector_network_json(vn);
        if !fills.is_empty() {
            fill_paths = decoded;
        } else {
            stroke_paths = decoded.into_iter().map(|(c, _)| c).collect();
        }
    }

    let text = if node_type == "TEXT" {
        let chars = v
            .get("characters")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let style = v.get("style");
        let font_family = style
            .and_then(|s| s.get("fontFamily"))
            .and_then(Value::as_str)
            .unwrap_or("Inter")
            .to_owned();
        let font_size = style
            .and_then(|s| s.get("fontSize"))
            .and_then(Value::as_f64)
            .unwrap_or(16.0);
        let weight = style
            .and_then(|s| s.get("fontWeight"))
            .and_then(Value::as_u64)
            .unwrap_or(400);
        let italic = style
            .and_then(|s| s.get("italic"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let align = match style
            .and_then(|s| s.get("textAlignHorizontal"))
            .and_then(Value::as_str)
        {
            Some("CENTER") => TextAlign::Center,
            Some("RIGHT") => TextAlign::Right,
            _ => TextAlign::Left,
        };
        let letter_spacing = style
            .and_then(|s| s.get("letterSpacing"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let line_height = style
            .and_then(|s| s.get("lineHeightPx"))
            .and_then(Value::as_f64);
        Some(FigmaTextData {
            characters: chars,
            font_family,
            font_size,
            bold: weight >= 600,
            italic,
            align,
            letter_spacing,
            line_height,
        })
    } else {
        None
    };

    let mut effects = Vec::new();
    if let Some(earr) = v.get("effects").and_then(Value::as_array) {
        for eff in earr {
            if !eff.get("visible").and_then(Value::as_bool).unwrap_or(true) {
                continue;
            }
            let etype = eff.get("type").and_then(Value::as_str).unwrap_or("");
            let radius = eff.get("radius").and_then(Value::as_f64).unwrap_or(4.0);
            match etype {
                "DROP_SHADOW" | "INNER_SHADOW" => {
                    let color = eff
                        .get("color")
                        .map(|c| json_color_to_rgba(c, 1.0))
                        .unwrap_or([0, 0, 0, 128]);
                    let ox = eff
                        .get("offset")
                        .and_then(|o| o.get("x"))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    let oy = eff
                        .get("offset")
                        .and_then(|o| o.get("y"))
                        .and_then(Value::as_f64)
                        .unwrap_or(4.0);
                    effects.push(FigmaEffect::DropShadow {
                        color,
                        offset_x: ox,
                        offset_y: oy,
                        radius,
                        inner: etype == "INNER_SHADOW",
                    });
                }
                "LAYER_BLUR" => effects.push(FigmaEffect::LayerBlur { radius }),
                "BACKGROUND_BLUR" => effects.push(FigmaEffect::BackgroundBlur { radius }),
                _ => {}
            }
        }
    }

    let child_parent_abs = match (abs_x, abs_y) {
        (Some(ax), Some(ay)) => Some((ax, ay)),
        _ => parent_abs,
    };

    let mut children = Vec::new();
    if let Some(carr) = v.get("children").and_then(Value::as_array) {
        for c in carr {
            children.push(parse_figma_json_node(
                c,
                child_parent_abs,
                node_count,
                depth + 1,
            )?);
        }
    }

    let background_color = v.get("backgroundColor").map(|c| json_color_to_rgba(c, 1.0));

    Ok(FigmaNode {
        id,
        name,
        node_type,
        visible,
        locked,
        opacity,
        blend_mode,
        is_mask,
        mask_type,
        clips_content,
        x,
        y,
        width,
        height,
        rotation_deg,
        relative_transform,
        corner_radii,
        arc_data,
        point_count: v
            .get("pointCount")
            .or_else(|| v.get("count"))
            .and_then(Value::as_u64)
            .map(|u| u as u32),
        star_inner_radius: v.get("innerRadius").and_then(Value::as_f64),
        fills,
        strokes,
        stroke_weight,
        fill_paths,
        stroke_paths,
        text,
        effects,
        component_id: v
            .get("componentId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        background_color,
        children,
    })
}

pub(super) fn parse_figma_json_doc(
    json_str: &str,
    source_name: &str,
) -> Result<FigmaDoc, ImportError> {
    let root: Value = serde_json::from_str(json_str)
        .map_err(|e| ImportError::Container(format!("Invalid Figma JSON: {e}")))?;

    let doc_name = root
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(source_name)
        .to_owned();

    let mut images = BTreeMap::new();
    let mut total_image_bytes = 0usize;
    for key in ["images", "imageFills"] {
        if let Some(map) = root.get(key).and_then(Value::as_object) {
            for (iref, val) in map {
                if let Some(s) = val.as_str() {
                    let b64 = s
                        .split_once(";base64,")
                        .map(|(_, tail)| tail)
                        .unwrap_or(s)
                        .trim();
                    if b64.len() > neoflash_core::MAX_RESOURCE_BYTES * 2 {
                        return Err(ImportError::Container(format!(
                            "Figma JSON image '{iref}' exceeds resource size budget"
                        )));
                    }
                    if let Ok(decoded) = BASE64.decode(b64.as_bytes())
                        && !decoded.is_empty()
                    {
                        if decoded.len() > neoflash_core::MAX_RESOURCE_BYTES {
                            return Err(ImportError::Container(format!(
                                "Figma JSON image '{iref}' exceeds resource size budget"
                            )));
                        }
                        total_image_bytes = total_image_bytes.saturating_add(decoded.len());
                        if total_image_bytes > MAX_FIGMA_IMAGE_TOTAL_BYTES {
                            return Err(ImportError::Container(
                                "Figma JSON embedded images exceed 64 MiB aggregate budget".into(),
                            ));
                        }
                        images.insert(iref.clone(), decoded);
                    }
                }
            }
        }
    }

    let mut node_count = 0usize;
    let mut pages = Vec::new();
    if let Some(doc) = root.get("document") {
        let parsed = parse_figma_json_node(doc, None, &mut node_count, 0)?;
        if parsed.node_type == "DOCUMENT" {
            pages = parsed.children;
        } else {
            pages.push(parsed);
        }
    } else if let Some(nodes_map) = root.get("nodes").and_then(Value::as_object) {
        for entry in nodes_map.values() {
            let target = entry.get("document").unwrap_or(entry);
            let parsed = parse_figma_json_node(target, None, &mut node_count, 0)?;
            if parsed.node_type == "DOCUMENT" {
                pages.extend(parsed.children);
            } else if parsed.node_type == "CANVAS" {
                pages.push(parsed);
            } else {
                let mut page = FigmaNode {
                    id: "page_0".into(),
                    name: "Page 1".into(),
                    node_type: "CANVAS".into(),
                    ..FigmaNode::default()
                };
                page.children.push(parsed);
                pages.push(page);
            }
        }
    } else {
        let parsed = parse_figma_json_node(&root, None, &mut node_count, 0)?;
        if parsed.node_type == "DOCUMENT" {
            pages = parsed.children;
        } else if parsed.node_type == "CANVAS" {
            pages.push(parsed);
        } else {
            let mut page = FigmaNode {
                id: "page_0".into(),
                name: "Page 1".into(),
                node_type: "CANVAS".into(),
                ..FigmaNode::default()
            };
            page.children.push(parsed);
            pages.push(page);
        }
    }

    Ok(FigmaDoc {
        name: doc_name,
        pages,
        images,
    })
}
