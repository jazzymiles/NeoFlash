use super::*;

impl Editor {
    pub(crate) fn can_break_apart_selection(&self) -> bool {
        let sampled = self.sampled_targets();
        if sampled.is_empty()
            || sampled
                .iter()
                .any(|(target, _)| !self.target_editable(target))
        {
            return false;
        }
        if self.can_ungroup_selection() {
            return true;
        }
        sampled.iter().any(|(_, el)| match &el.kind {
            ElementKind::Instance { symbol_id, .. } => self.project.symbol(symbol_id).is_some(),
            ElementKind::Bitmap { .. }
            | ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. } => true,
            ElementKind::Text { text, .. } | ElementKind::TextBox { text, .. } => {
                text.chars().any(|ch| !ch.is_whitespace())
            }
            ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => !el.merge,
            ElementKind::Video { .. } => false,
        })
    }

    pub(crate) fn break_apart_selection(&mut self) {
        self.finish_edits();
        let selected = self.sampled_targets();
        if selected.is_empty() {
            return;
        }
        let grouped_members: HashSet<SelectionTarget> = self
            .symbol()
            .timeline
            .layers
            .iter()
            .flat_map(|layer| {
                let layer_id = layer.id.clone();
                layer
                    .properties
                    .groups
                    .iter()
                    .filter(|group| !group.members.is_empty())
                    .flat_map(move |group| {
                        let lid = layer_id.clone();
                        group.members.iter().map(move |elem_id| SelectionTarget {
                            layer: lid.clone(),
                            element: elem_id.clone(),
                        })
                    })
            })
            .collect();
        let mut unpack_group_targets = HashSet::new();
        let mut unchanged_targets = HashSet::new();
        let mut did_break_any = false;
        let mut replacements = Vec::new();
        let mut unpacked_groups: Vec<(Id, AuthoringGroup)> = Vec::new();
        let mut unpacked_bitmap_fills: Vec<(Id, AuthoredBitmapFill)> = Vec::new();
        let mut expanded_instance = false;
        let mut uses_skew = false;
        let mut empty_text_skipped = false;
        for (target, instance) in selected {
            if !self.target_editable(&target) {
                self.message("This layer is locked or hidden.");
                return;
            }
            if grouped_members.contains(&target) {
                unpack_group_targets.insert(target.clone());
                unchanged_targets.insert(target.clone());
                replacements.push((target, vec![instance]));
                did_break_any = true;
                continue;
            }
            match &instance.kind {
                ElementKind::Rectangle {
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
                } => {
                    let (commands, fill_rule) = instance.to_path_commands().unwrap_or_else(|| {
                        (
                            rectangle_path_commands(0.0, 0.0, RectangleOptions::default()),
                            FillRule::NonZero,
                        )
                    });
                    let effective_fill = if matches!(instance.kind, ElementKind::Ellipse { .. })
                        && !instance.oval_options().close_path
                    {
                        [0, 0, 0, 0]
                    } else {
                        *fill
                    };
                    let mut broken = instance.clone();
                    broken.transform.primitive = None;
                    broken.merge = true;
                    broken.kind = ElementKind::Path {
                        commands,
                        fill_rule,
                        fill: effective_fill,
                        stroke: *stroke,
                        stroke_width: *stroke_width,
                    };
                    replacements.push((target, vec![broken]));
                    did_break_any = true;
                }
                ElementKind::Instance { symbol_id, .. } => {
                    let Some(symbol) = self.project.symbol(symbol_id).cloned() else {
                        return;
                    };
                    let child_frame = evaluate_instance_pose(
                        &self.project,
                        self.symbol_id(),
                        self.frame,
                        &target.layer,
                        &target.element,
                    )
                    .ok()
                    .map(|pose| pose.frame)
                    .unwrap_or(0);
                    let parent = instance.transform;
                    let parent_matrix = transform_matrix(parent);
                    let mut children = Vec::new();
                    for layer in &symbol.timeline.layers {
                        if !layer_effectively_visible(&symbol.timeline, layer)
                            || !matches!(layer.properties.role, LayerRole::Normal | LayerRole::Mask)
                        {
                            continue;
                        }
                        let active_key = layer.key_at(child_frame);
                        let mut id_map: HashMap<Id, Id> = HashMap::new();
                        for mut child in sample_layer_in_at_fps(
                            &symbol.timeline,
                            layer,
                            child_frame,
                            f64::from(self.project.fps),
                        ) {
                            let old_id = child.id.clone();
                            let Some(mut transform) =
                                affine_transform(child.transform, parent_matrix)
                            else {
                                self.message("Break Apart would exceed transform limits.");
                                return;
                            };
                            transform.alpha = 1.;
                            transform.color_transform = child
                                .transform
                                .color_transform
                                .with_alpha(child.transform.alpha)
                                .then(parent.color_transform.with_alpha(parent.alpha));
                            if transform.color_transform.validate().is_err() {
                                self.message("Break Apart cannot preserve this combined color transform within document limits.");
                                return;
                            }
                            if transform.skew != 0.0 {
                                uses_skew = true;
                            }
                            child.transform = transform;
                            child.visible &= instance.visible;
                            if child.blend_mode.is_normal() && !instance.blend_mode.is_normal() {
                                child.blend_mode = instance.blend_mode;
                            }
                            let new_child_id = new_id("object");
                            child.id = new_child_id.clone();
                            id_map.insert(old_id.clone(), new_child_id.clone());
                            if let Some(key) = active_key
                                && let Some(bf) =
                                    key.bitmap_fills.iter().find(|bf| bf.element_id == old_id)
                            {
                                let mut mapped_bf = bf.clone();
                                mapped_bf.element_id = new_child_id;
                                unpacked_bitmap_fills.push((target.layer.clone(), mapped_bf));
                            }
                            children.push(child);
                        }
                        for group in &layer.properties.groups {
                            let mapped_members: Vec<Id> = group
                                .members
                                .iter()
                                .filter_map(|m| id_map.get(m).cloned())
                                .collect();
                            if !mapped_members.is_empty() {
                                unpacked_groups.push((
                                    target.layer.clone(),
                                    AuthoringGroup {
                                        id: new_id("group"),
                                        name: group.name.clone(),
                                        members: mapped_members,
                                    },
                                ));
                            }
                        }
                    }
                    expanded_instance = true;
                    did_break_any = true;
                    replacements.push((target, children));
                }
                ElementKind::Bitmap {
                    resource_id,
                    width,
                    height,
                    smoothing,
                } => {
                    let mut broken = instance.clone();
                    broken.id = new_id("object");
                    broken.name.clear();
                    broken.scripts.clear();
                    broken.accessibility = None;
                    broken.transform.primitive = None;
                    broken.merge = true;
                    broken.kind = ElementKind::Path {
                        commands: rectangle_path_commands(
                            f64::from(*width),
                            f64::from(*height),
                            RectangleOptions::default(),
                        ),
                        fill_rule: FillRule::NonZero,
                        fill: [255, 255, 255, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    };
                    unpacked_bitmap_fills.push((
                        target.layer.clone(),
                        AuthoredBitmapFill {
                            element_id: broken.id.clone(),
                            resource_id: resource_id.clone(),
                            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                            repeat: false,
                            smoothing: *smoothing,
                        },
                    ));
                    did_break_any = true;
                    replacements.push((target, vec![broken]));
                }
                ElementKind::TextBox { text, .. } => {
                    let total_chars = text.chars().count();
                    let visible_chars = text.chars().filter(|ch| !ch.is_whitespace()).count();
                    let Some(field) = TextFieldState::from_kind(&instance.kind) else {
                        self.message("Break Apart could not read text box parameters.");
                        return;
                    };
                    if visible_chars == 0 {
                        empty_text_skipped = true;
                        unchanged_targets.insert(target.clone());
                        replacements.push((target, vec![instance]));
                        continue;
                    }
                    if total_chars <= 1 {
                        let commands = match self.fonts.field_outline_commands(&field) {
                            Ok(commands) if !commands.is_empty() => commands,
                            Ok(_) => {
                                self.message("Text has no visible glyph outlines.");
                                return;
                            }
                            Err(err) => {
                                self.message(format!("Break Apart could not outline text: {err}"));
                                return;
                            }
                        };
                        let format =
                            field.format_at_utf16(0, field.text.encode_utf16().count() as u32);
                        let mut broken = instance.clone();
                        broken.scripts.clear();
                        broken.accessibility = None;
                        broken.transform.primitive = None;
                        broken.merge = true;
                        broken.kind = ElementKind::Path {
                            commands,
                            fill_rule: FillRule::NonZero,
                            fill: [
                                ((format.color >> 16) & 0xff) as u8,
                                ((format.color >> 8) & 0xff) as u8,
                                (format.color & 0xff) as u8,
                                field.alpha,
                            ],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        };
                        did_break_any = true;
                        replacements.push((target, vec![broken]));
                        continue;
                    }
                    let placements = match self.fonts.field_character_placements(&field) {
                        Ok(placements) => placements,
                        Err(err) => {
                            self.message(format!("Break Apart text layout failed: {err}"));
                            return;
                        }
                    };
                    let parent = instance.transform;
                    let parent_matrix = transform_matrix(parent);
                    let mut children = Vec::with_capacity(visible_chars);
                    for (idx, placement) in placements
                        .into_iter()
                        .filter(|placement| !placement.ch.is_whitespace())
                        .enumerate()
                    {
                        let ch = placement.ch;
                        let cbox = placement.bounds;
                        let mut char_format = placement.format;
                        char_format.align = TextAlign::Left;
                        char_format.left_margin = 0.0;
                        char_format.right_margin = 0.0;
                        char_format.indent = 0.0;
                        char_format.block_indent = 0.0;
                        char_format.letter_spacing = 0.0;
                        char_format.bullet = false;
                        char_format.tab_stops.clear();
                        let local = Transform {
                            x: cbox.x - 2.0,
                            y: cbox.y - 2.0,
                            ..Transform::default()
                        };
                        let Some(mut transform) = affine_transform(local, parent_matrix) else {
                            self.message("Break Apart would exceed transform limits.");
                            return;
                        };
                        transform.alpha = parent.alpha;
                        transform.color_transform = parent.color_transform;
                        transform.pivot = None;
                        transform.primitive = None;
                        if transform.skew != 0.0 {
                            uses_skew = true;
                        }
                        let width = (cbox.width + 4.0)
                            .max(char_format.size * 0.4 + 4.0)
                            .clamp(4.0, 1_000_000.0);
                        let height = (cbox.height + 4.0)
                            .max(char_format.size * 1.2 + 4.0)
                            .clamp(4.0, 1_000_000.0);
                        let name = if instance.name.is_empty() {
                            String::new()
                        } else {
                            format!("{}_{}", instance.name, idx + 1)
                        };
                        children.push(Element {
                            id: new_id("object"),
                            name,
                            transform,
                            kind: ElementKind::TextBox {
                                text: ch.to_string(),
                                width,
                                height,
                                format: char_format,
                                word_wrap: false,
                                multiline: false,
                            },
                            scripts: vec![],
                            accessibility: None,
                            visible: instance.visible,
                            cache_as_bitmap: instance.cache_as_bitmap,
                            blend_mode: instance.blend_mode,
                            filters: instance.filters.clone(),
                            ..Element::default()
                        });
                    }
                    did_break_any = true;
                    replacements.push((target, children));
                }
                ElementKind::Text {
                    text,
                    font_family,
                    font_size,
                    color,
                } => {
                    let total_chars = text.chars().count();
                    let visible_chars = text.chars().filter(|ch| !ch.is_whitespace()).count();
                    if visible_chars == 0 {
                        empty_text_skipped = true;
                        unchanged_targets.insert(target.clone());
                        replacements.push((target, vec![instance]));
                        continue;
                    }
                    if total_chars <= 1 {
                        let Some(field) = TextFieldState::from_kind(&instance.kind) else {
                            self.message("Break Apart could not read text parameters.");
                            return;
                        };
                        let commands = match self.fonts.field_outline_commands(&field) {
                            Ok(commands) if !commands.is_empty() => commands,
                            Ok(_) => {
                                self.message("Text has no visible glyph outlines.");
                                return;
                            }
                            Err(err) => {
                                self.message(format!("Break Apart could not outline text: {err}"));
                                return;
                            }
                        };
                        let mut broken = instance.clone();
                        broken.scripts.clear();
                        broken.accessibility = None;
                        broken.transform.primitive = None;
                        broken.merge = true;
                        broken.kind = ElementKind::Path {
                            commands,
                            fill_rule: FillRule::NonZero,
                            fill: *color,
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        };
                        did_break_any = true;
                        replacements.push((target, vec![broken]));
                        continue;
                    }
                    let parent = instance.transform;
                    let parent_matrix = transform_matrix(parent);
                    let line_advance = (*font_size * 1.2).max(1.0);
                    let mut children = Vec::with_capacity(visible_chars);
                    let mut letter_idx = 0usize;
                    for (line_idx, line) in text.split('\n').enumerate() {
                        let line_y = line_idx as f64 * line_advance;
                        for (ch, char_x, _) in
                            self.fonts
                                .char_x_offsets(font_family, false, false, *font_size, line)
                        {
                            if ch.is_whitespace() {
                                continue;
                            }
                            letter_idx += 1;
                            let local = Transform {
                                x: char_x,
                                y: line_y,
                                ..Transform::default()
                            };
                            let Some(mut transform) = affine_transform(local, parent_matrix) else {
                                self.message("Break Apart would exceed transform limits.");
                                return;
                            };
                            transform.alpha = parent.alpha;
                            transform.color_transform = parent.color_transform;
                            transform.pivot = None;
                            transform.primitive = None;
                            if transform.skew != 0.0 {
                                uses_skew = true;
                            }
                            let name = if instance.name.is_empty() {
                                String::new()
                            } else {
                                format!("{}_{letter_idx}", instance.name)
                            };
                            children.push(Element {
                                id: new_id("object"),
                                name,
                                transform,
                                kind: ElementKind::Text {
                                    text: ch.to_string(),
                                    font_family: font_family.clone(),
                                    font_size: *font_size,
                                    color: *color,
                                },
                                scripts: vec![],
                                accessibility: None,
                                visible: instance.visible,
                                cache_as_bitmap: instance.cache_as_bitmap,
                                blend_mode: instance.blend_mode,
                                filters: instance.filters.clone(),
                                ..Element::default()
                            });
                        }
                    }
                    did_break_any = true;
                    replacements.push((target, children));
                }
                ElementKind::Path { .. } | ElementKind::PaintedPath { .. } if !instance.merge => {
                    if let Some((fill_opt, strokes)) = decompose_element_to_merge_parts(&instance) {
                        let mut parts = Vec::new();
                        if let Some(f) = fill_opt {
                            parts.push(f);
                        }
                        parts.extend(strokes);
                        if !parts.is_empty() {
                            did_break_any = true;
                            replacements.push((target, parts));
                            continue;
                        }
                    }
                    let mut broken = instance.clone();
                    broken.merge = true;
                    did_break_any = true;
                    replacements.push((target, vec![broken]));
                }
                _ => {
                    unchanged_targets.insert(target.clone());
                    replacements.push((target, vec![instance]));
                }
            }
        }
        if !did_break_any {
            if empty_text_skipped {
                self.message("Text contains no visible glyphs to break apart.");
            } else {
                self.message("Break Apart currently supports groups, symbol instances, bitmaps, text, and Object Drawing shapes.");
            }
            return;
        }
        self.playing = false;
        self.transact(|editor| {
            if !unpack_group_targets.is_empty() {
                for layer in &mut editor.symbol_mut().timeline.layers {
                    let lid = layer.id.clone();
                    layer.properties.groups.retain(|group| {
                        !group.members.iter().any(|elem_id| {
                            unpack_group_targets.contains(&SelectionTarget {
                                layer: lid.clone(),
                                element: elem_id.clone(),
                            })
                        })
                    });
                }
            }
            let mut selection = Vec::new();
            for (target, children) in replacements {
                if unchanged_targets.contains(&target) {
                    selection.push(target);
                    continue;
                }
                editor.ensure_key_for(&target.layer, false);
                let index = editor.target_layer_index(&target).unwrap();
                let frame = editor.frame;
                let elements = &mut editor.symbol_mut().timeline.layers[index]
                    .key_at_mut(frame)
                    .unwrap()
                    .elements;
                if let Some(at) = elements.iter().position(|e| e.id == target.element) {
                    selection.extend(children.iter().map(|e| SelectionTarget {
                        layer: target.layer.clone(),
                        element: e.id.clone(),
                    }));
                    elements.splice(at..=at, children);
                }
            }
            for (layer_id, bf) in unpacked_bitmap_fills {
                let frame = editor.frame;
                if let Some(layer) = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                    && let Some(key) = layer.key_at_mut(frame)
                {
                    key.set_bitmap_fill(bf);
                }
            }
            for (layer_id, group) in unpacked_groups {
                if let Some(layer) = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                {
                    layer.properties.groups.push(group);
                }
            }
            editor.prune_authoring_groups();
            if expanded_instance || uses_skew {
                editor.project.format_version = editor.project.format_version.max(5);
            }
            editor.project.upgrade_format_for_features();
            editor.set_selection(selection);
        });
    }
}
