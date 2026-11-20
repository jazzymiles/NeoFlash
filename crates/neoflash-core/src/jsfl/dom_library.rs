use super::*;

impl<'a> JsflVm<'a> {
    pub(super) fn collect_library_items(&self, doc: DocHandle) -> Vec<JsflValue> {
        let mut items = Vec::new();
        for folder in self.project.library_folder_paths() {
            items.push(JsflValue::Item(ItemHandle {
                doc,
                item_id: folder,
                is_folder: true,
            }));
        }
        let scene_ids: HashSet<Id> = self.project.scene_ids().into_iter().collect();
        for sym in &self.project.symbols {
            if scene_ids.contains(&sym.id) {
                continue;
            }
            items.push(JsflValue::Item(ItemHandle {
                doc,
                item_id: sym.id.clone(),
                is_folder: false,
            }));
        }
        items
    }

    pub(super) fn find_library_symbol_by_path(&self, name_path: &str) -> Option<Id> {
        let trimmed = name_path.trim().trim_matches('/');
        let scene_ids: HashSet<Id> = self.project.scene_ids().into_iter().collect();
        for sym in &self.project.symbols {
            if scene_ids.contains(&sym.id) {
                continue;
            }
            let full = if sym.folder.is_empty() {
                sym.name.clone()
            } else {
                format!("{}/{}", sym.folder, sym.name)
            };
            if full == trimmed || sym.name == trimmed || sym.id == trimmed {
                return Some(sym.id.clone());
            }
        }
        None
    }
}

impl<'a> JsflVm<'a> {
    pub(super) fn call_library_method(
        &mut self,
        doc: DocHandle,
        method: &str,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.ensure_doc(doc, span)?;
        match method {
            "itemExists" => {
                let path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let trimmed = path.trim().trim_matches('/');
                let exists = self.find_library_symbol_by_path(trimmed).is_some()
                    || self
                        .project
                        .library_folder_paths()
                        .iter()
                        .any(|f| f == trimmed);
                Ok(JsflValue::Bool(exists))
            }
            "findItemIndex" => {
                let path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let trimmed = path.trim().trim_matches('/');
                let items = self.collect_library_items(doc);
                let mut found: i64 = -1;
                for (i, item) in items.iter().enumerate() {
                    if let JsflValue::Item(ih) = item {
                        if ih.is_folder {
                            if ih.item_id == trimmed {
                                found = i as i64;
                                break;
                            }
                        } else if let Some(sym) = self.project.symbol(&ih.item_id) {
                            let full = if sym.folder.is_empty() {
                                sym.name.clone()
                            } else {
                                format!("{}/{}", sym.folder, sym.name)
                            };
                            if full == trimmed || sym.name == trimmed {
                                found = i as i64;
                                break;
                            }
                        }
                    }
                }
                Ok(JsflValue::Number(found as f64))
            }
            "newFolder" => {
                self.record_mutation(span)?;
                let folder_path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "untitled folder 1".to_owned());
                let trimmed = folder_path.trim().trim_matches('/').to_owned();
                if !valid_library_folder(&trimmed) || trimmed.is_empty() {
                    return Err(self.err(
                        span,
                        format!("Invalid Library folder path '{folder_path}'."),
                    ));
                }
                self.project
                    .add_library_folder(&trimmed)
                    .map_err(|e| self.err(span, e))?;
                self.state.selected_library_items = vec![trimmed];
                Ok(JsflValue::Bool(true))
            }
            "addNewItem" => {
                self.record_mutation(span)?;
                let type_str = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| "movie clip".to_owned())
                    .to_lowercase();
                let name_path = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .unwrap_or_else(|| format!("Symbol {}", self.project.symbols.len()));
                let trimmed = name_path.trim().trim_matches('/');
                if type_str == "folder" {
                    return self.call_library_method(
                        doc,
                        "newFolder",
                        &[JsflValue::String(trimmed.to_owned())],
                        span,
                    );
                }
                let kind = match type_str.as_str() {
                    "movie clip" | "movieclip" => SymbolKind::MovieClip,
                    "graphic" => SymbolKind::Graphic,
                    "button" => SymbolKind::Button,
                    _ => {
                        return Err(self.err(
                            span,
                            format!("Unsupported Library.addNewItem type '{type_str}'."),
                        ));
                    }
                };
                let (folder, leaf) = match trimmed.rsplit_once('/') {
                    Some((f, l)) => (f.to_owned(), l.to_owned()),
                    None => (String::new(), trimmed.to_owned()),
                };
                if leaf.is_empty() || leaf.chars().any(char::is_control) {
                    return Err(self.err(span, "Library item name cannot be empty."));
                }
                if !folder.is_empty() {
                    self.project
                        .add_library_folder(&folder)
                        .map_err(|e| self.err(span, e))?;
                }
                let sym_id = new_id("sym");
                self.project.symbols.push(Symbol {
                    id: sym_id.clone(),
                    name: leaf,
                    folder,
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
                self.state.selected_library_items = vec![sym_id];
                Ok(JsflValue::Bool(true))
            }
            "selectItem" => {
                self.record_mutation(span)?;
                let name_path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let replace = args.get(1).map(JsflValue::is_truthy).unwrap_or(true);
                let trimmed = name_path.trim().trim_matches('/');
                let resolved = self.find_library_symbol_by_path(trimmed).or_else(|| {
                    self.project
                        .library_folder_paths()
                        .into_iter()
                        .find(|f| f == trimmed)
                });
                let Some(item_id) = resolved else {
                    return Ok(JsflValue::Bool(false));
                };
                if replace {
                    self.state.selected_library_items = vec![item_id];
                } else if !self.state.selected_library_items.contains(&item_id) {
                    self.state.selected_library_items.push(item_id);
                }
                Ok(JsflValue::Bool(true))
            }
            "getSelectedItems" => {
                let folders: HashSet<String> =
                    self.project.library_folder_paths().into_iter().collect();
                let mut out = Vec::new();
                for id in &self.state.selected_library_items {
                    if folders.contains(id) {
                        out.push(JsflValue::Item(ItemHandle {
                            doc,
                            item_id: id.clone(),
                            is_folder: true,
                        }));
                    } else if self.project.symbol(id).is_some() {
                        out.push(JsflValue::Item(ItemHandle {
                            doc,
                            item_id: id.clone(),
                            is_folder: false,
                        }));
                    }
                }
                Ok(JsflValue::Array(out))
            }
            "renameItem" => {
                self.record_mutation(span)?;
                let new_name = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default();
                let leaf = new_name
                    .rsplit('/')
                    .next()
                    .unwrap_or(&new_name)
                    .trim()
                    .to_owned();
                if leaf.is_empty() || leaf.chars().any(char::is_control) {
                    return Err(self.err(span, "Library.renameItem requires a valid name."));
                }
                let target_id = if let Some(path_arg) = args.get(1) {
                    let p = path_arg.to_display_string();
                    self.find_library_symbol_by_path(&p)
                } else {
                    self.state
                        .selected_library_items
                        .first()
                        .cloned()
                        .or_else(|| {
                            let scenes: HashSet<Id> =
                                self.project.scene_ids().into_iter().collect();
                            self.project
                                .symbols
                                .iter()
                                .rev()
                                .find(|s| !scenes.contains(&s.id))
                                .map(|s| s.id.clone())
                        })
                };
                let Some(sym_id) = target_id else {
                    return Ok(JsflValue::Bool(false));
                };
                let sym = self.symbol_mut(&sym_id, span)?;
                sym.name = leaf;
                Ok(JsflValue::Bool(true))
            }
            "moveToFolder" => {
                self.record_mutation(span)?;
                let folder_path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .unwrap_or_default()
                    .trim()
                    .trim_matches('/')
                    .to_owned();
                let item_to_move = args
                    .get(1)
                    .map(JsflValue::to_display_string)
                    .filter(|s| !s.is_empty() && s != "undefined");
                let target_ids: Vec<Id> = if let Some(p) = item_to_move {
                    if let Some(id) = self.find_library_symbol_by_path(&p) {
                        vec![id]
                    } else {
                        return Ok(JsflValue::Bool(false));
                    }
                } else {
                    self.state.selected_library_items.clone()
                };
                if target_ids.is_empty() {
                    return Ok(JsflValue::Bool(false));
                }
                for sym_id in target_ids {
                    if self.project.symbol(&sym_id).is_some() {
                        self.project
                            .move_library_symbol(&sym_id, &folder_path)
                            .map_err(|e| self.err(span, e))?;
                    }
                }
                Ok(JsflValue::Bool(true))
            }
            "duplicateItem" => {
                self.record_mutation(span)?;
                let target_id = if let Some(arg) = args.first() {
                    let p = arg.to_display_string();
                    self.find_library_symbol_by_path(&p)
                } else {
                    self.state.selected_library_items.first().cloned()
                };
                let Some(sym_id) = target_id else {
                    return Ok(JsflValue::Bool(false));
                };
                let mut cloned = self.symbol_ref(&sym_id, span)?.clone();
                cloned.id = new_id("sym");
                cloned.name = format!("{} copy", cloned.name);
                cloned.linkage_id = None;
                cloned.class_name = None;
                for layer in &mut cloned.timeline.layers {
                    layer.id = new_id("layer");
                    for kf in &mut layer.keyframes {
                        for el in &mut kf.elements {
                            el.id = new_id("el");
                        }
                    }
                }
                let new_sym_id = cloned.id.clone();
                self.project.symbols.push(cloned);
                self.state.selected_library_items = vec![new_sym_id];
                Ok(JsflValue::Bool(true))
            }
            "deleteItem" => {
                self.record_mutation(span)?;
                let target_path = args
                    .first()
                    .map(JsflValue::to_display_string)
                    .filter(|s| !s.is_empty() && s != "undefined");
                if let Some(p) = &target_path {
                    let trimmed = p.trim().trim_matches('/');
                    if self
                        .project
                        .library_folder_paths()
                        .iter()
                        .any(|f| f == trimmed)
                    {
                        let prefix = format!("{trimmed}/");
                        self.project
                            .library_folders
                            .retain(|f| f != trimmed && !f.starts_with(&prefix));
                        let removed_ids: HashSet<Id> = self
                            .project
                            .symbols
                            .iter()
                            .filter(|s| s.folder == trimmed || s.folder.starts_with(&prefix))
                            .map(|s| s.id.clone())
                            .collect();
                        self.project
                            .symbols
                            .retain(|s| !removed_ids.contains(&s.id));
                        self.purge_deleted_symbol_instances(&removed_ids);
                        return Ok(JsflValue::Bool(true));
                    }
                }
                let target_id = match target_path {
                    Some(p) => self.find_library_symbol_by_path(&p),
                    None => self.state.selected_library_items.first().cloned(),
                };
                let Some(sym_id) = target_id else {
                    return Ok(JsflValue::Bool(false));
                };
                if self.project.is_scene(&sym_id) {
                    return Err(self.err(span, "Cannot delete a Scene symbol from Library."));
                }
                self.project.symbols.retain(|s| s.id != sym_id);
                let mut removed = HashSet::new();
                removed.insert(sym_id.clone());
                self.purge_deleted_symbol_instances(&removed);
                self.state.selected_library_items.retain(|id| id != &sym_id);
                if self.state.active_symbol == sym_id {
                    self.state.active_symbol = self.project.root.clone();
                }
                Ok(JsflValue::Bool(true))
            }
            "editItem" => {
                self.record_mutation(span)?;
                let Some(arg) = args.first() else {
                    self.state.active_symbol = self.project.root.clone();
                    self.state.current_layer = 0;
                    self.state.current_frame = 0;
                    self.state.selected_element_ids.clear();
                    return Ok(JsflValue::Bool(true));
                };
                let path = arg.to_display_string();
                let Some(sym_id) = self.find_library_symbol_by_path(&path) else {
                    return Ok(JsflValue::Bool(false));
                };
                self.state.active_symbol = sym_id;
                self.state.current_layer = 0;
                self.state.current_frame = 0;
                self.state.selected_element_ids.clear();
                Ok(JsflValue::Bool(true))
            }
            "addItemToDocument" => {
                let pos = args.first().cloned().unwrap_or(JsflValue::Undefined);
                let target_id = if let Some(arg) = args.get(1) {
                    let p = arg.to_display_string();
                    self.find_library_symbol_by_path(&p)
                } else {
                    self.state.selected_library_items.first().cloned()
                };
                let Some(sym_id) = target_id else {
                    return Err(
                        self.err(span, "Library.addItemToDocument: target symbol not found.")
                    );
                };
                self.place_symbol_on_stage(doc, &pos, &sym_id, span)
            }
            "batchRename" => self.call_jsfl_batch_rename(args, span),
            _ => Err(self.err(
                span,
                format!("Unsupported Library method 'library.{method}'."),
            )),
        }
    }

    pub(super) fn purge_deleted_symbol_instances(&mut self, removed_ids: &HashSet<Id>) {
        for sym in &mut self.project.symbols {
            for layer in &mut sym.timeline.layers {
                for kf in &mut layer.keyframes {
                    kf.elements.retain(|el| {
                        !matches!(&el.kind, ElementKind::Instance { symbol_id, .. } if removed_ids.contains(symbol_id))
                    });
                }
            }
        }
    }

    pub(super) fn place_symbol_on_stage(
        &mut self,
        doc: DocHandle,
        pos: &JsflValue,
        sym_id: &str,
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.record_mutation(span)?;
        if sym_id == self.state.active_symbol {
            return Err(self.err(span, "A symbol cannot be instantiated inside itself."));
        }
        let kind = self.symbol_ref(sym_id, span)?.kind;
        let (x, y) = if let JsflValue::Object(map) = pos {
            (
                map.get("x").and_then(JsflValue::as_number).unwrap_or(0.0),
                map.get("y").and_then(JsflValue::as_number).unwrap_or(0.0),
            )
        } else {
            (0.0, 0.0)
        };
        let inst_id = new_id("inst");
        let inst = Element {
            id: inst_id.clone(),
            name: String::new(),
            transform: Transform {
                x,
                y,
                ..Default::default()
            },
            kind: ElementKind::Instance {
                symbol_id: sym_id.to_owned(),
                playback: match kind {
                    SymbolKind::Graphic => InstancePlayback::Loop,
                    _ => InstancePlayback::Independent,
                },
            },
            ..Default::default()
        };
        let kf = self.ensure_active_keyframe_mut(span)?;
        let kf_frame = kf.frame;
        kf.elements.push(inst);
        self.state.selected_element_ids = vec![inst_id.clone()];
        let layer_id = self
            .symbol_ref(&self.state.active_symbol, span)?
            .timeline
            .layers[self.state.current_layer]
            .id
            .clone();
        Ok(JsflValue::Element(ElementHandle {
            doc,
            symbol_id: self.state.active_symbol.clone(),
            layer_id,
            keyframe: kf_frame,
            element_id: inst_id,
        }))
    }

    pub(super) fn call_jsfl_batch_rename(
        &mut self,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.record_mutation(span)?;
        let Some(JsflValue::Object(map)) = args.first() else {
            return Err(self.err(span, "batchRename expects an options Object."));
        };
        let mut opts = BatchRenameOptions::default();
        if let Some(scope_val) = map.get("scope") {
            opts.scope = match scope_val.to_display_string().to_lowercase().as_str() {
                "library" | "library_only" | "libraryitemsonly" => {
                    BatchRenameScope::LibraryItemsOnly
                }
                "instances" | "stage" | "stageinstancesonly" => {
                    BatchRenameScope::StageInstancesOnly
                }
                _ => BatchRenameScope::LibraryAndInstances,
            };
        }
        if let Some(f) = map.get("filter").or_else(|| map.get("nameFilter")) {
            let s = f.to_display_string();
            if !s.is_empty() && s != "undefined" {
                opts.name_filter = Some(s);
            }
        }
        if let Some(find) = map.get("find") {
            let s = find.to_display_string();
            if !s.is_empty() && s != "undefined" {
                opts.find = Some(s);
            }
        }
        if let Some(rep) = map.get("replace") {
            opts.replace = rep.to_display_string();
        }
        if let Some(pre) = map.get("prefix") {
            opts.prefix = pre.to_display_string();
        }
        if let Some(suf) = map.get("suffix") {
            opts.suffix = suf.to_display_string();
        }
        if let Some(seq) = map
            .get("sequenceStart")
            .or_else(|| map.get("sequence_start"))
            .and_then(JsflValue::as_number)
        {
            opts.sequence_start = Some(seq.max(0.0) as u32);
        }
        if let Some(digits) = map
            .get("sequenceDigits")
            .or_else(|| map.get("sequence_digits"))
            .and_then(JsflValue::as_number)
        {
            opts.sequence_digits = (digits.max(1.0) as usize).min(8);
        }
        if let Some(sync) = map.get("syncInstances") {
            opts.sync_instance_names_to_symbol = sync.is_truthy();
        }
        let report = batch_rename_library_and_instances(&mut self.project, &opts)
            .map_err(|d| self.err(span, d.message))?;
        let mut out = BTreeMap::new();
        out.insert(
            "renamedSymbols".to_owned(),
            JsflValue::Number(report.renamed_symbols.len() as f64),
        );
        out.insert(
            "renamedInstances".to_owned(),
            JsflValue::Number(report.renamed_instances.len() as f64),
        );
        Ok(JsflValue::Object(out))
    }

    pub(super) fn call_jsfl_change_color(
        &mut self,
        args: &[JsflValue],
        span: Span,
    ) -> Result<JsflValue, JsflDiagnostic> {
        self.record_mutation(span)?;
        let opts = if let Some(JsflValue::Object(map)) = args.first() {
            let from = map
                .get("from")
                .or_else(|| map.get("fromColor"))
                .and_then(parse_color_value)
                .ok_or_else(|| {
                    self.err(
                        span,
                        "doc.changeColor({ from, to }) requires a valid 'from' colour.",
                    )
                })?;
            let to = map
                .get("to")
                .or_else(|| map.get("toColor"))
                .and_then(parse_color_value)
                .ok_or_else(|| {
                    self.err(
                        span,
                        "doc.changeColor({ from, to }) requires a valid 'to' colour.",
                    )
                })?;
            let mut o = ChangeColorOptions::new(from, to);
            if let Some(rgb_only) = map.get("matchRgbOnly") {
                o.match_rgb_only = rgb_only.is_truthy();
            }
            if let Some(preserve_alpha) = map.get("preserveAlpha") {
                o.preserve_slot_alpha = preserve_alpha.is_truthy();
            }
            if let Some(swatches) = map.get("includeSwatches") {
                o.include_custom_swatches = swatches.is_truthy();
            }
            o
        } else {
            let from = args.first().and_then(parse_color_value).ok_or_else(|| {
                self.err(
                    span,
                    "doc.changeColor(fromColor, toColor, [options]) expects a '#RRGGBB' string or 0xRRGGBB integer for fromColor.",
                )
            })?;
            let to = args.get(1).and_then(parse_color_value).ok_or_else(|| {
                self.err(
                    span,
                    "doc.changeColor(fromColor, toColor, [options]) expects a '#RRGGBB' string or 0xRRGGBB integer for toColor.",
                )
            })?;
            let mut o = ChangeColorOptions::new(from, to);
            if let Some(JsflValue::Object(map)) = args.get(2) {
                if let Some(rgb_only) = map.get("matchRgbOnly") {
                    o.match_rgb_only = rgb_only.is_truthy();
                }
                if let Some(preserve_alpha) = map.get("preserveAlpha") {
                    o.preserve_slot_alpha = preserve_alpha.is_truthy();
                }
                if let Some(swatches) = map.get("includeSwatches") {
                    o.include_custom_swatches = swatches.is_truthy();
                }
            }
            o
        };
        let report = change_project_color(&mut self.project, &opts)
            .map_err(|d| self.err(span, d.message))?;
        let mut out = BTreeMap::new();
        out.insert(
            "totalReplaced".to_owned(),
            JsflValue::Number(report.total_replaced() as f64),
        );
        out.insert(
            "replacedFills".to_owned(),
            JsflValue::Number(report.replaced_fills as f64),
        );
        out.insert(
            "replacedStrokes".to_owned(),
            JsflValue::Number(report.replaced_strokes as f64),
        );
        out.insert(
            "replacedGradientStops".to_owned(),
            JsflValue::Number(report.replaced_gradient_stops as f64),
        );
        out.insert(
            "replacedTexts".to_owned(),
            JsflValue::Number(report.replaced_texts as f64),
        );
        out.insert(
            "replacedSwatches".to_owned(),
            JsflValue::Number(report.replaced_swatches as f64),
        );
        out.insert(
            "affectedElements".to_owned(),
            JsflValue::Number(report.affected_elements.len() as f64),
        );
        Ok(JsflValue::Object(out))
    }
}
