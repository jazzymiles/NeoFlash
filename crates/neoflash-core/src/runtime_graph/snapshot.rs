//! Runtime display snapshot assembly, camera layer projection, draw emission, and post-processing.
use super::*;

struct SnapshotEvalInputs {
    authored: HashMap<ObjectRef, DisplayNode>,
    layer_masks: Vec<crate::authored_layers::RuntimeLayerMask>,
    items: HashMap<ObjectRef, DrawItem>,
    button_symbols: HashMap<ObjectRef, Id>,
    camera_by_ref: HashMap<ObjectRef, ([f64; 6], ColorTransform)>,
    metadata: HashMap<ObjectRef, Accessibility>,
    linked_roots: HashMap<ObjectRef, DisplayNode>,
}

struct ResolvedSnapshotNode {
    display: DisplayNode,
    authored_path: Option<Vec<Id>>,
    namespace_ref: Option<ObjectRef>,
    source_path: Option<Vec<Id>>,
    graphics: Vec<RuntimeDrawKind>,
}

struct SnapshotDrawContext<'a> {
    reference: &'a ObjectRef,
    display: &'a DisplayNode,
    source_path: Option<&'a [Id]>,
    concatenated_color_transform: ColorTransform,
    items: &'a HashMap<ObjectRef, DrawItem>,
    video: Option<&'a VideoState>,
    text: Option<&'a TextFieldState>,
    bitmap: Option<&'a PixelDescriptor>,
    bitmap_state: &'a bitmap::BitmapState,
}

fn is_runtime_draw_visible(kind: &RuntimeDrawKind) -> bool {
    match kind {
        RuntimeDrawKind::Element(
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
            }
            | ElementKind::Path {
                fill,
                stroke,
                stroke_width,
                ..
            },
        ) => fill[3] > 0 || (*stroke_width > 0. && stroke[3] > 0),
        RuntimeDrawKind::TextField { field } => {
            field.alpha > 0
                && (!field.text.is_empty()
                    || field.as2_viewport.background
                    || field.as2_viewport.border)
        }
        RuntimeDrawKind::GradientPath {
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
            fill_bitmap
                .as_ref()
                .is_some_and(|b| b.color_transform.can_be_visible())
                || fill_gradient
                    .as_ref()
                    .map_or(fill[3] > 0, |g| g.stops.iter().any(|s| s.color[3] > 0))
                || ((*stroke_width > 0. || *stroke_hairline)
                    && (stroke_bitmap
                        .as_ref()
                        .is_some_and(|b| b.color_transform.can_be_visible())
                        || stroke_gradient
                            .as_ref()
                            .map_or(stroke[3] > 0, |g| g.stops.iter().any(|s| s.color[3] > 0))))
        }
        RuntimeDrawKind::Element(ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        }) => {
            fill_gradient
                .as_ref()
                .map_or(fill[3] > 0, |g| g.stops.iter().any(|s| s.color[3] > 0))
                || (*stroke_width > 0.
                    && stroke_gradient
                        .as_ref()
                        .map_or(stroke[3] > 0, |g| g.stops.iter().any(|s| s.color[3] > 0)))
        }
        // The browser's revision cache supplies actual pixel alpha
        // bounds. Core avoids rescanning bitmap buffers each snapshot.
        RuntimeDrawKind::BitmapPixels {
            color_transform, ..
        }
        | RuntimeDrawKind::VideoFrame {
            color_transform, ..
        } => color_transform.can_be_visible(),
        _ => false,
    }
}

impl Player {
    pub fn runtime_snapshot(&self, project: &Project) -> RuntimeSnapshot {
        self.snapshot_with_root(project, None)
    }

    /// Capture the current display subtree using the normal scene evaluator.
    /// The source placement/ancestor transforms are ignored. No timelines,
    /// scripts, references, buffers or display properties are mutated.
    pub fn bitmap_draw_snapshot(
        &self,
        project: &Project,
        source: &ObjectRef,
        matrix: [f64; 6],
        color: ColorTransform,
    ) -> Result<RuntimeSnapshot, BitmapError> {
        if !self.graph.alive(source) {
            return Err(BitmapError("draw source is stale or foreign".into()));
        }
        if !matrix
            .into_iter()
            .enumerate()
            .all(|(i, v)| v.is_finite() && v.abs() <= if i < 4 { 1e4 } else { 1e12 })
        {
            return Err(BitmapError(
                "draw matrix exceeds finite affine bounds".into(),
            ));
        }
        color.validate().map_err(|e| BitmapError(e.to_string()))?;
        let included: BTreeSet<_> = self.graph.subtree(source).into_iter().collect();
        if self
            .graph
            .masks
            .iter()
            .any(|(target, mask)| included.contains(target) && !included.contains(mask))
        {
            return Err(BitmapError("draw currently requires mask sources inside the captured subtree; external mask dependencies are unsupported".into()));
        }
        let result = self.snapshot_with_root(project, Some((source, matrix, color)));
        if result
            .objects
            .iter()
            .any(|node| node.bitmap.as_ref().is_some_and(|b| b.disposed))
        {
            return Err(BitmapError(
                "draw subtree contains a disposed BitmapData".into(),
            ));
        }
        Ok(result)
    }

    fn collect_eval_camera_layers(
        &self,
        project: &Project,
        nodes: &[crate::evaluate::Node],
        out: &mut HashMap<ObjectRef, ([f64; 6], ColorTransform)>,
        resolve_ref: impl Fn(&[Id]) -> Option<ObjectRef>,
    ) {
        let stage_center = [project.width as f64 * 0.5, project.height as f64 * 0.5];
        let fps = f64::from(project.fps.max(1));
        for node in nodes {
            let Some(sym_id) = &node.symbol_id else {
                continue;
            };
            let Some(sym) = project.symbol(sym_id) else {
                continue;
            };
            let Some(cam) = crate::evaluate::sample_timeline_camera(&sym.timeline, node.frame)
            else {
                continue;
            };
            for layer in &sym.timeline.layers {
                if layer.properties.attach_to_camera
                    || matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide)
                {
                    continue;
                }
                let cam_m = crate::evaluate::camera_layer_matrix(
                    &cam,
                    layer.properties.z_depth,
                    stage_center,
                );
                let cam_ct = cam.color_transform;
                let mut child_path = node.path.clone();
                for el in
                    crate::evaluate::sample_layer_in_at_fps(&sym.timeline, layer, node.frame, fps)
                {
                    child_path.push(el.id);
                    if let Some(obj_ref) = resolve_ref(&child_path)
                        && !self.graph.parent_overrides.contains_key(&obj_ref)
                    {
                        out.insert(obj_ref, (cam_m, cam_ct));
                    }
                    child_path.pop();
                }
            }
        }
    }

    fn collect_snapshot_eval_inputs(&self, project: &Project) -> SnapshotEvalInputs {
        let base = self.authored_snapshot(project);
        let mut authored: HashMap<_, _> = self
            .display_nodes_from_evaluation(project, &base)
            .into_iter()
            .filter_map(|node| self.object_ref(&node.path).map(|id| (id, node)))
            .collect();
        let mut layer_masks: Vec<_> = base
            .masks
            .iter()
            .map(|plan| plan.resolve("root", |path| self.object_ref(path)))
            .collect();
        let mut items: HashMap<_, _> = base
            .items
            .into_iter()
            .filter_map(|item| self.object_ref(&item.instance_path).map(|id| (id, item)))
            .collect();
        let mut button_symbols: HashMap<_, _> = base
            .nodes
            .iter()
            .filter_map(|node| {
                let symbol = node.symbol_id.as_ref().and_then(|id| project.symbol(id))?;
                (node.symbol_kind.unwrap_or(symbol.kind) == SymbolKind::Button)
                    .then(|| {
                        self.object_ref(&node.path)
                            .map(|id| (id, symbol.id.clone()))
                    })
                    .flatten()
            })
            .collect();
        let mut camera_by_ref: HashMap<ObjectRef, ([f64; 6], ColorTransform)> = HashMap::new();
        self.collect_eval_camera_layers(project, &base.nodes, &mut camera_by_ref, |path| {
            self.object_ref(path)
        });
        let mut metadata: HashMap<_, _> = base
            .nodes
            .into_iter()
            .filter_map(|node| {
                node.accessibility.and_then(|accessibility| {
                    self.object_ref(&node.path).map(|id| (id, accessibility))
                })
            })
            .collect();
        let mut linked_roots = HashMap::new();
        for (namespace, scope) in &self.graph.namespaces {
            let source = scope.player.authored_snapshot(project);
            for node in scope.player.display_nodes_from_evaluation(project, &source) {
                if node.path.is_empty() {
                    linked_roots.insert(namespace.clone(), node);
                } else if let Some(reference) = self.graph.linked_ref(namespace, &node.path) {
                    authored.insert(reference, node);
                }
            }
            layer_masks.extend(source.masks.iter().map(|plan| {
                plan.resolve(&format!("{:?}", namespace), |path| {
                    self.graph.linked_ref(namespace, path)
                })
            }));
            for item in source.items {
                if let Some(reference) = self.graph.linked_ref(namespace, &item.instance_path) {
                    items.insert(reference, item);
                }
            }
            self.collect_eval_camera_layers(project, &source.nodes, &mut camera_by_ref, |path| {
                self.graph.linked_ref(namespace, path)
            });
            for node in source.nodes {
                if let Some(symbol) = node.symbol_id.as_ref().and_then(|id| project.symbol(id))
                    && node.symbol_kind.unwrap_or(symbol.kind) == SymbolKind::Button
                    && let Some(reference) = self.graph.linked_ref(namespace, &node.path)
                {
                    button_symbols.insert(reference, symbol.id.clone());
                }
                if let Some(accessibility) = node.accessibility
                    && let Some(reference) = self.graph.linked_ref(namespace, &node.path)
                {
                    metadata.insert(reference, accessibility);
                }
            }
        }
        SnapshotEvalInputs {
            authored,
            layer_masks,
            items,
            button_symbols,
            camera_by_ref,
            metadata,
            linked_roots,
        }
    }

    fn snapshot_traversal_roots(
        &self,
        capture: Option<(&ObjectRef, [f64; 6], ColorTransform)>,
    ) -> Vec<ObjectRef> {
        let stage_roots = self.graph.stage_children();
        let mut roots: Vec<_> = capture.map_or_else(
            || stage_roots.clone(),
            |(source, _, _)| vec![source.clone()],
        );
        if capture.is_none() {
            roots.extend(
                self.graph
                    .dynamic
                    .iter()
                    .filter(|(id, _)| self.graph.parent(id).is_none() && !stage_roots.contains(id))
                    .map(|(id, _)| id.clone()),
            );
            roots.extend(
                self.graph
                    .parent_overrides
                    .iter()
                    .filter(|(id, _)| {
                        self.graph.parent(id).is_none()
                            && self.graph.alive(id)
                            && !stage_roots.contains(id)
                    })
                    .map(|(id, _)| id.clone()),
            );
        }
        roots
    }

    fn resolve_snapshot_display_node(
        &self,
        reference: &ObjectRef,
        authored: &HashMap<ObjectRef, DisplayNode>,
        linked_roots: &HashMap<ObjectRef, DisplayNode>,
    ) -> Option<ResolvedSnapshotNode> {
        let (mut display, authored_path, namespace_ref, source_path, graphics) =
            if let Some(node) = authored.get(reference) {
                let linked = linked_roots.get(reference);
                let namespace = if linked.is_some() {
                    Some(reference.clone())
                } else {
                    self.graph
                        .linked_paths
                        .get(reference)
                        .map(|address| address.namespace.clone())
                };
                let mut merged = node.clone();
                if let Some(source) = linked {
                    merged.frame = source.frame;
                    merged.total_frames = source.total_frames;
                    merged.symbol_id = source.symbol_id.clone();
                    merged.independent = source.owner == "movie_clip";
                }
                (
                    merged,
                    if self.graph.linked_paths.contains_key(reference) {
                        None
                    } else {
                        Some(node.path.clone())
                    },
                    namespace,
                    if linked.is_some() {
                        Some(vec![])
                    } else {
                        Some(node.path.clone())
                    },
                    vec![],
                )
            } else {
                let node = self.graph.dynamic.get(reference)?;
                let owner = match node.kind {
                    DynamicKind::Sprite => "sprite",
                    DynamicKind::Shape => "shape",
                    DynamicKind::MovieClip => "movie_clip",
                    DynamicKind::Bitmap => "bitmap",
                    DynamicKind::TextField => "text",
                    DynamicKind::Video => "video",
                    DynamicKind::Loader => "loader",
                    DynamicKind::Button => "button",
                    DynamicKind::TextLine => "text_line",
                    DynamicKind::MorphShape => "morph_shape",
                    DynamicKind::Avm1Movie => "avm1_movie",
                };
                let linked = linked_roots.get(reference);
                let movie = node.kind == DynamicKind::MovieClip;
                (
                    DisplayNode {
                        path: vec![],
                        lifetime_id: reference.id.clone(),
                        name: node.name.clone(),
                        owner: linked.map_or_else(|| owner.into(), |source| source.owner.clone()),
                        symbol_id: linked.and_then(|source| source.symbol_id.clone()),
                        class_name: linked.and_then(|source| source.class_name.clone()),
                        transform: node.transform,
                        world: [0.; 6],
                        visible: node.visible,
                        effective_visible: true,
                        frame: linked.map_or(0, |source| source.frame),
                        total_frames: linked
                            .map_or(if movie { 1 } else { 0 }, |source| source.total_frames),
                        independent: linked.map_or(movie, |source| source.owner == "movie_clip"),
                        blend_mode: linked.map_or(BlendMode::Normal, |s| s.blend_mode),
                        cache_as_bitmap: linked.is_some_and(|s| s.cache_as_bitmap),
                        filters: linked.map_or_else(Vec::new, |s| s.filters.clone()),
                        kind: linked
                            .and_then(|s| s.kind.clone())
                            .or_else(|| node.authored_kind.clone()),
                    },
                    None,
                    linked.map(|_| reference.clone()),
                    linked.map(|_| vec![]),
                    node.graphics.paints(&self.graph.pixels),
                )
            };
        if self.graph.loaded_as2(reference) {
            display.frame = 0;
            display.total_frames = 1;
            display.symbol_id = None;
            display.kind = None;
        }
        Some(ResolvedSnapshotNode {
            display,
            authored_path,
            namespace_ref,
            source_path,
            graphics,
        })
    }

    fn resolve_snapshot_accessibility(
        &self,
        reference: &ObjectRef,
        root: Option<&ObjectRef>,
        display_owner: &str,
        parent_silent: bool,
        parent_disabled: bool,
        metadata: &HashMap<ObjectRef, Accessibility>,
    ) -> (Option<Accessibility>, bool, bool) {
        let acc_override = self.graph.as2_accessibility.get(reference);
        let silent = parent_silent || acc_override.is_some_and(|a| a.silent);
        let as2_disabled = self
            .graph
            .as2_input_states
            .get(reference)
            .is_some_and(|state| state.enabled == Some(false));
        let own_accessibility = if silent {
            None
        } else if let Some(base) = metadata.get(reference) {
            let mut acc = base.clone();
            if let Some(ov) = acc_override {
                if let Some(name) = &ov.name {
                    acc.name = name.clone();
                }
                if let Some(description) = &ov.description {
                    acc.description = description.clone();
                }
            }
            if as2_disabled {
                acc.disabled = true;
            }
            Some(acc)
        } else if root != Some(reference)
            && let Some(ov) = acc_override
            && ov.name.as_deref().is_some_and(|name| !name.is_empty())
        {
            Some(Accessibility {
                name: ov.name.clone().unwrap_or_default(),
                description: ov.description.clone().unwrap_or_default(),
                role: if display_owner == "button" || ov.interactive {
                    AccessibleRole::Button
                } else {
                    AccessibleRole::Image
                },
                tab_order: 0,
                disabled: as2_disabled,
            })
        } else {
            None
        };
        let disabled = parent_disabled
            || as2_disabled
            || own_accessibility
                .as_ref()
                .is_some_and(|a| a.role == AccessibleRole::Button && a.disabled);
        let children_silent = silent || acc_override.is_some_and(|a| a.force_simple);
        (own_accessibility, disabled, children_silent)
    }

    fn emit_snapshot_node_draws(
        &self,
        ctx: &SnapshotDrawContext<'_>,
        draws: &mut Vec<RuntimeDrawItem>,
    ) {
        let reference = ctx.reference;
        let display = ctx.display;
        let concatenated_color_transform = ctx.concatenated_color_transform;
        let instance_path = || {
            ctx.source_path
                .map_or_else(Vec::new, |slice| slice.to_vec())
        };
        let item_layer_id = || {
            ctx.items
                .get(reference)
                .map_or_else(String::new, |item| item.layer_id.clone())
        };

        if let Some(source) = ctx.items.get(reference) {
            let mut item = source.clone().map_kind(RuntimeDrawKind::Element);
            item.transform = display.world;
            item.alpha = 1.;
            item.color_transform = ColorTransform::default();
            if let RuntimeDrawKind::Element(kind) = &mut item.kind {
                concatenated_color_transform.transform_kind(kind);
                if let Some(paint) = &item.bitmap_fill
                    && let Some(painted) =
                        self.graph
                            .authored_bitmap_shape(kind, paint, concatenated_color_transform)
                {
                    item.kind = painted;
                }
            }
            if !matches!(
                source.kind,
                ElementKind::Bitmap { .. }
                    | ElementKind::Video { .. }
                    | ElementKind::Text { .. }
                    | ElementKind::TextBox { .. }
            ) {
                draws.push(RuntimeDrawItem {
                    item,
                    owner_ref: reference.clone(),
                    clip_ids: vec![],
                    filter_id: None,
                    procedural_graphics: false,
                });
            }
        } else if let Some(mut kind) = self
            .graph
            .dynamic
            .get(reference)
            .and_then(|node| node.authored_kind.clone())
            && !matches!(
                kind,
                ElementKind::Bitmap { .. }
                    | ElementKind::Video { .. }
                    | ElementKind::Text { .. }
                    | ElementKind::TextBox { .. }
            )
        {
            concatenated_color_transform.transform_kind(&mut kind);
            draws.push(RuntimeDrawItem {
                owner_ref: reference.clone(),
                clip_ids: vec![],
                filter_id: None,
                procedural_graphics: false,
                item: DrawItem {
                    element_id: format!("runtime-{}-authored", reference.id),
                    name: display.name.clone(),
                    instance_path: vec![],
                    layer_id: String::new(),
                    transform: display.world,
                    alpha: 1.,
                    color_transform: ColorTransform::default(),
                    bitmap_fill: None,
                    path_fingerprint: None,
                    kind: RuntimeDrawKind::Element(kind),
                },
            });
        }
        if let Some(video) = ctx.video {
            draws.push(RuntimeDrawItem {
                owner_ref: reference.clone(),
                clip_ids: vec![],
                filter_id: None,
                procedural_graphics: false,
                item: DrawItem {
                    element_id: format!("runtime-{}-video", reference.id),
                    name: display.name.clone(),
                    instance_path: instance_path(),
                    layer_id: item_layer_id(),
                    transform: display.world,
                    alpha: 1.,
                    color_transform: ColorTransform::default(),
                    bitmap_fill: None,
                    path_fingerprint: None,
                    kind: RuntimeDrawKind::VideoFrame {
                        width: video.width,
                        height: video.height,
                        smoothing: video.smoothing,
                        color_transform: concatenated_color_transform,
                    },
                },
            });
        }
        if let Some(field) = ctx.text {
            for (index, ([x, y, width, height], color)) in self
                .graph
                .as2_selection_rectangles(reference, &field.revision)
                .into_iter()
                .enumerate()
            {
                let mut kind = ElementKind::Rectangle {
                    width,
                    height,
                    fill: [0, 0, 0, 0],
                    stroke: [(color >> 16) as u8, (color >> 8) as u8, color as u8, 255],
                    stroke_width: 1.,
                };
                concatenated_color_transform.transform_kind(&mut kind);
                draws.push(RuntimeDrawItem {
                    owner_ref: reference.clone(),
                    clip_ids: vec![],
                    filter_id: None,
                    procedural_graphics: false,
                    item: DrawItem {
                        element_id: format!("as2-selection-{}-{index}", reference.id),
                        name: display.name.clone(),
                        instance_path: instance_path(),
                        layer_id: item_layer_id(),
                        transform: affine_multiply(display.world, [1., 0., 0., 1., x, y]),
                        alpha: 1.,
                        color_transform: ColorTransform::default(),
                        bitmap_fill: None,
                        path_fingerprint: None,
                        kind: RuntimeDrawKind::Element(kind),
                    },
                });
            }
            draws.push(RuntimeDrawItem {
                owner_ref: reference.clone(),
                clip_ids: vec![],
                filter_id: None,
                procedural_graphics: false,
                item: DrawItem {
                    element_id: ctx.items.get(reference).map_or_else(
                        || format!("runtime-{}-text", reference.id),
                        |item| item.element_id.clone(),
                    ),
                    name: display.name.clone(),
                    instance_path: instance_path(),
                    layer_id: item_layer_id(),
                    transform: display.world,
                    alpha: 1.,
                    color_transform: ColorTransform::default(),
                    bitmap_fill: None,
                    path_fingerprint: None,
                    kind: RuntimeDrawKind::TextField {
                        field: Box::new(concatenated_color_transform.transform_text(field)),
                    },
                },
            });
        }
        if let Some(pixel) = ctx.bitmap
            && !pixel.disposed
        {
            draws.push(RuntimeDrawItem {
                owner_ref: reference.clone(),
                clip_ids: vec![],
                filter_id: None,
                procedural_graphics: false,
                item: DrawItem {
                    element_id: ctx.items.get(reference).map_or_else(
                        || format!("runtime-{}-bitmap", reference.id),
                        |item| item.element_id.clone(),
                    ),
                    name: display.name.clone(),
                    instance_path: instance_path(),
                    layer_id: item_layer_id(),
                    transform: display.world,
                    alpha: 1.,
                    color_transform: ColorTransform::default(),
                    bitmap_fill: None,
                    path_fingerprint: None,
                    kind: RuntimeDrawKind::BitmapPixels {
                        pixel_ref: pixel.pixel_ref.clone(),
                        width: pixel.width,
                        height: pixel.height,
                        revision: pixel.revision.clone(),
                        smoothing: ctx.bitmap_state.smoothing,
                        pixel_snapping: ctx.bitmap_state.pixel_snapping,
                        color_transform: concatenated_color_transform,
                    },
                },
            });
        }
        let colored_graphics = self
            .graph
            .dynamic
            .get(reference)
            .map(|node| {
                node.graphics
                    .paints_with_color(&self.graph.pixels, concatenated_color_transform)
            })
            .unwrap_or_default();
        for (index, kind) in colored_graphics.into_iter().enumerate() {
            draws.push(RuntimeDrawItem {
                owner_ref: reference.clone(),
                clip_ids: vec![],
                filter_id: None,
                procedural_graphics: true,
                item: DrawItem {
                    element_id: format!("runtime-{}-{index}", reference.id),
                    name: display.name.clone(),
                    instance_path: vec![],
                    layer_id: String::new(),
                    transform: display.world,
                    alpha: 1.,
                    color_transform: ColorTransform::default(),
                    bitmap_fill: None,
                    path_fingerprint: None,
                    kind,
                },
            });
        }
        if let Some(runs) = self.graph.fte_draws.get(reference) {
            for (index, run) in runs.iter().enumerate() {
                if let Some(mut field) = TextFieldState::from_kind(&ElementKind::Text {
                    text: run.text.clone(),
                    font_family: run.font_family.clone(),
                    font_size: run.font_size,
                    color: run.color,
                }) {
                    field.format.bold = run.bold;
                    field.format.italic = run.italic;
                    field.alpha = run.color[3];
                    let run_matrix = match run.rotation.as_str() {
                        "rotate90" => [0., 1., -1., 0., run.x, run.y],
                        "rotate180" => [-1., 0., 0., -1., run.x, run.y],
                        "rotate270" => [0., -1., 1., 0., run.x, run.y],
                        _ => [1., 0., 0., 1., run.x, run.y],
                    };
                    draws.push(RuntimeDrawItem {
                        owner_ref: reference.clone(),
                        clip_ids: vec![],
                        filter_id: None,
                        procedural_graphics: false,
                        item: DrawItem {
                            element_id: format!("runtime-{}-fte-{index}", reference.id),
                            name: display.name.clone(),
                            instance_path: vec![],
                            layer_id: String::new(),
                            transform: affine_multiply(display.world, run_matrix),
                            alpha: 1.,
                            color_transform: ColorTransform::default(),
                            bitmap_fill: None,
                            path_fingerprint: None,
                            kind: RuntimeDrawKind::TextField {
                                field: Box::new(
                                    concatenated_color_transform.transform_text(&field),
                                ),
                            },
                        },
                    });
                }
            }
        }
    }

    fn resolve_snapshot_button_hits(
        &self,
        project: &Project,
        reference: &ObjectRef,
        display_owner: &str,
        button_symbols: &HashMap<ObjectRef, Id>,
        items: &HashMap<ObjectRef, DrawItem>,
    ) -> (Vec<DrawItem>, bool) {
        if display_owner != "button" {
            return (vec![], false);
        }
        match self
            .graph
            .button_states
            .get(reference)
            .and_then(|state| state.slots[3].clone())
        {
            Some(Some(hit_ref)) => (
                self.collect_button_hit_draws(project, &hit_ref, items),
                true,
            ),
            Some(None) => (vec![], true),
            None => match button_symbols.get(reference) {
                Some(symbol) => {
                    let count = project.symbol(symbol).map_or(0, |s| s.timeline.frame_count);
                    if count >= 4 {
                        (crate::evaluate(project, symbol, 3), true)
                    } else {
                        (vec![], false)
                    }
                }
                None => (vec![], true),
            },
        }
    }

    fn finalize_snapshot_postprocess(
        &self,
        result: &mut RuntimeSnapshot,
        layer_masks: &[crate::authored_layers::RuntimeLayerMask],
    ) {
        RuntimeGraph::sanitize_input(result);
        crate::authored_layers::apply_runtime_masks(result, layer_masks);
        self.graph.apply_clipping(result);
        crate::mask_composition::factor_alpha_clip_groups(&mut result.clips);
        self.graph.apply_filters(result);
        crate::authored_layers::isolate_runtime_alpha_layers(result, layer_masks);
        // Eligibility follows final descendant paint alpha. A signed parent
        // transform may hide 0..255 itself while a child's unclamped transform
        // produces a visible concatenation, so never prune whole subtrees early.
        // Each ancestor is visited once; no per-semantic-node scene scan.
        let mut painted = BTreeSet::new();
        for draw in &result.draws {
            if is_runtime_draw_visible(&draw.item.kind) {
                let mut current = Some(draw.owner_ref.clone());
                while let Some(reference) = current {
                    if !painted.insert(reference.clone()) {
                        break;
                    }
                    current = self.graph.parent(&reference);
                }
            }
        }
        result
            .semantics
            .retain(|node| painted.contains(&node.owner_ref));
        result
            .semantics
            .sort_by_key(|node| node.node.accessibility.tab_order);
    }

    fn snapshot_with_root(
        &self,
        project: &Project,
        capture: Option<(&ObjectRef, [f64; 6], ColorTransform)>,
    ) -> RuntimeSnapshot {
        let inputs = self.collect_snapshot_eval_inputs(project);
        let mut result = RuntimeSnapshot::default();
        let root = self.object_ref(&[]);
        let roots = self.snapshot_traversal_roots(capture);
        let mut pending: Vec<_> = roots
            .into_iter()
            .rev()
            .map(|id| {
                (
                    id,
                    [1., 0., 0., 1., 0., 0.],
                    ColorTransform::default(),
                    true,
                    false,
                    false,
                    false,
                    false,
                )
            })
            .collect();
        while let Some((
            reference,
            parent_world,
            parent_color,
            parent_visible,
            parent_attached,
            parent_disabled,
            parent_singular,
            parent_silent,
        )) = pending.pop()
        {
            let Some(resolved) = self.resolve_snapshot_display_node(
                &reference,
                &inputs.authored,
                &inputs.linked_roots,
            ) else {
                continue;
            };
            let mut display = resolved.display;
            let text = self.graph.text_state(&reference, display.kind.as_ref());
            let video = self.graph.video_state(&reference, display.kind.as_ref());
            let mut bitmap_state = self.graph.bitmap_state(&reference, display.kind.as_ref());
            bitmap_state.smoothing |= self.graph.loaded_as2_smoothing(&reference);
            let bitmap = bitmap_state
                .pixel_ref
                .as_ref()
                .and_then(|id| self.graph.pixels.info(id).ok());
            let capture_root = capture.filter(|(source, _, _)| *source == &reference);
            let local_matrix = if let Some((_, matrix, _)) = capture_root {
                matrix
            } else if let Some(affine) = self.graph.affines.get(&reference) {
                affine.update_display(&mut display);
                affine.matrix
            } else {
                transform_matrix(display.transform)
            };
            let color_transform = capture_root.map_or_else(
                || {
                    self.graph
                        .color_transforms
                        .get(&reference)
                        .copied()
                        .unwrap_or_else(|| {
                            display
                                .transform
                                .color_transform
                                .with_alpha(display.transform.alpha)
                        })
                },
                |(_, _, color)| color,
            );
            display.transform.alpha = color_transform.multipliers[3];
            let (effective_parent_world, effective_parent_color) = if capture_root.is_none()
                && let Some(&(cam_m, cam_ct)) = inputs.camera_by_ref.get(&reference)
            {
                (
                    affine_multiply(parent_world, cam_m),
                    cam_ct.then(parent_color),
                )
            } else {
                (parent_world, parent_color)
            };
            let concatenated_color_transform = color_transform.then(effective_parent_color);
            let scroll_rect = self.graph.scroll_rects.get(&reference).copied();
            let content_local_matrix = scroll_rect.map_or(local_matrix, |rect| {
                affine_multiply(local_matrix, [1., 0., 0., 1., -rect.x, -rect.y])
            });
            display.world = affine_multiply(effective_parent_world, content_local_matrix);
            display.effective_visible =
                parent_visible && (capture_root.is_some() || display.visible);
            let on_stage =
                capture_root.is_some() || parent_attached || self.graph.is_stage_root(&reference);
            let singular = parent_singular
                || local_matrix[0] * local_matrix[3] - local_matrix[1] * local_matrix[2] == 0.;
            let (own_accessibility, disabled, children_silent) = self
                .resolve_snapshot_accessibility(
                    &reference,
                    root.as_ref(),
                    &display.owner,
                    parent_silent,
                    parent_disabled,
                    &inputs.metadata,
                );
            if on_stage
                && display.effective_visible
                && !singular
                && (display.world[0] * display.world[3] - display.world[1] * display.world[2]).abs()
                    >= 1e-12
                && let Some(mut accessibility) = own_accessibility
            {
                if parent_disabled {
                    accessibility.disabled = true;
                }
                result.semantics.push(RuntimeAccessibleNode {
                    node: AccessibleNode {
                        instance_path: resolved.source_path.clone().unwrap_or_default(),
                        lifetime_id: display.lifetime_id.clone(),
                        accessibility,
                    },
                    owner_ref: reference.clone(),
                    clip_ids: vec![],
                });
            }
            let children = self.graph.children(&reference);
            if on_stage && display.effective_visible {
                self.emit_snapshot_node_draws(
                    &SnapshotDrawContext {
                        reference: &reference,
                        display: &display,
                        source_path: resolved.source_path.as_deref(),
                        concatenated_color_transform,
                        items: &inputs.items,
                        video: video.as_ref(),
                        text: text.as_ref(),
                        bitmap: bitmap.as_ref(),
                        bitmap_state: &bitmap_state,
                    },
                    &mut result.draws,
                );
            }
            for child in children.iter().rev() {
                pending.push((
                    child.clone(),
                    display.world,
                    concatenated_color_transform,
                    display.effective_visible,
                    on_stage,
                    disabled,
                    singular,
                    children_silent,
                ));
            }
            let (button_hits, button_hit_authoritative) = self.resolve_snapshot_button_hits(
                project,
                &reference,
                &display.owner,
                &inputs.button_symbols,
                &inputs.items,
            );
            result.objects.push(RuntimeObject {
                input: self.graph.input_state(
                    &reference,
                    &display.owner,
                    text.as_ref()
                        .is_some_and(|field| field.field_type == TextFieldType::Input),
                    inputs
                        .metadata
                        .get(&reference)
                        .is_some_and(|accessibility| accessibility.role == AccessibleRole::Button),
                ),
                display,
                object_ref: reference.clone(),
                parent_ref: if capture_root.is_some() {
                    None
                } else {
                    self.graph.parent(&reference)
                },
                children,
                on_stage,
                authored_path: resolved.authored_path,
                namespace_ref: resolved.namespace_ref,
                source_path: resolved.source_path,
                graphics: resolved.graphics,
                button_hits,
                button_hit_authoritative,
                fte_draws: self
                    .graph
                    .fte_draws
                    .get(&reference)
                    .cloned()
                    .unwrap_or_default(),
                text,
                video,
                bitmap,
                smoothing: bitmap_state.smoothing,
                pixel_snapping: bitmap_state.pixel_snapping,
                local_matrix,
                content_local_matrix,
                scroll_rect,
                mask_ref: self.graph.masks.get(&reference).cloned(),
                clip_ids: vec![],
                filters: if capture.is_some() {
                    vec![]
                } else {
                    self.graph
                        .filters
                        .get(&reference)
                        .cloned()
                        .unwrap_or_default()
                },
                color_transform,
                concatenated_color_transform,
                authored_mask_source: false,
            });
        }
        self.finalize_snapshot_postprocess(&mut result, &inputs.layer_masks);
        result
    }
}
