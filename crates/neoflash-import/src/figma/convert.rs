use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// 3. Lowering `FigmaDoc` into Layered NeoFlash Compositions & Symbols
// ─────────────────────────────────────────────────────────────────────────────

struct FigmaBuildCtx<'a> {
    options: &'a FigmaImportOptions,
    images: &'a BTreeMap<String, Vec<u8>>,
    image_resources: HashMap<String, (String, u32, u32)>,
    component_symbols: HashMap<String, String>, // figma component id -> NeoFlash symbol id
    project: &'a mut Project,
    report: &'a mut ImportReport,
}

impl<'a> FigmaBuildCtx<'a> {
    fn map_blend_mode(&mut self, raw: &str, location: &str) -> BlendMode {
        if !self.options.preserve_blend_modes {
            return BlendMode::Normal;
        }
        match raw {
            "" | "PASS_THROUGH" | "NORMAL" => BlendMode::Normal,
            "MULTIPLY" | "LINEAR_BURN" => BlendMode::Multiply,
            "SCREEN" => BlendMode::Screen,
            "LIGHTEN" => BlendMode::Lighten,
            "DARKEN" => BlendMode::Darken,
            "DIFFERENCE" | "EXCLUSION" => BlendMode::Difference,
            "LINEAR_DODGE" | "ADD" | "COLOR_DODGE" => BlendMode::Add,
            "OVERLAY" | "SOFT_LIGHT" => BlendMode::Overlay,
            "HARD_LIGHT" => BlendMode::Hardlight,
            other => {
                self.report.warning(
                    "figma-blend-approximated",
                    location.to_owned(),
                    format!("Figma blend mode '{other}' approximated to Normal."),
                );
                BlendMode::Normal
            }
        }
    }

    fn map_effects(&mut self, effects: &[FigmaEffect], location: &str) -> Vec<FilterSpec> {
        let mut out = Vec::new();
        for eff in effects {
            match eff {
                FigmaEffect::DropShadow {
                    color,
                    offset_x,
                    offset_y,
                    radius,
                    inner,
                } => {
                    let distance = offset_x.hypot(*offset_y);
                    let angle = offset_y.atan2(*offset_x).to_degrees();
                    let rgb = (u32::from(color[0]) << 16)
                        | (u32::from(color[1]) << 8)
                        | u32::from(color[2]);
                    let alpha = (f64::from(color[3]) / 255.0).clamp(0.0, 1.0);
                    out.push(FilterSpec::DropShadow {
                        distance: distance.clamp(0.0, 255.0),
                        angle,
                        color: rgb,
                        alpha,
                        blur_x: (*radius).clamp(0.0, 255.0),
                        blur_y: (*radius).clamp(0.0, 255.0),
                        strength: 1.0,
                        quality: 2,
                        inner: *inner,
                        knockout: false,
                        hide_object: false,
                    });
                }
                FigmaEffect::LayerBlur { radius } => {
                    out.push(FilterSpec::Blur {
                        blur_x: (*radius).clamp(0.0, 255.0),
                        blur_y: (*radius).clamp(0.0, 255.0),
                        quality: 2,
                    });
                }
                FigmaEffect::BackgroundBlur { radius } => {
                    self.report.warning(
                        "figma-background-blur-approximated",
                        location.to_owned(),
                        format!(
                            "Figma BACKGROUND_BLUR ({radius:.1}px) approximated as layer Blur filter."
                        ),
                    );
                    out.push(FilterSpec::Blur {
                        blur_x: (*radius).clamp(0.0, 255.0),
                        blur_y: (*radius).clamp(0.0, 255.0),
                        quality: 2,
                    });
                }
            }
        }
        out.truncate(neoflash_core::MAX_OBJECT_FILTERS);
        out
    }

    fn ensure_image_resource(
        &mut self,
        image_ref: &str,
        location: &str,
    ) -> Option<(String, u32, u32)> {
        if let Some(cached) = self.image_resources.get(image_ref) {
            return Some(cached.clone());
        }
        let Some(bytes) = self.images.get(image_ref) else {
            self.report.warning(
                "figma-missing-image-blob",
                location.to_owned(),
                format!("Figma image fill '{image_ref}' has no embedded bytes; converted to placeholder shape."),
            );
            return None;
        };
        let Ok(fmt) = image::guess_format(bytes) else {
            self.report.warning(
                "figma-corrupt-image-blob",
                location.to_owned(),
                format!("Figma image fill '{image_ref}' has unrecognized image format; converted to placeholder shape."),
            );
            return None;
        };
        let (ext, mime) = match fmt {
            image::ImageFormat::Png => ("png", "image/png"),
            image::ImageFormat::Jpeg => ("jpg", "image/jpeg"),
            image::ImageFormat::Gif => ("gif", "image/gif"),
            image::ImageFormat::WebP => ("webp", "image/webp"),
            image::ImageFormat::Bmp => ("bmp", "image/bmp"),
            image::ImageFormat::Tiff => ("tiff", "image/tiff"),
            _ => ("png", "image/png"),
        };
        let res_id = new_id("figimg");
        let short_ref: String = image_ref
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect();
        let rel_path = format!(
            "figma/images/{}_{}.{}",
            if short_ref.is_empty() {
                "img"
            } else {
                &short_ref
            },
            self.project.resources.len(),
            ext
        );
        let Ok(unit) = ResourceUnit::from_bytes(res_id.clone(), rel_path, mime, bytes) else {
            self.report.warning(
                "figma-corrupt-image-blob",
                location.to_owned(),
                format!("Figma image fill '{image_ref}' exceeded resource limits; converted to placeholder shape."),
            );
            return None;
        };
        let Ok(decoded) = unit.decode_image() else {
            self.report.warning(
                "figma-corrupt-image-blob",
                location.to_owned(),
                format!("Figma image fill '{image_ref}' could not be decoded; converted to placeholder shape."),
            );
            return None;
        };
        let (w, h) = (decoded.width, decoded.height);
        self.project.resources.push(unit);
        self.image_resources
            .insert(image_ref.to_owned(), (res_id.clone(), w, h));
        Some((res_id, w, h))
    }

    fn node_transform(&self, node: &FigmaNode, offset_x: f64, offset_y: f64) -> Transform {
        let mut tf = if let Some(mut mat) = node.relative_transform {
            mat[4] -= offset_x;
            mat[5] -= offset_y;
            Transform::from_matrix(mat).unwrap_or_else(|| Transform {
                x: node.x - offset_x,
                y: node.y - offset_y,
                ..Transform::default()
            })
        } else {
            Transform {
                x: node.x - offset_x,
                y: node.y - offset_y,
                rotation: node.rotation_deg,
                ..Transform::default()
            }
        };
        let finite_or = |v: f64, def: f64| if v.is_finite() { v } else { def };
        tf.x = finite_or(tf.x, 0.0).clamp(-100_000.0, 100_000.0);
        tf.y = finite_or(tf.y, 0.0).clamp(-100_000.0, 100_000.0);
        tf.scale_x = finite_or(tf.scale_x, 1.0).clamp(-1000.0, 1000.0);
        if tf.scale_x.abs() < 1e-4 {
            tf.scale_x = if tf.scale_x.is_sign_negative() {
                -1e-4
            } else {
                1e-4
            };
        }
        tf.scale_y = finite_or(tf.scale_y, 1.0).clamp(-1000.0, 1000.0);
        if tf.scale_y.abs() < 1e-4 {
            tf.scale_y = if tf.scale_y.is_sign_negative() {
                -1e-4
            } else {
                1e-4
            };
        }
        tf.rotation = finite_or(tf.rotation, 0.0).rem_euclid(360.0);
        tf.skew = finite_or(tf.skew, 0.0).clamp(-89.9, 89.9);
        tf.alpha = finite_or(node.opacity, 1.0).clamp(0.0, 1.0);
        tf
    }

    fn fallback_shape_commands(
        &self,
        node: &FigmaNode,
    ) -> (Vec<PathCommand>, Option<PrimitiveOptions>) {
        let w = (if node.width.is_finite() {
            node.width
        } else {
            100.0
        })
        .clamp(1.0, 32_768.0);
        let h = (if node.height.is_finite() {
            node.height
        } else {
            100.0
        })
        .clamp(1.0, 32_768.0);
        match node.node_type.as_str() {
            "ELLIPSE" => {
                if let Some((start_rad, end_rad, inner_ratio)) = node.arc_data {
                    let s_deg = if start_rad.is_finite() {
                        start_rad.to_degrees().rem_euclid(360.0)
                    } else {
                        0.0
                    };
                    let span_rad = (end_rad - start_rad).abs();
                    let e_deg = if !end_rad.is_finite() || span_rad >= std::f64::consts::TAU - 1e-4
                    {
                        360.0
                    } else {
                        end_rad.to_degrees().rem_euclid(360.0)
                    };
                    let oval_opts = OvalOptions {
                        start_angle: s_deg,
                        end_angle: e_deg,
                        inner_radius: (if inner_ratio.is_finite() {
                            inner_ratio * 100.0
                        } else {
                            0.0
                        })
                        .clamp(0.0, 99.0),
                        close_path: true,
                    };
                    (
                        oval_path_commands(w, h, oval_opts),
                        (!oval_opts.is_default()).then_some(PrimitiveOptions::Oval(oval_opts)),
                    )
                } else {
                    (oval_path_commands(w, h, OvalOptions::default()), None)
                }
            }
            "STAR" => {
                let pts = node.point_count.unwrap_or(5).clamp(3, 32);
                let inner = node
                    .star_inner_radius
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.382)
                    .clamp(0.05, 0.95);
                let (cmds, _) = polystar_path_commands(
                    [w * 0.5, h * 0.5],
                    w.min(h) * 0.5,
                    -std::f64::consts::FRAC_PI_2,
                    pts,
                    true,
                    inner,
                );
                (cmds, None)
            }
            "REGULAR_POLYGON" => {
                let pts = node.point_count.unwrap_or(5).clamp(3, 32);
                let (cmds, _) = polystar_path_commands(
                    [w * 0.5, h * 0.5],
                    w.min(h) * 0.5,
                    -std::f64::consts::FRAC_PI_2,
                    pts,
                    false,
                    0.5,
                );
                (cmds, None)
            }
            "LINE" => (
                vec![
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::LineTo { x: w, y: 0.0 },
                ],
                None,
            ),
            _ => {
                let max_r = 0.5 * w.min(h);
                let r = node.corner_radii.map(|v| {
                    if v.is_finite() {
                        v.clamp(0.0, max_r)
                    } else {
                        0.0
                    }
                });
                if r.iter().any(|&v| v > 0.0) {
                    let rect_opts = RectangleOptions {
                        corner_radii: r,
                        corners_locked: (r[0] - r[1]).abs() < 1e-6
                            && (r[0] - r[2]).abs() < 1e-6
                            && (r[0] - r[3]).abs() < 1e-6,
                    };
                    (
                        rectangle_path_commands(w, h, rect_opts),
                        (!rect_opts.is_default()).then_some(PrimitiveOptions::Rectangle(rect_opts)),
                    )
                } else {
                    (
                        rectangle_path_commands(w, h, RectangleOptions::default()),
                        None,
                    )
                }
            }
        }
    }

    fn collect_component_nodes<'n>(
        &mut self,
        node: &'n FigmaNode,
        out: &mut Vec<&'n FigmaNode>,
        depth: usize,
    ) {
        if depth > 48 {
            return;
        }
        if node.node_type == "COMPONENT" && !self.component_symbols.contains_key(&node.id) {
            let sym_id = new_id("figsym");
            self.component_symbols
                .insert(node.id.clone(), sym_id.clone());
            self.project.symbols.push(Symbol {
                id: sym_id,
                name: node.name.clone(),
                folder: "Figma Components".into(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    frame_count: 1,
                    layers: vec![Layer {
                        id: new_id("layer"),
                        name: "Layer 1".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe::default()],
                        properties: LayerProperties::default(),
                    }],
                    scripts: Vec::new(),
                    guides: Vec::new(),
                },
                class_name: None,
                linkage_id: None,
            });
            out.push(node);
        }
        for child in &node.children {
            self.collect_component_nodes(child, out, depth + 1);
        }
    }

    fn populate_component_symbols(&mut self, components: &[&FigmaNode]) {
        for comp in components {
            let Some(sym_id) = self.component_symbols.get(&comp.id).cloned() else {
                continue;
            };
            let layers = self.build_layers_for_children(
                &comp.children,
                0.0,
                0.0,
                if comp.clips_content {
                    Some((comp.width.max(1.0), comp.height.max(1.0)))
                } else {
                    None
                },
                &comp.name,
                1,
            );
            if !layers.is_empty()
                && let Some(sym) = self.project.symbol_mut(&sym_id)
            {
                sym.timeline.layers = layers;
            }
        }
        self.report.statistics.symbols = self.project.symbols.len();
    }

    fn build_symbol_from_container(
        &mut self,
        container: &FigmaNode,
        folder: &str,
        depth: usize,
    ) -> String {
        let sym_id = new_id("figsym");
        let layers = self.build_layers_for_children(
            &container.children,
            0.0,
            0.0,
            if container.clips_content {
                Some((container.width.max(1.0), container.height.max(1.0)))
            } else {
                None
            },
            &container.name,
            depth + 1,
        );
        let sym = Symbol {
            id: sym_id.clone(),
            name: container.name.clone(),
            folder: folder.to_owned(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: if layers.is_empty() {
                    vec![Layer {
                        id: new_id("layer"),
                        name: "Layer 1".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe::default()],
                        properties: LayerProperties::default(),
                    }]
                } else {
                    layers
                },
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        };
        self.project.symbols.push(sym);
        self.report.statistics.symbols = self.project.symbols.len();
        sym_id
    }

    fn convert_leaf_to_elements(
        &mut self,
        node: &FigmaNode,
        offset_x: f64,
        offset_y: f64,
        location: &str,
        depth: usize,
    ) -> Vec<Element> {
        if depth > 48 {
            return Vec::new();
        }
        let blend_mode = self.map_blend_mode(&node.blend_mode, location);
        let filters = self.map_effects(&node.effects, location);
        let base_tf = self.node_transform(node, offset_x, offset_y);

        // 1. Instance of a registered Figma Component (when children aren't overridden/detached or matching component exists)
        if node.node_type == "INSTANCE"
            && let Some(cid) = &node.component_id
            && let Some(sym_id) = self.component_symbols.get(cid).cloned()
        {
            let el = Element {
                id: new_id("el"),
                name: sanitize_instance_name(&node.name),
                transform: base_tf,
                kind: ElementKind::Instance {
                    symbol_id: sym_id,
                    playback: InstancePlayback::Independent,
                },
                visible: node.visible,
                cache_as_bitmap: !filters.is_empty(),
                blend_mode,
                filters,
                ..Element::default()
            };
            self.report.statistics.elements += 1;
            return vec![el];
        }

        // 2. Non-empty container (`FRAME`, `GROUP`, `SECTION`, `COMPONENT`, `INSTANCE`, `BOOLEAN_OPERATION` with children)
        if !node.children.is_empty() && node.fill_paths.is_empty() && node.text.is_none() {
            // Also include container background fill inside a nested MovieClip symbol!
            let sym_id = self.build_symbol_with_optional_bg(node, "Figma Layers", depth + 1);
            let el = Element {
                id: new_id("el"),
                name: sanitize_instance_name(&node.name),
                transform: base_tf,
                kind: ElementKind::Instance {
                    symbol_id: sym_id,
                    playback: InstancePlayback::Independent,
                },
                visible: node.visible,
                cache_as_bitmap: !filters.is_empty(),
                blend_mode,
                filters,
                ..Element::default()
            };
            self.report.statistics.elements += 1;
            return vec![el];
        }

        // 3. Editable Text (`TEXT`)
        if let Some(td) = &node.text {
            let text_color = node
                .fills
                .iter()
                .find_map(|p| match p {
                    FigmaPaint::Solid { color } => Some(*color),
                    FigmaPaint::Gradient { stops, .. } => stops.first().map(|s| s.color),
                    _ => None,
                })
                .unwrap_or([255, 255, 255, 255]);
            let text_rgb = (u32::from(text_color[0]) << 16)
                | (u32::from(text_color[1]) << 8)
                | u32::from(text_color[2]);
            let mut text_tf = base_tf;
            text_tf.alpha = (text_tf.alpha * (f64::from(text_color[3]) / 255.0)).clamp(0.0, 1.0);
            let fmt = TextFormat {
                font: td.font_family.clone(),
                size: td.font_size.clamp(1.0, 1024.0),
                color: text_rgb,
                bold: td.bold,
                italic: td.italic,
                align: td.align,
                letter_spacing: td.letter_spacing.clamp(-60.0, 60.0),
                leading: td
                    .line_height
                    .map(|lh| (lh - td.font_size).clamp(-200.0, 500.0))
                    .unwrap_or(2.0),
                ..TextFormat::default()
            };
            let el = Element {
                id: new_id("el"),
                name: node.name.clone(),
                transform: text_tf,
                kind: ElementKind::TextBox {
                    text: td.characters.clone(),
                    width: node.width.max(16.0),
                    height: node.height.max(td.font_size * 1.25).max(16.0),
                    format: fmt,
                    word_wrap: true,
                    multiline: td.characters.contains('\n') || node.height > td.font_size * 1.6,
                },
                visible: node.visible,
                cache_as_bitmap: !filters.is_empty(),
                blend_mode,
                filters,
                ..Element::default()
            };
            self.report.statistics.elements += 1;
            return vec![el];
        }

        // 4. Vector / Primitive / Image Fill Elements
        let mut out = Vec::new();
        let (fallback_cmds, prim_opt) = self.fallback_shape_commands(node);
        let stroke_w = if node.strokes.is_empty() {
            0.0
        } else {
            node.stroke_weight.clamp(0.0, 256.0)
        };
        let (stroke_solid, stroke_grad) = match node.strokes.first() {
            Some(FigmaPaint::Solid { color }) => (*color, None),
            Some(FigmaPaint::Gradient {
                kind,
                stops,
                start,
                end,
            }) => (
                stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255]),
                Some(build_figma_gradient_paint(*kind, stops, *start, *end)),
            ),
            _ => ([0, 0, 0, 0], None),
        };

        let paths_to_fill: Vec<(Vec<PathCommand>, FillRule)> = if !node.fill_paths.is_empty() {
            node.fill_paths
                .iter()
                .filter(|(c, _)| validate_path_commands(c).is_ok())
                .cloned()
                .collect()
        } else if validate_path_commands(&fallback_cmds).is_ok() {
            vec![(fallback_cmds.clone(), FillRule::NonZero)]
        } else {
            Vec::new()
        };

        if node.fills.is_empty() && stroke_w > 0.0 {
            let scmds = node
                .stroke_paths
                .first()
                .filter(|c| validate_path_commands(c).is_ok())
                .cloned()
                .unwrap_or(fallback_cmds);
            if validate_path_commands(&scmds).is_ok() {
                let kind = if stroke_grad.is_some() {
                    ElementKind::PaintedPath {
                        commands: scmds,
                        fill_rule: FillRule::NonZero,
                        fill: [0, 0, 0, 0],
                        stroke: stroke_solid,
                        fill_gradient: None,
                        stroke_gradient: stroke_grad,
                        stroke_width: stroke_w,
                    }
                } else {
                    ElementKind::Path {
                        commands: scmds,
                        fill_rule: FillRule::NonZero,
                        fill: [0, 0, 0, 0],
                        stroke: stroke_solid,
                        stroke_width: stroke_w,
                    }
                };
                out.push(Element {
                    id: new_id("el"),
                    name: node.name.clone(),
                    transform: base_tf,
                    kind,
                    visible: node.visible,
                    cache_as_bitmap: !filters.is_empty(),
                    blend_mode,
                    filters,
                    ..Element::default()
                });
                self.report.statistics.elements += 1;
            }
            return out;
        }

        for (fi, paint) in node.fills.iter().enumerate() {
            let is_last_fill = fi + 1 == node.fills.len();
            let cur_stroke = if is_last_fill {
                stroke_solid
            } else {
                [0, 0, 0, 0]
            };
            let cur_stroke_w = if is_last_fill { stroke_w } else { 0.0 };
            let cur_stroke_grad = if is_last_fill {
                stroke_grad.clone()
            } else {
                None
            };

            match paint {
                FigmaPaint::Image { image_ref, alpha } => {
                    if let Some((res_id, iw, ih)) = self.ensure_image_resource(image_ref, location)
                    {
                        let mut img_tf = base_tf;
                        if iw > 0 && ih > 0 && node.width > 0.0 && node.height > 0.0 {
                            img_tf.scale_x *= node.width / (iw as f64);
                            img_tf.scale_y *= node.height / (ih as f64);
                        }
                        img_tf.alpha = (img_tf.alpha * alpha).clamp(0.0, 1.0);
                        out.push(Element {
                            id: new_id("el"),
                            name: node.name.clone(),
                            transform: img_tf,
                            kind: ElementKind::Bitmap {
                                resource_id: res_id,
                                width: iw,
                                height: ih,
                                smoothing: true,
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if is_last_fill {
                                filters.clone()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.report.statistics.elements += 1;
                    } else if let Some((cmds, rule)) = paths_to_fill.first() {
                        out.push(Element {
                            id: new_id("el"),
                            name: node.name.clone(),
                            transform: base_tf,
                            kind: ElementKind::Path {
                                commands: cmds.clone(),
                                fill_rule: *rule,
                                fill: [180, 180, 190, 255],
                                stroke: cur_stroke,
                                stroke_width: cur_stroke_w,
                            },
                            visible: node.visible,
                            ..Element::default()
                        });
                        self.report.statistics.elements += 1;
                    }
                }
                FigmaPaint::Solid { color } => {
                    if node.node_type == "RECTANGLE" && node.fill_paths.is_empty() {
                        let mut tf = base_tf;
                        tf.primitive = prim_opt;
                        out.push(Element {
                            id: new_id("el"),
                            name: node.name.clone(),
                            transform: tf,
                            kind: if cur_stroke_grad.is_some() {
                                ElementKind::PaintedPath {
                                    commands: fallback_cmds.clone(),
                                    fill_rule: FillRule::NonZero,
                                    fill: *color,
                                    stroke: cur_stroke,
                                    fill_gradient: None,
                                    stroke_gradient: cur_stroke_grad,
                                    stroke_width: cur_stroke_w,
                                }
                            } else {
                                ElementKind::Rectangle {
                                    width: node.width.max(1.0),
                                    height: node.height.max(1.0),
                                    fill: *color,
                                    stroke: cur_stroke,
                                    stroke_width: cur_stroke_w,
                                }
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if is_last_fill {
                                filters.clone()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.report.statistics.elements += 1;
                    } else if node.node_type == "ELLIPSE" && node.fill_paths.is_empty() {
                        let mut tf = base_tf;
                        tf.primitive = prim_opt;
                        out.push(Element {
                            id: new_id("el"),
                            name: node.name.clone(),
                            transform: tf,
                            kind: ElementKind::Ellipse {
                                width: node.width.max(1.0),
                                height: node.height.max(1.0),
                                fill: *color,
                                stroke: cur_stroke,
                                stroke_width: cur_stroke_w,
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if is_last_fill {
                                filters.clone()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.report.statistics.elements += 1;
                    } else {
                        for (cmds, rule) in &paths_to_fill {
                            out.push(Element {
                                id: new_id("el"),
                                name: node.name.clone(),
                                transform: base_tf,
                                kind: if cur_stroke_grad.is_some() {
                                    ElementKind::PaintedPath {
                                        commands: cmds.clone(),
                                        fill_rule: *rule,
                                        fill: *color,
                                        stroke: cur_stroke,
                                        fill_gradient: None,
                                        stroke_gradient: cur_stroke_grad.clone(),
                                        stroke_width: cur_stroke_w,
                                    }
                                } else {
                                    ElementKind::Path {
                                        commands: cmds.clone(),
                                        fill_rule: *rule,
                                        fill: *color,
                                        stroke: cur_stroke,
                                        stroke_width: cur_stroke_w,
                                    }
                                },
                                visible: node.visible,
                                cache_as_bitmap: !filters.is_empty(),
                                blend_mode,
                                filters: if is_last_fill {
                                    filters.clone()
                                } else {
                                    Vec::new()
                                },
                                ..Element::default()
                            });
                            self.report.statistics.elements += 1;
                        }
                    }
                }
                FigmaPaint::Gradient {
                    kind,
                    stops,
                    start,
                    end,
                } => {
                    let grad = build_figma_gradient_paint(*kind, stops, *start, *end);
                    let fallback_c = stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255]);
                    for (cmds, rule) in &paths_to_fill {
                        out.push(Element {
                            id: new_id("el"),
                            name: node.name.clone(),
                            transform: base_tf,
                            kind: ElementKind::PaintedPath {
                                commands: cmds.clone(),
                                fill_rule: *rule,
                                fill: fallback_c,
                                stroke: cur_stroke,
                                fill_gradient: Some(grad.clone()),
                                stroke_gradient: cur_stroke_grad.clone(),
                                stroke_width: cur_stroke_w,
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if is_last_fill {
                                filters.clone()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.report.statistics.elements += 1;
                    }
                }
            }
        }
        out
    }

    fn build_symbol_with_optional_bg(
        &mut self,
        container: &FigmaNode,
        folder: &str,
        depth: usize,
    ) -> String {
        let sym_id = new_id("figsym");
        let mut layers = Vec::new();
        // Optional frame background fill layer at bottom (`layers[0]`)
        if !container.fills.is_empty() && container.width > 0.0 && container.height > 0.0 {
            let mut bg_node = container.clone();
            bg_node.children.clear();
            bg_node.x = 0.0;
            bg_node.y = 0.0;
            bg_node.relative_transform = None;
            let bg_els = self.convert_leaf_to_elements(
                &bg_node,
                0.0,
                0.0,
                &format!("{} / Background", container.name),
                depth + 1,
            );
            if !bg_els.is_empty() {
                layers.push(Layer {
                    id: new_id("layer"),
                    name: "Background".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: bg_els,
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties::default(),
                });
                self.report.statistics.layers += 1;
                self.report.statistics.keyframes += 1;
            }
        }
        let child_layers = self.build_layers_for_children(
            &container.children,
            0.0,
            0.0,
            if container.clips_content {
                Some((container.width.max(1.0), container.height.max(1.0)))
            } else {
                None
            },
            &container.name,
            depth + 1,
        );
        layers.extend(child_layers);
        if layers.is_empty() {
            layers.push(Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties::default(),
            });
        }
        self.project.symbols.push(Symbol {
            id: sym_id.clone(),
            name: container.name.clone(),
            folder: folder.to_owned(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers,
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        self.report.statistics.symbols = self.project.symbols.len();
        sym_id
    }

    fn build_layers_for_children(
        &mut self,
        children: &[FigmaNode],
        offset_x: f64,
        offset_y: f64,
        clip_bounds: Option<(f64, f64)>,
        parent_loc: &str,
        depth: usize,
    ) -> Vec<Layer> {
        if depth > 48 {
            return Vec::new();
        }
        let mut layers = Vec::new();
        let mut active_mask_layer_id: Option<String> = None;

        // Optional frame clip mask (`clipsContent: true`)
        if self.options.preserve_masks
            && let Some((cw, ch)) = clip_bounds
            && !children.is_empty()
        {
            let clip_id = new_id("layer");
            let clip_el = Element {
                id: new_id("el"),
                name: "Clip Bounds".into(),
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: cw,
                    height: ch,
                    fill: [0, 0, 0, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            };
            layers.push(Layer {
                id: clip_id.clone(),
                name: "Frame Clip".into(),
                visible: true,
                locked: true,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![clip_el],
                    ..Keyframe::default()
                }],
                properties: LayerProperties {
                    role: LayerRole::Mask,
                    mask_mode: MaskMode::Geometry,
                    ..LayerProperties::default()
                },
            });
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
            self.report.statistics.elements += 1;
            active_mask_layer_id = Some(clip_id);
        }

        if self.options.layer_mapping == FigmaLayerMapping::FlattenSingleLayer && depth <= 1 {
            let mut all_elements = Vec::new();
            for child in children {
                if !child.visible && !self.options.import_hidden_layers {
                    continue;
                }
                let loc = format!("{parent_loc} / {}", child.name);
                all_elements.extend(self.convert_leaf_to_elements(
                    child,
                    offset_x,
                    offset_y,
                    &loc,
                    depth + 1,
                ));
            }
            let lid = new_id("layer");
            layers.push(Layer {
                id: lid,
                name: parent_loc.to_owned(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: all_elements,
                    ..Keyframe::default()
                }],
                properties: LayerProperties {
                    mask: active_mask_layer_id,
                    ..LayerProperties::default()
                },
            });
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
            return layers;
        }

        for child in children {
            if !child.visible && !self.options.import_hidden_layers {
                continue;
            }
            let loc = format!("{parent_loc} / {}", child.name);

            // Decompose pure `GROUP` / `SECTION` at root composition level into a `Folder` + child layers
            // when no group-level mask/filter/instance forces symbol wrapping!
            if matches!(child.node_type.as_str(), "GROUP" | "SECTION")
                && !child.is_mask
                && child.effects.is_empty()
                && (child.opacity - 1.0).abs() < 1e-6
                && matches!(child.blend_mode.as_str(), "" | "PASS_THROUGH" | "NORMAL")
                && self.options.layer_mapping == FigmaLayerMapping::TimelineLayers
                && !child.children.is_empty()
                && depth <= 3
            {
                let folder_id = new_id("folder");
                layers.push(Layer {
                    id: folder_id.clone(),
                    name: child.name.clone(),
                    visible: child.visible,
                    locked: child.locked,
                    keyframes: Vec::new(),
                    properties: LayerProperties {
                        role: LayerRole::Folder,
                        ..LayerProperties::default()
                    },
                });
                self.report.statistics.layers += 1;

                // Note: in Figma JSON, `GROUP` children already have coordinates relative to the
                // enclosing Frame (via `parent_abs` or `relativeTransform`); in `.fig` Kiwi, group
                // children use local transforms relative to parent if group has an offset, so apply
                // `-offset` cleanly.
                let group_sub_layers = self.build_layers_for_children(
                    &child.children,
                    offset_x - child.x,
                    offset_y - child.y,
                    None,
                    &loc,
                    depth + 1,
                );
                for mut sl in group_sub_layers {
                    if sl.properties.parent.is_none() {
                        sl.properties.parent = Some(folder_id.clone());
                    }
                    if sl.properties.mask.is_none() && sl.properties.role == LayerRole::Normal {
                        sl.properties.mask = active_mask_layer_id.clone();
                    }
                    layers.push(sl);
                }
                continue;
            }

            let mut elements =
                self.convert_leaf_to_elements(child, offset_x, offset_y, &loc, depth + 1);

            if self.options.layer_mapping == FigmaLayerMapping::MovieClipSymbols
                && elements.len() == 1
                && !matches!(elements[0].kind, ElementKind::Instance { .. })
            {
                let sym_id = new_id("figmc");
                let inner_el = elements.remove(0);
                self.project.symbols.push(Symbol {
                    id: sym_id.clone(),
                    name: child.name.clone(),
                    folder: "Figma Layers".into(),
                    kind: SymbolKind::MovieClip,
                    timeline: Timeline {
                        frame_count: 1,
                        layers: vec![Layer {
                            id: new_id("layer"),
                            name: child.name.clone(),
                            visible: true,
                            locked: false,
                            keyframes: vec![Keyframe {
                                frame: 0,
                                elements: vec![inner_el],
                                ..Keyframe::default()
                            }],
                            properties: LayerProperties::default(),
                        }],
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                    class_name: None,
                    linkage_id: None,
                });
                self.report.statistics.symbols = self.project.symbols.len();
                elements.push(Element {
                    id: new_id("el"),
                    name: sanitize_instance_name(&child.name),
                    transform: Transform::default(),
                    kind: ElementKind::Instance {
                        symbol_id: sym_id,
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                });
            }

            let lid = new_id("layer");
            let mut props = LayerProperties {
                blend_mode: self.map_blend_mode(&child.blend_mode, &loc),
                ..LayerProperties::default()
            };

            if child.is_mask && self.options.preserve_masks {
                props.role = LayerRole::Mask;
                props.mask_mode = match child.mask_type.as_str() {
                    "VECTOR" | "OUTLINE" => MaskMode::Geometry,
                    "LUMINANCE" => MaskMode::Luminance,
                    _ => MaskMode::Alpha,
                };
                // Sanitize mask elements so `validate_layers` always passes:
                // - `TextBox` has no vector path in mask evaluation -> convert to bounding rectangle
                // - `Bitmap` on a `Geometry` mask requires paint coverage -> upgrade to `MaskMode::Alpha`
                let mut had_text = false;
                let mut had_bitmap = false;
                for el in &mut elements {
                    match &el.kind {
                        ElementKind::TextBox { width, height, .. } => {
                            let w = (*width).clamp(1.0, 32_768.0);
                            let h = (*height).clamp(1.0, 32_768.0);
                            el.kind = ElementKind::Rectangle {
                                width: w,
                                height: h,
                                fill: [0, 0, 0, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            };
                            had_text = true;
                        }
                        ElementKind::Bitmap { .. } => {
                            had_bitmap = true;
                        }
                        _ => {}
                    }
                }
                if had_text {
                    self.report.warning(
                        "figma-mask-text-bounds-approximated",
                        loc.clone(),
                        "Figma text mask approximated as its bounding rectangle mask.",
                    );
                }
                if had_bitmap && props.mask_mode == MaskMode::Geometry {
                    props.mask_mode = MaskMode::Alpha;
                    self.report.warning(
                        "figma-mask-bitmap-alpha",
                        loc.clone(),
                        "Figma vector mask with bitmap fill upgraded to Alpha mask mode.",
                    );
                }
                active_mask_layer_id = Some(lid.clone());
            } else if let Some(mid) = &active_mask_layer_id {
                props.mask = Some(mid.clone());
            }

            layers.push(Layer {
                id: lid,
                name: child.name.clone(),
                visible: child.visible,
                locked: child.locked,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements,
                    ..Keyframe::default()
                }],
                properties: props,
            });
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
        }
        layers
    }
}

fn sanitize_instance_name(raw: &str) -> String {
    let mut s = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            s.push(ch);
        } else if (ch == ' ' || ch == '-') && !s.ends_with('_') && !s.is_empty() {
            s.push('_');
        }
    }
    let trimmed = s.trim_matches('_');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.as_bytes()[0].is_ascii_digit() {
        format!("_{trimmed}")
    } else {
        trimmed.chars().take(64).collect()
    }
}

pub(super) fn build_project_from_figma_doc(
    doc: FigmaDoc,
    options: &FigmaImportOptions,
    source_label: &str,
) -> Result<(Project, ImportReport), ImportError> {
    let mut project = Project::blank();
    project.name = doc.name.clone();
    let mut report = ImportReport {
        source: source_label.to_owned(),
        format: ImportFormat::Figma,
        version: Some("Figma".into()),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    if doc.pages.is_empty() {
        return Err(ImportError::Blocked(
            "Figma document contains no importable pages or frames.".into(),
        ));
    }

    let mut ctx = FigmaBuildCtx {
        options,
        images: &doc.images,
        image_resources: HashMap::new(),
        component_symbols: HashMap::new(),
        project: &mut project,
        report: &mut report,
    };

    // Pass 1: Pre-allocate all `COMPONENT` symbol IDs first, then populate their timelines,
    // so forward-referenced or nested `INSTANCE`s resolve deterministically and cycles cannot loop.
    let mut comp_nodes = Vec::new();
    for page in &doc.pages {
        ctx.collect_component_nodes(page, &mut comp_nodes, 0);
    }
    ctx.populate_component_symbols(&comp_nodes);

    // Pass 2: Pick primary page & primary composition artboard (if the page has a main artboard Frame,
    // decompose its children directly into the composition's Timeline layers!).
    let primary_page = doc
        .pages
        .iter()
        .find(|p| p.visible && !p.children.is_empty())
        .unwrap_or(&doc.pages[0]);

    if let Some(bg) = primary_page.background_color {
        ctx.project.background = bg;
    }

    let visible_top: Vec<&FigmaNode> = primary_page
        .children
        .iter()
        .filter(|n| n.visible || options.import_hidden_layers)
        .collect();

    let root_layers = if visible_top.len() == 1
        && matches!(
            visible_top[0].node_type.as_str(),
            "FRAME" | "COMPONENT" | "SECTION"
        ) {
        let artboard = visible_top[0];
        if options.resize_stage_to_artboard && artboard.width >= 1.0 && artboard.height >= 1.0 {
            ctx.project.width = (artboard.width.round() as u32).clamp(1, 8192);
            ctx.project.height = (artboard.height.round() as u32).clamp(1, 8192);
        }
        if let Some(FigmaPaint::Solid { color }) = artboard.fills.first() {
            ctx.project.background = *color;
        }
        ctx.build_layers_for_children(
            &artboard.children,
            0.0,
            0.0,
            if artboard.clips_content {
                Some((artboard.width.max(1.0), artboard.height.max(1.0)))
            } else {
                None
            },
            &artboard.name,
            1,
        )
    } else {
        // Multiple top-level frames/elements on the page: fit stage to bounding box or first frame
        if options.resize_stage_to_artboard
            && let Some(first_frame) = visible_top
                .iter()
                .find(|n| matches!(n.node_type.as_str(), "FRAME" | "SECTION"))
            && first_frame.width >= 1.0
            && first_frame.height >= 1.0
        {
            ctx.project.width = (first_frame.width.round() as u32).clamp(1, 8192);
            ctx.project.height = (first_frame.height.round() as u32).clamp(1, 8192);
        }
        let base_x = visible_top.first().map(|n| n.x).unwrap_or(0.0);
        let base_y = visible_top.first().map(|n| n.y).unwrap_or(0.0);
        ctx.build_layers_for_children(
            &primary_page.children,
            base_x,
            base_y,
            None,
            &primary_page.name,
            1,
        )
    };

    // Additional pages -> imported as Library MovieClip compositions so multi-page Figma files lose nothing!
    for extra_page in doc.pages.iter().filter(|p| p.id != primary_page.id) {
        if extra_page.children.is_empty() {
            continue;
        }
        ctx.build_symbol_from_container(extra_page, "Figma Pages", 1);
    }

    let root_id = project.root.clone();
    if let Some(root_sym) = project.symbol_mut(&root_id)
        && !root_layers.is_empty()
    {
        root_sym.timeline.layers = root_layers;
    }

    if !project.resources.is_empty() {
        project.format_version = project.format_version.max(3);
    }
    if project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .any(|l| !l.properties.is_default())
    {
        project.format_version = project.format_version.max(4);
    }
    project.upgrade_format_for_features();
    project.validate().map_err(|e| {
        ImportError::Blocked(format!("Imported Figma composition failed validation: {e}"))
    })?;
    report.statistics.symbols = project.symbols.len();
    report.diagnostics.push(Diagnostic {
        severity: Severity::Info,
        code: "figma-composition-imported".into(),
        location: source_label.to_owned(),
        message: format!(
            "Imported Figma composition '{}' into {} layer(s), {} symbol(s) and {} element(s).",
            project.name,
            report.statistics.layers,
            report.statistics.symbols,
            report.statistics.elements
        ),
    });
    report.can_import = true;
    Ok((project, report))
}

fn build_figma_gradient_paint(
    kind: GradientKind,
    stops: &[GradientStop],
    start: [f64; 2],
    end: [f64; 2],
) -> GradientPaint {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let len = dx.hypot(dy).max(1.0);
    let matrix = match kind {
        GradientKind::Linear => {
            let cos_a = dx / len;
            let sin_a = dy / len;
            let s = len / 1638.4;
            let mx = (start[0] + end[0]) * 0.5;
            let my = (start[1] + end[1]) * 0.5;
            [s * cos_a, s * sin_a, -s * sin_a, s * cos_a, mx, my]
        }
        GradientKind::Radial => {
            let s = len / 819.2;
            [s, 0.0, 0.0, s, start[0], start[1]]
        }
    };
    let mut sorted = stops.to_vec();
    sorted.sort_by_key(|st| st.ratio);
    sorted.truncate(15);
    GradientPaint {
        kind,
        matrix,
        stops: sorted,
        focal_point_ratio: 0.0,
        ..Default::default()
    }
}
