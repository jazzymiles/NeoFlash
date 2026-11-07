//! Trace Bitmap controller: one cancellable worker, latest preview wins, full-resolution commit
//! is validated against the original placement and resource before one history transaction.
use crate::{app::Editor, selection::SelectionTarget};
use eframe::egui;
use neoflash_core::{bitmap_trace::*, *};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

pub(crate) const TRACE_PRESETS: &[&str] = &[
    "[Default]",
    "Photo — high",
    "Photo — low",
    "Line art",
    "Logo",
    "Technical drawing",
    "Classic Flash",
];
pub(crate) fn preset(index: usize) -> TraceOptions {
    let mut p = TraceOptions::default();
    match index {
        1 => {
            p.colours = 30;
            p.paths = 80.;
            p.corners = 25.;
            p.noise = 2;
        }
        2 => {
            p.colours = 8;
            p.paths = 35.;
            p.corners = 20.;
            p.noise = 20;
        }
        3 => {
            p.mode = TraceMode::BlackAndWhite;
            p.colours = 2;
            p.paths = 80.;
            p.corners = 65.;
            p.noise = 4;
            p.ignore_white = true;
        }
        4 => {
            p.colours = 6;
            p.paths = 85.;
            p.corners = 85.;
            p.noise = 4;
            p.snap_curves = true;
        }
        5 => {
            p.mode = TraceMode::BlackAndWhite;
            p.colours = 2;
            p.paths = 95.;
            p.corners = 100.;
            p.noise = 2;
            p.snap_curves = true;
            p.ignore_white = true;
        }
        6 => {
            p.colours = 16;
            p.paths = 50.;
            p.corners = 50.;
            p.noise = 8;
        }
        _ => {}
    }
    p
}
#[derive(Clone)]
pub(crate) struct TraceTarget {
    root: Id,
    symbol: Id,
    frame: u32,
    pub target: SelectionTarget,
    pub source_element: Element,
    pub element: Element,
    opaque_placement: bool,
}
pub(crate) struct TracePreview {
    pub trace: BitmapTrace,
    pub source: egui::TextureHandle,
    pub options: TraceOptions,
}
pub(crate) struct TraceDialog {
    pub target: TraceTarget,
    pub resource: ResourceUnit,
    pub options: TraceOptions,
    pub preset: usize,
    pub live_preview: bool,
    pub advanced: bool,
    pub preview: Option<TracePreview>,
    pub error: Option<String>,
    pub tracing: bool,
}
struct Request {
    generation: u64,
    resource: ResourceUnit,
    options: TraceOptions,
    full: bool,
    opaque_placement: bool,
}
struct Outcome {
    request: Request,
    image: Arc<DecodedImage>,
    trace: BitmapTrace,
}
struct Running {
    generation: u64,
    full: bool,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    result: Receiver<Result<Option<Outcome>, String>>,
}
#[derive(Default)]
pub(crate) struct BitmapTraceController {
    pub dialog: Option<TraceDialog>,
    generation: u64,
    pending: Option<Request>,
    running: Option<Running>,
}
impl Drop for BitmapTraceController {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl BitmapTraceController {
    pub(crate) fn is_open(&self) -> bool {
        self.dialog.is_some()
    }
    pub(crate) fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.dialog = None;
        self.pending = None;
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
            || self
                .running
                .as_ref()
                .is_some_and(|r| r.generation == self.generation)
    }
    pub(crate) fn progress(&self) -> f32 {
        self.running
            .as_ref()
            .filter(|r| r.generation == self.generation)
            .map(|r| f32::from_bits(r.progress.load(Ordering::Relaxed)))
            .unwrap_or(0.)
    }
    pub(crate) fn request(&mut self, full: bool) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };
        self.generation = self.generation.wrapping_add(1);
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
        dialog.error = None;
        dialog.tracing = full;
        self.pending = Some(Request {
            generation: self.generation,
            resource: dialog.resource.clone(),
            options: dialog.options.clone(),
            full,
            opaque_placement: dialog.target.opaque_placement,
        });
    }
    fn poll(&mut self, ctx: &egui::Context) -> Option<Result<Outcome, String>> {
        let mut ready = None;
        if let Some(r) = &self.running {
            let result = match r.result.try_recv() {
                Ok(v) => Some(v),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("Tracing stopped unexpectedly. Try Trace again.".into()))
                }
            };
            if let Some(result) = result {
                let r = self.running.take().unwrap();
                if r.generation == self.generation && self.dialog.is_some() {
                    if let Some(dialog) = self.dialog.as_mut()
                        && r.full
                    {
                        dialog.tracing = false;
                    }
                    ready = match result {
                        Ok(Some(out)) => Some(Ok(out)),
                        Err(e) => Some(Err(e)),
                        _ => None,
                    };
                }
            }
        }
        if self.running.is_none()
            && let Some(request) = self.pending.take()
        {
            let cancel = Arc::new(AtomicBool::new(false));
            let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
            let (sender, result) = mpsc::channel();
            let generation = request.generation;
            let full = request.full;
            let cancel_worker = cancel.clone();
            let progress_worker = progress.clone();
            let context = ctx.clone();
            let worker = std::thread::Builder::new()
                .name("bitmap-trace".into())
                .spawn(move || {
                    let output = run_request(request, &cancel_worker, &progress_worker);
                    let _ = sender.send(output);
                    context.request_repaint();
                });
            match worker {
                Ok(_) => {
                    self.running = Some(Running {
                        generation,
                        full,
                        cancel,
                        progress,
                        result,
                    })
                }
                Err(error) => ready = Some(Err(format!("Could not start tracing: {error}"))),
            }
        }
        if self.running.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        ready
    }
}
fn run_request(
    request: Request,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<Option<Outcome>, String> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let source = request.resource.decode_image().map_err(|e| e.to_string())?;
    let image = if request.full || source.width.max(source.height) <= 384 {
        source
    } else {
        let ratio = 384. / source.width.max(source.height) as f64;
        let (w, h) = (
            (source.width as f64 * ratio).round().max(1.) as u32,
            (source.height as f64 * ratio).round().max(1.) as u32,
        );
        let raw = image::RgbaImage::from_raw(source.width, source.height, source.rgba.to_vec())
            .ok_or("Invalid bitmap pixels.")?;
        let scaled = image::imageops::resize(&raw, w, h, image::imageops::FilterType::Triangle);
        Arc::new(DecodedImage {
            width: w,
            height: h,
            rgba: scaled.into_raw().into(),
        })
    };
    let mut options = request.options.clone();
    // A flat authoring group has no composite-opacity wrapper. Preserve the bitmap alpha
    // by keeping disjoint boundaries whenever the placement changes opacity or colour.
    if !request.opaque_placement {
        options.method = TraceMethod::Abutting;
    }
    if !request.full {
        let source = request.resource.decode_image().map_err(|e| e.to_string())?;
        let scale = image.width as f64 / source.width as f64;
        options.noise = (f64::from(options.noise) * scale * scale)
            .round()
            .clamp(1., 100.) as u32;
        options.stroke_width = (options.stroke_width * scale).max(0.01);
    }
    let result = trace_bitmap(&image, &options, |p| {
        progress.store(p.to_bits(), Ordering::Relaxed);
        !cancel.load(Ordering::Relaxed)
    })?;
    Ok(result.map(|trace| Outcome {
        request,
        image,
        trace,
    }))
}
fn combine_bitmap_transforms(child: Transform, parent: Transform) -> Option<Transform> {
    if child == Transform::default() {
        return Some(parent);
    }
    let mut combined = crate::authoring_modify::affine_transform(child, transform_matrix(parent))?;
    if child.alpha == 1.0 && child.color_transform.is_identity() {
        combined.alpha = parent.alpha;
        combined.color_transform = parent.color_transform;
    } else {
        combined.alpha = 1.0;
        combined.color_transform = child
            .color_transform
            .with_alpha(child.alpha)
            .then(parent.color_transform.with_alpha(parent.alpha));
        if combined.color_transform.validate().is_err() {
            return None;
        }
    }
    Some(combined)
}

fn resolve_bitmap_element_depth(
    project: &Project,
    source: &Element,
    sampled: &Element,
    depth: usize,
) -> Option<(Element, bool)> {
    if depth > 4 {
        return None;
    }
    match &source.kind {
        ElementKind::Bitmap { .. } => {
            let opaque = source.transform.alpha == 1.0
                && source.transform.color_transform.is_identity()
                && sampled.transform.alpha == 1.0
                && sampled.transform.color_transform.is_identity();
            Some((source.clone(), opaque))
        }
        ElementKind::Instance {
            symbol_id,
            playback,
        } => {
            let symbol = project.symbol(symbol_id)?;
            let single_frame = match playback {
                InstancePlayback::SingleFrame(f) => {
                    Some((*f).min(symbol.timeline.frame_count.saturating_sub(1)))
                }
                _ if symbol.timeline.frame_count == 1 => Some(0),
                _ => None,
            }?;
            if symbol.class_name.is_some()
                || !symbol.timeline.scripts.is_empty()
                || symbol
                    .timeline
                    .layers
                    .iter()
                    .any(|l| !l.visible || l.locked || l.properties != LayerProperties::default())
            {
                return None;
            }
            let mut children: Vec<Element> = symbol
                .timeline
                .layers
                .iter()
                .flat_map(|l| sample_layer_at_fps(l, single_frame, f64::from(project.fps)))
                .collect();
            if children.len() != 1 {
                return None;
            }
            let child = children.remove(0);
            let (resolved_child, child_opaque) =
                resolve_bitmap_element_depth(project, &child, &child, depth + 1)?;
            let combined = combine_bitmap_transforms(resolved_child.transform, source.transform)?;
            let combined_sampled =
                combine_bitmap_transforms(resolved_child.transform, sampled.transform)?;
            let opaque = child_opaque
                && combined.alpha == 1.0
                && combined.color_transform.is_identity()
                && combined_sampled.alpha == 1.0
                && combined_sampled.color_transform.is_identity();
            let mut resolved = source.clone();
            if resolved.name.is_empty() {
                resolved.name = if !symbol.name.is_empty() {
                    symbol.name.clone()
                } else {
                    resolved_child.name
                };
            }
            resolved.transform = combined;
            resolved.kind = resolved_child.kind;
            if resolved.scripts.is_empty() {
                resolved.scripts = resolved_child.scripts;
            }
            if resolved.filters.is_empty() {
                resolved.filters = resolved_child.filters;
            }
            if resolved.blend_mode == BlendMode::Normal {
                resolved.blend_mode = resolved_child.blend_mode;
            }
            resolved.cache_as_bitmap |= resolved_child.cache_as_bitmap;
            if resolved.accessibility.is_none() {
                resolved.accessibility = resolved_child.accessibility;
            }
            Some((resolved, opaque))
        }
        _ => None,
    }
}

fn resolve_bitmap_element(
    project: &Project,
    source: &Element,
    sampled: &Element,
) -> Option<(Element, bool)> {
    resolve_bitmap_element_depth(project, source, sampled, 0)
}

impl Editor {
    fn resolve_trace_target(&self) -> Option<(SelectionTarget, Element, Element, bool)> {
        if !self.selection.is_empty() {
            let mut targets = self.sampled_targets();
            if targets.len() != 1 {
                return None;
            }
            let (target, sampled) = targets.remove(0);
            if !self.target_editable(&target) {
                return None;
            }
            let index = self.target_layer_index(&target)?;
            let source_element = self.symbol().timeline.layers[index]
                .key_at(self.frame)
                .and_then(|k| k.elements.iter().find(|e| e.id == target.element))?
                .clone();
            let (resolved, opaque) =
                resolve_bitmap_element(&self.project, &source_element, &sampled)?;
            return Some((target, source_element, resolved, opaque));
        }
        let timeline = &self.symbol().timeline;
        let mut candidates = Vec::new();
        for layer in &timeline.layers {
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
            {
                continue;
            }
            let Some(key) = layer.key_at(self.frame) else {
                continue;
            };
            for sampled in
                sample_layer_in_at_fps(timeline, layer, self.frame, f64::from(self.project.fps))
            {
                let target = SelectionTarget {
                    layer: layer.id.clone(),
                    element: sampled.id.clone(),
                };
                if !self.target_editable(&target) {
                    continue;
                }
                let Some(source_element) =
                    key.elements.iter().find(|e| e.id == sampled.id).cloned()
                else {
                    continue;
                };
                if let Some((resolved, opaque)) =
                    resolve_bitmap_element(&self.project, &source_element, &sampled)
                {
                    candidates.push((target, source_element, resolved, opaque));
                }
            }
        }
        if candidates.len() == 1 {
            return candidates.pop();
        }
        if let Some(lib_sym) = self.library_selected.as_deref() {
            let mut matched: Vec<_> = candidates
                .iter()
                .filter(|(_, src, _, _)| {
                    matches!(&src.kind, ElementKind::Instance { symbol_id, .. } if symbol_id == lib_sym)
                })
                .cloned()
                .collect();
            if matched.len() == 1 {
                return matched.pop();
            }
        }
        if let Some(res_id) = self.resource_panel.selected.as_deref() {
            let mut matched: Vec<_> = candidates
                .into_iter()
                .filter(|(_, _, resolved, _)| {
                    matches!(&resolved.kind, ElementKind::Bitmap { resource_id, .. } if resource_id == res_id)
                })
                .collect();
            if matched.len() == 1 {
                return matched.pop();
            }
        }
        None
    }
    pub(crate) fn can_trace_bitmap(&self) -> bool {
        self.resolve_trace_target().is_some()
    }
    pub(crate) fn open_trace_bitmap(&mut self, ctx: &egui::Context) {
        let Some((target, source_element, element, opaque_placement)) = self.resolve_trace_target()
        else {
            self.message("Select one editable bitmap to trace.");
            return;
        };
        self.finish_edits();
        self.set_playing(false);
        if self.selection.targets != std::slice::from_ref(&target) {
            self.set_selection(vec![target.clone()]);
        }
        let ElementKind::Bitmap {
            ref resource_id, ..
        } = element.kind
        else {
            return;
        };
        let Some(resource) = self
            .project
            .resources
            .iter()
            .find(|r| r.id == *resource_id)
            .cloned()
        else {
            self.message("Bitmap resource is missing.");
            return;
        };
        self.bitmap_trace.cancel();
        self.bitmap_trace.dialog = Some(TraceDialog {
            target: TraceTarget {
                root: self.project.root.clone(),
                symbol: self.symbol_id().to_owned(),
                frame: self.frame,
                target,
                source_element,
                opaque_placement,
                element,
            },
            resource,
            options: TraceOptions::default(),
            preset: 0,
            live_preview: true,
            advanced: true,
            preview: None,
            error: None,
            tracing: false,
        });
        self.bitmap_trace.request(false);
        ctx.request_repaint();
    }
    pub(crate) fn poll_bitmap_trace(&mut self, ctx: &egui::Context) {
        let Some(result) = self.bitmap_trace.poll(ctx) else {
            return;
        };
        match result {
            Err(error) => {
                if let Some(dialog) = &mut self.bitmap_trace.dialog {
                    dialog.tracing = false;
                    dialog.error = Some(error);
                }
            }
            Ok(out) if out.request.full => {
                if let Err(error) = self.commit_bitmap_trace(out.trace, &out.request.options) {
                    if let Some(dialog) = &mut self.bitmap_trace.dialog {
                        dialog.error = Some(error);
                    }
                } else {
                    self.bitmap_trace.cancel();
                }
            }
            Ok(out) => {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [out.image.width as usize, out.image.height as usize],
                    &out.image.rgba,
                );
                let source = ctx.load_texture("trace-source", image, egui::TextureOptions::LINEAR);
                if let Some(dialog) = &mut self.bitmap_trace.dialog {
                    dialog.preview = Some(TracePreview {
                        trace: out.trace,
                        source,
                        options: out.request.options,
                    });
                }
            }
        }
    }
    pub(crate) fn commit_bitmap_trace(
        &mut self,
        trace: BitmapTrace,
        options: &TraceOptions,
    ) -> Result<(), String> {
        let dialog = self
            .bitmap_trace
            .dialog
            .as_ref()
            .ok_or("Trace dialog was closed.")?;
        let target = dialog.target.clone();
        if self.project.root != target.root
            || self.symbol_id() != target.symbol
            || self.frame != target.frame
            || !self.target_editable(&target.target)
            || !self.project.resources.iter().any(|r| r == &dialog.resource)
        {
            return Err(
                "The document or bitmap changed during tracing. Close and reopen Trace bitmap."
                    .into(),
            );
        }
        let index = self
            .target_layer_index(&target.target)
            .ok_or("The bitmap's layer was removed.")?;
        let layer = &self.symbol().timeline.layers[index];
        let current = layer
            .key_at(self.frame)
            .and_then(|k| k.elements.iter().find(|e| e.id == target.target.element));
        if current != Some(&target.source_element) {
            return Err("The bitmap changed during tracing. Close and reopen Trace bitmap.".into());
        }
        let sampled_now = sample_layer_in_at_fps(
            &self.symbol().timeline,
            layer,
            self.frame,
            f64::from(self.project.fps),
        )
        .into_iter()
        .find(|e| e.id == target.target.element);
        let resolved_now = sampled_now
            .as_ref()
            .and_then(|s| resolve_bitmap_element(&self.project, &target.source_element, s));
        if resolved_now.as_ref().map(|(el, _)| el) != Some(&target.element) {
            return Err("The bitmap changed during tracing. Close and reopen Trace bitmap.".into());
        }
        if !target.element.scripts.is_empty()
            || !target.element.filters.is_empty()
            || target.element.blend_mode != BlendMode::Normal
            || target.element.cache_as_bitmap
        {
            return Err("Remove bitmap scripts, filters, blending and bitmap caching before tracing; these cannot be applied to an authoring group.".into());
        }
        if layer.properties.motion_guide.is_some()
            && layer
                .key_at(self.frame)
                .is_some_and(|k| k.frame != self.frame)
            && self.workspace.timeline_prefs.auto_keyframe
        {
            return Err("Create a keyframe before tracing a guided bitmap between keys.".into());
        }
        options.validate()?;
        let ElementKind::Bitmap { width, height, .. } = target.element.kind else {
            return Err("Selection is no longer a bitmap.".into());
        };
        if trace.width == 0 || trace.height == 0 || trace.shapes.is_empty() {
            return Err("No traced paths to insert.".into());
        }
        let scale = [
            f64::from(width) / f64::from(trace.width),
            f64::from(height) / f64::from(trace.height),
        ];
        let mut shapes = trace.shapes;
        for shape in &mut shapes {
            scale_commands(&mut shape.commands, scale);
            validate_path_commands(&shape.commands).map_err(|e| e.to_string())?;
        }
        let count = trace.path_count;
        self.transact_named("Trace bitmap", |editor| {
            // The shared history hook upgrades formats 6+, while authoring groups still
            // require the explicit legacy format-4 bump used by Group / Break apart.
            if shapes.len() > 1 {
                editor.project.format_version = editor.project.format_version.max(4);
            }
            editor.ensure_key_for(&target.target.layer, false);
            let raw_base = editor.symbol().timeline.layers[index]
                .key_at(target.frame)
                .unwrap()
                .elements
                .iter()
                .find(|e| e.id == target.target.element)
                .unwrap()
                .clone();
            let base = resolve_bitmap_element(&editor.project, &raw_base, &raw_base)
                .map(|(el, _)| el)
                .unwrap_or(raw_base);
            if base.transform.skew != 0.0 || !base.transform.color_transform.is_identity() {
                editor.project.format_version = editor.project.format_version.max(5);
            }
            let layer = &mut editor.symbol_mut().timeline.layers[index];
            let key = layer.key_at_mut(target.frame).unwrap();
            let at = key
                .elements
                .iter()
                .position(|e| e.id == target.target.element)
                .unwrap();
            let members: Vec<_> = shapes
                .into_iter()
                .enumerate()
                .map(|(i, shape)| {
                    let mut element = base.clone();
                    if i > 0 {
                        element.id = new_id("trace");
                        element.accessibility = None;
                    }
                    element.name = format!("{} — trace {}", base.name, i + 1);
                    element.kind = ElementKind::Path {
                        commands: shape.commands,
                        fill_rule: shape.fill_rule,
                        fill: if options.fills { shape.colour } else { [0; 4] },
                        stroke: if options.strokes {
                            shape.colour
                        } else {
                            [0; 4]
                        },
                        stroke_width: if options.strokes {
                            options.stroke_width
                        } else {
                            0.
                        },
                    };
                    element.merge = false;
                    element
                })
                .collect();
            let ids: Vec<_> = members.iter().map(|e| e.id.clone()).collect();
            key.elements.splice(at..=at, members);
            let mut in_group = false;
            for group in &mut layer.properties.groups {
                if let Some(i) = group
                    .members
                    .iter()
                    .position(|id| id == &target.target.element)
                {
                    group.members.splice(i..=i, ids.clone());
                    in_group = true;
                }
            }
            if !in_group && ids.len() > 1 {
                layer.properties.groups.push(AuthoringGroup {
                    id: new_id("group"),
                    name: format!("{} — traced", base.name),
                    members: ids.clone(),
                });
            }
            editor.set_selection(
                ids.into_iter()
                    .map(|element| SelectionTarget {
                        layer: target.target.layer.clone(),
                        element,
                    })
                    .collect(),
            );
        });
        self.message(format!("Traced bitmap: {count} paths."));
        Ok(())
    }
}
fn scale_commands(commands: &mut [PathCommand], scale: [f64; 2]) {
    for c in commands {
        match c {
            PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                *x *= scale[0];
                *y *= scale[1];
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                *cx *= scale[0];
                *cy *= scale[1];
                *x *= scale[0];
                *y *= scale[1];
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                *cx1 *= scale[0];
                *cy1 *= scale[1];
                *cx2 *= scale[0];
                *cy2 *= scale[1];
                *x *= scale[0];
                *y *= scale[1];
            }
            PathCommand::Close => {}
        }
    }
}
#[cfg(test)]
#[path = "bitmap_trace_tests.rs"]
mod tests;
