//! Vector geometry rendering and direct authoring. Draft gestures are not document state.
use crate::app::{Editor, Tool};
use crate::theme::UiPalette;
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2};
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
    math::{Point, point},
    path::Path,
};
use neoflash_core::*;

pub(crate) fn tessellate(
    commands: &[PathCommand],
    rule: FillRule,
    width: Option<f64>,
    tolerance: f32,
) -> Result<VertexBuffers<Point, u32>, String> {
    let mut builder = Path::builder().with_svg();
    for command in commands {
        match *command {
            PathCommand::MoveTo { x, y } => {
                builder.move_to(point(x as f32, y as f32));
            }
            PathCommand::LineTo { x, y } => {
                builder.line_to(point(x as f32, y as f32));
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                builder.quadratic_bezier_to(point(cx as f32, cy as f32), point(x as f32, y as f32));
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                builder.cubic_bezier_to(
                    point(cx1 as f32, cy1 as f32),
                    point(cx2 as f32, cy2 as f32),
                    point(x as f32, y as f32),
                );
            }
            PathCommand::Close => builder.close(),
        }
    }
    let path = builder.build();
    let mut mesh = VertexBuffers::<Point, u32>::new();
    if let Some(width) = width {
        StrokeTessellator::new()
            .tessellate_path(
                &path,
                &StrokeOptions::default()
                    .with_line_width(width as f32)
                    .with_line_cap(lyon_tessellation::LineCap::Round)
                    .with_line_join(lyon_tessellation::LineJoin::Round)
                    .with_tolerance(tolerance),
                &mut BuffersBuilder::new(&mut mesh, |v: StrokeVertex| v.position()),
            )
            .map_err(|e| format!("Stroke tessellation: {e:?}"))?;
    } else {
        FillTessellator::new()
            .tessellate_path(
                &path,
                &FillOptions::default()
                    .with_tolerance(tolerance)
                    .with_fill_rule(match rule {
                        FillRule::NonZero => lyon_tessellation::FillRule::NonZero,
                        FillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
                    }),
                &mut BuffersBuilder::new(&mut mesh, |v: FillVertex| v.position()),
            )
            .map_err(|e| format!("Fill tessellation: {e:?}"))?;
    }
    Ok(mesh)
}

pub(crate) struct PathPaint {
    pub rule: FillRule,
    pub fill: Color32,
    pub stroke: Color32,
    pub width: f64,
    pub tolerance: f32,
}

pub(crate) fn paint_path(
    p: &egui::Painter,
    commands: &[PathCommand],
    paint: PathPaint,
    screen: impl Fn(Pos2) -> Pos2,
) {
    let PathPaint {
        rule,
        fill,
        stroke,
        width,
        tolerance,
    } = paint;
    for (c, width) in [(fill, None), (stroke, Some(width))] {
        if c.a() == 0 || width.is_some_and(|w| w <= 0.) {
            continue;
        }
        match tessellate(commands, rule, width, tolerance) {
            Ok(geometry) => {
                let mut mesh = egui::epaint::Mesh::default();
                for vertex in geometry.vertices {
                    mesh.colored_vertex(screen(Pos2::new(vertex.x, vertex.y)), c);
                }
                mesh.indices = geometry.indices;
                p.add(Shape::mesh(mesh));
            }
            Err(message) => {
                p.text(
                    screen(Pos2::ZERO),
                    egui::Align2::LEFT_TOP,
                    message,
                    egui::FontId::monospace(crate::theme::PANEL_FONT),
                    UiPalette::current(p.ctx()).error_text,
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub point: Pos2,
    pub tangent: Vec2,
}
#[derive(Clone, Debug)]
pub(crate) struct PathDraft {
    pub anchors: Vec<Anchor>,
    pub pencil: bool,
    pub dragging: bool,
    pub closing: bool,
    pub limit_reached: bool,
    pub symbol: String,
    pub frame: u32,
    pub layer: usize,
    pub layer_id: String,
    /// View captured for the current press/drag phase. Pen anchors may span
    /// several phases and views; an individual pointer drag may not.
    pub pointer_origin: Pos2,
    pub pointer_zoom: f32,
}
impl PathDraft {
    pub fn commands(&self, closed: bool) -> Vec<PathCommand> {
        let Some(first) = self.anchors.first() else {
            return vec![];
        };
        let mut commands = vec![PathCommand::MoveTo {
            x: first.point.x as f64,
            y: first.point.y as f64,
        }];
        let mut previous = first;
        for next in self
            .anchors
            .iter()
            .skip(1)
            .chain(if closed { Some(first) } else { None })
        {
            if previous.tangent.length_sq() > 0.01 || next.tangent.length_sq() > 0.01 {
                let c1 = previous.point + previous.tangent;
                let c2 = next.point - next.tangent;
                commands.push(PathCommand::CubicTo {
                    cx1: c1.x as f64,
                    cy1: c1.y as f64,
                    cx2: c2.x as f64,
                    cy2: c2.y as f64,
                    x: next.point.x as f64,
                    y: next.point.y as f64,
                });
            } else {
                commands.push(PathCommand::LineTo {
                    x: next.point.x as f64,
                    y: next.point.y as f64,
                });
            }
            previous = next;
        }
        if closed {
            commands.push(PathCommand::Close);
        }
        commands
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PointRole {
    Anchor,
    Control1,
    Control2,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct PathHandle {
    pub command: usize,
    pub role: PointRole,
    pub point: Pos2,
}
#[derive(Clone, Debug)]
pub(crate) struct PointDrag {
    pub handle: PathHandle,
    pub original: Vec<PathCommand>,
    pub inverse: [f64; 6],
    pub symbol: String,
    pub frame: u32,
    pub layer: String,
    pub origin: Pos2,
    pub zoom: f32,
    pub press: Pos2,
    pub began: bool,
    pub changed: bool,
    pub element: String,
}
#[derive(Default)]
pub(crate) struct VectorState {
    pub draft: Option<PathDraft>,
    pub point_drag: Option<PointDrag>,
}

pub(crate) fn handles(commands: &[PathCommand]) -> Vec<PathHandle> {
    let mut out = vec![];
    for (i, command) in commands.iter().enumerate() {
        let mut add = |role, x: f64, y: f64| {
            out.push(PathHandle {
                command: i,
                role,
                point: Pos2::new(x as f32, y as f32),
            })
        };
        match *command {
            PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                add(PointRole::Anchor, x, y)
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                add(PointRole::Anchor, x, y);
                add(PointRole::Control1, cx, cy);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                add(PointRole::Anchor, x, y);
                add(PointRole::Control1, cx1, cy1);
                add(PointRole::Control2, cx2, cy2);
            }
            PathCommand::Close => {}
        }
    }
    out
}

/// Move an anchor with its incident controls, including the duplicate endpoint of
/// a curved closing segment. A shared quadratic control is translated only once.
pub(crate) fn move_handle(commands: &mut [PathCommand], handle: PathHandle, to: Pos2) {
    let delta = to - handle.point;
    let dx = delta.x as f64;
    let dy = delta.y as f64;
    let mut anchors = vec![handle.command];
    if handle.role == PointRole::Anchor {
        let start = (0..=handle.command)
            .rev()
            .find(|&i| matches!(commands.get(i), Some(PathCommand::MoveTo { .. })));
        let close = (handle.command..commands.len())
            .take_while(|&i| {
                i == handle.command || !matches!(commands[i], PathCommand::MoveTo { .. })
            })
            .find(|&i| matches!(commands[i], PathCommand::Close));
        if let (Some(start), Some(close)) = (start, close)
            && close > start + 1
        {
            let last = close - 1;
            if handle.command == start || handle.command == last {
                let other = if handle.command == start { last } else { start };
                if handles(commands).iter().any(|h| {
                    h.command == other
                        && h.role == PointRole::Anchor
                        && h.point.distance_sq(handle.point) < 1e-8
                }) {
                    anchors.push(other);
                }
            }
        }
    }
    let mut targets = vec![(handle.command, handle.role)];
    if handle.role == PointRole::Anchor {
        for index in anchors {
            targets.push((index, PointRole::Anchor));
            match commands.get(index) {
                Some(PathCommand::QuadraticTo { .. }) => targets.push((index, PointRole::Control1)),
                Some(PathCommand::CubicTo { .. }) => targets.push((index, PointRole::Control2)),
                _ => {}
            }
            if matches!(
                commands.get(index + 1),
                Some(PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. })
            ) {
                targets.push((index + 1, PointRole::Control1));
            }
        }
    }
    let mut applied = Vec::new();
    for target in targets {
        if applied.contains(&target) {
            continue;
        }
        applied.push(target);
        let (index, role) = target;
        match (commands.get_mut(index), role) {
            (
                Some(
                    PathCommand::MoveTo { x, y }
                    | PathCommand::LineTo { x, y }
                    | PathCommand::QuadraticTo { x, y, .. }
                    | PathCommand::CubicTo { x, y, .. },
                ),
                PointRole::Anchor,
            ) => {
                *x += dx;
                *y += dy;
            }
            (Some(PathCommand::QuadraticTo { cx, cy, .. }), PointRole::Control1) => {
                *cx += dx;
                *cy += dy;
            }
            (Some(PathCommand::CubicTo { cx1, cy1, .. }), PointRole::Control1) => {
                *cx1 += dx;
                *cy1 += dy;
            }
            (Some(PathCommand::CubicTo { cx2, cy2, .. }), PointRole::Control2) => {
                *cx2 += dx;
                *cy2 += dy;
            }
            _ => {}
        }
    }
}

/// Iterative Ramer–Douglas–Peucker keeps pencil geometry compact without recursion.
fn simplify(points: &[Anchor], tolerance: f32) -> Vec<Anchor> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((start, end)) = stack.pop() {
        let a = points[start].point;
        let line = points[end].point - a;
        let mut best = tolerance * tolerance;
        let mut index = None;
        for (i, anchor) in points.iter().enumerate().take(end).skip(start + 1) {
            let t = if line.length_sq() > 0. {
                ((anchor.point - a).dot(line) / line.length_sq()).clamp(0., 1.)
            } else {
                0.
            };
            let distance = anchor.point.distance_sq(a + line * t);
            if distance > best {
                best = distance;
                index = Some(i);
            }
        }
        if let Some(i) = index {
            keep[i] = true;
            stack.push((start, i));
            stack.push((i, end));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, keep)| keep.then_some(*p))
        .collect()
}

impl Editor {
    /// End only the active pointer phase before changing the viewport. Pen
    /// keeps its existing anchors/controls for the next press; Pencil commits
    /// samples already collected in the old view. A later move/release cannot
    /// append geometry until a new primary press starts another phase.
    pub(crate) fn end_vector_pointer_phase(&mut self) {
        let finish_pencil = self.vector.draft.as_mut().is_some_and(|draft| {
            let active = draft.dragging || draft.closing;
            draft.dragging = false;
            draft.closing = false;
            draft.pencil && active
        });
        if finish_pencil {
            self.finish_path(false);
        }
    }

    pub(crate) fn finish_point_drag(&mut self) {
        if let Some(drag) = self.vector.point_drag.take()
            && drag.began
        {
            if drag.changed {
                self.history.commit(&self.project);
            } else {
                self.history.cancel(&mut self.project);
            }
        }
    }
    pub(crate) fn finish_edits(&mut self) {
        self.authoring_tools.cancel();
        self.finish_property_edit();
        self.finish_selection_gesture();
        self.shape_draft = None;
        self.finish_point_drag();
        if self.vector.draft.is_some() {
            self.finish_path(false);
        }
    }
    pub(crate) fn cancel_vector(&mut self) {
        if self
            .vector
            .point_drag
            .as_ref()
            .is_some_and(|drag| drag.began)
        {
            self.history.cancel(&mut self.project);
        }
        self.vector = Default::default();
    }
    pub(crate) fn cancel_edits(&mut self) {
        self.authoring_tools.cancel();
        self.finish_property_edit();
        self.cancel_selection_gesture();
        self.cancel_vector();
        self.shape_draft = None;
        self.timeline_key_drag = None;
    }
    pub(crate) fn set_playing(&mut self, playing: bool) {
        self.finish_edits();
        self.timeline_key_drag = None;
        self.playing = playing;
        self.accumulator = 0.;
        self.sync_authored_preview();
        self.last_tick = std::time::Instant::now();
    }
    pub(crate) fn set_tool(&mut self, tool: Tool) {
        if tool != self.tool {
            self.finish_edits();
            self.timeline_key_drag = None;
            self.vector = Default::default();
        }
        self.tool = tool;
    }
    pub(crate) fn finish_path(&mut self, closed: bool) {
        let Some(mut draft) = self.vector.draft.take() else {
            return;
        };
        if draft.limit_reached {
            self.message("Pencil stroke exceeds 16000 samples. Draw shorter strokes; this draft was discarded.");
            return;
        }
        if draft.pencil {
            draft.anchors = simplify(&draft.anchors, 0.8 / draft.pointer_zoom);
        }
        if draft.anchors.len() < 2 {
            return;
        }
        let target = self.project.symbol(&draft.symbol).and_then(|symbol| {
            (draft.frame < symbol.timeline.frame_count)
                .then(|| {
                    symbol.timeline.layers.iter().position(|layer| {
                        layer.id == draft.layer_id && layer.visible && !layer.locked
                    })
                })
                .flatten()
        });
        let Some(target_layer) = target else {
            self.message("Cannot finish path: its original layer/frame is no longer editable. Draft cancelled.");
            return;
        };
        let restore = (draft.symbol != self.symbol_id()
            || draft.frame != self.frame
            || target_layer != self.layer)
            .then(|| {
                (
                    self.editing.clone(),
                    self.frame,
                    self.layer,
                    self.selection.clone(),
                )
            });
        if restore.is_some() {
            self.editing = vec![draft.symbol.clone()];
            self.frame = draft.frame;
            self.layer = target_layer;
        }
        let commands = draft.commands(closed);
        let tool = self.tool;
        self.add_element(
            ElementKind::Path {
                commands,
                fill_rule: FillRule::NonZero,
                fill: if closed { self.fill } else { [0, 0, 0, 0] },
                stroke: self.stroke,
                stroke_width: self.stroke_width,
            },
            0.,
            0.,
        );
        self.tool = tool;
        if let Some((editing, frame, layer, selected)) = restore {
            self.editing = editing;
            self.frame = frame;
            self.layer = layer;
            self.selection = selected;
        }
        self.message(if closed {
            "Closed vector path created."
        } else {
            "Open vector path created. Use A to edit its points."
        });
    }

    pub(crate) fn vector_stage(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        page: Rect,
        response: &egui::Response,
    ) {
        let zoom = self.zoom;
        let local = |pos: Pos2| Pos2::ZERO + (pos - origin) / zoom;
        let screen = |pos: Pos2| origin + pos.to_vec2() * zoom;
        if self.vector.point_drag.as_ref().is_some_and(|d| {
            !self.point_drag_matches(d) || d.origin != origin || d.zoom != self.zoom
        }) {
            self.cancel_vector();
        }
        if self.vector.draft.as_ref().is_some_and(|draft| {
            (draft.dragging || draft.closing)
                && (draft.pointer_origin != origin || draft.pointer_zoom != zoom)
        }) {
            self.end_vector_pointer_phase();
        }
        if self.vector.draft.as_ref().is_some_and(|d| {
            d.symbol != self.symbol_id()
                || d.frame != self.frame
                || d.layer != self.layer
                || self.symbol().timeline.layers[self.layer].id != d.layer_id
                || (d.pencil && self.tool != Tool::Pencil)
                || (!d.pencil && self.tool != Tool::Pen)
        }) {
            self.finish_path(false);
        }
        let events = ui.input(|i| i.events.clone());
        if matches!(self.tool, Tool::Pen | Tool::Pencil) && self.layer_editable() {
            for event in &events {
                match *event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    } if page.contains(pos)
                        && response.rect.contains(pos)
                        && ui
                            .ctx()
                            .layer_id_at(pos)
                            .is_none_or(|layer| layer == response.layer_id) =>
                    {
                        self.playing = false;
                        let position = local(pos);
                        let symbol = self.symbol_id().to_owned();
                        let layer_id = self.symbol().timeline.layers[self.layer].id.clone();
                        let draft = self.vector.draft.get_or_insert_with(|| PathDraft {
                            anchors: vec![],
                            pencil: self.tool == Tool::Pencil,
                            dragging: false,
                            closing: false,
                            limit_reached: false,
                            symbol,
                            frame: self.frame,
                            layer: self.layer,
                            layer_id,
                            pointer_origin: origin,
                            pointer_zoom: zoom,
                        });
                        draft.pointer_origin = origin;
                        draft.pointer_zoom = zoom;
                        if !draft.pencil
                            && draft.anchors.len() >= 2
                            && screen(draft.anchors[0].point).distance(pos) < 8.
                        {
                            draft.closing = true;
                        } else {
                            draft.anchors.push(Anchor {
                                point: position,
                                tangent: Vec2::ZERO,
                            });
                            draft.dragging = true;
                        }
                    }
                    egui::Event::PointerMoved(pos) => {
                        if let Some(draft) = &mut self.vector.draft
                            && draft.dragging
                        {
                            if draft.pencil {
                                if draft.anchors.len() >= 16_000 {
                                    draft.limit_reached = true;
                                }
                                if draft.anchors.len() < 16_000
                                    && draft
                                        .anchors
                                        .last()
                                        .is_some_and(|a| a.point.distance(local(pos)) * zoom >= 1.5)
                                {
                                    draft.anchors.push(Anchor {
                                        point: local(pos),
                                        tangent: Vec2::ZERO,
                                    });
                                }
                            } else if let Some(anchor) = draft.anchors.last_mut() {
                                anchor.tangent = local(pos) - anchor.point;
                            }
                        }
                    }
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        ..
                    } => {
                        let mut finish = None;
                        if let Some(draft) = &mut self.vector.draft {
                            if draft.closing {
                                finish = Some(true);
                            } else if draft.dragging && draft.pencil {
                                if draft
                                    .anchors
                                    .last()
                                    .is_some_and(|a| a.point.distance(local(pos)) > 0.1)
                                {
                                    draft.anchors.push(Anchor {
                                        point: local(pos),
                                        tangent: Vec2::ZERO,
                                    });
                                }
                                finish = Some(false);
                            } else if draft.dragging
                                && let Some(anchor) = draft.anchors.last_mut()
                            {
                                // Release position is authoritative even if no
                                // separate PointerMoved event was delivered.
                                anchor.tangent = local(pos) - anchor.point;
                            }
                            draft.dragging = false;
                        }
                        if let Some(closed) = finish {
                            self.finish_path(closed);
                        }
                    }
                    _ => {}
                }
            }
        }
        if self.tool == Tool::Subselect {
            self.edit_path_points(ui, p, origin, response);
        }
        if let Some(draft) = &self.vector.draft {
            let blue = UiPalette::current(p.ctx()).overlay;
            let halo = UiPalette::current(p.ctx()).overlay_halo;
            paint_path(
                p,
                &draft.commands(false),
                PathPaint {
                    rule: FillRule::NonZero,
                    fill: Color32::TRANSPARENT,
                    stroke: blue,
                    width: 1.5 / zoom as f64,
                    tolerance: 0.2,
                },
                screen,
            );
            for anchor in &draft.anchors {
                let at = screen(anchor.point);
                p.rect_filled(Rect::from_center_size(at, Vec2::splat(8.)), 0., halo);
                p.rect_filled(Rect::from_center_size(at, Vec2::splat(6.)), 0., blue);
                if anchor.tangent.length_sq() > 0.1 {
                    for handle in [anchor.point + anchor.tangent, anchor.point - anchor.tangent] {
                        p.line_segment([at, screen(handle)], Stroke::new(3.0_f32, halo));
                        p.circle_filled(screen(handle), 4.5, halo);
                        p.line_segment([at, screen(handle)], Stroke::new(1.0_f32, blue));
                        p.circle_filled(screen(handle), 3., blue);
                    }
                }
            }
            if let (Some(last), Some(hover)) = (draft.anchors.last(), response.hover_pos())
                && !draft.dragging
            {
                p.line_segment(
                    [screen(last.point), hover],
                    Stroke::new(1.0_f32, blue.gamma_multiply(0.5)),
                );
            }
        }
    }

    fn edit_path_points(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        response: &egui::Response,
    ) {
        let Some(element) = self.selected_sampled() else {
            return;
        };
        let (ElementKind::Path { ref commands, .. }
        | ElementKind::PaintedPath { ref commands, .. }) = element.kind
        else {
            return;
        };
        let matrix = transform_matrix(element.transform);
        let zoom = self.zoom;
        let screen = |pos: Pos2| origin + crate::stage::affine(matrix, pos).to_vec2() * zoom;
        let handles = handles(commands);
        let blue = UiPalette::current(p.ctx()).overlay;
        let mut previous = Pos2::ZERO;
        for command in commands {
            match *command {
                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                    previous = Pos2::new(x as f32, y as f32)
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let control = Pos2::new(cx as f32, cy as f32);
                    let end = Pos2::new(x as f32, y as f32);
                    p.line_segment(
                        [screen(previous), screen(control)],
                        Stroke::new(1.0_f32, blue),
                    );
                    p.line_segment([screen(control), screen(end)], Stroke::new(1.0_f32, blue));
                    previous = end;
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let end = Pos2::new(x as f32, y as f32);
                    p.line_segment(
                        [screen(previous), screen(Pos2::new(cx1 as f32, cy1 as f32))],
                        Stroke::new(1.0_f32, blue),
                    );
                    p.line_segment(
                        [screen(end), screen(Pos2::new(cx2 as f32, cy2 as f32))],
                        Stroke::new(1.0_f32, blue),
                    );
                    previous = end;
                }
                PathCommand::Close => {}
            }
        }
        for handle in &handles {
            if handle.role == PointRole::Anchor {
                p.rect_filled(
                    Rect::from_center_size(screen(handle.point), Vec2::splat(9.)),
                    0.,
                    UiPalette::current(p.ctx()).overlay_halo,
                );
                p.rect_filled(
                    Rect::from_center_size(screen(handle.point), Vec2::splat(7.)),
                    0.,
                    blue,
                );
            } else {
                p.circle_filled(
                    screen(handle.point),
                    5.,
                    UiPalette::current(p.ctx()).overlay_halo,
                );
                p.circle_filled(screen(handle.point), 3.5, blue);
            }
        }
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } if response.rect.contains(pos)
                    && self.selected_editable()
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == response.layer_id) =>
                {
                    if let Some(handle) = handles
                        .iter()
                        .rev()
                        .filter(|h| screen(h.point).distance(pos) < 8.)
                        .min_by(|a, b| {
                            screen(a.point)
                                .distance_sq(pos)
                                .total_cmp(&screen(b.point).distance_sq(pos))
                        })
                        .copied()
                    {
                        self.finish_edits();
                        self.playing = false;
                        self.vector.point_drag = Some(PointDrag {
                            handle,
                            original: commands.clone(),
                            inverse: matrix,
                            symbol: self.symbol_id().to_owned(),
                            frame: self.frame,
                            layer: self.selection.single().unwrap().layer.clone(),
                            origin,
                            zoom,
                            press: pos,
                            began: false,
                            changed: false,
                            element: element.id.clone(),
                        });
                    }
                }
                egui::Event::PointerMoved(pos) => self.update_path_drag(pos, origin),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    self.update_path_drag(pos, origin);
                    self.finish_point_drag();
                }
                _ => {}
            }
        }
    }
    fn update_path_drag(&mut self, pos: Pos2, origin: Pos2) {
        let Some(mut drag) = self.vector.point_drag.take() else {
            return;
        };
        if !self.point_drag_matches(&drag) || drag.origin != origin || drag.zoom != self.zoom {
            if drag.began {
                self.history.cancel(&mut self.project);
            }
            return;
        }
        if !drag.began && pos.distance(drag.press) < 3. {
            self.vector.point_drag = Some(drag);
            return;
        }
        let local = |point| {
            crate::stage::inverse(drag.inverse, Pos2::ZERO + (point - drag.origin) / drag.zoom)
        };
        let (Some(current), Some(press)) = (local(pos), local(drag.press)) else {
            self.vector.point_drag = Some(drag);
            return;
        };
        let mut commands = drag.original.clone();
        move_handle(
            &mut commands,
            drag.handle,
            drag.handle.point + (current - press),
        );
        drag.changed = commands != drag.original;
        if drag.changed && !drag.began {
            self.history.begin(&self.project);
            self.ensure_selected_key();
            drag.began = true;
        }
        if drag.began
            && let Some(Element {
                kind:
                    ElementKind::Path {
                        commands: destination,
                        ..
                    }
                    | ElementKind::PaintedPath {
                        commands: destination,
                        ..
                    },
                ..
            }) = self.selected_element_mut()
        {
            *destination = commands;
        }
        self.vector.point_drag = Some(drag);
    }
    fn point_drag_matches(&self, drag: &PointDrag) -> bool {
        self.tool == Tool::Subselect
            && self.symbol_id() == drag.symbol
            && self.frame == drag.frame
            && self
                .selection
                .single()
                .is_some_and(|target| target.layer == drag.layer && target.element == drag.element)
            && self.selected_editable()
    }
}

/// Gradient geometry stays authored; native preview samples the shared brush on
/// a bounded adaptive mesh. HTML5 uses Canvas gradients on the original path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_gradient_path(
    painter: &egui::Painter,
    commands: &[PathCommand],
    rule: FillRule,
    width: f64,
    fill: (&Color, Option<&GradientPaint>),
    stroke: (&Color, Option<&GradientPaint>),
    screen: impl Fn(Pos2) -> Pos2,
    color: impl Fn(Color) -> Color32,
) {
    let mut limited = false;
    for ((solid, gradient), width) in [(fill, None), (stroke, Some(width))] {
        if width.is_some_and(|w| w <= 0.)
            || gradient.map_or(solid[3] == 0, |g| g.stops.iter().all(|s| s.color[3] == 0))
        {
            continue;
        }
        let geometry = match tessellate(commands, rule, width, 0.1) {
            Ok(value) => value,
            Err(error) => {
                painter.text(
                    screen(Pos2::ZERO),
                    egui::Align2::LEFT_TOP,
                    error,
                    egui::FontId::monospace(crate::theme::PANEL_FONT),
                    UiPalette::current(painter.ctx()).error_text,
                );
                continue;
            }
        };
        let mut mesh = egui::Mesh::default();
        let sample = |p: Pos2| color(gradient.map_or(*solid, |g| g.sample(p.x as f64, p.y as f64)));
        for indices in geometry.indices.as_chunks::<3>().0 {
            let points = [indices[0], indices[1], indices[2]].map(|i| {
                let p = geometry.vertices[i as usize];
                Pos2::new(p.x, p.y)
            });
            let mut pending = vec![(points, 0u8)];
            while let Some(([a, b, c], depth)) = pending.pop() {
                let center = (a.to_vec2() + b.to_vec2() + c.to_vec2()) / 3.;
                let colors = [sample(a), sample(b), sample(c)];
                let mid = sample(center.to_pos2());
                let error = (0..4)
                    .map(|i| {
                        ((u16::from(colors[0][i])
                            + u16::from(colors[1][i])
                            + u16::from(colors[2][i])) as f32
                            / 3.
                            - f32::from(mid[i]))
                        .abs()
                    })
                    .fold(0f32, f32::max);
                let extent = (screen(a) - screen(b))
                    .length()
                    .max((screen(a) - screen(c)).length())
                    .max((screen(b) - screen(c)).length());
                let refine = gradient.is_some() && (error > 2. || extent > 24.);
                if refine && depth < 7 && mesh.vertices.len() + pending.len() * 3 < 150_000 {
                    let ab = a + (b - a) * 0.5;
                    let ac = a + (c - a) * 0.5;
                    let bc = b + (c - b) * 0.5;
                    pending.extend([
                        ([a, ab, ac], depth + 1),
                        ([ab, b, bc], depth + 1),
                        ([ac, bc, c], depth + 1),
                        ([ab, bc, ac], depth + 1),
                    ]);
                    continue;
                }
                limited |= refine && depth < 7;
                let index = mesh.vertices.len() as u32;
                for (p, c) in [a, b, c].into_iter().zip(colors) {
                    mesh.colored_vertex(screen(p), c);
                }
                mesh.indices.extend([index, index + 1, index + 2]);
            }
        }
        painter.add(Shape::mesh(mesh));
    }
    if limited {
        painter.text(
            screen(Pos2::ZERO),
            egui::Align2::LEFT_TOP,
            "Gradient preview mesh limit",
            egui::FontId::monospace(crate::theme::PANEL_FONT),
            UiPalette::current(painter.ctx()).error_text,
        );
    }
}
