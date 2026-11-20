//! SimpleButton state slots, snapshot point hit-testing, and timeline/scene queries.
use super::*;

impl Player {
    fn button_symbol_id(&self, project: &Project, reference: &ObjectRef) -> Option<Id> {
        if let Some(address) = self.graph.source_address(reference) {
            let scope = self.graph.namespaces.get(&address.namespace)?;
            let node = scope.player.previous.get(&address.path)?;
            let sym_id = node.symbol_id.as_ref()?;
            let sym = project.symbol(sym_id)?;
            return (node.symbol_kind.unwrap_or(sym.kind) == SymbolKind::Button)
                .then(|| sym.id.clone());
        }
        let path = self.graph.paths.get(reference)?;
        let node = self.previous.get(path)?;
        let sym_id = node.symbol_id.as_ref()?;
        let sym = project.symbol(sym_id)?;
        (node.symbol_kind.unwrap_or(sym.kind) == SymbolKind::Button).then(|| sym.id.clone())
    }

    fn is_button_object(&self, project: &Project, reference: &ObjectRef) -> bool {
        self.graph
            .dynamic
            .get(reference)
            .is_some_and(|node| node.kind == DynamicKind::Button)
            || self.button_symbol_id(project, reference).is_some()
    }

    pub fn object_button_state(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        state: u32,
    ) -> bool {
        if state > 2 || !self.graph.alive(reference) || !self.is_button_object(project, reference) {
            return false;
        }
        let (state_changed, sound_transform) = {
            let entry = self
                .graph
                .button_states
                .entry(reference.clone())
                .or_default();
            let changed = entry.active_state != state as u8;
            entry.active_state = state as u8;
            (changed, entry.sound_transform)
        };
        if let Some(address) = self.graph.source_address(reference) {
            let changed = self
                .graph
                .namespaces
                .get_mut(&address.namespace)
                .expect("namespace")
                .player
                .button_state_with_sound(project, &address.path, state, sound_transform);
            if changed {
                self.sync_linked(project);
                self.collect_linked_scripts(&address.namespace);
            }
            return changed || state_changed;
        }
        if let Some(path) = self.graph.paths.get(reference).cloned() {
            return self.button_state_with_sound(project, &path, state, sound_transform)
                || state_changed;
        }
        true
    }

    pub fn object_button_slot(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        slot: u8,
    ) -> Result<Option<ObjectRef>, DisplayError> {
        if slot > 3 {
            return Err(error("SimpleButton state slot must be 0..=3"));
        }
        if !self.graph.alive(reference) || !self.is_button_object(project, reference) {
            return Err(error("SimpleButton state requires a live button"));
        }
        if let Some(Some(existing)) = self
            .graph
            .button_states
            .get(reference)
            .map(|s| s.slots[slot as usize].clone())
        {
            return Ok(existing.filter(|id| self.graph.alive(id)));
        }
        let Some(symbol_id) = self.button_symbol_id(project, reference) else {
            return Ok(None);
        };
        let frame_count = project
            .symbol(&symbol_id)
            .map_or(0, |sym| sym.timeline.frame_count);
        let items = if u32::from(slot) < frame_count {
            crate::evaluate(project, &symbol_id, u32::from(slot))
        } else {
            vec![]
        };
        let materialized = if items.is_empty() {
            None
        } else if items.len() == 1 {
            let item = items.into_iter().next().unwrap();
            let child = self.create_display(DynamicKind::Shape)?;
            if let Some(node) = self.graph.dynamic.get_mut(&child) {
                if !item.name.is_empty() {
                    node.name = item.name;
                }
                node.authored_kind = Some(item.kind);
            }
            let _ = self.object_set_matrix(&child, item.transform);
            Some(child)
        } else {
            let group = self.create_display(DynamicKind::Sprite)?;
            for (idx, item) in items.into_iter().enumerate() {
                let child = self.create_display(DynamicKind::Shape)?;
                if let Some(node) = self.graph.dynamic.get_mut(&child) {
                    if !item.name.is_empty() {
                        node.name = item.name;
                    }
                    node.authored_kind = Some(item.kind);
                }
                let _ = self.object_set_matrix(&child, item.transform);
                self.graph.attach_child(&group, &child, idx);
            }
            Some(group)
        };
        self.graph
            .button_states
            .entry(reference.clone())
            .or_default()
            .slots[slot as usize] = Some(materialized.clone());
        Ok(materialized)
    }

    pub fn object_set_button_slot(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        slot: u8,
        value: Option<ObjectRef>,
    ) -> Result<bool, DisplayError> {
        if slot > 3 {
            return Err(error("SimpleButton state slot must be 0..=3"));
        }
        if !self.graph.alive(reference) || !self.is_button_object(project, reference) {
            return Err(error("SimpleButton state requires a live button"));
        }
        if let Some(ref child) = value {
            if !self.graph.alive(child) {
                return Err(error(
                    "SimpleButton state DisplayObject is stale or foreign",
                ));
            }
            if child == reference || self.graph.subtree(child).contains(reference) {
                return Err(error(
                    "SimpleButton state cannot be or contain the SimpleButton itself",
                ));
            }
            if !self.graph.dynamic.contains_key(child) {
                self.validate_authored_hierarchy_change(project, child)?;
            }
            let prior_same_btn = self
                .graph
                .button_states
                .get(reference)
                .map(|s| s.slots.clone());
            self.graph.detach_child(child);
            let entry = self
                .graph
                .button_states
                .entry(reference.clone())
                .or_default();
            if let Some(prior) = prior_same_btn {
                for (i, p) in prior.into_iter().enumerate() {
                    if i != slot as usize {
                        entry.slots[i] = p;
                    }
                }
            }
            entry.slots[slot as usize] = Some(Some(child.clone()));
        } else {
            self.graph
                .button_states
                .entry(reference.clone())
                .or_default()
                .slots[slot as usize] = Some(None);
        }
        Ok(true)
    }

    pub fn object_button_sound_transform(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Result<[f64; 2], DisplayError> {
        if !self.graph.alive(reference) || !self.is_button_object(project, reference) {
            return Err(error("SimpleButton soundTransform requires a live button"));
        }
        Ok(self
            .graph
            .button_states
            .get(reference)
            .and_then(|s| s.sound_transform)
            .unwrap_or([1.0, 0.0]))
    }

    pub fn object_set_button_sound_transform(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        volume: f64,
        pan: f64,
    ) -> Result<bool, DisplayError> {
        if !self.graph.alive(reference) || !self.is_button_object(project, reference) {
            return Err(error("SimpleButton soundTransform requires a live button"));
        }
        if !volume.is_finite() || !pan.is_finite() {
            return Err(error(
                "SoundTransform volume and pan must be finite numbers",
            ));
        }
        self.graph
            .button_states
            .entry(reference.clone())
            .or_default()
            .sound_transform = Some([volume.max(0.0), pan.clamp(-1.0, 1.0)]);
        Ok(true)
    }

    pub(super) fn collect_button_hit_draws(
        &self,
        project: &Project,
        hit_ref: &ObjectRef,
        items: &HashMap<ObjectRef, DrawItem>,
    ) -> Vec<DrawItem> {
        fn ensure_hit_element(mut kind: ElementKind) -> Option<ElementKind> {
            match &mut kind {
                ElementKind::Rectangle {
                    fill, stroke_width, ..
                }
                | ElementKind::Ellipse {
                    fill, stroke_width, ..
                }
                | ElementKind::Path {
                    fill, stroke_width, ..
                } => {
                    if *stroke_width == 0.0 && fill[3] == 0 {
                        fill[3] = 255;
                    }
                    Some(kind)
                }
                ElementKind::PaintedPath { .. }
                | ElementKind::Text { .. }
                | ElementKind::TextBox { .. } => Some(kind),
                ElementKind::Bitmap { width, height, .. } => Some(ElementKind::Rectangle {
                    width: f64::from(*width),
                    height: f64::from(*height),
                    fill: [0, 0, 0, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                }),
                ElementKind::Video { width, height } => Some(ElementKind::Rectangle {
                    width: *width,
                    height: *height,
                    fill: [0, 0, 0, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                }),
                ElementKind::Instance { .. } => None,
            }
        }
        fn runtime_draw_to_hit(kind: RuntimeDrawKind) -> Option<ElementKind> {
            match kind {
                RuntimeDrawKind::Element(el) => ensure_hit_element(el),
                RuntimeDrawKind::GradientPath {
                    commands,
                    fill_rule,
                    fill,
                    stroke,
                    fill_gradient,
                    stroke_gradient,
                    fill_bitmap,
                    stroke_bitmap,
                    stroke_width,
                    stroke_hairline,
                    ..
                } => {
                    let has_fill = fill[3] > 0
                        || fill_gradient.is_some()
                        || fill_bitmap.is_some()
                        || (stroke_width == 0.0 && !stroke_hairline);
                    let has_stroke = (stroke_width > 0.0 || stroke_hairline)
                        && (stroke[3] > 0 || stroke_gradient.is_some() || stroke_bitmap.is_some());
                    Some(ElementKind::Path {
                        commands,
                        fill_rule,
                        fill: if has_fill { [0, 0, 0, 255] } else { [0; 4] },
                        stroke: if has_stroke { [0, 0, 0, 255] } else { [0; 4] },
                        stroke_width: if stroke_hairline { 1.0 } else { stroke_width },
                    })
                }
                RuntimeDrawKind::BitmapPixels { width, height, .. } => {
                    Some(ElementKind::Rectangle {
                        width: f64::from(width),
                        height: f64::from(height),
                        fill: [0, 0, 0, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    })
                }
                RuntimeDrawKind::VideoFrame { width, height, .. } => Some(ElementKind::Rectangle {
                    width,
                    height,
                    fill: [0, 0, 0, 255],
                    stroke: [0; 4],
                    stroke_width: 0.0,
                }),
                RuntimeDrawKind::TextField { field } => {
                    let (w, h) = field
                        .box_size
                        .map_or((100.0, field.format.size * 1.2), |b| (b.width, b.height));
                    Some(ElementKind::Rectangle {
                        width: w,
                        height: h,
                        fill: [0, 0, 0, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    })
                }
            }
        }

        if !self.graph.alive(hit_ref) {
            return vec![];
        }
        let mut out = Vec::new();
        let mut visited = BTreeSet::new();
        let root_matrix = self
            .object_matrix(project, hit_ref)
            .unwrap_or([1., 0., 0., 1., 0., 0.]);
        let mut stack = vec![(hit_ref.clone(), root_matrix)];
        while let Some((cur, mat)) = stack.pop() {
            if !visited.insert(cur.clone()) {
                continue;
            }
            let push_kind = |out: &mut Vec<DrawItem>, idx: usize, kind: ElementKind| {
                out.push(DrawItem {
                    element_id: format!("btn-hit-{}-{idx}", cur.id),
                    name: String::new(),
                    instance_path: vec![],
                    layer_id: String::new(),
                    transform: mat,
                    alpha: 1.0,
                    color_transform: ColorTransform::default(),
                    bitmap_fill: None,
                    path_fingerprint: None,
                    kind,
                });
            };
            if let Some(authored_item) = items.get(&cur) {
                if let Some(kind) = ensure_hit_element(authored_item.kind.clone()) {
                    push_kind(&mut out, 0, kind);
                }
            } else if let Some(dyn_obj) = self.graph.dynamic.get(&cur)
                && let Some(authored_kind) = &dyn_obj.authored_kind
                && let Some(kind) = ensure_hit_element(authored_kind.clone())
            {
                push_kind(&mut out, 0, kind);
            }
            if let Some(dyn_obj) = self.graph.dynamic.get(&cur) {
                for (idx, rkind) in dyn_obj
                    .graphics
                    .paints(&self.graph.pixels)
                    .into_iter()
                    .enumerate()
                {
                    if let Some(kind) = runtime_draw_to_hit(rkind) {
                        push_kind(&mut out, idx + 1, kind);
                    }
                }
            }
            if let Some(bmp_state) = self.graph.bitmap_states.get(&cur)
                && let Some(pref) = &bmp_state.pixel_ref
                && let Ok(info) = self.graph.pixels.info(pref)
                && !info.disposed
            {
                push_kind(
                    &mut out,
                    999_001,
                    ElementKind::Rectangle {
                        width: f64::from(info.width),
                        height: f64::from(info.height),
                        fill: [0, 0, 0, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    },
                );
            }
            if let Some(txt) = self.graph.text_states.get(&cur) {
                let (w, h) = txt
                    .box_size
                    .map_or((100.0, txt.format.size * 1.2), |b| (b.width, b.height));
                push_kind(
                    &mut out,
                    999_002,
                    ElementKind::Rectangle {
                        width: w,
                        height: h,
                        fill: [0, 0, 0, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    },
                );
            }
            for child in self.graph.children(&cur).into_iter().rev() {
                let cm = self
                    .object_matrix(project, &child)
                    .unwrap_or([1., 0., 0., 1., 0., 0.]);
                stack.push((child, affine_multiply(mat, cm)));
            }
        }
        out
    }

    /// Native point hit-test against the live `RuntimeSnapshot` (matching HTML5 `pickDetails`).
    pub fn hit_test_snapshot_point(&self, project: &Project, x: f64, y: f64) -> Option<ObjectRef> {
        fn invert(m: [f64; 6]) -> Option<[f64; 6]> {
            let det = m[0] * m[3] - m[1] * m[2];
            if !det.is_finite() || det.abs() < 1e-12 {
                return None;
            }
            let inv = 1.0 / det;
            Some([
                m[3] * inv,
                -m[1] * inv,
                -m[2] * inv,
                m[0] * inv,
                (m[2] * m[5] - m[3] * m[4]) * inv,
                (m[1] * m[4] - m[0] * m[5]) * inv,
            ])
        }
        fn effective_local_stroke(
            world: [f64; 6],
            width: f64,
            scale_mode: crate::runtime_graphics::LineScaleMode,
            hairline: bool,
        ) -> f64 {
            let sx = world[0].hypot(world[1]);
            let sy = world[2].hypot(world[3]);
            let avg = ((sx + sy) * 0.5).max(1e-6);
            if hairline {
                return 1.0 / avg;
            }
            match scale_mode {
                crate::runtime_graphics::LineScaleMode::Normal => width,
                crate::runtime_graphics::LineScaleMode::None => width / avg,
                crate::runtime_graphics::LineScaleMode::Horizontal => width * sx / avg,
                crate::runtime_graphics::LineScaleMode::Vertical => width * sy / avg,
            }
        }
        fn draw_contains(world: [f64; 6], kind: &RuntimeDrawKind, x: f64, y: f64) -> bool {
            let Some(inv) = invert(world) else {
                return false;
            };
            let lx = inv[0] * x + inv[2] * y + inv[4];
            let ly = inv[1] * x + inv[3] * y + inv[5];
            match kind {
                RuntimeDrawKind::Element(el) => match el {
                    ElementKind::Rectangle {
                        width,
                        height,
                        fill,
                        stroke,
                        stroke_width,
                    } => {
                        let sw = if *stroke_width > 0.0 && stroke[3] > 0 {
                            *stroke_width * 0.5
                        } else {
                            0.0
                        };
                        let inside = lx >= 0.0 && ly >= 0.0 && lx <= *width && ly <= *height;
                        let outer =
                            lx >= -sw && ly >= -sw && lx <= *width + sw && ly <= *height + sw;
                        let inner = lx > sw && ly > sw && lx < *width - sw && ly < *height - sw;
                        (fill[3] > 0 && inside) || (sw > 0.0 && outer && !inner)
                    }
                    ElementKind::Ellipse {
                        width,
                        height,
                        fill,
                        stroke,
                        stroke_width,
                    } => {
                        if *width <= 0.0 || *height <= 0.0 {
                            return false;
                        }
                        let sw = if *stroke_width > 0.0 && stroke[3] > 0 {
                            *stroke_width * 0.5
                        } else {
                            0.0
                        };
                        let cx = *width * 0.5;
                        let cy = *height * 0.5;
                        let in_el = |rx: f64, ry: f64| {
                            rx > 0.0
                                && ry > 0.0
                                && ((lx - cx) / rx).powi(2) + ((ly - cy) / ry).powi(2) <= 1.0
                        };
                        (fill[3] > 0 && in_el(cx, cy))
                            || (sw > 0.0 && in_el(cx + sw, cy + sw) && !in_el(cx - sw, cy - sw))
                    }
                    ElementKind::Path {
                        commands,
                        fill_rule,
                        fill,
                        stroke,
                        stroke_width,
                    } => {
                        let fill_hit = fill[3] > 0
                            && crate::hit_test_path(
                                commands,
                                *fill_rule,
                                [lx, ly],
                                true,
                                0.0,
                                Default::default(),
                            )
                            .unwrap_or(false);
                        let stroke_hit = *stroke_width > 0.0
                            && stroke[3] > 0
                            && crate::runtime_graphics::hit_test_styled_stroke(
                                commands,
                                [lx, ly],
                                *stroke_width,
                                crate::runtime_graphics::LineCaps::Round,
                                crate::runtime_graphics::LineJoints::Round,
                                3.0,
                            );
                        fill_hit || stroke_hit
                    }
                    ElementKind::PaintedPath {
                        commands,
                        fill_rule,
                        fill,
                        stroke,
                        fill_gradient,
                        stroke_gradient,
                        stroke_width,
                    } => {
                        let fill_hit = (fill[3] > 0 || fill_gradient.is_some())
                            && crate::hit_test_path(
                                commands,
                                *fill_rule,
                                [lx, ly],
                                true,
                                0.0,
                                Default::default(),
                            )
                            .unwrap_or(false);
                        let stroke_hit = *stroke_width > 0.0
                            && (stroke[3] > 0 || stroke_gradient.is_some())
                            && crate::runtime_graphics::hit_test_styled_stroke(
                                commands,
                                [lx, ly],
                                *stroke_width,
                                crate::runtime_graphics::LineCaps::Round,
                                crate::runtime_graphics::LineJoints::Round,
                                3.0,
                            );
                        fill_hit || stroke_hit
                    }
                    ElementKind::Text {
                        text, font_size, ..
                    } => {
                        let lines: Vec<&str> = text.split('\n').collect();
                        let max_w = lines
                            .iter()
                            .map(|l| l.chars().count() as f64 * *font_size * 0.6)
                            .fold(0.0_f64, f64::max);
                        let h = lines.len() as f64 * *font_size * 1.2;
                        lx >= 0.0 && ly >= 0.0 && lx <= max_w && ly <= h
                    }
                    ElementKind::TextBox { width, height, .. } => {
                        lx >= 0.0 && ly >= 0.0 && lx <= *width && ly <= *height
                    }
                    _ => false,
                },
                RuntimeDrawKind::GradientPath {
                    commands,
                    fill_rule,
                    fill,
                    stroke,
                    fill_gradient,
                    stroke_gradient,
                    fill_bitmap,
                    stroke_bitmap,
                    stroke_width,
                    stroke_caps,
                    stroke_joints,
                    stroke_miter_limit,
                    stroke_scale_mode,
                    stroke_hairline,
                    ..
                } => {
                    let has_fill = fill[3] > 0 || fill_gradient.is_some() || fill_bitmap.is_some();
                    let fill_hit = has_fill
                        && crate::hit_test_path(
                            commands,
                            *fill_rule,
                            [lx, ly],
                            true,
                            0.0,
                            Default::default(),
                        )
                        .unwrap_or(false);
                    let has_stroke = (*stroke_width > 0.0 || *stroke_hairline)
                        && (stroke[3] > 0 || stroke_gradient.is_some() || stroke_bitmap.is_some());
                    let eff_w = effective_local_stroke(
                        world,
                        *stroke_width,
                        *stroke_scale_mode,
                        *stroke_hairline,
                    );
                    let stroke_hit = has_stroke
                        && crate::runtime_graphics::hit_test_styled_stroke(
                            commands,
                            [lx, ly],
                            eff_w,
                            *stroke_caps,
                            *stroke_joints,
                            *stroke_miter_limit,
                        );
                    fill_hit || stroke_hit
                }
                RuntimeDrawKind::BitmapPixels { width, height, .. } => {
                    lx >= 0.0 && ly >= 0.0 && lx < f64::from(*width) && ly < f64::from(*height)
                }
                RuntimeDrawKind::VideoFrame { width, height, .. } => {
                    lx >= 0.0 && ly >= 0.0 && lx < *width && ly < *height
                }
                RuntimeDrawKind::TextField { field } => {
                    let (w, h) = field
                        .box_size
                        .map_or((100.0, field.format.size * 1.2), |b| (b.width, b.height));
                    lx >= 0.0 && ly >= 0.0 && lx <= w && ly <= h
                }
            }
        }

        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let snap = self.runtime_snapshot(project);
        let by_ref: HashMap<&ObjectRef, &RuntimeObject> =
            snap.objects.iter().map(|o| (&o.object_ref, o)).collect();
        let mut draws_by_owner: HashMap<&ObjectRef, Vec<&RuntimeDrawItem>> = HashMap::new();
        for d in &snap.draws {
            draws_by_owner.entry(&d.owner_ref).or_default().push(d);
        }
        let button_hit = |obj: &RuntimeObject| -> bool {
            obj.button_hits.iter().any(|item| {
                draw_contains(
                    affine_multiply(obj.display.world, item.transform),
                    &RuntimeDrawKind::Element(item.kind.clone()),
                    x,
                    y,
                )
            })
        };
        let mask_coverage = if snap.clips.is_empty() {
            None
        } else {
            crate::MaskCoverage::new(project, &snap.clips).ok()
        };
        let clips_contain = |clip_ids: &[String]| -> bool {
            if let Some(cov) = &mask_coverage {
                return cov.alpha(clip_ids, [x, y]) > 0.0;
            }
            clip_ids.iter().all(|cid| {
                let Some(clip) = snap.clips.iter().find(|c| &c.id == cid) else {
                    return true;
                };
                clip.components.iter().any(|comp| {
                    let Some(inv) = invert(comp.matrix) else {
                        return false;
                    };
                    let lx = inv[0] * x + inv[2] * y + inv[4];
                    let ly = inv[1] * x + inv[3] * y + inv[5];
                    crate::hit_test_path(
                        &comp.commands,
                        comp.fill_rule,
                        [lx, ly],
                        true,
                        0.0,
                        Default::default(),
                    )
                    .unwrap_or(false)
                })
            })
        };
        let subtree_hit = |root_ref: &ObjectRef| -> bool {
            snap.objects.iter().any(|cand| {
                let mut cur = Some(cand);
                let mut in_sub = false;
                while let Some(c) = cur {
                    if &c.object_ref == root_ref {
                        in_sub = true;
                        break;
                    }
                    cur = c.parent_ref.as_ref().and_then(|p| by_ref.get(p).copied());
                }
                in_sub
                    && (cand.clip_ids.is_empty() || clips_contain(&cand.clip_ids))
                    && draws_by_owner.get(&cand.object_ref).is_some_and(|ds| {
                        ds.iter()
                            .any(|d| draw_contains(d.item.transform, &d.item.kind, x, y))
                    })
            })
        };

        for obj in snap.objects.iter().rev() {
            if !obj.on_stage || !obj.display.effective_visible {
                continue;
            }
            if !obj.clip_ids.is_empty() && !clips_contain(&obj.clip_ids) {
                continue;
            }
            let hit = if let Some(area_ref) = obj.input.as_ref().and_then(|i| i.hit_area.as_ref()) {
                subtree_hit(area_ref)
            } else if obj.display.owner == "button"
                && (obj.button_hit_authoritative || !obj.button_hits.is_empty())
            {
                button_hit(obj)
            } else {
                draws_by_owner.get(&obj.object_ref).is_some_and(|ds| {
                    ds.iter()
                        .any(|d| draw_contains(d.item.transform, &d.item.kind, x, y))
                })
            };
            if !hit {
                continue;
            }
            let mut chosen: Option<&RuntimeObject> = None;
            let mut cur = Some(obj);
            let mut rejected = false;
            while let Some(node) = cur {
                if !node.on_stage || !node.display.effective_visible {
                    rejected = true;
                    break;
                }
                if let Some(inp) = &node.input {
                    if node.display.owner == "button"
                        && (node.button_hit_authoritative || !node.button_hits.is_empty())
                        && !button_hit(node)
                    {
                        rejected = true;
                        break;
                    }
                    if node.display.owner == "button" && inp.mouse_enabled {
                        chosen = Some(node);
                    }
                    if chosen.is_none() && inp.mouse_enabled {
                        chosen = Some(node);
                    }
                    if !inp.mouse_children && node.object_ref != obj.object_ref {
                        if !inp.mouse_enabled {
                            rejected = true;
                            break;
                        }
                        chosen = Some(node);
                    }
                }
                cur = node
                    .parent_ref
                    .as_ref()
                    .and_then(|p| by_ref.get(p).copied());
            }
            if !rejected && let Some(target) = chosen {
                if let Some(area_ref) = target.input.as_ref().and_then(|i| i.hit_area.as_ref())
                    && !subtree_hit(area_ref)
                {
                    continue;
                }
                return Some(target.object_ref.clone());
            }
        }
        None
    }

    pub fn object_control(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        command: PlaybackCommand,
    ) -> bool {
        if self.graph.loaded_as2(reference) {
            return matches!(
                command,
                PlaybackCommand::Play
                    | PlaybackCommand::Stop
                    | PlaybackCommand::GotoAndPlay(0)
                    | PlaybackCommand::GotoAndStop(0)
            );
        }
        if let Some(address) = self.graph.source_address(reference) {
            let result = self
                .graph
                .namespaces
                .get_mut(&address.namespace)
                .expect("namespace")
                .player
                .control(project, &address.path, command);
            if result {
                self.sync_linked(project);
                self.collect_linked_scripts(&address.namespace);
            }
            return result;
        }
        if let Some(path) = self.graph.paths.get(reference).cloned() {
            return self.control(project, &path, command);
        }
        let Some(object) = self
            .graph
            .dynamic
            .get_mut(reference)
            .filter(|node| node.kind == DynamicKind::MovieClip)
        else {
            return false;
        };
        match command {
            PlaybackCommand::Play | PlaybackCommand::GotoAndPlay(0) => object.playing = true,
            PlaybackCommand::Stop | PlaybackCommand::GotoAndStop(0) => object.playing = false,
            _ => return false,
        }
        true
    }
    pub fn object_metadata(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Option<TimelineMetadata> {
        if !self.graph.alive(reference) || self.graph.loaded_as2(reference) {
            return None;
        }
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .clip_metadata(project, &address.path);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.clip_metadata(project, path);
        }
        self.graph
            .dynamic
            .get(reference)
            .filter(|node| node.kind == DynamicKind::MovieClip)
            .map(|_| TimelineMetadata {
                current_label: None,
                current_frame_label: None,
                labels: vec![],
                frames_loaded: 1,
                total_frames: 1,
                scene_name: String::new(),
                scene_frame: 1,
                scenes: vec![SceneMetadata {
                    name: String::new(),
                    num_frames: 1,
                    labels: vec![],
                }],
                // Empty MovieClip instances have one frame; explicit play
                // cannot put them into the AS3 isPlaying state.
                is_playing: false,
            })
    }
    pub fn object_frame(&self, reference: &ObjectRef) -> Option<u32> {
        if self.graph.loaded_as2(reference) {
            return Some(0);
        }
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .clip_frame(&address.path);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.clip_frame(path);
        }
        self.graph
            .dynamic
            .get(reference)
            .filter(|node| node.kind == DynamicKind::MovieClip)
            .map(|_| 0)
    }
    pub fn object_frame_count(&self, project: &Project, reference: &ObjectRef) -> Option<u32> {
        if self.graph.loaded_as2(reference) {
            return Some(1);
        }
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .clip_frame_count(project, &address.path);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.clip_frame_count(project, path);
        }
        self.object_frame(reference).map(|_| 1)
    }
    pub fn object_resolve_frame(
        &self,
        project: &Project,
        reference: &ObjectRef,
        label: &str,
        as2: bool,
    ) -> Option<u32> {
        if self.graph.loaded_as2(reference) {
            return None;
        }
        if let Some(address) = self.graph.source_address(reference) {
            let scope = &self.graph.namespaces.get(&address.namespace)?.player;
            return if as2 {
                scope.resolve_frame_as2(project, &address.path, label)
            } else {
                scope.resolve_frame(project, &address.path, label)
            };
        }
        let path = self.graph.paths.get(reference)?;
        if as2 {
            self.resolve_frame_as2(project, path, label)
        } else {
            self.resolve_frame(project, path, label)
        }
    }
    /// Scene count of a live timeline object; dynamic and loaded AS2 clips have one.
    pub fn object_scene_count(&self, project: &Project, reference: &ObjectRef) -> Option<u32> {
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .clip_scene_count(project, &address.path);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.clip_scene_count(project, path);
        }
        self.object_frame(reference).map(|_| 1)
    }
    /// Scene-aware goto resolution for a live timeline object.
    pub fn object_resolve_goto(
        &self,
        project: &Project,
        reference: &ObjectRef,
        request: &GotoRequest,
    ) -> Result<Option<u32>, String> {
        if !self.graph.loaded_as2(reference)
            && let Some(address) = self.graph.source_address(reference)
        {
            return self
                .graph
                .namespaces
                .get(&address.namespace)
                .ok_or("Display object is no longer active.")?
                .player
                .resolve_goto(project, &address.path, request);
        }
        if !self.graph.loaded_as2(reference)
            && let Some(path) = self.graph.paths.get(reference)
        {
            return self.resolve_goto(project, path, request);
        }
        if self.object_frame(reference).is_none() {
            return Err("Timeline controls apply to symbols and the root scene.".into());
        }
        // Empty runtime MovieClips and loaded AS2 content expose one frame.
        let single = [crate::SceneRange {
            name: String::new(),
            start: 0,
            frames: 1,
        }];
        crate::resolve_goto(&single, &[], 0, request)
    }
    pub fn object_scene_step(
        &self,
        project: &Project,
        reference: &ObjectRef,
        step: i32,
    ) -> Option<u32> {
        if self.graph.loaded_as2(reference) {
            return None;
        }
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .resolve_scene_step(project, &address.path, step);
        }
        let path = self.graph.paths.get(reference)?;
        self.resolve_scene_step(project, path, step)
    }

    pub fn object_frame_scripts(
        &self,
        project: &Project,
        reference: &ObjectRef,
        frame: u32,
    ) -> Vec<Script> {
        if self.graph.loaded_as2(reference) {
            return Vec::new();
        }
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)
                .map_or_else(Vec::new, |ns| {
                    ns.player.frame_scripts(project, &address.path, frame)
                });
        }
        self.graph
            .paths
            .get(reference)
            .map_or_else(Vec::new, |path| self.frame_scripts(project, path, frame))
    }
}
