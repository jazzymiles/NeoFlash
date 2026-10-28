//! Direct authoring tools. Gestures are drafts; a completed edit is one undo step.
//! Interaction follows the user's Flash CS4 guide (drawing, color, selection, view).
use crate::{
    app::{Editor, Tool},
    selection::SelectionTarget,
    stage::{affine, inverse},
    theme::UiPalette,
};
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};
use neoflash_core::*;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum EraserMode {
    #[default]
    Objects,
    Fills,
    Strokes,
}

pub(crate) struct AuthoringToolsState {
    gesture: Option<Gesture>,
    pub brush_size: f32,
    pub eraser_size: f32,
    pub eraser_mode: EraserMode,
    pub lasso_contact: bool,
    pub zoom_out: bool,
}
impl Default for AuthoringToolsState {
    fn default() -> Self {
        Self {
            gesture: None,
            brush_size: 12.,
            eraser_size: 16.,
            eraser_mode: EraserMode::Objects,
            lasso_contact: true,
            zoom_out: false,
        }
    }
}
impl AuthoringToolsState {
    pub fn cancel(&mut self) {
        self.gesture = None;
    }
    pub fn is_drawing(&self) -> bool {
        self.gesture.is_some()
    }
}
struct Gesture {
    tool: Tool,
    symbol: Id,
    frame: u32,
    layer: Id,
    origin: Pos2,
    zoom: f32,
    pan: Vec2,
    start: Pos2,
    latest: Pos2,
    points: Vec<Pos2>,
    shift: bool,
    alt: bool,
    targets: Vec<SelectionTarget>,
    gradient: Option<GradientGesture>,
}
struct GradientGesture {
    target: SelectionTarget,
    kind: ElementKind,
    matrix: [f64; 6],
    element_matrix: [f64; 6],
    handle: bool,
}
impl Gesture {
    fn local(&self, point: Pos2) -> Pos2 {
        Pos2::ZERO + (point - self.origin) / self.zoom
    }
}
fn extended(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::Line
            | Tool::Brush
            | Tool::Bucket
            | Tool::Ink
            | Tool::Eyedropper
            | Tool::Eraser
            | Tool::Lasso
            | Tool::Hand
            | Tool::Zoom
            | Tool::Gradient
    )
}
fn paint_parts(kind: &ElementKind) -> Option<(Color, Color, f64)> {
    match kind {
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
        }
        | ElementKind::PaintedPath {
            fill,
            stroke,
            stroke_width,
            ..
        } => Some((*fill, *stroke, *stroke_width)),
        _ => None,
    }
}
fn paint_gradient(kind: &ElementKind) -> Option<&GradientPaint> {
    if let ElementKind::PaintedPath { fill_gradient, .. } = kind {
        fill_gradient.as_ref()
    } else {
        None
    }
}
fn apply_paint(kind: &mut ElementKind, fill_color: Option<Color>, line: Option<(Color, f64)>) {
    match kind {
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
        }
        | ElementKind::PaintedPath {
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            if let Some(color) = fill_color {
                *fill = color;
            }
            if let Some((color, width)) = line {
                *stroke = color;
                *stroke_width = width;
            }
        }
        _ => {}
    }
    if let ElementKind::PaintedPath {
        fill_gradient,
        stroke_gradient,
        ..
    } = kind
    {
        if fill_color.is_some() {
            *fill_gradient = None;
        }
        if line.is_some() {
            *stroke_gradient = None;
        }
    }
}
fn path_geometry(kind: &ElementKind) -> Option<(Vec<PathCommand>, FillRule, bool)> {
    let point = |x, y| PathCommand::LineTo { x, y };
    match kind {
        ElementKind::Rectangle { width, height, .. } => Some((
            vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                point(*width, 0.),
                point(*width, *height),
                point(0., *height),
                PathCommand::Close,
            ],
            FillRule::NonZero,
            true,
        )),
        ElementKind::Ellipse { width, height, .. } => {
            let (rx, ry) = (*width / 2., *height / 2.);
            let (kx, ky) = (rx * 0.5522847498, ry * 0.5522847498);
            Some((
                vec![
                    PathCommand::MoveTo { x: rx, y: 0. },
                    PathCommand::CubicTo {
                        cx1: rx + kx,
                        cy1: 0.,
                        cx2: *width,
                        cy2: ry - ky,
                        x: *width,
                        y: ry,
                    },
                    PathCommand::CubicTo {
                        cx1: *width,
                        cy1: ry + ky,
                        cx2: rx + kx,
                        cy2: *height,
                        x: rx,
                        y: *height,
                    },
                    PathCommand::CubicTo {
                        cx1: rx - kx,
                        cy1: *height,
                        cx2: 0.,
                        cy2: ry + ky,
                        x: 0.,
                        y: ry,
                    },
                    PathCommand::CubicTo {
                        cx1: 0.,
                        cy1: ry - ky,
                        cx2: rx - kx,
                        cy2: 0.,
                        x: rx,
                        y: 0.,
                    },
                    PathCommand::Close,
                ],
                FillRule::NonZero,
                true,
            ))
        }
        ElementKind::Path {
            commands,
            fill_rule,
            ..
        }
        | ElementKind::PaintedPath {
            commands,
            fill_rule,
            ..
        } => Some((
            commands.clone(),
            *fill_rule,
            commands.iter().any(|c| matches!(c, PathCommand::Close)),
        )),
        _ => None,
    }
}
fn geometry_contains(
    commands: &[PathCommand],
    rule: FillRule,
    width: Option<f64>,
    point: Pos2,
) -> bool {
    let Ok(mesh) = crate::vector::tessellate(commands, rule, width, 0.3) else {
        return false;
    };
    mesh.indices.as_chunks::<3>().0.iter().any(|indices| {
        let p: Vec<_> = indices
            .iter()
            .map(|index| mesh.vertices[*index as usize])
            .collect();
        let cross = |a: usize, b: usize| {
            (p[b].x - p[a].x) * (point.y - p[a].y) - (p[b].y - p[a].y) * (point.x - p[a].x)
        };
        let (a, b, c) = (cross(0, 1), cross(1, 2), cross(2, 0));
        (a >= -0.001 && b >= -0.001 && c >= -0.001) || (a <= 0.001 && b <= 0.001 && c <= 0.001)
    })
}
struct PaintHit {
    target: SelectionTarget,
    kind: ElementKind,
    local: Pos2,
    stroke: bool,
    matrix: [f64; 6],
}

impl Editor {
    fn tool_paint_hit(
        &self,
        scene: &[DrawItem],
        p: &egui::Painter,
        point: Pos2,
        closed_only: bool,
    ) -> Option<PaintHit> {
        let eligible: HashMap<_, _> = self
            .editable_targets()
            .into_iter()
            .map(|target| (target.element.clone(), target))
            .collect();
        let authored = evaluate_scene(
            &self.project,
            self.symbol_id(),
            self.frame,
            true,
            self.show_layer_masking(p.ctx()),
        );
        scene.iter().rev().find_map(|item| {
            if item.instance_path.len() != 1 {
                return None;
            }
            let target = eligible.get(item.instance_path.first()?)?;
            if !authored.contains_point(&item.instance_path, [point.x as f64, point.y as f64]) {
                return None;
            }
            let local = inverse(item.transform, point)?;
            let (commands, rule, closed) = path_geometry(&item.kind)?;
            let (_, stroke, width) = paint_parts(&item.kind)?;
            let outline = stroke[3] > 0
                && width > 0.
                && geometry_contains(
                    &commands,
                    rule,
                    Some(width.max(5. / self.zoom as f64)),
                    local,
                );
            let inside = closed && geometry_contains(&commands, rule, None, local);
            ((inside || outline) && (!closed_only || closed)).then(|| PaintHit {
                target: target.clone(),
                kind: item.kind.clone(),
                local,
                stroke: outline,
                matrix: item.transform,
            })
        })
    }
    fn change_tool_elements(&mut self, changes: Vec<(SelectionTarget, Option<ElementKind>)>) {
        if changes.is_empty() {
            return;
        }
        self.transact(|editor| {
            let before = editor.project.clone();
            for (target, kind) in changes {
                if !editor.target_editable(&target) {
                    continue;
                }
                editor.ensure_key_for(&target.layer, false);
                let Some(index) = editor.target_layer_index(&target) else {
                    continue;
                };
                let frame = editor.frame;
                let elements = &mut editor.symbol_mut().timeline.layers[index]
                    .key_at_mut(frame)
                    .unwrap()
                    .elements;
                if let Some(kind) = kind {
                    if let Some(element) = elements.iter_mut().find(|e| e.id == target.element) {
                        element.kind = kind;
                    }
                } else {
                    elements.retain(|e| e.id != target.element);
                }
            }
            if let Err(error) = editor.project.validate() {
                editor.project = before;
                editor.message(error.to_string());
            }
        });
        self.normalize_targets();
    }
    fn paint_tool_click(&mut self, scene: &[DrawItem], p: &egui::Painter, point: Pos2, alt: bool) {
        let tool = self.tool;
        let Some(hit) = self.tool_paint_hit(scene, p, point, tool == Tool::Bucket) else {
            self.message(if tool==Tool::Bucket {"Paint Bucket fills existing closed vector shapes. Close an open path first; enter a symbol to edit its artwork."} else {"Choose vector artwork on an unlocked layer; enter a symbol to edit its artwork."});
            return;
        };
        let (fill, stroke, width) = paint_parts(&hit.kind).unwrap();
        if tool == Tool::Eyedropper {
            if hit.stroke || alt {
                self.stroke = if let ElementKind::PaintedPath {
                    stroke_gradient: Some(gradient),
                    ..
                } = &hit.kind
                {
                    gradient.sample(hit.local.x as f64, hit.local.y as f64)
                } else {
                    stroke
                };
                self.stroke_width = width;
                self.set_tool(Tool::Ink);
            } else {
                self.fill = paint_gradient(&hit.kind)
                    .map(|g| g.sample(hit.local.x as f64, hit.local.y as f64))
                    .unwrap_or(fill);
                self.set_tool(Tool::Bucket);
            }
            return;
        }
        let mut kind = hit.kind;
        apply_paint(
            &mut kind,
            (tool == Tool::Bucket).then_some(self.fill),
            (tool == Tool::Ink).then_some((self.stroke, self.stroke_width)),
        );
        self.change_tool_elements(vec![(hit.target.clone(), Some(kind))]);
        self.set_selection(vec![hit.target]);
    }
    /// Returns true when extended tools/temporary Hand own this pointer phase.
    pub(crate) fn authoring_tool_stage(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        page: Rect,
        view: Rect,
        scene: &[DrawItem],
    ) -> bool {
        let space = ui.input(|i| i.key_down(egui::Key::Space)) && !ui.ctx().wants_keyboard_input();
        let owned = extended(self.tool) || space || self.authoring_tools.gesture.is_some();
        if !owned {
            return false;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape) || !i.focused) {
            self.authoring_tools.cancel();
            return true;
        }
        let stale = self.authoring_tools.gesture.as_ref().is_some_and(|g| {
            g.symbol != self.symbol_id()
                || g.frame != self.frame
                || (g.tool != Tool::Hand && (g.origin != origin || g.zoom != self.zoom))
        });
        if stale {
            self.authoring_tools.cancel();
        }
        for event in ui.input(|i| i.events.clone()) {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                } if view.contains(pos)
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == ui.layer_id()) =>
                {
                    let tool = if space { Tool::Hand } else { self.tool };
                    if !matches!(tool, Tool::Hand | Tool::Zoom | Tool::Lasso)
                        && (!page.contains(pos)
                            || (matches!(tool, Tool::Line | Tool::Brush) && !self.layer_editable()))
                    {
                        continue;
                    }
                    if tool == Tool::Hand {
                        self.end_vector_pointer_phase();
                        self.cancel_selection_gesture();
                        self.shape_draft = None;
                        if self.vector.point_drag.is_some() {
                            self.cancel_vector();
                        }
                    } else {
                        self.finish_edits();
                    }
                    self.playing = false;
                    let local = Pos2::ZERO + (pos - origin) / self.zoom;
                    if matches!(tool, Tool::Bucket | Tool::Ink | Tool::Eyedropper) {
                        self.paint_tool_click(scene, p, local, modifiers.alt);
                        continue;
                    }
                    let gradient = if tool == Tool::Gradient {
                        // A fill's transform handle may lie outside its artwork.
                        let selected_handle = self.selected_sampled().and_then(|element| {
                            let fill = paint_gradient(&element.kind)?;
                            let matrix = transform_matrix(element.transform);
                            let edge = affine(matrix, affine(fill.matrix, Pos2::new(819.2, 0.)));
                            let center = affine(
                                matrix,
                                Pos2::new(fill.matrix[4] as f32, fill.matrix[5] as f32),
                            );
                            if (origin + edge.to_vec2() * self.zoom).distance(pos) >= 12.
                                && (origin + center.to_vec2() * self.zoom).distance(pos) >= 12.
                            {
                                return None;
                            }
                            Some(PaintHit {
                                target: self.selection.single()?.clone(),
                                kind: element.kind,
                                local: inverse(matrix, local)?,
                                stroke: false,
                                matrix,
                            })
                        });
                        let Some(hit) =
                            selected_handle.or_else(|| self.tool_paint_hit(scene, p, local, true))
                        else {
                            continue;
                        };
                        let Some(gradient) = paint_gradient(&hit.kind) else {
                            self.message("Gradient Transform edits an existing gradient fill. Select imported gradient artwork.");
                            continue;
                        };
                        let matrix = gradient.matrix;
                        let edge = affine(hit.matrix, affine(matrix, Pos2::new(819.2, 0.)));
                        let handle = (origin + edge.to_vec2() * self.zoom).distance(pos) < 12.;
                        self.set_selection(vec![hit.target.clone()]);
                        Some(GradientGesture {
                            target: hit.target,
                            kind: hit.kind,
                            matrix,
                            element_matrix: hit.matrix,
                            handle,
                        })
                    } else {
                        None
                    };
                    self.authoring_tools.gesture = Some(Gesture {
                        tool,
                        symbol: self.symbol_id().to_owned(),
                        frame: self.frame,
                        layer: self.symbol().timeline.layers[self.layer].id.clone(),
                        origin,
                        zoom: self.zoom,
                        pan: self.pan,
                        start: pos,
                        latest: pos,
                        points: vec![local],
                        shift: modifiers.shift,
                        alt: modifiers.alt,
                        targets: vec![],
                        gradient,
                    });
                    self.update_authoring_gesture(pos, scene, p);
                }
                egui::Event::PointerMoved(pos) => self.update_authoring_gesture(pos, scene, p),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    self.update_authoring_gesture(pos, scene, p);
                    if let Some(gesture) = self.authoring_tools.gesture.take() {
                        self.finish_authoring_gesture(gesture, scene, p, view);
                    }
                }
                _ => {}
            }
        }
        self.paint_authoring_gesture(p, origin, view, scene);
        if let Some(pos) = ui.input(|i| i.pointer.latest_pos())
            && view.contains(pos)
        {
            let tool = if space { Tool::Hand } else { self.tool };
            ui.ctx().set_cursor_icon(match tool {
                Tool::Hand => {
                    if self.authoring_tools.gesture.is_some() {
                        egui::CursorIcon::Grabbing
                    } else {
                        egui::CursorIcon::Grab
                    }
                }
                Tool::Zoom => {
                    if ui.input(|i| i.modifiers.alt) || self.authoring_tools.zoom_out {
                        egui::CursorIcon::ZoomOut
                    } else {
                        egui::CursorIcon::ZoomIn
                    }
                }
                _ => egui::CursorIcon::Crosshair,
            });
            if matches!(tool, Tool::Brush | Tool::Eraser) {
                let radius = if tool == Tool::Brush {
                    self.authoring_tools.brush_size
                } else {
                    self.authoring_tools.eraser_size
                } / 2.;
                p.circle_stroke(
                    pos,
                    radius,
                    Stroke::new(3.0_f32, UiPalette::current(ui.ctx()).overlay_halo),
                );
                p.circle_stroke(
                    pos,
                    radius,
                    Stroke::new(1.0_f32, UiPalette::current(ui.ctx()).overlay),
                );
            }
        }
        owned
    }
    fn update_authoring_gesture(&mut self, pos: Pos2, scene: &[DrawItem], p: &egui::Painter) {
        let Some(mut g) = self.authoring_tools.gesture.take() else {
            return;
        };
        g.latest = pos;
        let mut point = g.local(pos);
        if g.tool == Tool::Line && g.shift {
            let vector = point - g.points[0];
            let angle = (vector.angle() / std::f32::consts::FRAC_PI_4).round()
                * std::f32::consts::FRAC_PI_4;
            point = g.points[0] + Vec2::angled(angle) * vector.length();
        }
        match g.tool {
            Tool::Hand => {
                self.pan = g.pan + (pos - g.start);
                p.ctx().request_repaint();
            }
            Tool::Line => {
                g.points.truncate(1);
                g.points.push(point);
            }
            Tool::Brush | Tool::Lasso => {
                if g.points
                    .last()
                    .is_none_or(|last| last.distance(point) * g.zoom >= 1.5)
                {
                    if g.points.len() >= 2048 {
                        self.message("Gesture exceeds 2048 points and was cancelled. Draw a shorter stroke or loop.");
                        return;
                    }
                    g.points.push(point);
                }
            }
            Tool::Eraser => {
                let radius = self.authoring_tools.eraser_size / 2. / g.zoom;
                for delta in [
                    Vec2::ZERO,
                    Vec2::new(radius, 0.),
                    Vec2::new(-radius, 0.),
                    Vec2::new(0., radius),
                    Vec2::new(0., -radius),
                ] {
                    if let Some(target) = self.pick_placement(scene, p, point + delta)
                        && !g.targets.contains(&target)
                    {
                        g.targets.push(target);
                    }
                }
            }
            _ => {}
        }
        self.authoring_tools.gesture = Some(g);
    }
    fn finish_authoring_gesture(
        &mut self,
        g: Gesture,
        scene: &[DrawItem],
        p: &egui::Painter,
        view: Rect,
    ) {
        if g.symbol != self.symbol_id() || g.frame != self.frame {
            return;
        }
        match g.tool {
            Tool::Line | Tool::Brush => {
                let Some(layer) = self
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .position(|layer| layer.id == g.layer)
                else {
                    return;
                };
                self.layer = layer;
                if !self.layer_editable() {
                    return;
                }
                let commands = if g.tool == Tool::Brush {
                    brush_outline(&g.points, self.authoring_tools.brush_size / g.zoom)
                } else {
                    polyline(&g.points, false)
                };
                if commands.len() < 2
                    || (g.tool == Tool::Line
                        && g.points[0].distance(*g.points.last().unwrap()) < 0.1)
                {
                    return;
                }
                if commands.len() > 100_000 {
                    self.message("Stroke is too complex; draw shorter brush strokes.");
                    return;
                }
                self.add_element_transformed(
                    ElementKind::Path {
                        commands,
                        fill_rule: FillRule::NonZero,
                        fill: if g.tool == Tool::Brush {
                            self.fill
                        } else {
                            [0; 4]
                        },
                        stroke: if g.tool == Tool::Line {
                            self.stroke
                        } else {
                            [0; 4]
                        },
                        stroke_width: if g.tool == Tool::Line {
                            self.stroke_width
                        } else {
                            0.
                        },
                    },
                    Transform::default(),
                );
                self.tool = g.tool;
            }
            Tool::Eraser => {
                let mode = self.authoring_tools.eraser_mode;
                let mut changes = vec![];
                for target in g.targets {
                    let Some(index) = self.target_layer_index(&target) else {
                        continue;
                    };
                    let Some(element) =
                        sample_layer(&self.symbol().timeline.layers[index], self.frame)
                            .into_iter()
                            .find(|e| e.id == target.element)
                    else {
                        continue;
                    };
                    let kind = if mode == EraserMode::Objects {
                        None
                    } else {
                        if paint_parts(&element.kind).is_none() {
                            continue;
                        }
                        let mut kind = element.kind;
                        apply_paint(
                            &mut kind,
                            (mode == EraserMode::Fills).then_some([0; 4]),
                            (mode == EraserMode::Strokes).then_some(([0; 4], 0.)),
                        );
                        Some(kind)
                    };
                    changes.push((target, kind));
                }
                self.change_tool_elements(changes);
            }
            Tool::Lasso => {
                if g.points.len() < 3 {
                    return;
                }
                let eligible: HashMap<_, _> = self
                    .editable_targets()
                    .into_iter()
                    .map(|t| (t.element.clone(), t))
                    .collect();
                let mut boxes = HashMap::<Id, Rect>::new();
                for item in scene {
                    if let Some(id) = item.instance_path.first()
                        && eligible.contains_key(id)
                    {
                        let bounds = crate::stage::world_bounds(item, p);
                        boxes
                            .entry(id.clone())
                            .and_modify(|r| *r = r.union(bounds))
                            .or_insert(bounds);
                    }
                }
                let mut targets = if g.shift {
                    self.selection.targets.clone()
                } else {
                    vec![]
                };
                targets.extend(
                    scene
                        .iter()
                        .filter_map(|item| item.instance_path.first())
                        .filter(|id| {
                            boxes.get(*id).is_some_and(|bounds| {
                                lasso_rect(&g.points, *bounds, self.authoring_tools.lasso_contact)
                            })
                        })
                        .filter_map(|id| eligible.get(id).cloned()),
                );
                self.set_selection(targets);
            }
            Tool::Zoom => {
                let region = Rect::from_two_pos(g.start, g.latest);
                let zoom_out = g.alt || self.authoring_tools.zoom_out;
                let previous = self.zoom;
                let next = if !zoom_out && region.width() > 6. && region.height() > 6. {
                    (previous
                        * (view.width() / region.width()).min(view.height() / region.height()))
                    .clamp(0.08, 20.)
                } else {
                    (previous * if zoom_out { 0.5 } else { 2. }).clamp(0.08, 20.)
                };
                let anchor = if !zoom_out && region.width() > 6. && region.height() > 6. {
                    region.center()
                } else {
                    g.latest
                };
                let destination = if !zoom_out && region.width() > 6. && region.height() > 6. {
                    view.center()
                } else {
                    anchor
                };
                self.pan = destination
                    - view.center()
                    - (anchor - view.center() - g.pan) * (next / previous);
                self.zoom = next;
            }
            Tool::Gradient => {
                if let Some(gradient) = &g.gradient
                    && let Some(matrix) = gradient_matrix(&g, gradient)
                {
                    let mut kind = gradient.kind.clone();
                    if let ElementKind::PaintedPath {
                        fill_gradient: Some(fill),
                        ..
                    } = &mut kind
                    {
                        fill.matrix = matrix;
                    }
                    self.change_tool_elements(vec![(gradient.target.clone(), Some(kind))]);
                }
            }
            _ => {}
        }
    }
    fn paint_authoring_gesture(
        &self,
        p: &egui::Painter,
        origin: Pos2,
        view: Rect,
        scene: &[DrawItem],
    ) {
        let palette = UiPalette::current(p.ctx());
        if let Some(g) = &self.authoring_tools.gesture {
            let screen = |point: Pos2| g.origin + point.to_vec2() * g.zoom;
            match g.tool {
                Tool::Line | Tool::Brush => {
                    let color = if g.tool == Tool::Brush {
                        self.fill
                    } else {
                        self.stroke
                    };
                    let width = if g.tool == Tool::Brush {
                        self.authoring_tools.brush_size
                    } else {
                        self.stroke_width as f32 * g.zoom
                    };
                    let stroke = Stroke::new(
                        width.max(1.),
                        Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
                    );
                    if g.points.len() == 1 {
                        p.circle_filled(screen(g.points[0]), width / 2., stroke.color);
                    } else {
                        p.add(Shape::line(
                            g.points.iter().map(|point| screen(*point)).collect(),
                            stroke,
                        ));
                    }
                }
                Tool::Lasso => {
                    p.add(Shape::closed_line(
                        g.points.iter().map(|point| screen(*point)).collect(),
                        Stroke::new(1.5_f32, palette.overlay),
                    ));
                }
                Tool::Zoom => {
                    p.rect_stroke(
                        Rect::from_two_pos(g.start, g.latest).intersect(view),
                        0,
                        Stroke::new(1.5_f32, palette.overlay),
                        StrokeKind::Inside,
                    );
                }
                Tool::Eraser => {
                    for item in scene {
                        if item
                            .instance_path
                            .first()
                            .is_some_and(|id| g.targets.iter().any(|target| &target.element == id))
                        {
                            let bounds = crate::stage::world_bounds(item, p);
                            let bounds = Rect::from_min_max(screen(bounds.min), screen(bounds.max));
                            p.rect_stroke(
                                bounds,
                                0,
                                Stroke::new(1.5_f32, palette.error_text),
                                StrokeKind::Inside,
                            );
                        }
                    }
                }
                Tool::Gradient => {
                    if let Some(gradient) = &g.gradient
                        && let Some(matrix) = gradient_matrix(g, gradient)
                    {
                        paint_gradient_handles(
                            p,
                            g.origin,
                            g.zoom,
                            gradient.element_matrix,
                            matrix,
                        );
                    }
                }
                _ => {}
            }
        } else if self.tool == Tool::Gradient
            && let Some(element) = self.selected_sampled()
            && let Some(fill) = paint_gradient(&element.kind)
        {
            paint_gradient_handles(
                p,
                origin,
                self.zoom,
                transform_matrix(element.transform),
                fill.matrix,
            );
        }
    }
}
fn polyline(points: &[Pos2], closed: bool) -> Vec<PathCommand> {
    let Some(first) = points.first() else {
        return vec![];
    };
    let mut commands = vec![PathCommand::MoveTo {
        x: first.x as f64,
        y: first.y as f64,
    }];
    commands.extend(points.iter().skip(1).map(|p| PathCommand::LineTo {
        x: p.x as f64,
        y: p.y as f64,
    }));
    if closed {
        commands.push(PathCommand::Close);
    }
    commands
}
fn brush_outline(points: &[Pos2], width: f32) -> Vec<PathCommand> {
    if points.len() == 1 {
        return polyline(
            &(0..24)
                .map(|n| {
                    points[0] + Vec2::angled(n as f32 * std::f32::consts::TAU / 24.) * width / 2.
                })
                .collect::<Vec<_>>(),
            true,
        );
    }
    let Ok(mesh) = crate::vector::tessellate(
        &polyline(points, false),
        FillRule::NonZero,
        Some(width as f64),
        0.3,
    ) else {
        return vec![];
    };
    let mut commands = Vec::with_capacity(mesh.indices.len() / 3 * 4);
    for triangle in mesh.indices.as_chunks::<3>().0 {
        for (index, vertex) in triangle.iter().enumerate() {
            let p = mesh.vertices[*vertex as usize];
            commands.push(if index == 0 {
                PathCommand::MoveTo {
                    x: p.x as f64,
                    y: p.y as f64,
                }
            } else {
                PathCommand::LineTo {
                    x: p.x as f64,
                    y: p.y as f64,
                }
            });
        }
        commands.push(PathCommand::Close);
    }
    commands
}
fn inside_polygon(points: &[Pos2], point: Pos2) -> bool {
    let mut inside = false;
    for (a, b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        if (a.y > point.y) != (b.y > point.y)
            && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
    }
    inside
}
fn lasso_rect(points: &[Pos2], rect: Rect, contact: bool) -> bool {
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    if !contact {
        return corners.iter().all(|p| inside_polygon(points, *p));
    }
    if corners.iter().any(|p| inside_polygon(points, *p))
        || points.iter().any(|p| rect.contains(*p))
    {
        return true;
    }
    let cross = |a: Pos2, b: Pos2, c: Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .any(|(a, b)| {
            corners
                .iter()
                .zip(corners.iter().cycle().skip(1))
                .take(4)
                .any(|(c, d)| {
                    cross(*a, *b, *c) * cross(*a, *b, *d) <= 0.
                        && cross(*c, *d, *a) * cross(*c, *d, *b) <= 0.
                        && Rect::from_two_pos(*a, *b).intersects(Rect::from_two_pos(*c, *d))
                })
        })
}
fn gradient_matrix(g: &Gesture, gradient: &GradientGesture) -> Option<[f64; 6]> {
    let start = inverse(gradient.element_matrix, g.local(g.start))?;
    let current = inverse(gradient.element_matrix, g.local(g.latest))?;
    let mut matrix = gradient.matrix;
    if gradient.handle {
        let center = Pos2::new(matrix[4] as f32, matrix[5] as f32);
        let from = start - center;
        let to = current - center;
        if from.length() < 0.001 || to.length() < 0.001 {
            return None;
        }
        let angle = to.angle() - from.angle();
        let angle = if g.shift {
            (angle / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4
        } else {
            angle
        };
        let scale = (to.length() / from.length()).clamp(0.01, 100.) as f64;
        let (sin, cos) = (angle as f64).sin_cos();
        for offset in [0, 2] {
            let x = matrix[offset];
            let y = matrix[offset + 1];
            matrix[offset] = (x * cos - y * sin) * scale;
            matrix[offset + 1] = (x * sin + y * cos) * scale;
        }
    } else {
        matrix[4] += (current.x - start.x) as f64;
        matrix[5] += (current.y - start.y) as f64;
    }
    Some(matrix)
}
fn paint_gradient_handles(
    p: &egui::Painter,
    origin: Pos2,
    zoom: f32,
    element: [f64; 6],
    gradient: [f64; 6],
) {
    let screen = |local| origin + affine(element, affine(gradient, local)).to_vec2() * zoom;
    let center = screen(Pos2::ZERO);
    let edge = screen(Pos2::new(819.2, 0.));
    let palette = UiPalette::current(p.ctx());
    p.line_segment([center, edge], Stroke::new(3.0_f32, palette.overlay_halo));
    p.line_segment([center, edge], Stroke::new(1.0_f32, palette.overlay));
    p.circle(
        center,
        5.,
        palette.handle_fill,
        Stroke::new(1.5_f32, palette.overlay),
    );
    p.rect(
        Rect::from_center_size(edge, Vec2::splat(9.)),
        0,
        palette.handle_fill,
        Stroke::new(1.5_f32, palette.overlay),
        StrokeKind::Inside,
    );
}

impl Editor {
    pub(crate) fn authoring_tool_options(&mut self, ui: &mut egui::Ui) {
        match self.tool {
            Tool::Brush => {
                ui.small("Brush size");
                ui.add_sized(
                    [ui.available_width(), 24.],
                    egui::DragValue::new(&mut self.authoring_tools.brush_size)
                        .range(1. ..=128.)
                        .suffix(" px"),
                );
                ui.small("Round brush · uses Fill").on_hover_text("Size stays constant on screen for new strokes, as in classic Flash. Existing artwork is unchanged by zoom.");
            }
            Tool::Eraser => {
                ui.small("Erase");
                egui::ComboBox::from_id_salt("eraser_mode")
                    .width(ui.available_width())
                    .selected_text(match self.authoring_tools.eraser_mode {
                        EraserMode::Objects => "Objects",
                        EraserMode::Fills => "Fills",
                        EraserMode::Strokes => "Strokes",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.authoring_tools.eraser_mode,
                            EraserMode::Objects,
                            "Whole objects",
                        );
                        ui.selectable_value(
                            &mut self.authoring_tools.eraser_mode,
                            EraserMode::Fills,
                            "Whole fills",
                        );
                        ui.selectable_value(
                            &mut self.authoring_tools.eraser_mode,
                            EraserMode::Strokes,
                            "Whole strokes",
                        );
                    });
                ui.small("Size");
                ui.add_sized(
                    [ui.available_width(), 24.],
                    egui::DragValue::new(&mut self.authoring_tools.eraser_size)
                        .range(1. ..=128.)
                        .suffix(" px"),
                );
                ui.small("Drag over artwork").on_hover_text("Removes touched objects or their entire fill/stroke. This mode does not cut holes into vector geometry.");
            }
            Tool::Lasso => {
                ui.checkbox(&mut self.authoring_tools.lasso_contact,"Touch selects").on_hover_text("Select whole placements whose bounds touch the freehand loop; turn off to require enclosure. Hold Shift to add.");
            }
            Tool::Zoom => {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.authoring_tools.zoom_out, false, "+");
                    ui.selectable_value(&mut self.authoring_tools.zoom_out, true, "−");
                });
                ui.small("Click or drag area").on_hover_text(
                    "Option temporarily zooms out. Drag a rectangle to fit that area.",
                );
            }
            Tool::Gradient => {
                ui.small("Existing fill");
                ui.small("Center: move");
                ui.small("Square: scale / rotate");
            }
            Tool::Bucket => {
                ui.small("Closed shapes").on_hover_text("Fills a closed authored vector shape. Open paths and pixel flood fill are not supported.");
            }
            Tool::Ink => {
                ui.small("Apply stroke").on_hover_text(
                    "Click vector artwork to apply the current stroke color and width.",
                );
            }
            Tool::Eyedropper => {
                ui.small("Sample color").on_hover_text("Click a stroke or fill, preserving alpha. Switches to Ink Bottle or Paint Bucket. Option chooses the stroke.");
            }
            Tool::Line => {
                ui.small("Shift: 45°");
            }
            Tool::Hand => {
                ui.small("Drag to pan");
                ui.small("Space: temporary");
            }
            _ => {}
        }
    }
}
