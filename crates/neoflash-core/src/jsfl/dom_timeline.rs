use super::*;

impl<'a> JsflVm<'a> {
    pub(super) fn collect_doc_selection(
        &self,
        doc: DocHandle,
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        let sym = self.symbol_ref(&self.state.active_symbol, span)?;
        let selected_set: HashSet<&str> = self
            .state
            .selected_element_ids
            .iter()
            .map(String::as_str)
            .collect();
        let mut out = Vec::new();
        for layer in &sym.timeline.layers {
            if let Some(kf) = layer.key_at(self.state.current_frame) {
                for el in &kf.elements {
                    if selected_set.contains(el.id.as_str()) {
                        out.push(JsflValue::Element(ElementHandle {
                            doc,
                            symbol_id: sym.id.clone(),
                            layer_id: layer.id.clone(),
                            keyframe: kf.frame,
                            element_id: el.id.clone(),
                        }));
                    }
                }
            }
        }
        Ok(JsflValue::Array(out))
    }
}

impl<'a> JsflVm<'a> {
    pub(super) fn ensure_active_keyframe_mut(
        &mut self,
        span: Span,
    ) -> Result<&mut Keyframe, JsflDiagnostic> {
        let active_sym = self.state.active_symbol.clone();
        let cur_layer = self.state.current_layer;
        let cur_frame = self.state.current_frame;
        let err_layer = self.err(span, "Active layer index is out of bounds.");
        let sym = self.symbol_mut(&active_sym, span)?;
        let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
        if let Some(pos) = layer.keyframes.iter().position(|k| k.frame == cur_frame) {
            return Ok(&mut layer.keyframes[pos]);
        }
        let Some(pos) = layer
            .keyframes
            .iter()
            .enumerate()
            .filter(|(_, k)| k.frame <= cur_frame)
            .max_by_key(|(_, k)| k.frame)
            .map(|(i, _)| i)
        else {
            layer.keyframes.push(Keyframe {
                frame: 0,
                ..Default::default()
            });
            return Ok(&mut layer.keyframes[0]);
        };
        Ok(&mut layer.keyframes[pos])
    }

    pub(super) fn call_doc_method(
        &mut self,
        doc: DocHandle,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.ensure_doc(doc, span)?;
        match method {
            "getTimeline" => Ok(JsflValue::Timeline(TimelineHandle {
                doc,
                symbol_id: self.state.active_symbol.clone(),
            })),
            "selectAll" => {
                self.record_mutation(span)?;
                let sym = self.symbol_ref(&self.state.active_symbol, span)?;
                let mut ids = Vec::new();
                for layer in &sym.timeline.layers {
                    if !layer.visible || layer.locked {
                        continue;
                    }
                    if let Some(kf) = layer.key_at(self.state.current_frame) {
                        for el in &kf.elements {
                            ids.push(el.id.clone());
                        }
                    }
                }
                self.state.selected_element_ids = ids;
                Ok(JsflValue::Undefined)
            }
            "selectNone" => {
                self.record_mutation(span)?;
                self.state.selected_element_ids.clear();
                Ok(JsflValue::Undefined)
            }
            "group" => {
                self.record_mutation(span)?;
                if self.state.selected_element_ids.is_empty() {
                    return Err(self.err(span, "Document.group() requires a non-empty selection."));
                }
                let members = self.state.selected_element_ids.clone();
                let active_sym = self.state.active_symbol.clone();
                let cur_layer = self.state.current_layer;
                let err_layer = self.err(span, "Active layer index is out of bounds.");
                let sym = self.symbol_mut(&active_sym, span)?;
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                layer.properties.groups.push(AuthoringGroup {
                    id: new_id("group"),
                    name: "Group".to_owned(),
                    members,
                });
                Ok(JsflValue::Undefined)
            }
            "unGroup" => {
                self.record_mutation(span)?;
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let active_sym = self.state.active_symbol.clone();
                let cur_layer = self.state.current_layer;
                let err_layer = self.err(span, "Active layer index is out of bounds.");
                let sym = self.symbol_mut(&active_sym, span)?;
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                layer
                    .properties
                    .groups
                    .retain(|g| !g.members.iter().any(|m| selected.contains(m)));
                Ok(JsflValue::Undefined)
            }
            "breakApart" => {
                self.record_mutation(span)?;
                if self.state.selected_element_ids.is_empty() {
                    return Err(self.err(
                        span,
                        "Document.breakApart() requires a non-empty selection.",
                    ));
                }
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let sym_map: HashMap<Id, Vec<Element>> = self
                    .project
                    .symbols
                    .iter()
                    .map(|s| {
                        let first_els: Vec<Element> = s
                            .timeline
                            .layers
                            .iter()
                            .filter_map(|l| l.key_at(0))
                            .flat_map(|k| k.elements.iter().cloned())
                            .collect();
                        (s.id.clone(), first_els)
                    })
                    .collect();

                let kf = self.ensure_active_keyframe_mut(span)?;
                let mut new_elements = Vec::new();
                let mut new_selection = Vec::new();
                for el in kf.elements.drain(..) {
                    if !selected.contains(&el.id) {
                        new_elements.push(el);
                        continue;
                    }
                    match &el.kind {
                        ElementKind::Instance { symbol_id, .. } => {
                            if let Some(children) = sym_map.get(symbol_id)
                                && !children.is_empty()
                            {
                                for mut child in children.clone() {
                                    child.id = new_id("el");
                                    child.transform.x += el.transform.x;
                                    child.transform.y += el.transform.y;
                                    child.transform.scale_x *= el.transform.scale_x;
                                    child.transform.scale_y *= el.transform.scale_y;
                                    child.transform.rotation += el.transform.rotation;
                                    new_selection.push(child.id.clone());
                                    new_elements.push(child);
                                }
                            } else {
                                let mut path_el = el.clone();
                                path_el.kind = ElementKind::Path {
                                    commands: crate::geometry::rectangle_path_commands(
                                        40.0,
                                        40.0,
                                        Default::default(),
                                    ),
                                    fill_rule: FillRule::NonZero,
                                    fill: [128, 128, 128, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                };
                                path_el.merge = true;
                                new_selection.push(path_el.id.clone());
                                new_elements.push(path_el);
                            }
                        }
                        ElementKind::Rectangle {
                            width,
                            height,
                            fill,
                            stroke,
                            stroke_width,
                        } => {
                            let cmds = crate::geometry::rectangle_path_commands(
                                *width,
                                *height,
                                el.rectangle_options(),
                            );
                            let mut broken = el.clone();
                            broken.kind = ElementKind::Path {
                                commands: cmds,
                                fill_rule: FillRule::NonZero,
                                fill: *fill,
                                stroke: *stroke,
                                stroke_width: *stroke_width,
                            };
                            broken.merge = true;
                            new_selection.push(broken.id.clone());
                            new_elements.push(broken);
                        }
                        ElementKind::Ellipse {
                            width,
                            height,
                            fill,
                            stroke,
                            stroke_width,
                        } => {
                            let cmds = crate::geometry::oval_path_commands(
                                *width,
                                *height,
                                el.oval_options(),
                            );
                            let mut broken = el.clone();
                            broken.kind = ElementKind::Path {
                                commands: cmds,
                                fill_rule: FillRule::NonZero,
                                fill: *fill,
                                stroke: *stroke,
                                stroke_width: *stroke_width,
                            };
                            broken.merge = true;
                            new_selection.push(broken.id.clone());
                            new_elements.push(broken);
                        }
                        _ => {
                            let mut broken = el;
                            broken.merge = true;
                            new_selection.push(broken.id.clone());
                            new_elements.push(broken);
                        }
                    }
                }
                kf.elements = new_elements;
                self.state.selected_element_ids = new_selection;
                Ok(JsflValue::Undefined)
            }
            "convertToSymbol" => {
                self.record_mutation(span)?;
                if self.state.selected_element_ids.is_empty() {
                    return Err(self.err(
                        span,
                        "Document.convertToSymbol() requires a non-empty selection.",
                    ));
                }
                let type_str = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "movie clip".to_owned())
                    .to_lowercase();
                let kind = match type_str.as_str() {
                    "movie clip" | "movieclip" => SymbolKind::MovieClip,
                    "graphic" => SymbolKind::Graphic,
                    "button" => SymbolKind::Button,
                    _ => {
                        return Err(
                            self.err(span, format!("Invalid convertToSymbol type '{type_str}'."))
                        );
                    }
                };
                let raw_name = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let sym_name = if raw_name.trim().is_empty() {
                    format!("Symbol {}", self.project.symbols.len())
                } else {
                    raw_name.trim().to_owned()
                };
                let reg_point = args
                    .get(2)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "center".to_owned())
                    .to_lowercase();

                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let kf = self.ensure_active_keyframe_mut(span)?;
                let mut extracted = Vec::new();
                let mut remaining = Vec::new();
                let mut insert_idx = None;
                for el in kf.elements.drain(..) {
                    if selected.contains(&el.id) {
                        if insert_idx.is_none() {
                            insert_idx = Some(remaining.len());
                        }
                        extracted.push(el);
                    } else {
                        remaining.push(el);
                    }
                }
                if extracted.is_empty() {
                    kf.elements = remaining;
                    return Err(self.err(
                        span,
                        "Selected elements were not found on the active keyframe.",
                    ));
                }

                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                for el in &extracted {
                    let (w, h) = Self::element_local_size(el);
                    min_x = min_x.min(el.transform.x);
                    min_y = min_y.min(el.transform.y);
                    max_x = max_x.max(el.transform.x + w * el.transform.scale_x.abs());
                    max_y = max_y.max(el.transform.y + h * el.transform.scale_y.abs());
                }
                let (origin_x, origin_y) = match reg_point.as_str() {
                    "top left" => (min_x, min_y),
                    "bottom right" => (max_x, max_y),
                    _ => (0.5 * (min_x + max_x), 0.5 * (min_y + max_y)),
                };
                for el in &mut extracted {
                    el.transform.x -= origin_x;
                    el.transform.y -= origin_y;
                }

                let sym_id = new_id("sym");
                let inst_id = new_id("inst");
                let inst = Element {
                    id: inst_id.clone(),
                    name: String::new(),
                    transform: Transform {
                        x: origin_x,
                        y: origin_y,
                        ..Default::default()
                    },
                    kind: ElementKind::Instance {
                        symbol_id: sym_id.clone(),
                        playback: match kind {
                            SymbolKind::Graphic => InstancePlayback::Loop,
                            _ => InstancePlayback::Independent,
                        },
                    },
                    ..Default::default()
                };
                let idx = insert_idx.unwrap_or(remaining.len());
                remaining.insert(idx, inst);
                kf.elements = remaining;

                self.project.symbols.push(Symbol {
                    id: sym_id.clone(),
                    name: sym_name,
                    folder: String::new(),
                    kind,
                    timeline: crate::Timeline {
                        frame_count: 1,
                        layers: vec![Layer {
                            id: new_id("layer"),
                            name: "Layer 1".to_owned(),
                            visible: true,
                            locked: false,
                            keyframes: vec![Keyframe {
                                frame: 0,
                                elements: extracted,
                                ..Default::default()
                            }],
                            properties: Default::default(),
                        }],
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                    class_name: None,
                    linkage_id: None,
                });
                self.state.selected_element_ids = vec![inst_id];
                Ok(JsflValue::Item(ItemHandle {
                    doc,
                    item_id: sym_id,
                    is_folder: false,
                    is_resource: false,
                }))
            }
            "moveSelectionBy" => {
                self.record_mutation(span)?;
                let Some(JsflValue::Object(map)) = args.first() else {
                    return Err(self.err(
                        span,
                        "Document.moveSelectionBy expects a {x, y} distance object.",
                    ));
                };
                let dx = map.get("x").and_then(JsflValue::as_number).unwrap_or(0.0);
                let dy = map.get("y").and_then(JsflValue::as_number).unwrap_or(0.0);
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let kf = self.ensure_active_keyframe_mut(span)?;
                for el in &mut kf.elements {
                    if selected.contains(&el.id) {
                        el.transform.x += dx;
                        el.transform.y += dy;
                    }
                }
                Ok(JsflValue::Undefined)
            }
            "scaleSelection" => {
                self.record_mutation(span)?;
                let sx = args.first().and_then(JsflValue::as_number).unwrap_or(1.0);
                let sy = args.get(1).and_then(JsflValue::as_number).unwrap_or(1.0);
                if !sx.is_finite() || !sy.is_finite() || sx.abs() < 1e-6 || sy.abs() < 1e-6 {
                    return Err(self.err(
                        span,
                        "Document.scaleSelection requires finite non-zero scale factors.",
                    ));
                }
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let kf = self.ensure_active_keyframe_mut(span)?;
                for el in &mut kf.elements {
                    if selected.contains(&el.id) {
                        el.transform.scale_x *= sx;
                        el.transform.scale_y *= sy;
                    }
                }
                Ok(JsflValue::Undefined)
            }
            "rotateSelection" => {
                self.record_mutation(span)?;
                let angle = args.first().and_then(JsflValue::as_number).unwrap_or(0.0);
                if !angle.is_finite() {
                    return Err(self.err(span, "Document.rotateSelection requires a finite angle."));
                }
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let kf = self.ensure_active_keyframe_mut(span)?;
                for el in &mut kf.elements {
                    if selected.contains(&el.id) {
                        el.transform.rotation += angle;
                    }
                }
                Ok(JsflValue::Undefined)
            }
            "deleteSelection" => {
                self.record_mutation(span)?;
                let selected: HashSet<Id> =
                    self.state.selected_element_ids.iter().cloned().collect();
                let kf = self.ensure_active_keyframe_mut(span)?;
                kf.elements.retain(|el| !selected.contains(&el.id));
                self.state.selected_element_ids.clear();
                Ok(JsflValue::Undefined)
            }
            "addNewRectangle" | "addNewOval" => {
                self.record_mutation(span)?;
                let Some(JsflValue::Object(rect)) = args.first() else {
                    return Err(self.err(
                        span,
                        format!(
                            "Document.{method} requires a {{left, top, right, bottom}} rectangle."
                        ),
                    ));
                };
                let left = rect
                    .get("left")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0);
                let top = rect
                    .get("top")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0);
                let right = rect
                    .get("right")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(100.0);
                let bottom = rect
                    .get("bottom")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(100.0);
                let width = (right - left).abs().max(1.0);
                let height = (bottom - top).abs().max(1.0);
                let id = new_id("shape");
                let mut el = Element {
                    id: id.clone(),
                    name: String::new(),
                    transform: Transform {
                        x: left.min(right),
                        y: top.min(bottom),
                        ..Default::default()
                    },
                    kind: if method == "addNewRectangle" {
                        ElementKind::Rectangle {
                            width,
                            height,
                            fill: [74, 144, 226, 255],
                            stroke: [20, 20, 20, 255],
                            stroke_width: 1.0,
                        }
                    } else {
                        ElementKind::Ellipse {
                            width,
                            height,
                            fill: [74, 144, 226, 255],
                            stroke: [20, 20, 20, 255],
                            stroke_width: 1.0,
                        }
                    },
                    ..Default::default()
                };
                if method == "addNewRectangle"
                    && let Some(r) = args.get(1).and_then(JsflValue::as_number)
                    && r > 0.0
                {
                    el.set_rectangle_options(crate::RectangleOptions {
                        corner_radii: [r; 4],
                        corners_locked: true,
                    });
                }
                let kf = self.ensure_active_keyframe_mut(span)?;
                kf.elements.push(el);
                self.state.selected_element_ids = vec![id];
                Ok(JsflValue::Undefined)
            }
            "addNewText" => {
                self.record_mutation(span)?;
                let Some(JsflValue::Object(rect)) = args.first() else {
                    return Err(self.err(
                        span,
                        "Document.addNewText requires a {left, top, right, bottom} rectangle.",
                    ));
                };
                let left = rect
                    .get("left")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0);
                let top = rect
                    .get("top")
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0);
                let text_str = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "Text".to_owned());
                let id = new_id("text");
                let el = Element {
                    id: id.clone(),
                    name: String::new(),
                    transform: Transform {
                        x: left,
                        y: top,
                        ..Default::default()
                    },
                    kind: ElementKind::Text {
                        text: text_str,
                        font_family: "sans-serif".to_owned(),
                        font_size: 16.0,
                        color: [0, 0, 0, 255],
                    },
                    ..Default::default()
                };
                let kf = self.ensure_active_keyframe_mut(span)?;
                kf.elements.push(el);
                self.state.selected_element_ids = vec![id];
                Ok(JsflValue::Undefined)
            }
            "addItem" => {
                let pos = args.first().cloned().unwrap_or(JsflValue::Undefined);
                let Some(JsflValue::Item(ih)) = args.get(1) else {
                    return Err(self.err(span, "Document.addItem expects (position, Item)."));
                };
                if ih.is_folder {
                    return Err(self.err(span, "Cannot place a folder Item onto the stage."));
                }
                let sym_id = ih.item_id.clone();
                self.place_symbol_on_stage(doc, &pos, &sym_id, span)
            }
            "exitEditMode" => {
                self.record_mutation(span)?;
                self.state.active_symbol = self.project.root.clone();
                self.state.current_layer = 0;
                self.state.current_frame = 0;
                self.state.selected_element_ids.clear();
                Ok(JsflValue::Undefined)
            }
            "batchRename" => self.call_jsfl_batch_rename(args, span),
            "getProjectColors" => {
                let entries = collect_project_colors(&self.project);
                let arr: Vec<JsflValue> = entries
                    .into_iter()
                    .map(|entry| {
                        let mut m = BTreeMap::new();
                        m.insert(
                            "color".to_owned(),
                            JsflValue::String(format_rgba_hex(entry.color)),
                        );
                        m.insert(
                            "fillCount".to_owned(),
                            JsflValue::Number(entry.fill_count as f64),
                        );
                        m.insert(
                            "strokeCount".to_owned(),
                            JsflValue::Number(entry.stroke_count as f64),
                        );
                        m.insert(
                            "gradientCount".to_owned(),
                            JsflValue::Number(entry.gradient_count as f64),
                        );
                        m.insert(
                            "textCount".to_owned(),
                            JsflValue::Number(entry.text_count as f64),
                        );
                        m.insert(
                            "inSwatches".to_owned(),
                            JsflValue::Bool(entry.in_custom_swatches),
                        );
                        m.insert(
                            "totalUses".to_owned(),
                            JsflValue::Number(entry.total_uses() as f64),
                        );
                        JsflValue::Object(m)
                    })
                    .collect();
                Ok(JsflValue::Array(arr))
            }
            "changeColor" | "replaceColor" => self.call_jsfl_change_color(args, span),
            _ => Err(self.err(span, format!("Unsupported Document method 'doc.{method}'."))),
        }
    }
}

impl<'a> JsflVm<'a> {
    /// JSFL timeline methods are allowed to operate on hidden or locked rows. Reuse the shared
    /// atomic frame editor while temporarily bypassing its interactive-UI row guards, then restore
    /// every authored visibility/lock bit on both success and failure.
    pub(super) fn apply_jsfl_timeline_edit(
        &mut self,
        symbol_id: &str,
        edit: crate::TimelineEdit,
        span: Span,
    ) -> Result<(), JsflDiagnostic> {
        let authored_flags = self
            .symbol_ref(symbol_id, span)?
            .timeline
            .layers
            .iter()
            .map(|layer| (layer.id.clone(), layer.visible, layer.locked))
            .collect::<Vec<_>>();
        if let Some(symbol) = self.project.symbol_mut(symbol_id) {
            for layer in &mut symbol.timeline.layers {
                layer.visible = true;
                layer.locked = false;
            }
        }

        let result = crate::edit_frames(&mut self.project, symbol_id, edit);
        if let Some(symbol) = self.project.symbol_mut(symbol_id) {
            for layer in &mut symbol.timeline.layers {
                if let Some((_, visible, locked)) =
                    authored_flags.iter().find(|(id, _, _)| id == &layer.id)
                {
                    layer.visible = *visible;
                    layer.locked = *locked;
                }
            }
        }
        result.map_err(|error| self.err(span, error))
    }

    pub(super) fn call_timeline_method(
        &mut self,
        tl: TimelineHandle,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.ensure_doc(tl.doc, span)?;
        match method {
            "getSelectedLayers" => {
                let arr = self
                    .state
                    .selected_layers
                    .iter()
                    .map(|&i| JsflValue::Number(i as f64))
                    .collect();
                Ok(JsflValue::Array(arr))
            }
            "setSelectedLayers" => {
                self.record_mutation(span)?;
                let idx = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0)
                    .max(0.0) as usize;
                let replace = args.get(1).map(JsflValue::is_truthy).unwrap_or(true);
                let sym = self.symbol_ref(&tl.symbol_id, span)?;
                if idx >= sym.timeline.layers.len() {
                    return Err(self.err(span, format!("Layer index {idx} out of bounds.")));
                }
                self.state.current_layer = idx;
                if replace {
                    self.state.selected_layers = vec![idx];
                } else if !self.state.selected_layers.contains(&idx) {
                    self.state.selected_layers.push(idx);
                }
                Ok(JsflValue::Undefined)
            }
            "getSelectedFrames" => {
                let mut flat = Vec::new();
                for &(layer_idx, start_f, end_f) in &self.state.selected_frames {
                    flat.push(JsflValue::Number(layer_idx as f64));
                    flat.push(JsflValue::Number(start_f as f64));
                    flat.push(JsflValue::Number(end_f as f64));
                }
                Ok(JsflValue::Array(flat))
            }
            "setSelectedFrames" => {
                self.record_mutation(span)?;
                if let Some(JsflValue::Array(arr)) = args.first() {
                    let mut spans = Vec::new();
                    for chunk in arr.chunks(3) {
                        if chunk.len() == 3 {
                            let l = chunk[0].as_number().unwrap_or(0.0).max(0.0) as usize;
                            let s = chunk[1].as_number().unwrap_or(0.0).max(0.0) as u32;
                            let e = chunk[2].as_number().unwrap_or((s + 1) as f64).max(0.0) as u32;
                            spans.push((l, s, e.max(s + 1)));
                        }
                    }
                    if let Some(&(l, s, _)) = spans.first() {
                        self.state.current_layer = l;
                        self.state.current_frame = s;
                    }
                    self.state.selected_frames = spans;
                    return Ok(JsflValue::Undefined);
                }
                let start_f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0)
                    .max(0.0) as u32;
                let end_f = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .unwrap_or((start_f + 1) as f64)
                    .max((start_f + 1) as f64) as u32;
                let replace = args.get(2).map(JsflValue::is_truthy).unwrap_or(true);
                let l = self.state.current_layer;
                self.state.current_frame = start_f;
                if replace {
                    self.state.selected_frames = vec![(l, start_f, end_f)];
                } else {
                    self.state.selected_frames.push((l, start_f, end_f));
                }
                Ok(JsflValue::Undefined)
            }
            "addNewLayer" => {
                self.record_mutation(span)?;
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                let name = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .filter(|s| !s.is_empty() && s != "undefined")
                    .unwrap_or_else(|| format!("Layer {}", sym.timeline.layers.len() + 1));
                let layer_type = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "normal".to_owned())
                    .to_lowercase();
                let role = match layer_type.as_str() {
                    "guide" => LayerRole::Guide,
                    "mask" => LayerRole::Mask,
                    "folder" => LayerRole::Folder,
                    _ => LayerRole::Normal,
                };
                let mut new_layer = Layer {
                    id: new_id("layer"),
                    name,
                    visible: true,
                    locked: false,
                    keyframes: if role == LayerRole::Folder {
                        Vec::new()
                    } else {
                        vec![Keyframe {
                            frame: 0,
                            ..Default::default()
                        }]
                    },
                    properties: Default::default(),
                };
                new_layer.properties.role = role;
                sym.timeline.layers.push(new_layer);
                let new_idx = sym.timeline.layers.len() - 1;
                self.state.current_layer = new_idx;
                self.state.selected_layers = vec![new_idx];
                Ok(JsflValue::Number(new_idx as f64))
            }
            "deleteLayer" => {
                self.record_mutation(span)?;
                let idx = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as usize)
                    .unwrap_or(self.state.current_layer);
                let err_min = self.err(span, "Cannot delete the last remaining layer.");
                let err_oob = self.err(span, format!("Layer index {idx} out of bounds."));
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                if sym.timeline.layers.len() <= 1 {
                    return Err(err_min);
                }
                if idx >= sym.timeline.layers.len() {
                    return Err(err_oob);
                }
                sym.timeline.layers.remove(idx);
                let max_idx = sym.timeline.layers.len() - 1;
                self.state.current_layer = self.state.current_layer.min(max_idx);
                self.state.selected_layers = vec![self.state.current_layer];
                self.project.prune_motion_guides();
                Ok(JsflValue::Undefined)
            }
            "reorderLayer" => {
                self.record_mutation(span)?;
                let from = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0)
                    .max(0.0) as usize;
                let to = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .unwrap_or(0.0)
                    .max(0.0) as usize;
                let add_before = args.get(2).map(JsflValue::is_truthy).unwrap_or(true);
                let err_oob = self.err(span, "reorderLayer index out of bounds.");
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                let len = sym.timeline.layers.len();
                if from >= len || to >= len {
                    return Err(err_oob);
                }
                if from != to {
                    let layer = sym.timeline.layers.remove(from);
                    let mut target = if from < to { to - 1 } else { to };
                    if !add_before {
                        target = (target + 1).min(sym.timeline.layers.len());
                    }
                    sym.timeline.layers.insert(target, layer);
                    self.state.current_layer = target;
                }
                Ok(JsflValue::Undefined)
            }
            "insertFrames" => {
                self.record_mutation(span)?;
                let num = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .unwrap_or(1.0)
                    .max(1.0) as u32;
                let all_layers = args.get(1).map(JsflValue::is_truthy).unwrap_or(false);
                let at_frame = args
                    .get(2)
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(self.state.current_frame);
                let cur_layer = self.state.current_layer;
                let layer_ids = self
                    .symbol_ref(&tl.symbol_id, span)?
                    .timeline
                    .layers
                    .iter()
                    .enumerate()
                    .filter(|(index, layer)| {
                        (all_layers || *index == cur_layer)
                            && layer.properties.role != LayerRole::Folder
                    })
                    .map(|(_, layer)| layer.id.clone())
                    .collect::<Vec<_>>();
                if layer_ids.is_empty() {
                    let sym = self.symbol_mut(&tl.symbol_id, span)?;
                    sym.timeline.frame_count = sym.timeline.frame_count.saturating_add(num);
                } else {
                    self.apply_jsfl_timeline_edit(
                        &tl.symbol_id,
                        crate::TimelineEdit::Insert {
                            layers: layer_ids,
                            at: at_frame,
                            count: num,
                            include_scripts: true,
                        },
                        span,
                    )?;
                }
                Ok(JsflValue::Undefined)
            }
            "removeFrames" => {
                self.record_mutation(span)?;
                let start_f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(self.state.current_frame);
                let end_f = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max((start_f + 1) as f64) as u32)
                    .unwrap_or(start_f + 1);
                let cur_layer = self.state.current_layer;
                let (frame_count, layer_id) = {
                    let sym = self.symbol_ref(&tl.symbol_id, span)?;
                    (
                        sym.timeline.frame_count,
                        sym.timeline.layers.get(cur_layer).and_then(|layer| {
                            (layer.properties.role != LayerRole::Folder).then(|| layer.id.clone())
                        }),
                    )
                };
                if start_f < frame_count
                    && let Some(layer_id) = layer_id
                {
                    let bounded_end = end_f.min(frame_count);
                    self.apply_jsfl_timeline_edit(
                        &tl.symbol_id,
                        crate::TimelineEdit::Delete {
                            range: crate::FrameRange {
                                layers: vec![layer_id],
                                start: start_f,
                                end: bounded_end - 1,
                            },
                            include_scripts: true,
                        },
                        span,
                    )?;
                }
                let new_fc = self.symbol_ref(&tl.symbol_id, span)?.timeline.frame_count;
                self.state.current_frame = self.state.current_frame.min(new_fc - 1);
                Ok(JsflValue::Undefined)
            }
            "insertKeyframe" | "insertBlankKeyframe" => {
                self.record_mutation(span)?;
                let f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(self.state.current_frame);
                let blank = method == "insertBlankKeyframe";
                let cur_layer = self.state.current_layer;
                let fps = self.project.fps as f64;
                let err_layer = self.err(span, "Current layer out of bounds.");
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                if f >= sym.timeline.frame_count {
                    sym.timeline.frame_count = f + 1;
                }
                let sampled_elements = if blank {
                    Vec::new()
                } else {
                    let layer_ref = sym
                        .timeline
                        .layers
                        .get(cur_layer)
                        .ok_or_else(|| err_layer.clone())?;
                    crate::evaluate::sample_layer_in_at_fps(&sym.timeline, layer_ref, f, fps)
                };
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                if let Some(existing) = layer.keyframes.iter_mut().find(|k| k.frame == f) {
                    if blank {
                        existing.elements.clear();
                    }
                } else {
                    layer.keyframes.push(Keyframe {
                        frame: f,
                        elements: sampled_elements,
                        ..Default::default()
                    });
                    layer.keyframes.sort_by_key(|k| k.frame);
                }
                self.state.current_frame = f;
                Ok(JsflValue::Undefined)
            }
            "clearKeyframes" => {
                self.record_mutation(span)?;
                let start_f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(self.state.current_frame);
                let end_f = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max((start_f + 1) as f64) as u32)
                    .unwrap_or(start_f + 1);
                let cur_layer = self.state.current_layer;
                let err_layer = self.err(span, "Current layer out of bounds.");
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                layer
                    .keyframes
                    .retain(|k| k.frame == 0 || k.frame < start_f || k.frame >= end_f);
                layer.normalize_motion_keys();
                Ok(JsflValue::Undefined)
            }
            "createMotionTween" => {
                self.record_mutation(span)?;
                let start_f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(self.state.current_frame);
                let end_f = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max((start_f + 1) as f64) as u32)
                    .unwrap_or(start_f + 1);
                let cur_layer = self.state.current_layer;
                let err_layer = self.err(span, "Current layer out of bounds.");
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                let target_start = layer.key_at(start_f).map(|k| k.frame).unwrap_or(0);
                for kf in &mut layer.keyframes {
                    if kf.frame >= target_start && kf.frame < end_f {
                        kf.tween = Some(Easing::Linear);
                        kf.tween_kind = TweenKind::Classic;
                    }
                }
                layer.normalize_motion_keys();
                Ok(JsflValue::Undefined)
            }
            "reverseFrames" => {
                self.record_mutation(span)?;
                let start_f = args
                    .first()
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max(0.0) as u32)
                    .unwrap_or(0);
                let sym_fc = self.symbol_ref(&tl.symbol_id, span)?.timeline.frame_count;
                let end_f = args
                    .get(1)
                    .and_then(JsflValue::as_number)
                    .map(|n| n.max((start_f + 1) as f64) as u32)
                    .unwrap_or(sym_fc);
                if end_f <= start_f + 1 {
                    return Ok(JsflValue::Undefined);
                }
                let last_inclusive = end_f - 1;
                let cur_layer = self.state.current_layer;
                let err_layer = self.err(span, "Current layer out of bounds.");
                let sym = self.symbol_mut(&tl.symbol_id, span)?;
                let layer = sym.timeline.layers.get_mut(cur_layer).ok_or(err_layer)?;
                for kf in &mut layer.keyframes {
                    if kf.frame >= start_f && kf.frame <= last_inclusive {
                        kf.frame = start_f + (last_inclusive - kf.frame);
                    }
                }
                layer.keyframes.sort_by_key(|k| k.frame);
                if let Some(first) = layer.keyframes.first_mut()
                    && first.frame != 0
                {
                    first.frame = 0;
                }
                layer.normalize_motion_keys();
                Ok(JsflValue::Undefined)
            }
            _ => Err(self.err(
                span,
                format!("Unsupported Timeline method 'timeline.{method}'."),
            )),
        }
    }
}
