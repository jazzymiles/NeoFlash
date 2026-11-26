use super::*;

impl<'a> JsflVm<'a> {
    pub(super) fn element_local_size(el: &Element) -> (f64, f64) {
        match &el.kind {
            ElementKind::Rectangle { width, height, .. }
            | ElementKind::Ellipse { width, height, .. }
            | ElementKind::Video { width, height }
            | ElementKind::TextBox { width, height, .. } => (*width, *height),
            ElementKind::Bitmap { width, height, .. } => (*width as f64, *height as f64),
            ElementKind::Text {
                text, font_size, ..
            } => (
                (text.chars().count().max(1) as f64) * font_size * 0.6,
                *font_size,
            ),
            ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
                if let Ok(Some(b)) = crate::geometry::path_bounds(commands) {
                    ((b.x_max - b.x_min).max(1.0), (b.y_max - b.y_min).max(1.0))
                } else {
                    (50.0, 50.0)
                }
            }
            ElementKind::Instance { .. } => (100.0, 100.0),
        }
    }
}

impl<'a> JsflVm<'a> {
    pub(super) fn set_element_property(
        &mut self,
        eh: &ElementHandle,
        prop: &str,
        val: JsflValue,
        span: Span,
    ) -> Result<(), JsflDiagnostic> {
        match prop {
            "name" => {
                let s = val.to_display_string();
                let el = self.resolve_element_mut(eh, span)?;
                el.name = s;
                Ok(())
            }
            "x" | "left" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.x must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.x = n;
                Ok(())
            }
            "y" | "top" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.y must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.y = n;
                Ok(())
            }
            "scaleX" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.scaleX must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.scale_x = n;
                Ok(())
            }
            "scaleY" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.scaleY must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.scale_y = n;
                Ok(())
            }
            "rotation" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.rotation must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.rotation = n;
                Ok(())
            }
            "skewX" | "skewY" => {
                let n = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.skew must be a number."))?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.skew = n;
                Ok(())
            }
            "width" => {
                let w = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.width must be a number."))?;
                if !w.is_finite() || w <= 0.0 {
                    return Err(self.err(span, "Element.width must be positive."));
                }
                let el = self.resolve_element_mut(eh, span)?;
                match &mut el.kind {
                    ElementKind::Rectangle { width, .. }
                    | ElementKind::Ellipse { width, .. }
                    | ElementKind::Video { width, .. }
                    | ElementKind::TextBox { width, .. } => {
                        *width = w / el.transform.scale_x.abs().max(1e-6);
                    }
                    _ => {
                        let base_w = Self::element_local_size(el).0.max(1e-6);
                        el.transform.scale_x = w / base_w;
                    }
                }
                Ok(())
            }
            "height" => {
                let h = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.height must be a number."))?;
                if !h.is_finite() || h <= 0.0 {
                    return Err(self.err(span, "Element.height must be positive."));
                }
                let el = self.resolve_element_mut(eh, span)?;
                match &mut el.kind {
                    ElementKind::Rectangle { height, .. }
                    | ElementKind::Ellipse { height, .. }
                    | ElementKind::Video { height, .. }
                    | ElementKind::TextBox { height, .. } => {
                        *height = h / el.transform.scale_y.abs().max(1e-6);
                    }
                    _ => {
                        let base_h = Self::element_local_size(el).1.max(1e-6);
                        el.transform.scale_y = h / base_h;
                    }
                }
                Ok(())
            }
            "visible" => {
                let b = val.is_truthy();
                let el = self.resolve_element_mut(eh, span)?;
                el.visible = b;
                Ok(())
            }
            "align" => {
                let raw = val.to_display_string().to_ascii_lowercase();
                let (next_align, next_mode) = match raw.as_str() {
                    "left" => (crate::TextAlign::Left, None),
                    "center" => (crate::TextAlign::Center, None),
                    "right" => (crate::TextAlign::Right, None),
                    "justify" => (crate::TextAlign::Left, Some("justify".to_owned())),
                    _ => {
                        return Err(self.err(
                            span,
                            format!("Unsupported Element.align '{raw}' (expected left|center|right|justify)."),
                        ));
                    }
                };
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::TextBox { format, .. } = &mut el.kind {
                    format.align = next_align;
                    format.set_align_mode(next_mode);
                    self.project.upgrade_format_for_features();
                    Ok(())
                } else {
                    Err(self.err(span, "Element.align can only be set on a TextBox."))
                }
            }
            "bold" => {
                let b = val.is_truthy();
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::TextBox { format, .. } = &mut el.kind {
                    format.bold = b;
                    self.project.upgrade_format_for_features();
                    Ok(())
                } else {
                    Err(self.err(span, "Element.bold can only be set on a TextBox."))
                }
            }
            "italic" => {
                let b = val.is_truthy();
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::TextBox { format, .. } = &mut el.kind {
                    format.italic = b;
                    self.project.upgrade_format_for_features();
                    Ok(())
                } else {
                    Err(self.err(span, "Element.italic can only be set on a TextBox."))
                }
            }
            "face" => {
                let f = val.to_display_string();
                let el = self.resolve_element_mut(eh, span)?;
                match &mut el.kind {
                    ElementKind::Text { font_family, .. } => {
                        *font_family = f;
                        Ok(())
                    }
                    ElementKind::TextBox { format, .. } => {
                        format.font = f;
                        Ok(())
                    }
                    _ => Err(self.err(span, "Element.face can only be set on a Text element.")),
                }
            }
            "size" => {
                let sz = val
                    .as_number()
                    .ok_or_else(|| self.err(span, "Element.size must be a number."))?;
                if !sz.is_finite() || sz <= 0.0 {
                    return Err(self.err(span, "Element.size must be positive."));
                }
                let el = self.resolve_element_mut(eh, span)?;
                match &mut el.kind {
                    ElementKind::Text { font_size, .. } => {
                        *font_size = sz;
                        Ok(())
                    }
                    ElementKind::TextBox { format, .. } => {
                        format.size = sz;
                        Ok(())
                    }
                    _ => Err(self.err(span, "Element.size can only be set on a Text element.")),
                }
            }
            "selectable" => {
                let b = val.is_truthy();
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::TextBox { format, .. } = &mut el.kind {
                    let mut opts = format.field_options().unwrap_or_default();
                    opts.selectable = b;
                    format.set_field_options((!opts.is_default()).then_some(opts));
                    self.project.upgrade_format_for_features();
                    Ok(())
                } else {
                    Err(self.err(span, "Element.selectable can only be set on a TextBox."))
                }
            }
            "embedFonts" => {
                let b = val.is_truthy();
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::TextBox { format, .. } = &mut el.kind {
                    let mut opts = format.field_options().unwrap_or_default();
                    opts.embed_fonts = b;
                    format.set_field_options((!opts.is_default()).then_some(opts));
                    self.project.upgrade_format_for_features();
                    Ok(())
                } else {
                    Err(self.err(span, "Element.embedFonts can only be set on a TextBox."))
                }
            }
            "textType" => {
                let raw = val.to_display_string().to_ascii_lowercase();
                let next_ft = match raw.as_str() {
                    "dynamic" => crate::TextFieldType::Dynamic,
                    "input" => crate::TextFieldType::Input,
                    _ => {
                        return Err(self.err(
                            span,
                            format!(
                                "Unsupported TextBox.textType '{raw}' (expected dynamic|input)."
                            ),
                        ));
                    }
                };
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::TextBox { format, .. } = &mut el.kind {
                    let mut opts = format.field_options().unwrap_or_default();
                    opts.field_type = next_ft;
                    format.set_field_options((!opts.is_default()).then_some(opts));
                    self.project.upgrade_format_for_features();
                    Ok(())
                } else {
                    Err(self.err(span, "Element.textType can only be set on a TextBox."))
                }
            }
            "blendMode" => {
                let s = val.to_display_string().to_lowercase();
                let bm = BlendMode::from_name(&s).ok_or_else(|| {
                    self.err(span, format!("Unsupported Element.blendMode '{s}'."))
                })?;
                let el = self.resolve_element_mut(eh, span)?;
                el.blend_mode = bm;
                Ok(())
            }
            "matrix" => {
                let JsflValue::Object(map) = val else {
                    return Err(self.err(
                        span,
                        "Element.matrix must be an Object {a, b, c, d, tx, ty}.",
                    ));
                };
                let get_f = |k: &str, def: f64| -> f64 {
                    map.get(k).and_then(JsflValue::as_number).unwrap_or(def)
                };
                let mat = [
                    get_f("a", 1.0),
                    get_f("b", 0.0),
                    get_f("c", 0.0),
                    get_f("d", 1.0),
                    get_f("tx", 0.0),
                    get_f("ty", 0.0),
                ];
                let new_t = Transform::from_matrix(mat).ok_or_else(|| {
                    self.err(
                        span,
                        "Element.matrix values are non-finite or out of bounds.",
                    )
                })?;
                let el = self.resolve_element_mut(eh, span)?;
                el.transform.x = new_t.x;
                el.transform.y = new_t.y;
                el.transform.scale_x = new_t.scale_x;
                el.transform.scale_y = new_t.scale_y;
                el.transform.rotation = new_t.rotation;
                el.transform.skew = new_t.skew;
                Ok(())
            }
            "libraryItem" => {
                let target_sym_id = match val {
                    JsflValue::Item(ih) if !ih.is_folder => ih.item_id,
                    JsflValue::String(path) => {
                        self.find_library_symbol_by_path(&path).ok_or_else(|| {
                            self.err(span, format!("Library symbol '{path}' not found."))
                        })?
                    }
                    _ => {
                        return Err(self.err(
                            span,
                            "Element.libraryItem must be a symbol ItemHandle or symbol path.",
                        ));
                    }
                };
                let _ = self.symbol_ref(&target_sym_id, span)?;
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::Instance { symbol_id, .. } = &mut el.kind {
                    *symbol_id = target_sym_id;
                    Ok(())
                } else {
                    Err(self.err(
                        span,
                        "Element.libraryItem can only be set on a SymbolInstance.",
                    ))
                }
            }
            "symbolType" => {
                let st = val.to_display_string().to_lowercase();
                let next_kind = match st.as_str() {
                    "movie clip" | "movieclip" => SymbolKind::MovieClip,
                    "graphic" => SymbolKind::Graphic,
                    "button" => SymbolKind::Button,
                    _ => {
                        return Err(self.err(
                            span,
                            format!("Unsupported SymbolInstance.symbolType '{st}'."),
                        ));
                    }
                };
                let sym_id = {
                    let el = self.resolve_element(eh, span)?;
                    let ElementKind::Instance { symbol_id, .. } = &el.kind else {
                        return Err(self.err(
                            span,
                            "Element.symbolType can only be set on a SymbolInstance.",
                        ));
                    };
                    symbol_id.clone()
                };
                let lib_kind = self.symbol_ref(&sym_id, span)?.kind;
                let el = self.resolve_element_mut(eh, span)?;
                el.instance_behavior = (next_kind != lib_kind).then_some(next_kind);
                if let ElementKind::Instance { playback, .. } = &mut el.kind {
                    *playback = match next_kind {
                        SymbolKind::MovieClip => InstancePlayback::Independent,
                        SymbolKind::Graphic => InstancePlayback::Loop,
                        SymbolKind::Button => InstancePlayback::SingleFrame(0),
                    };
                }
                self.project.upgrade_format_for_features();
                Ok(())
            }
            "firstFrame" => {
                let ff = val.as_number().unwrap_or(0.0).max(0.0) as u32;
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::Instance { playback, .. } = &mut el.kind {
                    *playback = match *playback {
                        InstancePlayback::SingleFrame(_) => InstancePlayback::SingleFrame(ff),
                        InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_) => {
                            if ff == 0 {
                                InstancePlayback::PlayOnce
                            } else {
                                InstancePlayback::PlayOnceFrom(ff)
                            }
                        }
                        _ => {
                            if ff == 0 {
                                InstancePlayback::Loop
                            } else {
                                InstancePlayback::LoopFrom(ff)
                            }
                        }
                    };
                    Ok(())
                } else {
                    Err(self.err(
                        span,
                        "Element.firstFrame can only be set on a SymbolInstance.",
                    ))
                }
            }
            "loop" => {
                let lp = val.to_display_string().to_lowercase();
                let el = self.resolve_element_mut(eh, span)?;
                if let ElementKind::Instance { playback, .. } = &mut el.kind {
                    let cur_ff = match *playback {
                        InstancePlayback::LoopFrom(f)
                        | InstancePlayback::PlayOnceFrom(f)
                        | InstancePlayback::SingleFrame(f) => f,
                        _ => 0,
                    };
                    *playback = match lp.as_str() {
                        "loop" => {
                            if cur_ff == 0 {
                                InstancePlayback::Loop
                            } else {
                                InstancePlayback::LoopFrom(cur_ff)
                            }
                        }
                        "play once" => {
                            if cur_ff == 0 {
                                InstancePlayback::PlayOnce
                            } else {
                                InstancePlayback::PlayOnceFrom(cur_ff)
                            }
                        }
                        "single frame" => InstancePlayback::SingleFrame(cur_ff),
                        _ => {
                            return Err(self.err(
                                span,
                                format!("Unsupported SymbolInstance.loop mode '{lp}'."),
                            ));
                        }
                    };
                    Ok(())
                } else {
                    Err(self.err(span, "Element.loop can only be set on a SymbolInstance."))
                }
            }
            "fillColor" | "color" => {
                let new_color = parse_color_value(&val).ok_or_else(|| {
                    self.err(
                        span,
                        format!("Element.{prop} must be a '#RRGGBB' string or 0xRRGGBB integer."),
                    )
                })?;
                let el = self.resolve_element_mut(eh, span)?;
                match &mut el.kind {
                    ElementKind::Rectangle { fill, .. }
                    | ElementKind::Ellipse { fill, .. }
                    | ElementKind::Path { fill, .. } => {
                        *fill = new_color;
                        Ok(())
                    }
                    ElementKind::PaintedPath {
                        fill,
                        fill_gradient,
                        ..
                    } => {
                        *fill = new_color;
                        *fill_gradient = None;
                        Ok(())
                    }
                    ElementKind::Text { color, .. } => {
                        *color = new_color;
                        Ok(())
                    }
                    ElementKind::TextBox { format, .. } => {
                        format.color = (u32::from(new_color[0]) << 16)
                            | (u32::from(new_color[1]) << 8)
                            | u32::from(new_color[2]);
                        Ok(())
                    }
                    _ => Err(self.err(
                        span,
                        format!("Element.{prop} can only be set on a Shape or Text element."),
                    )),
                }
            }
            "strokeColor" => {
                let new_color = parse_color_value(&val).ok_or_else(|| {
                    self.err(
                        span,
                        "Element.strokeColor must be a '#RRGGBB' string or 0xRRGGBB integer.",
                    )
                })?;
                let el = self.resolve_element_mut(eh, span)?;
                match &mut el.kind {
                    ElementKind::Rectangle { stroke, .. }
                    | ElementKind::Ellipse { stroke, .. }
                    | ElementKind::Path { stroke, .. } => {
                        *stroke = new_color;
                        Ok(())
                    }
                    ElementKind::PaintedPath {
                        stroke,
                        stroke_gradient,
                        ..
                    } => {
                        *stroke = new_color;
                        *stroke_gradient = None;
                        Ok(())
                    }
                    _ => Err(self.err(
                        span,
                        "Element.strokeColor can only be set on a Shape element.",
                    )),
                }
            }
            _ => Err(self.err(span, format!("Cannot assign to Element property '{prop}'."))),
        }
    }
}

impl<'a> JsflVm<'a> {
    pub(super) fn call_element_method(
        &mut self,
        eh: ElementHandle,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        match method {
            "getTextString" => {
                let el = self.resolve_element(&eh, span)?;
                match &el.kind {
                    ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
                        Ok(JsflValue::String(text.clone()))
                    }
                    _ => Err(self.err(span, "Element.getTextString() requires a Text element.")),
                }
            }
            "setTextString" => {
                self.record_mutation(span)?;
                let new_text = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let el = self.resolve_element_mut(&eh, span)?;
                match &mut el.kind {
                    ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
                        *text = new_text;
                        Ok(JsflValue::Undefined)
                    }
                    _ => Err(self.err(span, "Element.setTextString() requires a Text element.")),
                }
            }
            "getTextAttr" => {
                let attr = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "fillColor".to_owned());
                let el = self.resolve_element(&eh, span)?;
                match attr.as_str() {
                    "fillColor" | "color" => match &el.kind {
                        ElementKind::Text { color, .. } => {
                            Ok(JsflValue::String(format_rgba_hex(*color)))
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
                        _ => Err(self.err(span, "Element.getTextAttr() requires a Text element.")),
                    },
                    "size" => match &el.kind {
                        ElementKind::Text { font_size, .. } => Ok(JsflValue::Number(*font_size)),
                        ElementKind::TextBox { format, .. } => Ok(JsflValue::Number(format.size)),
                        _ => Err(self.err(span, "Element.getTextAttr() requires a Text element.")),
                    },
                    "face" => match &el.kind {
                        ElementKind::Text { font_family, .. } => {
                            Ok(JsflValue::String(font_family.clone()))
                        }
                        ElementKind::TextBox { format, .. } => {
                            Ok(JsflValue::String(format.font.clone()))
                        }
                        _ => Err(self.err(span, "Element.getTextAttr() requires a Text element.")),
                    },
                    _ => Ok(JsflValue::Undefined),
                }
            }
            "setTextAttr" => {
                self.record_mutation(span)?;
                let attr = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "fillColor".to_owned());
                let val = args.get(1).cloned().unwrap_or(JsflValue::Undefined);
                match attr.as_str() {
                    "fillColor" | "color" => {
                        let new_color = parse_color_value(&val).ok_or_else(|| {
                            self.err(
                                span,
                                "Element.setTextAttr('fillColor', value) expects a '#RRGGBB' string or 0xRRGGBB integer.",
                            )
                        })?;
                        let el = self.resolve_element_mut(&eh, span)?;
                        match &mut el.kind {
                            ElementKind::Text { color, .. } => {
                                *color = new_color;
                                Ok(JsflValue::Undefined)
                            }
                            ElementKind::TextBox { format, .. } => {
                                let rgb = (u32::from(new_color[0]) << 16)
                                    | (u32::from(new_color[1]) << 8)
                                    | u32::from(new_color[2]);
                                format.color = rgb;
                                Ok(JsflValue::Undefined)
                            }
                            _ => {
                                Err(self
                                    .err(span, "Element.setTextAttr() requires a Text element."))
                            }
                        }
                    }
                    "size" => {
                        let sz = val.as_number().unwrap_or(16.0).clamp(1.0, 500.0);
                        let el = self.resolve_element_mut(&eh, span)?;
                        match &mut el.kind {
                            ElementKind::Text { font_size, .. } => {
                                *font_size = sz;
                                Ok(JsflValue::Undefined)
                            }
                            ElementKind::TextBox { format, .. } => {
                                format.size = sz;
                                Ok(JsflValue::Undefined)
                            }
                            _ => {
                                Err(self
                                    .err(span, "Element.setTextAttr() requires a Text element."))
                            }
                        }
                    }
                    "face" => {
                        let face = val.to_display_string();
                        let el = self.resolve_element_mut(&eh, span)?;
                        match &mut el.kind {
                            ElementKind::Text { font_family, .. } => {
                                *font_family = face;
                                Ok(JsflValue::Undefined)
                            }
                            ElementKind::TextBox { format, .. } => {
                                format.font = face;
                                Ok(JsflValue::Undefined)
                            }
                            _ => {
                                Err(self
                                    .err(span, "Element.setTextAttr() requires a Text element."))
                            }
                        }
                    }
                    _ => Ok(JsflValue::Undefined),
                }
            }
            "getCustomFill" => {
                let el = self.resolve_element(&eh, span)?;
                let mut obj = BTreeMap::new();
                if let ElementKind::PaintedPath {
                    fill_gradient: Some(grad),
                    ..
                } = &el.kind
                {
                    let style = match grad.kind {
                        crate::GradientKind::Linear => "linearGradient",
                        crate::GradientKind::Radial => "radialGradient",
                    };
                    obj.insert("style".to_owned(), JsflValue::String(style.to_owned()));
                    let colors: Vec<JsflValue> = grad
                        .stops
                        .iter()
                        .map(|s| JsflValue::String(format_rgba_hex(s.color)))
                        .collect();
                    let positions: Vec<JsflValue> = grad
                        .stops
                        .iter()
                        .map(|s| JsflValue::Number(f64::from(s.ratio)))
                        .collect();
                    let first_color = grad
                        .stops
                        .first()
                        .map(|s| s.color)
                        .unwrap_or([0, 0, 0, 255]);
                    obj.insert(
                        "color".to_owned(),
                        JsflValue::String(format_rgba_hex(first_color)),
                    );
                    obj.insert("colorArray".to_owned(), JsflValue::Array(colors));
                    obj.insert("posArray".to_owned(), JsflValue::Array(positions));
                    return Ok(JsflValue::Object(obj));
                }
                let fill_color = match &el.kind {
                    ElementKind::Rectangle { fill, .. }
                    | ElementKind::Ellipse { fill, .. }
                    | ElementKind::Path { fill, .. }
                    | ElementKind::PaintedPath { fill, .. } => *fill,
                    _ => [0, 0, 0, 255],
                };
                obj.insert(
                    "style".to_owned(),
                    JsflValue::String(
                        if fill_color[3] == 0 {
                            "noFill"
                        } else {
                            "solid"
                        }
                        .to_owned(),
                    ),
                );
                obj.insert(
                    "color".to_owned(),
                    JsflValue::String(format_rgba_hex(fill_color)),
                );
                Ok(JsflValue::Object(obj))
            }
            "setCustomFill" => {
                self.record_mutation(span)?;
                let Some(JsflValue::Object(map)) = args.first() else {
                    return Err(self.err(span, "setCustomFill expects a Fill object."));
                };
                let style = map
                    .get("style")
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "solid".to_owned());
                let parsed_stops: Option<Vec<crate::GradientStop>> =
                    if let Some(JsflValue::Array(color_arr)) = map.get("colorArray") {
                        let pos_arr = match map.get("posArray") {
                            Some(JsflValue::Array(p)) => Some(p.as_slice()),
                            _ => None,
                        };
                        let len = color_arr.len().max(1);
                        let mut stops = Vec::with_capacity(color_arr.len());
                        for (idx, c_val) in color_arr.iter().enumerate() {
                            let c = parse_color_value(c_val).unwrap_or([0, 0, 0, 255]);
                            let default_ratio = if len <= 1 {
                                0u8
                            } else {
                                ((idx * 255) / (len - 1)).min(255) as u8
                            };
                            let ratio = pos_arr
                                .and_then(|p| p.get(idx))
                                .and_then(JsflValue::as_number)
                                .map(|n| n.round().clamp(0.0, 255.0) as u8)
                                .unwrap_or(default_ratio);
                            stops.push(crate::GradientStop { ratio, color: c });
                        }
                        if stops.is_empty() { None } else { Some(stops) }
                    } else {
                        None
                    };
                let new_fill = if style == "noFill" {
                    [0, 0, 0, 0]
                } else if let Some(stops) = &parsed_stops {
                    stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255])
                } else {
                    map.get("color")
                        .and_then(parse_color_value)
                        .unwrap_or([0, 0, 0, 255])
                };
                let el = self.resolve_element_mut(&eh, span)?;
                match &mut el.kind {
                    ElementKind::PaintedPath {
                        fill,
                        fill_gradient,
                        ..
                    } => {
                        *fill = new_fill;
                        if style == "linearGradient" || style == "radialGradient" {
                            let kind = if style == "radialGradient" {
                                crate::GradientKind::Radial
                            } else {
                                crate::GradientKind::Linear
                            };
                            if let Some(grad) = fill_gradient {
                                grad.kind = kind;
                                if let Some(stops) = parsed_stops {
                                    grad.stops = stops;
                                }
                            } else if let Some(stops) = parsed_stops {
                                *fill_gradient = Some(crate::GradientPaint {
                                    kind,
                                    matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                                    stops,
                                    ..Default::default()
                                });
                            }
                        } else if style == "solid" || style == "noFill" {
                            *fill_gradient = None;
                        }
                        Ok(JsflValue::Undefined)
                    }
                    ElementKind::Rectangle { fill, .. }
                    | ElementKind::Ellipse { fill, .. }
                    | ElementKind::Path { fill, .. } => {
                        *fill = new_fill;
                        Ok(JsflValue::Undefined)
                    }
                    _ => Err(self.err(span, "setCustomFill requires a Shape element.")),
                }
            }
            "getCustomStroke" => {
                let el = self.resolve_element(&eh, span)?;
                let mut obj = BTreeMap::new();
                if let ElementKind::PaintedPath {
                    stroke_gradient: Some(grad),
                    stroke_width,
                    ..
                } = &el.kind
                {
                    let style = match grad.kind {
                        crate::GradientKind::Linear => "linearGradient",
                        crate::GradientKind::Radial => "radialGradient",
                    };
                    obj.insert("style".to_owned(), JsflValue::String(style.to_owned()));
                    let colors: Vec<JsflValue> = grad
                        .stops
                        .iter()
                        .map(|s| JsflValue::String(format_rgba_hex(s.color)))
                        .collect();
                    let positions: Vec<JsflValue> = grad
                        .stops
                        .iter()
                        .map(|s| JsflValue::Number(f64::from(s.ratio)))
                        .collect();
                    let first_color = grad
                        .stops
                        .first()
                        .map(|s| s.color)
                        .unwrap_or([0, 0, 0, 255]);
                    obj.insert(
                        "color".to_owned(),
                        JsflValue::String(format_rgba_hex(first_color)),
                    );
                    obj.insert("thickness".to_owned(), JsflValue::Number(*stroke_width));
                    obj.insert("colorArray".to_owned(), JsflValue::Array(colors));
                    obj.insert("posArray".to_owned(), JsflValue::Array(positions));
                    return Ok(JsflValue::Object(obj));
                }
                let (stroke_color, width) = match &el.kind {
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
                    } => (*stroke, *stroke_width),
                    _ => ([0, 0, 0, 0], 0.0),
                };
                obj.insert(
                    "style".to_owned(),
                    JsflValue::String(
                        if width <= 0.0 || stroke_color[3] == 0 {
                            "noStroke"
                        } else {
                            "solid"
                        }
                        .to_owned(),
                    ),
                );
                obj.insert(
                    "color".to_owned(),
                    JsflValue::String(format_rgba_hex(stroke_color)),
                );
                obj.insert("thickness".to_owned(), JsflValue::Number(width));
                Ok(JsflValue::Object(obj))
            }
            "setCustomStroke" => {
                self.record_mutation(span)?;
                let Some(JsflValue::Object(map)) = args.first() else {
                    return Err(self.err(span, "setCustomStroke expects a Stroke object."));
                };
                let style = map
                    .get("style")
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "solid".to_owned());
                let thickness = map
                    .get("thickness")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(1.0)
                    .max(0.0);
                let parsed_stops: Option<Vec<crate::GradientStop>> =
                    if let Some(JsflValue::Array(color_arr)) = map.get("colorArray") {
                        let pos_arr = match map.get("posArray") {
                            Some(JsflValue::Array(p)) => Some(p.as_slice()),
                            _ => None,
                        };
                        let len = color_arr.len().max(1);
                        let mut stops = Vec::with_capacity(color_arr.len());
                        for (idx, c_val) in color_arr.iter().enumerate() {
                            let c = parse_color_value(c_val).unwrap_or([0, 0, 0, 255]);
                            let default_ratio = if len <= 1 {
                                0u8
                            } else {
                                ((idx * 255) / (len - 1)).min(255) as u8
                            };
                            let ratio = pos_arr
                                .and_then(|p| p.get(idx))
                                .and_then(JsflValue::as_number)
                                .map(|n| n.round().clamp(0.0, 255.0) as u8)
                                .unwrap_or(default_ratio);
                            stops.push(crate::GradientStop { ratio, color: c });
                        }
                        if stops.is_empty() { None } else { Some(stops) }
                    } else {
                        None
                    };
                let new_stroke = if style == "noStroke" || thickness <= 0.0 {
                    [0, 0, 0, 0]
                } else if let Some(stops) = &parsed_stops {
                    stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255])
                } else {
                    map.get("color")
                        .and_then(parse_color_value)
                        .unwrap_or([0, 0, 0, 255])
                };
                let el = self.resolve_element_mut(&eh, span)?;
                match &mut el.kind {
                    ElementKind::PaintedPath {
                        stroke,
                        stroke_gradient,
                        stroke_width,
                        ..
                    } => {
                        *stroke = new_stroke;
                        *stroke_width = if style == "noStroke" { 0.0 } else { thickness };
                        if style == "linearGradient" || style == "radialGradient" {
                            let kind = if style == "radialGradient" {
                                crate::GradientKind::Radial
                            } else {
                                crate::GradientKind::Linear
                            };
                            if let Some(grad) = stroke_gradient {
                                grad.kind = kind;
                                if let Some(stops) = parsed_stops {
                                    grad.stops = stops;
                                }
                            } else if let Some(stops) = parsed_stops {
                                *stroke_gradient = Some(crate::GradientPaint {
                                    kind,
                                    matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                                    stops,
                                    ..Default::default()
                                });
                            }
                        } else if style == "solid" || style == "noStroke" {
                            *stroke_gradient = None;
                        }
                        Ok(JsflValue::Undefined)
                    }
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
                    } => {
                        *stroke = new_stroke;
                        *stroke_width = if style == "noStroke" { 0.0 } else { thickness };
                        Ok(JsflValue::Undefined)
                    }
                    _ => Err(self.err(span, "setCustomStroke requires a Shape element.")),
                }
            }
            _ => Err(self.err(span, format!("Unsupported Element method '{method}'."))),
        }
    }
}
