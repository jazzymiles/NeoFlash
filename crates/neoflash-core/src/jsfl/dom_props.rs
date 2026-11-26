use super::*;

impl<'a> JsflVm<'a> {
    pub(super) fn get_property(
        &mut self,
        obj: &JsflValue,
        prop: &str,
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        match obj {
            JsflValue::Array(items) => match prop {
                "length" => Ok(JsflValue::Number(items.len() as f64)),
                _ => Err(self.err(span, format!("Unsupported Array property '{prop}'."))),
            },
            JsflValue::String(s) => match prop {
                "length" => Ok(JsflValue::Number(s.chars().count() as f64)),
                _ => Err(self.err(span, format!("Unsupported String property '{prop}'."))),
            },
            JsflValue::Object(map) => Ok(map.get(prop).cloned().unwrap_or(JsflValue::Undefined)),
            JsflValue::FlGlobal => match prop {
                "version" => Ok(JsflValue::String(self.sandbox.version.clone())),
                "configURI" => Ok(JsflValue::String(self.sandbox.config_uri.clone())),
                "scriptURI" => Ok(JsflValue::String(self.sandbox.script_uri.clone())),
                "outputPanel" => Ok(JsflValue::OutputPanel),
                "documents" => Ok(JsflValue::Array(vec![JsflValue::Doc(DocHandle(0))])),
                "xmlui" | "swfPanels" | "componentsPanel" | "presetPanel" | "compilerErrors" => {
                    Err(self.err(
                        span,
                        format!(
                            "Unsupported JSFL property 'fl.{prop}': legacy UI/extension panels are not supported in the NF2 sandbox."
                        ),
                    ))
                }
                _ => Err(self.err(span, format!("Unsupported property 'fl.{prop}'."))),
            },
            JsflValue::Doc(doc) => {
                self.ensure_doc(*doc, span)?;
                match prop {
                    "width" => Ok(JsflValue::Number(self.project.width as f64)),
                    "height" => Ok(JsflValue::Number(self.project.height as f64)),
                    "frameRate" => Ok(JsflValue::Number(self.project.fps as f64)),
                    "backgroundColor" => {
                        Ok(JsflValue::String(format_hex_color(self.project.background)))
                    }
                    "name" => Ok(JsflValue::String(self.project.name.clone())),
                    "docClass" => Ok(JsflValue::String(
                        self.project.document_class.clone().unwrap_or_default(),
                    )),
                    "library" => Ok(JsflValue::Library(*doc)),
                    "timelines" => {
                        let timelines: Vec<JsflValue> = self
                            .project
                            .scene_ids()
                            .into_iter()
                            .map(|symbol_id| JsflValue::Timeline(TimelineHandle { doc: *doc, symbol_id }))
                            .collect();
                        Ok(JsflValue::Array(timelines))
                    }
                    "currentTimeline" => {
                        let idx = self
                            .project
                            .scene_ids()
                            .iter()
                            .position(|id| *id == self.state.active_symbol)
                            .unwrap_or(0);
                        Ok(JsflValue::Number(idx as f64))
                    }
                    "selection" => self.collect_doc_selection(*doc, span),
                    _ => Err(self.err(span, format!("Unsupported Document property '{prop}'."))),
                }
            }
            JsflValue::Library(doc) => {
                self.ensure_doc(*doc, span)?;
                match prop {
                    "items" => Ok(JsflValue::Array(self.collect_library_items(*doc))),
                    _ => Err(self.err(span, format!("Unsupported Library property '{prop}'."))),
                }
            }
            JsflValue::Timeline(tl) => {
                self.ensure_doc(tl.doc, span)?;
                let sym = self.symbol_ref(&tl.symbol_id, span)?;
                match prop {
                    "name" => Ok(JsflValue::String(sym.name.clone())),
                    "currentFrame" => Ok(JsflValue::Number(self.state.current_frame as f64)),
                    "currentLayer" => Ok(JsflValue::Number(self.state.current_layer as f64)),
                    "frameCount" => Ok(JsflValue::Number(sym.timeline.frame_count as f64)),
                    "layerCount" => Ok(JsflValue::Number(sym.timeline.layers.len() as f64)),
                    "layers" => {
                        let layers: Vec<JsflValue> = sym
                            .timeline
                            .layers
                            .iter()
                            .map(|l| {
                                JsflValue::Layer(LayerHandle {
                                    doc: tl.doc,
                                    symbol_id: tl.symbol_id.clone(),
                                    layer_id: l.id.clone(),
                                })
                            })
                            .collect();
                        Ok(JsflValue::Array(layers))
                    }
                    _ => Err(self.err(span, format!("Unsupported Timeline property '{prop}'."))),
                }
            }
            JsflValue::Layer(lh) => {
                let sym = self.symbol_ref(&lh.symbol_id, span)?;
                let frame_count = sym.timeline.frame_count;
                let layer = self.resolve_layer(lh, span)?;
                match prop {
                    "name" => Ok(JsflValue::String(layer.name.clone())),
                    "visible" => Ok(JsflValue::Bool(layer.visible)),
                    "locked" => Ok(JsflValue::Bool(layer.locked)),
                    "outline" => Ok(JsflValue::Bool(layer.properties.outline)),
                    "color" => {
                        let rgb = layer.properties.outline_color.unwrap_or([74, 144, 226]);
                        Ok(JsflValue::String(format!(
                            "#{:02x}{:02x}{:02x}",
                            rgb[0], rgb[1], rgb[2]
                        )))
                    }
                    "layerType" => {
                        let lt = if layer.properties.motion_guide.is_some() {
                            "guided"
                        } else if layer.properties.mask.is_some() {
                            "masked"
                        } else {
                            match layer.properties.role {
                                LayerRole::Normal => "normal",
                                LayerRole::Guide => "guide",
                                LayerRole::Mask => "mask",
                                LayerRole::Folder => "folder",
                            }
                        };
                        Ok(JsflValue::String(lt.to_owned()))
                    }
                    "frameCount" => Ok(JsflValue::Number(frame_count as f64)),
                    "frames" => {
                        let frames: Vec<JsflValue> = (0..frame_count)
                            .map(|f| {
                                JsflValue::Frame(FrameHandle {
                                    doc: lh.doc,
                                    symbol_id: lh.symbol_id.clone(),
                                    layer_id: lh.layer_id.clone(),
                                    frame: f,
                                })
                            })
                            .collect();
                        Ok(JsflValue::Array(frames))
                    }
                    _ => Err(self.err(span, format!("Unsupported Layer property '{prop}'."))),
                }
            }
            JsflValue::Frame(fh) => {
                let sym = self.symbol_ref(&fh.symbol_id, span)?;
                let layer_index = sym
                    .timeline
                    .layers
                    .iter()
                    .position(|layer| layer.id == fh.layer_id)
                    .ok_or_else(|| self.err(span, "Frame layer no longer exists."))?;
                let script_src = sym
                    .timeline
                    .frame_script_for_layer(layer_index, fh.frame)
                    .map(|s| s.script.source.clone())
                    .unwrap_or_default();
                let (_layer, kf, duration) = self.resolve_frame_keyframe(fh, span)?;
                match prop {
                    "name" => Ok(JsflValue::String(kf.label.clone())),
                    "labelType" => Ok(JsflValue::String(
                        if kf.label.is_empty() && kf.label_type.is_default() {
                            "none"
                        } else if kf.label_type == crate::FrameLabelType::Comment
                            || kf.label.starts_with("//")
                        {
                            "comment"
                        } else if kf.label_type == crate::FrameLabelType::Anchor {
                            "anchor"
                        } else {
                            "name"
                        }
                        .to_owned(),
                    )),
                    "actionScript" => Ok(JsflValue::String(script_src)),
                    "tweenType" => {
                        let tt = match kf.tween {
                            None => "none",
                            Some(_) => match kf.tween_kind {
                                TweenKind::Classic | TweenKind::Motion => "motion",
                                TweenKind::Shape => "shape",
                            },
                        };
                        Ok(JsflValue::String(tt.to_owned()))
                    }
                    "tweenEasing" => {
                        let ease_val = match kf.tween {
                            Some(Easing::Flash { acceleration }) => acceleration,
                            Some(Easing::EaseIn) => -100.0,
                            Some(Easing::EaseOut) => 100.0,
                            _ => 0.0,
                        };
                        Ok(JsflValue::Number(ease_val))
                    }
                    "startFrame" => Ok(JsflValue::Number(kf.frame as f64)),
                    "duration" => Ok(JsflValue::Number(duration as f64)),
                    "elements" => {
                        let kf_frame = kf.frame;
                        let els: Vec<JsflValue> = kf
                            .elements
                            .iter()
                            .map(|e| {
                                JsflValue::Element(ElementHandle {
                                    doc: fh.doc,
                                    symbol_id: fh.symbol_id.clone(),
                                    layer_id: fh.layer_id.clone(),
                                    keyframe: kf_frame,
                                    element_id: e.id.clone(),
                                })
                            })
                            .collect();
                        Ok(JsflValue::Array(els))
                    }
                    _ => Err(self.err(span, format!("Unsupported Frame property '{prop}'."))),
                }
            }
            JsflValue::Item(ih) => {
                self.ensure_doc(ih.doc, span)?;
                if ih.is_folder {
                    let folders = self.project.library_folder_paths();
                    if !folders.iter().any(|f| f == &ih.item_id) {
                        return Err(self.err(
                            span,
                            format!("Stale ItemHandle '{}': folder no longer exists.", ih.item_id),
                        ));
                    }
                    return match prop {
                        "name" => Ok(JsflValue::String(ih.item_id.clone())),
                        "itemType" => Ok(JsflValue::String("folder".to_owned())),
                        "timeline" => Ok(JsflValue::Undefined),
                        _ => Err(self.err(span, format!("Unsupported Folder Item property '{prop}'."))),
                    };
                }
                if ih.is_resource {
                    let res = self.project.resource(&ih.item_id).ok_or_else(|| {
                        self.err(
                            span,
                            format!("Stale ItemHandle '{}': resource no longer exists.", ih.item_id),
                        )
                    })?;
                    let norm = res.path.replace('\\', "/");
                    let stem = Path::new(&norm)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&res.id)
                        .to_owned();
                    let kind = if res.is_audio() {
                        "sound"
                    } else if res.is_image() {
                        "bitmap"
                    } else if res.media_type.starts_with("video/") {
                        "video"
                    } else if res.media_type.starts_with("font/")
                        || norm.ends_with(".ttf")
                        || norm.ends_with(".otf")
                        || norm.ends_with(".woff")
                        || norm.ends_with(".woff2")
                    {
                        "font"
                    } else {
                        "resource"
                    };
                    return match prop {
                        "name" | "sourceFilePath" => Ok(JsflValue::String(norm)),
                        "itemType" => Ok(JsflValue::String(kind.to_owned())),
                        "linkageIdentifier" => Ok(JsflValue::String(stem)),
                        "linkageExportForAS" => Ok(JsflValue::Bool(res.is_audio() || res.is_image())),
                        "timeline" => Ok(JsflValue::Undefined),
                        _ => Err(self.err(span, format!("Unsupported Resource Item property '{prop}'."))),
                    };
                }
                let sym = self.symbol_ref(&ih.item_id, span)?;
                match prop {
                    "name" => {
                        let full = if sym.folder.is_empty() {
                            sym.name.clone()
                        } else {
                            format!("{}/{}", sym.folder, sym.name)
                        };
                        Ok(JsflValue::String(full))
                    }
                    "itemType" => {
                        let kind = match sym.kind {
                            SymbolKind::MovieClip => "movie clip",
                            SymbolKind::Graphic => "graphic",
                            SymbolKind::Button => "button",
                        };
                        Ok(JsflValue::String(kind.to_owned()))
                    }
                    "timeline" => Ok(JsflValue::Timeline(TimelineHandle {
                        doc: ih.doc,
                        symbol_id: sym.id.clone(),
                    })),
                    "linkageIdentifier" => Ok(JsflValue::String(
                        sym.linkage_id.clone().unwrap_or_default(),
                    )),
                    "linkageClassName" => Ok(JsflValue::String(
                        sym.class_name.clone().unwrap_or_default(),
                    )),
                    "linkageExportForAS" => Ok(JsflValue::Bool(
                        sym.linkage_id.is_some() || sym.class_name.is_some(),
                    )),
                    _ => Err(self.err(span, format!("Unsupported Item property '{prop}'."))),
                }
            }
            JsflValue::Element(eh) => {
                let layer = self.resolve_layer(
                    &LayerHandle {
                        doc: eh.doc,
                        symbol_id: eh.symbol_id.clone(),
                        layer_id: eh.layer_id.clone(),
                    },
                    span,
                )?;
                let is_grouped = layer
                    .properties
                    .groups
                    .iter()
                    .any(|g| g.members.contains(&eh.element_id));
                let el = self.resolve_element(eh, span)?;
                match prop {
                    "elementType" => {
                        let et = match &el.kind {
                            ElementKind::Instance { .. } => "instance",
                            ElementKind::Text { .. } | ElementKind::TextBox { .. } => "text",
                            ElementKind::Rectangle { .. }
                            | ElementKind::Ellipse { .. }
                            | ElementKind::Path { .. }
                            | ElementKind::PaintedPath { .. } => {
                                if el.merge { "shape" } else { "shapeObj" }
                            }
                            ElementKind::Bitmap { .. } | ElementKind::Video { .. } => "instance",
                        };
                        Ok(JsflValue::String(et.to_owned()))
                    }
                    "name" => Ok(JsflValue::String(el.name.clone())),
                    "x" | "left" => Ok(JsflValue::Number(el.transform.x)),
                    "y" | "top" => Ok(JsflValue::Number(el.transform.y)),
                    "scaleX" => Ok(JsflValue::Number(el.transform.scale_x)),
                    "scaleY" => Ok(JsflValue::Number(el.transform.scale_y)),
                    "rotation" => Ok(JsflValue::Number(el.transform.rotation)),
                    "skewX" | "skewY" => Ok(JsflValue::Number(el.transform.skew)),
                    "visible" => Ok(JsflValue::Bool(el.visible)),
                    "blendMode" => Ok(JsflValue::String(el.blend_mode.as_str().to_owned())),
                    "isGroup" => Ok(JsflValue::Bool(is_grouped || !el.merge)),
                    "width" => Ok(JsflValue::Number(
                        Self::element_local_size(el).0 * el.transform.scale_x.abs(),
                    )),
                    "height" => Ok(JsflValue::Number(
                        Self::element_local_size(el).1 * el.transform.scale_y.abs(),
                    )),
                    "matrix" => {
                        let [a, b, c, d, tx, ty] = crate::evaluate::transform_matrix(el.transform);
                        let mut m = BTreeMap::new();
                        m.insert("a".to_owned(), JsflValue::Number(a));
                        m.insert("b".to_owned(), JsflValue::Number(b));
                        m.insert("c".to_owned(), JsflValue::Number(c));
                        m.insert("d".to_owned(), JsflValue::Number(d));
                        m.insert("tx".to_owned(), JsflValue::Number(tx));
                        m.insert("ty".to_owned(), JsflValue::Number(ty));
                        Ok(JsflValue::Object(m))
                    }
                    "libraryItem" => match &el.kind {
                        ElementKind::Instance { symbol_id, .. } => Ok(JsflValue::Item(ItemHandle {
                            doc: eh.doc,
                            item_id: symbol_id.clone(),
                            is_folder: false,
                            is_resource: false,
                        })),
                        ElementKind::Bitmap { resource_id, .. } => Ok(JsflValue::Item(ItemHandle {
                            doc: eh.doc,
                            item_id: resource_id.clone(),
                            is_folder: false,
                            is_resource: true,
                        })),
                        _ => Ok(JsflValue::Null),
                    },
                    "symbolType" => match &el.kind {
                        ElementKind::Instance { symbol_id, .. } => {
                            let sym = self.symbol_ref(symbol_id, span)?;
                            let st = match el.effective_symbol_kind_for(sym) {
                                SymbolKind::MovieClip => "movie clip",
                                SymbolKind::Graphic => "graphic",
                                SymbolKind::Button => "button",
                            };
                            Ok(JsflValue::String(st.to_owned()))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "firstFrame" => match &el.kind {
                        ElementKind::Instance { playback, .. } => {
                            let ff = match playback {
                                InstancePlayback::LoopFrom(f)
                                | InstancePlayback::PlayOnceFrom(f)
                                | InstancePlayback::SingleFrame(f) => *f,
                                _ => 0,
                            };
                            Ok(JsflValue::Number(ff as f64))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "loop" => match &el.kind {
                        ElementKind::Instance { playback, .. } => {
                            let lp = match playback {
                                InstancePlayback::Independent
                                | InstancePlayback::Loop
                                | InstancePlayback::LoopFrom(_) => "loop",
                                InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_) => {
                                    "play once"
                                }
                                InstancePlayback::SingleFrame(_) => "single frame",
                            };
                            Ok(JsflValue::String(lp.to_owned()))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "contours" | "edges" | "vertices" => {
                        let cmds = el.to_path_commands().map(|(c, _)| c).unwrap_or_default();
                        let count = cmds.len();
                        let mut arr = Vec::with_capacity(count);
                        for (idx, cmd) in cmds.into_iter().enumerate() {
                            let mut obj = BTreeMap::new();
                            obj.insert("index".to_owned(), JsflValue::Number(idx as f64));
                            match cmd {
                                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                                    obj.insert("x".to_owned(), JsflValue::Number(x));
                                    obj.insert("y".to_owned(), JsflValue::Number(y));
                                }
                                PathCommand::QuadraticTo { x, y, .. }
                                | PathCommand::CubicTo { x, y, .. } => {
                                    obj.insert("x".to_owned(), JsflValue::Number(x));
                                    obj.insert("y".to_owned(), JsflValue::Number(y));
                                }
                                PathCommand::Close => {}
                            }
                            arr.push(JsflValue::Object(obj));
                        }
                        Ok(JsflValue::Array(arr))
                    }
                    "fillColor" | "color" => match &el.kind {
                        ElementKind::Rectangle { fill, .. }
                        | ElementKind::Ellipse { fill, .. }
                        | ElementKind::Path { fill, .. }
                        | ElementKind::PaintedPath { fill, .. } => {
                            if fill[3] == 0 {
                                Ok(JsflValue::String(String::new()))
                            } else {
                                Ok(JsflValue::String(format_hex_color(*fill)))
                            }
                        }
                        ElementKind::Text { color, .. } => {
                            Ok(JsflValue::String(format_hex_color(*color)))
                        }
                        ElementKind::TextBox { format, .. } => {
                            let c = [
                                ((format.color >> 16) & 0xFF) as u8,
                                ((format.color >> 8) & 0xFF) as u8,
                                (format.color & 0xFF) as u8,
                                255,
                            ];
                            Ok(JsflValue::String(format_hex_color(c)))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "strokeColor" => match &el.kind {
                        ElementKind::Rectangle {
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Ellipse {
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Path {
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::PaintedPath {
                            stroke,
                            stroke_width,
                            ..
                        } => {
                            if *stroke_width <= 0.0 || stroke[3] == 0 {
                                Ok(JsflValue::String(String::new()))
                            } else {
                                Ok(JsflValue::String(format_hex_color(*stroke)))
                            }
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "textType" => match &el.kind {
                        ElementKind::Text { .. } => Ok(JsflValue::String("static".to_owned())),
                        ElementKind::TextBox { format, .. } => {
                            let ft = format
                                .field_options()
                                .map(|o| match o.field_type {
                                    crate::TextFieldType::Dynamic => "dynamic",
                                    crate::TextFieldType::Input => "input",
                                })
                                .unwrap_or("dynamic");
                            Ok(JsflValue::String(ft.to_owned()))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "align" => match &el.kind {
                        ElementKind::Text { .. } => Ok(JsflValue::String("left".to_owned())),
                        ElementKind::TextBox { format, .. } => {
                            Ok(JsflValue::String(format.effective_align_mode().to_owned()))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "bold" => match &el.kind {
                        ElementKind::Text { .. } => Ok(JsflValue::Bool(false)),
                        ElementKind::TextBox { format, .. } => Ok(JsflValue::Bool(format.bold)),
                        _ => Ok(JsflValue::Undefined),
                    },
                    "italic" => match &el.kind {
                        ElementKind::Text { .. } => Ok(JsflValue::Bool(false)),
                        ElementKind::TextBox { format, .. } => Ok(JsflValue::Bool(format.italic)),
                        _ => Ok(JsflValue::Undefined),
                    },
                    "face" => match &el.kind {
                        ElementKind::Text { font_family, .. } => {
                            Ok(JsflValue::String(font_family.clone()))
                        }
                        ElementKind::TextBox { format, .. } => {
                            Ok(JsflValue::String(format.font.clone()))
                        }
                        _ => Ok(JsflValue::Undefined),
                    },
                    "size" => match &el.kind {
                        ElementKind::Text { font_size, .. } => Ok(JsflValue::Number(*font_size)),
                        ElementKind::TextBox { format, .. } => Ok(JsflValue::Number(format.size)),
                        _ => Ok(JsflValue::Undefined),
                    },
                    "selectable" => match &el.kind {
                        ElementKind::TextBox { format, .. } => Ok(JsflValue::Bool(
                            format.field_options().is_none_or(|o| o.selectable),
                        )),
                        _ => Ok(JsflValue::Undefined),
                    },
                    "embedFonts" => match &el.kind {
                        ElementKind::TextBox { format, .. } => Ok(JsflValue::Bool(
                            format.field_options().is_some_and(|o| o.embed_fonts),
                        )),
                        _ => Ok(JsflValue::Undefined),
                    },
                    _ => Err(self.err(span, format!("Unsupported Element property '{prop}'."))),
                }
            }
            _ => Err(self.err(
                span,
                format!("Cannot read property '{prop}' of {}.", obj.to_display_string()),
            )),
        }
    }
}

impl<'a> JsflVm<'a> {
    pub(super) fn set_property(
        &mut self,
        obj: &JsflValue,
        prop: &str,
        val: JsflValue,
        span: Span,
    ) -> Result<(), JsflDiagnostic> {
        self.record_mutation(span)?;
        match obj {
            JsflValue::Doc(doc) => {
                self.ensure_doc(*doc, span)?;
                match prop {
                    "width" => {
                        let w = val
                            .as_number()
                            .ok_or_else(|| self.err(span, "Document.width must be a number."))?;
                        if !w.is_finite() || w < 1.0 || w > 8192.0 {
                            return Err(self
                                .err(span, format!("Document.width ({w}) must be in 1..=8192.")));
                        }
                        self.project.width = w.round() as u32;
                        Ok(())
                    }
                    "height" => {
                        let h = val
                            .as_number()
                            .ok_or_else(|| self.err(span, "Document.height must be a number."))?;
                        if !h.is_finite() || h < 1.0 || h > 8192.0 {
                            return Err(self
                                .err(span, format!("Document.height ({h}) must be in 1..=8192.")));
                        }
                        self.project.height = h.round() as u32;
                        Ok(())
                    }
                    "frameRate" => {
                        let fps = val.as_number().ok_or_else(|| {
                            self.err(span, "Document.frameRate must be a number.")
                        })?;
                        if !fps.is_finite() || fps < 1.0 || fps > 240.0 {
                            return Err(self.err(
                                span,
                                format!("Document.frameRate ({fps}) must be in 1..=240."),
                            ));
                        }
                        self.project.fps = fps.round() as u32;
                        Ok(())
                    }
                    "backgroundColor" => {
                        let color = parse_color_value(&val).ok_or_else(|| {
                            self.err(
                                span,
                                "Document.backgroundColor must be a '#RRGGBB' string or 0xRRGGBB integer.",
                            )
                        })?;
                        self.project.background = color;
                        Ok(())
                    }
                    "name" => {
                        self.project.name = val.to_display_string();
                        Ok(())
                    }
                    "docClass" => {
                        let s = val.to_display_string();
                        self.project.document_class = if s.trim().is_empty() {
                            None
                        } else {
                            self.project.enable_program();
                            Some(s.trim().to_owned())
                        };
                        Ok(())
                    }
                    "currentTimeline" => {
                        let Some(raw_idx) = val.as_number() else {
                            return Err(self.err(
                                span,
                                "Document.currentTimeline must be a non-negative integer scene index.",
                            ));
                        };
                        if !raw_idx.is_finite() || raw_idx < 0.0 || raw_idx.fract() != 0.0 {
                            return Err(self.err(
                                span,
                                format!(
                                    "Invalid scene index {raw_idx} for Document.currentTimeline."
                                ),
                            ));
                        }
                        let idx = raw_idx as usize;
                        let scenes = self.project.scene_ids();
                        let Some(target) = scenes.get(idx) else {
                            return Err(self.err(
                                span,
                                format!("Invalid scene index {idx} for Document.currentTimeline."),
                            ));
                        };
                        self.state.active_symbol = target.clone();
                        self.state.current_layer = 0;
                        self.state.current_frame = 0;
                        self.state.selected_element_ids.clear();
                        Ok(())
                    }
                    "selection" => {
                        let JsflValue::Array(items) = val else {
                            return Err(self.err(
                                span,
                                "Document.selection must be assigned an Array of Element handles.",
                            ));
                        };
                        let mut ids = Vec::with_capacity(items.len());
                        for item in items {
                            let JsflValue::Element(eh) = item else {
                                return Err(self.err(
                                    span,
                                    "Document.selection array must contain Element handles.",
                                ));
                            };
                            let _ = self.resolve_element(&eh, span)?;
                            if !ids.contains(&eh.element_id) {
                                ids.push(eh.element_id);
                            }
                        }
                        self.state.selected_element_ids = ids;
                        Ok(())
                    }
                    _ => Err(self.err(
                        span,
                        format!("Cannot assign to Document property '{prop}'."),
                    )),
                }
            }
            JsflValue::Timeline(tl) => {
                self.ensure_doc(tl.doc, span)?;
                let sym = self.symbol_ref(&tl.symbol_id, span)?;
                let frame_count = sym.timeline.frame_count;
                let layer_count = sym.timeline.layers.len();
                match prop {
                    "name" => {
                        let new_name = val.to_display_string();
                        let sym = self.symbol_mut(&tl.symbol_id, span)?;
                        sym.name = new_name;
                        Ok(())
                    }
                    "currentFrame" => {
                        let f = val.as_number().unwrap_or(0.0).max(0.0) as u32;
                        if f >= frame_count {
                            return Err(self.err(
                                span,
                                format!(
                                    "Timeline.currentFrame ({f}) out of range 0..{frame_count}."
                                ),
                            ));
                        }
                        self.state.current_frame = f;
                        Ok(())
                    }
                    "currentLayer" => {
                        let l = val.as_number().unwrap_or(0.0).max(0.0) as usize;
                        if l >= layer_count {
                            return Err(self.err(
                                span,
                                format!(
                                    "Timeline.currentLayer ({l}) out of range 0..{layer_count}."
                                ),
                            ));
                        }
                        self.state.current_layer = l;
                        self.state.selected_layers = vec![l];
                        Ok(())
                    }
                    _ => Err(self.err(
                        span,
                        format!("Cannot assign to Timeline property '{prop}'."),
                    )),
                }
            }
            JsflValue::Layer(lh) => match prop {
                "name" => {
                    let s = val.to_display_string();
                    let layer = self.resolve_layer_mut(lh, span)?;
                    layer.name = s;
                    Ok(())
                }
                "visible" => {
                    let b = val.is_truthy();
                    let layer = self.resolve_layer_mut(lh, span)?;
                    layer.visible = b;
                    Ok(())
                }
                "locked" => {
                    let b = val.is_truthy();
                    let layer = self.resolve_layer_mut(lh, span)?;
                    layer.locked = b;
                    Ok(())
                }
                "outline" => {
                    let b = val.is_truthy();
                    let layer = self.resolve_layer_mut(lh, span)?;
                    layer.properties.outline = b;
                    Ok(())
                }
                "color" => {
                    let c = parse_color_value(&val).ok_or_else(|| {
                        self.err(span, "Layer.color must be '#RRGGBB' or 0xRRGGBB.")
                    })?;
                    let layer = self.resolve_layer_mut(lh, span)?;
                    layer.properties.outline_color = Some([c[0], c[1], c[2]]);
                    Ok(())
                }
                "layerType" => {
                    let lt = val.to_display_string().to_lowercase();
                    let stale_err = self.err(span, format!("Stale LayerHandle '{}'.", lh.layer_id));
                    let bad_lt_err = self.err(span, format!("Unsupported Layer.layerType '{lt}'."));
                    let sym = self.symbol_mut(&lh.symbol_id, span)?;
                    let idx = sym
                        .timeline
                        .layers
                        .iter()
                        .position(|l| l.id == lh.layer_id)
                        .ok_or(stale_err)?;
                    match lt.as_str() {
                        "normal" => {
                            sym.timeline.layers[idx].properties.role = LayerRole::Normal;
                            sym.timeline.layers[idx].properties.motion_guide = None;
                            sym.timeline.layers[idx].properties.mask = None;
                        }
                        "guide" => {
                            sym.timeline.layers[idx].properties.role = LayerRole::Guide;
                            sym.timeline.layers[idx].properties.motion_guide = None;
                        }
                        "guided" => {
                            let guide_id = sym.timeline.layers[..idx]
                                .iter()
                                .rev()
                                .find(|l| l.properties.role == LayerRole::Guide)
                                .or_else(|| {
                                    sym.timeline
                                        .layers
                                        .iter()
                                        .find(|l| l.properties.role == LayerRole::Guide)
                                })
                                .map(|l| l.id.clone());
                            sym.timeline.layers[idx].properties.role = LayerRole::Normal;
                            sym.timeline.layers[idx].properties.motion_guide = guide_id;
                        }
                        "mask" => {
                            sym.timeline.layers[idx].properties.role = LayerRole::Mask;
                            sym.timeline.layers[idx].properties.mask = None;
                        }
                        "masked" => {
                            let mask_id = sym.timeline.layers[..idx]
                                .iter()
                                .rev()
                                .find(|l| l.properties.role == LayerRole::Mask)
                                .or_else(|| {
                                    sym.timeline
                                        .layers
                                        .iter()
                                        .find(|l| l.properties.role == LayerRole::Mask)
                                })
                                .map(|l| l.id.clone());
                            sym.timeline.layers[idx].properties.role = LayerRole::Normal;
                            sym.timeline.layers[idx].properties.mask = mask_id;
                        }
                        "folder" => {
                            sym.timeline.layers[idx].properties.role = LayerRole::Folder;
                        }
                        _ => {
                            return Err(bad_lt_err);
                        }
                    }
                    Ok(())
                }
                _ => Err(self.err(span, format!("Cannot assign to Layer property '{prop}'."))),
            },
            JsflValue::Frame(fh) => {
                let (_, kf_ref, _) = self.resolve_frame_keyframe(fh, span)?;
                let kf_start = kf_ref.frame;
                match prop {
                    "name" => {
                        let label = val.to_display_string();
                        let layer = self.resolve_layer_mut(
                            &LayerHandle {
                                doc: fh.doc,
                                symbol_id: fh.symbol_id.clone(),
                                layer_id: fh.layer_id.clone(),
                            },
                            span,
                        )?;
                        if let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == kf_start) {
                            kf.label = label;
                        }
                        Ok(())
                    }
                    "labelType" => {
                        let lt = val.to_display_string().to_lowercase();
                        let layer = self.resolve_layer_mut(
                            &LayerHandle {
                                doc: fh.doc,
                                symbol_id: fh.symbol_id.clone(),
                                layer_id: fh.layer_id.clone(),
                            },
                            span,
                        )?;
                        if let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == kf_start) {
                            match lt.as_str() {
                                "none" => {
                                    kf.label.clear();
                                    kf.label_type = crate::FrameLabelType::Name;
                                }
                                "name" => {
                                    kf.label_type = crate::FrameLabelType::Name;
                                    if let Some(stripped) = kf.label.strip_prefix("//") {
                                        kf.label = stripped.to_owned();
                                    }
                                }
                                "comment" => {
                                    kf.label_type = crate::FrameLabelType::Comment;
                                }
                                "anchor" => {
                                    kf.label_type = crate::FrameLabelType::Anchor;
                                }
                                _ => {
                                    return Err(self.err(
                                        span,
                                        format!("Unsupported Frame.labelType '{lt}'."),
                                    ));
                                }
                            }
                        }
                        self.project.upgrade_format_for_features();
                        Ok(())
                    }
                    "actionScript" => {
                        let code = val.to_display_string();
                        let layer_index = self
                            .symbol_ref(&fh.symbol_id, span)?
                            .timeline
                            .layers
                            .iter()
                            .position(|layer| layer.id == fh.layer_id)
                            .ok_or_else(|| self.err(span, "Frame layer no longer exists."))?;
                        let sym = self.symbol_mut(&fh.symbol_id, span)?;
                        let existing = sym
                            .timeline
                            .frame_script_index_for_layer(layer_index, kf_start);
                        if code.trim().is_empty() {
                            if let Some(index) = existing {
                                sym.timeline.scripts.remove(index);
                            }
                        } else if let Some(index) = existing {
                            let slot = &mut sym.timeline.scripts[index];
                            slot.layer = Some(fh.layer_id.clone());
                            slot.script.source = code;
                        } else {
                            sym.timeline.scripts.push(FrameScript {
                                layer: Some(fh.layer_id.clone()),
                                frame: kf_start,
                                script: Script {
                                    language: ScriptLanguage::ActionScript2,
                                    source: code,
                                },
                            });
                            sym.timeline.scripts.sort_by_key(|s| s.frame);
                        }
                        Ok(())
                    }
                    "tweenType" => {
                        let tt = val.to_display_string().to_lowercase();
                        let layer = self.resolve_layer_mut(
                            &LayerHandle {
                                doc: fh.doc,
                                symbol_id: fh.symbol_id.clone(),
                                layer_id: fh.layer_id.clone(),
                            },
                            span,
                        )?;
                        if let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == kf_start) {
                            match tt.as_str() {
                                "none" => {
                                    kf.tween = None;
                                    kf.tween_kind = TweenKind::Classic;
                                    kf.keyed_properties = None;
                                }
                                "motion" | "classic" => {
                                    kf.tween = Some(kf.tween.unwrap_or(Easing::Linear));
                                    kf.tween_kind = if tt == "motion" {
                                        TweenKind::Motion
                                    } else {
                                        TweenKind::Classic
                                    };
                                }
                                "shape" => {
                                    kf.tween = Some(kf.tween.unwrap_or(Easing::Linear));
                                    kf.tween_kind = TweenKind::Shape;
                                    kf.keyed_properties = None;
                                }
                                _ => {
                                    return Err(self.err(
                                        span,
                                        format!("Unsupported Frame.tweenType '{tt}'."),
                                    ));
                                }
                            }
                        }
                        layer.normalize_motion_keys();
                        Ok(())
                    }
                    "tweenEasing" => {
                        let acc = val.as_number().unwrap_or(0.0).clamp(-100.0, 100.0);
                        let layer = self.resolve_layer_mut(
                            &LayerHandle {
                                doc: fh.doc,
                                symbol_id: fh.symbol_id.clone(),
                                layer_id: fh.layer_id.clone(),
                            },
                            span,
                        )?;
                        if let Some(kf) = layer.keyframes.iter_mut().find(|k| k.frame == kf_start) {
                            kf.tween = Some(if acc.abs() <= 1e-6 {
                                Easing::Linear
                            } else {
                                Easing::Flash { acceleration: acc }
                            });
                        }
                        Ok(())
                    }
                    _ => Err(self.err(span, format!("Cannot assign to Frame property '{prop}'."))),
                }
            }
            JsflValue::Item(ih) => {
                self.ensure_doc(ih.doc, span)?;
                if ih.is_folder {
                    if prop == "name" {
                        let new_folder = val.to_display_string();
                        self.project
                            .relocate_library_folder(&ih.item_id, &new_folder)
                            .map_err(|e| self.err(span, e))?;
                        return Ok(());
                    }
                    return Err(self.err(
                        span,
                        format!("Cannot assign to Folder Item property '{prop}'."),
                    ));
                }
                match prop {
                    "name" => {
                        let raw = val.to_display_string();
                        let leaf = raw.rsplit('/').next().unwrap_or(&raw).trim().to_owned();
                        if leaf.is_empty() {
                            return Err(self.err(span, "Item.name cannot be empty."));
                        }
                        let sym = self.symbol_mut(&ih.item_id, span)?;
                        sym.name = leaf;
                        Ok(())
                    }
                    "linkageIdentifier" => {
                        let s = val.to_display_string();
                        let sym = self.symbol_mut(&ih.item_id, span)?;
                        sym.linkage_id = if s.trim().is_empty() {
                            None
                        } else {
                            Some(s.trim().to_owned())
                        };
                        self.project.enable_program();
                        Ok(())
                    }
                    "linkageClassName" => {
                        let s = val.to_display_string();
                        let sym = self.symbol_mut(&ih.item_id, span)?;
                        sym.class_name = if s.trim().is_empty() {
                            None
                        } else {
                            Some(s.trim().to_owned())
                        };
                        self.project.enable_program();
                        Ok(())
                    }
                    "linkageExportForAS" => Ok(()),
                    _ => Err(self.err(span, format!("Cannot assign to Item property '{prop}'."))),
                }
            }
            JsflValue::Element(eh) => self.set_element_property(eh, prop, val, span),
            _ => Err(self.err(
                span,
                format!(
                    "Cannot set property '{prop}' on {}.",
                    obj.to_display_string()
                ),
            )),
        }
    }
}
