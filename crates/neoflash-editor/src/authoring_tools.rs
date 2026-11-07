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
    StrokeSegments,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum GapCloseMode {
    #[default]
    None,
    Small,
    Medium,
    Large,
}

impl GapCloseMode {
    fn label(self) -> &'static str {
        match self {
            Self::None => "Don’t close gaps",
            Self::Small => "Close small gaps",
            Self::Medium => "Close medium gaps",
            Self::Large => "Close large gaps",
        }
    }

    pub(crate) fn max_gap_px(self) -> f64 {
        match self {
            Self::None => 0.0,
            Self::Small => 6.0,
            Self::Medium => 14.0,
            Self::Large => 28.0,
        }
    }
}

pub(crate) struct AuthoringToolsState {
    gesture: Option<Gesture>,
    pressure_contact: Option<PressureContact>,
    pub brush_size: f32,
    pub brush_shape: neoflash_ui::components::BrushShapeKind,
    pub brush_smoothing: f64,
    pub brush_pressure: bool,
    pub brush_tilt: bool,
    pub brush_lock_fill: bool,
    pub brush_paint_mode: BrushPaintMode,
    pub brush_inside_seed: Option<[f64; 2]>,
    pub custom_brush_presets: Vec<(String, neoflash_ui::components::BrushShapeKind, f32)>,
    brush_resize: Option<(Pos2, f32)>,
    pub brush_search: String,
    pub eraser_size: f32,
    pub eraser_mode: EraserMode,
    pub gap_close: GapCloseMode,
    pub lasso_contact: bool,
    pub lasso_partial_fills: bool,
    pub zoom_out: bool,
    pub polystar_star: bool,
    pub polystar_sides: u32,
    pub polystar_point_size: f64,
    pub polystar_dialog: Option<(bool, u32, f64)>,
    pub rect_options: RectangleOptions,
    pub oval_options: OvalOptions,
}
impl Default for AuthoringToolsState {
    fn default() -> Self {
        Self {
            gesture: None,
            pressure_contact: None,
            brush_size: 12.,
            brush_shape: Default::default(),
            brush_smoothing: 50.,
            brush_pressure: true,
            brush_tilt: false,
            brush_lock_fill: false,
            brush_paint_mode: BrushPaintMode::PaintNormal,
            brush_inside_seed: None,
            custom_brush_presets: Vec::new(),
            brush_resize: None,
            brush_search: String::new(),
            eraser_size: 16.,
            eraser_mode: EraserMode::Objects,
            gap_close: GapCloseMode::None,
            lasso_contact: true,
            lasso_partial_fills: false,
            zoom_out: false,
            polystar_star: false,
            polystar_sides: 5,
            polystar_point_size: 0.5,
            polystar_dialog: None,
            rect_options: RectangleOptions::default(),
            oval_options: OvalOptions::default(),
        }
    }
}
impl AuthoringToolsState {
    pub fn cancel(&mut self) {
        self.gesture = None;
        self.pressure_contact = None;
        if let Some((_, size)) = self.brush_resize.take() {
            self.brush_size = size;
        }
    }
    pub fn is_drawing(&self) -> bool {
        self.gesture.is_some()
    }
}
struct PressureContact {
    device: egui::TouchDeviceId,
    id: egui::TouchId,
    force: f32,
}

fn brush_force(force: Option<f32>) -> f32 {
    force
        .filter(|force| force.is_finite())
        .unwrap_or(1.)
        .clamp(0.1, 1.)
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
    pressures: Vec<f32>,
    shift: bool,
    alt: bool,
    targets: Vec<SelectionTarget>,
    eraser_meshes: Vec<(SelectionTarget, egui::Mesh)>,
    gradient: Option<GradientGesture>,
}
struct GradientGesture {
    target: SelectionTarget,
    kind: ElementKind,
    matrix: [f64; 6],
    element_matrix: [f64; 6],
    handle: bool,
    focal_handle: bool,
    radial: bool,
    focal_point_ratio: f64,
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
            | Tool::PolyStar
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
            if item.instance_path.len() != 1
                || !authored.contains_point(&item.instance_path, [point.x as f64, point.y as f64])
            {
                return None;
            }
            let target = eligible.get(item.instance_path.first()?)?;
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
            let mut metric = item.transform;
            for value in &mut metric {
                *value *= f64::from(self.zoom);
            }
            let (is_effectively_closed, gap_closed_cmds) = if self.tool == Tool::Bucket {
                let closure = neoflash_core::close_path_gaps(
                    &commands,
                    metric,
                    self.authoring_tools.gap_close.max_gap_px(),
                )
                .ok()?;
                (
                    closure.remaining_open_contours == 0,
                    (closure.commands != commands).then_some(closure.commands),
                )
            } else {
                (closed, None)
            };
            let inside = is_effectively_closed
                && geometry_contains(
                    gap_closed_cmds.as_deref().unwrap_or(&commands),
                    rule,
                    None,
                    local,
                );
            ((inside || outline) && (!closed_only || is_effectively_closed)).then(|| {
                let mut kind = item.kind.clone();
                if let Some(closed_cmds) = gap_closed_cmds {
                    match &mut kind {
                        ElementKind::Path { commands: c, .. }
                        | ElementKind::PaintedPath { commands: c, .. } => *c = closed_cmds,
                        _ => {}
                    }
                }
                PaintHit {
                    target: target.clone(),
                    kind,
                    local,
                    stroke: outline,
                    matrix: item.transform,
                }
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
                        if !matches!(
                            kind,
                            ElementKind::Rectangle { .. } | ElementKind::Ellipse { .. }
                        ) {
                            element.transform.primitive = None;
                        }
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
                self.authoring_modify.drawing_stroke_gradient = None;
            } else {
                self.fill = paint_gradient(&hit.kind)
                    .map(|g| g.sample(hit.local.x as f64, hit.local.y as f64))
                    .unwrap_or(fill);
                self.authoring_modify.drawing_fill_gradient = None;
            }
            return;
        }
        let mut element = self
            .target_layer_index(&hit.target)
            .and_then(|idx| {
                sample_layer(&self.symbol().timeline.layers[idx], self.frame)
                    .into_iter()
                    .find(|e| e.id == hit.target.element)
            })
            .unwrap_or_else(|| Element {
                kind: hit.kind.clone(),
                ..Default::default()
            });
        if tool == Tool::Bucket
            && let (
                ElementKind::Path {
                    commands: hit_c, ..
                }
                | ElementKind::PaintedPath {
                    commands: hit_c, ..
                },
                ElementKind::Path {
                    commands: dst_c, ..
                }
                | ElementKind::PaintedPath {
                    commands: dst_c, ..
                },
            ) = (&hit.kind, &mut element.kind)
            && hit_c != dst_c
        {
            *dst_c = hit_c.clone();
        }
        self.apply_active_tool_paint(&mut element, tool == Tool::Ink);
        self.change_tool_elements(vec![(hit.target.clone(), Some(element.kind))]);
        self.set_selection(vec![hit.target]);
    }

    /// The fill keeps its original contour; only the stroke receives cut commands.
    /// New stroke fragments share one element and preserve gradient/affine appearance.
    fn erase_stroke_parts(&mut self, gesture: &Gesture, painter: &egui::Painter) {
        let points: Vec<_> = gesture
            .points
            .iter()
            .map(|p| [f64::from(p.x), f64::from(p.y)])
            .collect();
        let authored =
            self.presented_interactive_authored_scene(self.show_layer_masking(painter.ctx()));
        let mut changes = Vec::new();
        for target in &gesture.targets {
            let Some(draw) = authored.draws.iter().find(|draw| {
                draw.item.instance_path.len() == 1
                    && draw.item.instance_path.first() == Some(&target.element)
            }) else {
                continue;
            };
            if !draw.clip_ids.is_empty() {
                self.notify(neoflash_ui::components::ToastType::Warning, "Partial stroke erasing on masked artwork is not available yet; no artwork was changed.");
                return;
            }
            let Some(index) = self.target_layer_index(target) else {
                continue;
            };
            let Some(element) = sample_layer(&self.symbol().timeline.layers[index], self.frame)
                .into_iter()
                .find(|element| element.id == target.element)
            else {
                continue;
            };
            let layer = &self.symbol().timeline.layers[index];
            if layer.properties.motion_guide.is_some()
                && !layer.keyframes.iter().any(|key| key.frame == self.frame)
                && (self.workspace.timeline_prefs.auto_keyframe
                    || motion_span(layer, self.frame).is_some())
                && draw.item.transform != transform_matrix(element.transform)
            {
                self.notify(neoflash_ui::components::ToastType::Warning, "Partial stroke erasing cannot insert a keyframe inside guided motion yet. Erase at an existing keyframe, or turn off Auto keyframe for a classic tween; no artwork was changed.");
                return;
            }
            let Some((commands, rule)) = element.to_path_commands() else {
                continue;
            };
            let Some((fill, stroke, width)) = paint_parts(&element.kind) else {
                continue;
            };
            let result = match neoflash_core::erase_path_stroke(
                &commands,
                // Hit testing sees the presented pose, including motion guides
                // and Orient to path. Cut in that same space without baking the
                // presentation transform into the authored span key.
                draw.item.transform,
                &points,
                f64::from(self.authoring_tools.eraser_size / 2. / gesture.zoom),
                0.2 / f64::from(gesture.zoom),
            ) {
                Ok(result) => result,
                Err(error) => {
                    self.notify(
                        neoflash_ui::components::ToastType::Warning,
                        error.to_string(),
                    );
                    return;
                }
            };
            if !result.changed {
                continue;
            }
            let (fill_gradient, stroke_gradient) = match &element.kind {
                ElementKind::PaintedPath {
                    fill_gradient,
                    stroke_gradient,
                    ..
                } => (fill_gradient.clone(), stroke_gradient.clone()),
                _ => (None, None),
            };
            let has_fill = fill[3] > 0 || fill_gradient.is_some();
            if has_fill && (!element.filters.is_empty() || element.blend_mode != BlendMode::Normal)
            {
                self.notify(neoflash_ui::components::ToastType::Warning, "Partial stroke erasing on a combined fill/stroke with filters or blend modes is not available yet; no artwork was changed.");
                return;
            }
            let mut replacements = Vec::new();
            if has_fill {
                let mut retained = element.clone();
                apply_paint(&mut retained.kind, None, Some(([0; 4], 0.)));
                replacements.push(retained);
            }
            if !result.commands.is_empty() {
                let mut retained = element.clone();
                if has_fill {
                    retained.id = new_id("object");
                    // Keep object scripts/instance identity on the original fill.
                    retained.scripts.clear();
                    retained.accessibility = None;
                    retained.name.clear();
                }
                retained.transform.primitive = None;
                retained.kind = if stroke_gradient.is_some() {
                    ElementKind::PaintedPath {
                        commands: result.commands,
                        fill_rule: rule,
                        fill: [0; 4],
                        stroke,
                        stroke_width: width,
                        fill_gradient: None,
                        stroke_gradient,
                    }
                } else {
                    ElementKind::Path {
                        commands: result.commands,
                        fill_rule: rule,
                        fill: [0; 4],
                        stroke,
                        stroke_width: width,
                    }
                };
                replacements.push(retained);
            }
            changes.push((target.clone(), replacements));
        }
        if changes.is_empty() {
            return;
        }
        self.transact(|editor| {
            let before = editor.project.clone();
            for (target, mut replacements) in changes {
                if !editor.target_editable(&target) {
                    continue;
                }
                editor.ensure_key_for(&target.layer, false);
                let Some(index) = editor.target_layer_index(&target) else {
                    continue;
                };
                let frame = editor.frame;
                let layer = &mut editor.symbol_mut().timeline.layers[index];
                let elements = &mut layer.key_at_mut(frame).unwrap().elements;
                if let Some(position) = elements
                    .iter()
                    .position(|element| element.id == target.element)
                {
                    // With Auto keyframe off, the destination is the existing
                    // span key, not the interpolated pose sampled for geometry.
                    let transform = elements[position].transform;
                    for replacement in &mut replacements {
                        replacement.transform = transform;
                        if matches!(
                            replacement.kind,
                            ElementKind::Path { .. } | ElementKind::PaintedPath { .. }
                        ) {
                            replacement.transform.primitive = None;
                        }
                    }
                    let ids: Vec<_> = replacements
                        .iter()
                        .map(|element| element.id.clone())
                        .collect();
                    elements.splice(position..=position, replacements);
                    for group in &mut layer.properties.groups {
                        if let Some(position) =
                            group.members.iter().position(|id| *id == target.element)
                        {
                            // Keep the original id if other keyframes still use it.
                            for (offset, id) in ids
                                .into_iter()
                                .filter(|id| *id != target.element)
                                .enumerate()
                            {
                                group.members.insert(position + 1 + offset, id);
                            }
                            break;
                        }
                    }
                }
            }
            editor.prune_authoring_groups();
            if let Err(error) = editor.project.validate() {
                editor.project = before;
                editor.message(error.to_string());
            }
        });
        self.normalize_targets();
    }

    /// Bucket and Ink share the same active paints as newly drawn artwork. Only
    /// the chosen well changes; the opposite paint and gradient keep their data.
    fn apply_active_tool_paint(&self, element: &mut Element, stroke_well: bool) {
        apply_paint(
            &mut element.kind,
            (!stroke_well).then_some(self.fill),
            stroke_well.then_some((self.stroke, self.stroke_width)),
        );
        let gradient = if stroke_well {
            &self.authoring_modify.drawing_stroke_gradient
        } else {
            &self.authoring_modify.drawing_fill_gradient
        };
        let Some(mut gradient) = gradient.clone() else {
            return;
        };
        let Some((commands, fill_rule)) = element.to_path_commands() else {
            return;
        };
        let Some((fill, stroke, stroke_width)) = paint_parts(&element.kind) else {
            return;
        };
        gradient.matrix = crate::authoring_modify::default_gradient_matrix(&commands);
        let (mut fill_gradient, mut stroke_gradient) = match &element.kind {
            ElementKind::PaintedPath {
                fill_gradient,
                stroke_gradient,
                ..
            } => (fill_gradient.clone(), stroke_gradient.clone()),
            _ => (None, None),
        };
        if stroke_well {
            stroke_gradient = Some(gradient);
        } else {
            fill_gradient = Some(gradient);
        }
        element.kind = ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
            fill_gradient,
            stroke_gradient,
        };
    }

    /// Snapshot the visible geometry once per eraser gesture. Mask clipping and
    /// vector tessellation are shared with the stage rather than sampled at a
    /// handful of points inside the displayed eraser circle.
    fn eraser_meshes(
        &self,
        p: &egui::Painter,
    ) -> Result<Vec<(SelectionTarget, egui::Mesh)>, String> {
        let eligible: HashMap<_, _> = self
            .editable_targets()
            .into_iter()
            .map(|target| (target.element.clone(), target))
            .collect();
        let authored = self.presented_interactive_authored_scene(self.show_layer_masking(p.ctx()));
        let clips = crate::stage_clipping::ClipMeshes::new(&authored.clips, Pos2::ZERO, 1.)?;
        let mut meshes = Vec::new();
        for draw in &authored.draws {
            let Some(target) = draw
                .item
                .instance_path
                .first()
                .and_then(|id| eligible.get(id))
            else {
                continue;
            };
            let mut item = draw.item.clone();
            let vector = paint_parts(&item.kind).is_some();
            if self.authoring_tools.eraser_mode != EraserMode::Objects {
                // Whole-fill/stroke operations edit raw vector artwork, never a
                // containing symbol merely because a child was touched.
                if !vector || item.instance_path.len() != 1 {
                    continue;
                }
                match self.authoring_tools.eraser_mode {
                    EraserMode::Fills => apply_paint(&mut item.kind, None, Some(([0; 4], 0.))),
                    EraserMode::Strokes | EraserMode::StrokeSegments => {
                        apply_paint(&mut item.kind, Some([0; 4]), None)
                    }
                    EraserMode::Objects => {}
                }
            }
            let mesh = if vector {
                let Some(mesh) = crate::stage_clipping::vector_mesh(&item, Pos2::ZERO, 1.)? else {
                    continue;
                };
                mesh
            } else {
                // Text/bitmap placements use their local bounds, like selection;
                // preserve the affine transform instead of using a world AABB.
                let bounds = crate::stage::local_bounds(&item, p);
                if !bounds.is_finite() || bounds.is_negative() || item.alpha <= 0. {
                    continue;
                }
                let mut mesh = egui::Mesh::default();
                for point in [
                    bounds.left_top(),
                    bounds.right_top(),
                    bounds.right_bottom(),
                    bounds.left_bottom(),
                ] {
                    mesh.colored_vertex(affine(item.transform, point), Color32::WHITE);
                }
                mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                mesh
            };
            let mesh = clips.clip_mesh(&mesh, &draw.clip_ids)?;
            if !mesh.indices.is_empty() {
                meshes.push((target.clone(), mesh));
            }
        }
        Ok(meshes)
    }
    /// Returns true when extended tools/temporary Hand own this pointer phase.
    pub(crate) fn authoring_tool_stage(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        _page: Rect,
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
        // The approved resize chord owns its whole gesture, so it cannot also lay down paint.
        if self.tool == Tool::Brush && self.authoring_tools.gesture.is_none() {
            let pointer = ui.input(|i| i.pointer.clone());
            if self.authoring_tools.brush_resize.is_none()
                && ui.input(|i| i.modifiers.alt && i.modifiers.command)
                && pointer.primary_pressed()
                && let Some(pos) = pointer.interact_pos().filter(|pos| view.contains(*pos))
            {
                self.authoring_tools.brush_resize = Some((pos, self.authoring_tools.brush_size));
            }
            if let Some((start, size)) = self.authoring_tools.brush_resize {
                if let Some(pos) = pointer.interact_pos() {
                    self.authoring_tools.brush_size =
                        (size + (pos.x - start.x) / self.zoom.max(0.001)).clamp(1., 200.);
                    neoflash_ui::components::brush_cursor(
                        p,
                        pos,
                        self.authoring_tools.brush_shape,
                        self.authoring_tools.brush_size * self.zoom,
                        neoflash_ui::components::BrushCursorState::Resize,
                        Some(&format!("{} px", self.authoring_tools.brush_size.round())),
                    );
                }
                if pointer.primary_released() {
                    self.authoring_tools.brush_resize = None;
                }
                return true;
            }
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
                egui::Event::Touch {
                    device_id,
                    id,
                    phase,
                    pos,
                    force,
                } => {
                    let current = self
                        .authoring_tools
                        .pressure_contact
                        .as_ref()
                        .is_some_and(|contact| contact.device == device_id && contact.id == id);
                    match phase {
                        egui::TouchPhase::Start
                            if self.authoring_tools.pressure_contact.is_none()
                                && self.tool == Tool::Brush
                                && !space
                                && view.contains(pos)
                                && ui
                                    .ctx()
                                    .layer_id_at(pos)
                                    .is_none_or(|layer| layer == ui.layer_id()) =>
                        {
                            self.authoring_tools.pressure_contact = Some(PressureContact {
                                device: device_id,
                                id,
                                force: brush_force(force),
                            });
                        }
                        egui::TouchPhase::Move if current => {
                            if let Some(contact) = &mut self.authoring_tools.pressure_contact {
                                contact.force = brush_force(force);
                            }
                            self.update_authoring_gesture(pos, scene, p);
                        }
                        egui::TouchPhase::End if current => {
                            // Pen-up often reports zero; retain the last contact width.
                            self.update_authoring_gesture(pos, scene, p);
                        }
                        egui::TouchPhase::Cancel if current => self.authoring_tools.cancel(),
                        _ => {}
                    }
                }
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
                    if matches!(tool, Tool::Line | Tool::PolyStar | Tool::Brush)
                        && !self.layer_editable()
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
                        // finish_edits cancels the previous draft, not this pen-down sample.
                        let contact = self.authoring_tools.pressure_contact.take();
                        self.finish_edits();
                        self.authoring_tools.pressure_contact = contact;
                    }
                    self.playing = false;
                    let local = Pos2::ZERO + (pos - origin) / self.zoom;
                    // Line ends and PolyStar centres snap to guides, objects, grid and pixels (ST-04).
                    let local = if matches!(tool, Tool::Line | Tool::PolyStar) {
                        Pos2::ZERO + (self.snap_screen(origin, pos) - origin) / self.zoom
                    } else {
                        local
                    };
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
                            let focal = affine(
                                matrix,
                                affine(
                                    fill.matrix,
                                    Pos2::new((fill.focal_point_ratio * 819.2) as f32, 0.),
                                ),
                            );
                            if (origin + edge.to_vec2() * self.zoom).distance(pos) >= 12.
                                && (origin + center.to_vec2() * self.zoom).distance(pos) >= 12.
                                && (fill.kind != GradientKind::Radial
                                    || (origin + focal.to_vec2() * self.zoom).distance(pos) >= 12.)
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
                        let radial = gradient.kind == GradientKind::Radial;
                        let focal_point_ratio = gradient.focal_point_ratio;
                        let edge = affine(hit.matrix, affine(matrix, Pos2::new(819.2, 0.)));
                        let center = affine(
                            hit.matrix,
                            Pos2::new(matrix[4] as f32, matrix[5] as f32),
                        );
                        let focal = affine(
                            hit.matrix,
                            affine(matrix, Pos2::new((focal_point_ratio * 819.2) as f32, 0.)),
                        );
                        let handle = (origin + edge.to_vec2() * self.zoom).distance(pos) < 12.;
                        let focal_screen = origin + focal.to_vec2() * self.zoom;
                        let center_screen = origin + center.to_vec2() * self.zoom;
                        let focal_handle = radial
                            && !handle
                            && focal_screen.distance(pos) < 12.
                            && (modifiers.alt
                                || (focal_point_ratio.abs() > 0.03
                                    && focal_screen.distance(pos)
                                        < center_screen.distance(pos)));
                        self.set_selection(vec![hit.target.clone()]);
                        Some(GradientGesture {
                            target: hit.target,
                            kind: hit.kind,
                            matrix,
                            element_matrix: hit.matrix,
                            handle,
                            focal_handle,
                            radial,
                            focal_point_ratio,
                        })
                    } else {
                        None
                    };
                    let eraser_meshes = if tool == Tool::Eraser {
                        match self.eraser_meshes(p) {
                            Ok(meshes) => meshes,
                            Err(error) => {
                                self.message(error);
                                continue;
                            }
                        }
                    } else {
                        Vec::new()
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
                        pressures: vec![
                            self.authoring_tools
                                .pressure_contact
                                .as_ref()
                                .map_or(1., |c| c.force),
                        ],
                        shift: modifiers.shift,
                        alt: modifiers.alt,
                        targets: vec![],
                        eraser_meshes,
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
                    self.authoring_tools.pressure_contact = None;
                }
                _ => {}
            }
        }
        self.paint_authoring_gesture(p, origin, view, scene);
        if let Some(pos) = ui.input(|i| i.pointer.latest_pos())
            && view.contains(pos)
        {
            let tool = if space { Tool::Hand } else { self.tool };
            use neoflash_ui::cursors as nf;
            self.stage_cursor = Some(match tool {
                Tool::Hand if self.authoring_tools.gesture.is_some() => nf::HAND_CLOSED,
                Tool::Zoom if ui.input(|i| i.modifiers.alt) || self.authoring_tools.zoom_out => {
                    nf::ZOOM_OUT
                }
                _ => crate::stage_cursor::tool_cursor_precise(
                    tool,
                    self.workspace.drawing.precise_cursors,
                ),
            });
            if matches!(tool, Tool::Brush | Tool::Eraser) {
                let (size, shape, state) = if tool == Tool::Brush {
                    (
                        self.authoring_tools.brush_size * self.zoom,
                        self.authoring_tools.brush_shape,
                        neoflash_ui::components::BrushCursorState::Idle,
                    )
                } else {
                    (
                        self.authoring_tools.eraser_size,
                        neoflash_ui::components::BrushShapeKind::Round,
                        neoflash_ui::components::BrushCursorState::Eraser,
                    )
                };
                neoflash_ui::components::brush_cursor(
                    p,
                    pos,
                    shape,
                    size,
                    state,
                    Some(&format!(
                        "{} px",
                        if tool == Tool::Brush {
                            self.authoring_tools.brush_size
                        } else {
                            self.authoring_tools.eraser_size
                        }
                        .round()
                    )),
                );
            }
        }
        owned
    }
    fn update_authoring_gesture(&mut self, pos: Pos2, _scene: &[DrawItem], p: &egui::Painter) {
        let Some(mut g) = self.authoring_tools.gesture.take() else {
            return;
        };
        g.latest = pos;
        let mut point = g.local(pos);
        if matches!(g.tool, Tool::Line | Tool::PolyStar) && !g.shift {
            point = g.local(self.snap_screen(g.origin, pos));
        }
        if matches!(g.tool, Tool::Line | Tool::PolyStar) && g.shift {
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
            Tool::Line | Tool::PolyStar => {
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
                    g.pressures.push(
                        self.authoring_tools
                            .pressure_contact
                            .as_ref()
                            .map_or(1., |c| c.force),
                    );
                } else if g.tool == Tool::Brush {
                    // Stationary pressure changes update the current contact, not a duplicate point.
                    if let Some(last) = g.pressures.last_mut() {
                        *last = self
                            .authoring_tools
                            .pressure_contact
                            .as_ref()
                            .map_or(1., |c| c.force);
                    }
                }
            }
            Tool::Eraser => {
                let radius = self.authoring_tools.eraser_size / 2. / g.zoom;
                let previous = *g.points.last().unwrap_or(&point);
                for (target, mesh) in &g.eraser_meshes {
                    if !g.targets.contains(target)
                        && swept_disk_hits_mesh(mesh, previous, point, radius)
                    {
                        g.targets.push(target.clone());
                    }
                }
                if point != previous {
                    g.points.push(point);
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
                    self.brush_gesture_commands(&g)
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
                if g.tool == Tool::Brush {
                    self.authoring_tools.brush_inside_seed = g
                        .points
                        .first()
                        .map(|pt| [f64::from(pt.x), f64::from(pt.y)]);
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
                        // A fill-only brush has zero stroke width, not an erased stroke paint.
                        // Keep the chosen RGB/alpha available in Properties and the next tool.
                        stroke: self.stroke,
                        stroke_width: if g.tool == Tool::Line {
                            self.stroke_width
                        } else {
                            0.
                        },
                    },
                    Transform::default(),
                );
            }
            Tool::Eraser => {
                let mode = self.authoring_tools.eraser_mode;
                if mode == EraserMode::StrokeSegments {
                    self.erase_stroke_parts(&g, p);
                    return;
                }
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
                if (self.authoring_tools.lasso_partial_fills || g.alt) && self.layer_editable() {
                    let pts: Vec<[f64; 2]> = g
                        .points
                        .iter()
                        .map(|pt| [f64::from(pt.x), f64::from(pt.y)])
                        .collect();
                    if !self.lasso_select_partial_fill(&pts).is_empty() {
                        return;
                    }
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
                if let Some(gradient) = &g.gradient {
                    if gradient.focal_handle {
                        if let Some(focal_ratio) = gradient_focal_ratio(&g, gradient) {
                            let mut kind = gradient.kind.clone();
                            if let ElementKind::PaintedPath {
                                fill_gradient: Some(fill),
                                ..
                            } = &mut kind
                            {
                                fill.focal_point_ratio = focal_ratio;
                            }
                            self.change_tool_elements(vec![(gradient.target.clone(), Some(kind))]);
                        }
                    } else if let Some(matrix) = gradient_matrix(&g, gradient) {
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
            }
            Tool::PolyStar => {
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
                if !self.layer_editable() || g.points.len() < 2 {
                    return;
                }
                let center = [g.points[0].x as f64, g.points[0].y as f64];
                let outer = [g.points[1].x as f64, g.points[1].y as f64];
                let (dx, dy) = (outer[0] - center[0], outer[1] - center[1]);
                let radius = dx.hypot(dy);
                if radius <= (2.0 / g.zoom as f64).max(0.5) {
                    return;
                }
                let angle = dy.atan2(dx);
                let (commands, origin_xy) = polystar_path_commands(
                    center,
                    radius,
                    angle,
                    self.authoring_tools.polystar_sides,
                    self.authoring_tools.polystar_star,
                    self.authoring_tools.polystar_point_size,
                );
                if commands.len() < 3 {
                    return;
                }
                self.add_element_transformed(
                    ElementKind::Path {
                        commands,
                        fill_rule: FillRule::NonZero,
                        fill: self.fill,
                        stroke: self.stroke,
                        stroke_width: self.stroke_width,
                    },
                    Transform {
                        x: origin_xy[0],
                        y: origin_xy[1],
                        ..Default::default()
                    },
                );
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
                    if g.tool == Tool::Brush {
                        let commands = self.brush_gesture_commands(g);
                        if let Ok(geometry) = crate::vector::tessellate_cached(
                            &commands,
                            FillRule::NonZero,
                            None,
                            0.3,
                        ) {
                            let mut mesh = egui::Mesh::default();
                            mesh.vertices.reserve(geometry.vertices.len());
                            let color = Color32::from_rgba_unmultiplied(
                                self.fill[0],
                                self.fill[1],
                                self.fill[2],
                                self.fill[3],
                            );
                            for point in &geometry.vertices {
                                mesh.colored_vertex(screen(Pos2::new(point.x, point.y)), color);
                            }
                            mesh.indices.clone_from(&geometry.indices);
                            p.add(Shape::mesh(mesh));
                        }
                        return;
                    }
                    let color = if g.tool == Tool::Brush {
                        self.fill
                    } else {
                        self.stroke
                    };
                    let width = if g.tool == Tool::Brush {
                        self.authoring_tools.brush_size * g.zoom
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
                    if let Some(gradient) = &g.gradient {
                        let matrix = if gradient.focal_handle {
                            Some(gradient.matrix)
                        } else {
                            gradient_matrix(g, gradient)
                        };
                        let focal_ratio = if gradient.focal_handle {
                            gradient_focal_ratio(g, gradient).unwrap_or(gradient.focal_point_ratio)
                        } else {
                            gradient.focal_point_ratio
                        };
                        if let Some(matrix) = matrix {
                            paint_gradient_handles(
                                p,
                                g.origin,
                                g.zoom,
                                gradient.element_matrix,
                                matrix,
                                gradient.radial.then_some(focal_ratio),
                            );
                        }
                    }
                }
                Tool::PolyStar if g.points.len() >= 2 => {
                    let center = [g.points[0].x as f64, g.points[0].y as f64];
                    let outer = [g.points[1].x as f64, g.points[1].y as f64];
                    let (dx, dy) = (outer[0] - center[0], outer[1] - center[1]);
                    let radius = dx.hypot(dy);
                    if radius > 0.5 {
                        let (commands, origin_xy) = polystar_path_commands(
                            center,
                            radius,
                            dy.atan2(dx),
                            self.authoring_tools.polystar_sides,
                            self.authoring_tools.polystar_star,
                            self.authoring_tools.polystar_point_size,
                        );
                        let pts: Vec<Pos2> = commands
                            .iter()
                            .filter_map(|cmd| match *cmd {
                                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                                    Some(screen(Pos2::new(
                                        (origin_xy[0] + x) as f32,
                                        (origin_xy[1] + y) as f32,
                                    )))
                                }
                                _ => None,
                            })
                            .collect();
                        if pts.len() >= 3 {
                            let stroke_color = if self.stroke[3] > 0 {
                                Color32::from_rgba_unmultiplied(
                                    self.stroke[0],
                                    self.stroke[1],
                                    self.stroke[2],
                                    self.stroke[3],
                                )
                            } else {
                                palette.overlay
                            };
                            p.add(Shape::closed_line(
                                pts,
                                Stroke::new(
                                    (self.stroke_width as f32 * g.zoom).max(1.5),
                                    stroke_color,
                                ),
                            ));
                        }
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
                (fill.kind == GradientKind::Radial).then_some(fill.focal_point_ratio),
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

/// A swept disk is a capsule. Its intersection with a triangle is either an
/// endpoint inside the triangle or an edge within the radius of the sweep.
fn swept_disk_hits_mesh(mesh: &egui::Mesh, from: Pos2, to: Pos2, radius: f32) -> bool {
    fn cross(a: Pos2, b: Pos2, c: Pos2) -> f32 {
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    }
    fn point_segment_distance_sq(point: Pos2, a: Pos2, b: Pos2) -> f32 {
        let edge = b - a;
        let t = if edge.length_sq() > 0. {
            ((point - a).dot(edge) / edge.length_sq()).clamp(0., 1.)
        } else {
            0.
        };
        point.distance_sq(a + edge * t)
    }
    let swept_bounds = Rect::from_two_pos(from, to).expand(radius);
    mesh.indices.as_chunks::<3>().0.iter().any(|indices| {
        let triangle = indices.map(|index| mesh.vertices[index as usize].pos);
        let bounds = Rect::from_points(&triangle);
        if !swept_bounds.intersects(bounds)
            || cross(triangle[0], triangle[1], triangle[2]).abs() < 1e-8
        {
            return false;
        }
        let inside = |point| {
            let sides = [
                cross(triangle[0], triangle[1], point),
                cross(triangle[1], triangle[2], point),
                cross(triangle[2], triangle[0], point),
            ];
            sides.iter().all(|side| *side >= 0.) || sides.iter().all(|side| *side <= 0.)
        };
        inside(from)
            || inside(to)
            || (0..3).any(|i| {
                let (a, b) = (triangle[i], triangle[(i + 1) % 3]);
                let intersects = Rect::from_two_pos(from, to).intersects(Rect::from_two_pos(a, b))
                    && cross(from, to, a) * cross(from, to, b) <= 0.
                    && cross(a, b, from) * cross(a, b, to) <= 0.;
                intersects
                    || [
                        point_segment_distance_sq(from, a, b),
                        point_segment_distance_sq(to, a, b),
                        point_segment_distance_sq(a, from, to),
                        point_segment_distance_sq(b, from, to),
                    ]
                    .into_iter()
                    .any(|distance| distance <= radius * radius)
            })
    })
}
fn simplify_round_spine(
    points: &[Pos2],
    radii: &[f32],
    pos_tol: f32,
    rad_tol: f32,
) -> (Vec<Pos2>, Vec<f32>) {
    let mut dedup_pts: Vec<Pos2> = Vec::with_capacity(points.len());
    let mut dedup_rad: Vec<f32> = Vec::with_capacity(points.len());
    for (&pt, &r) in points.iter().zip(radii.iter()) {
        if let Some(last_pt) = dedup_pts.last().copied()
            && pt.distance_sq(last_pt) < 0.04
        {
            if let Some(last_r) = dedup_rad.last_mut() {
                *last_r = last_r.max(r);
            }
            continue;
        }
        dedup_pts.push(pt);
        dedup_rad.push(r);
    }
    let n = dedup_pts.len();
    if n <= 2 {
        return (dedup_pts, dedup_rad);
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((start, end)) = stack.pop() {
        if end <= start + 1 {
            continue;
        }
        let a = dedup_pts[start];
        let b = dedup_pts[end];
        let r_a = dedup_rad[start];
        let r_b = dedup_rad[end];
        let ab = b - a;
        let len_sq = ab.length_sq();
        let mut worst_idx = start;
        let mut worst_score = 0.0_f32;
        for i in (start + 1)..end {
            let p = dedup_pts[i];
            let t = if len_sq > 1e-8 {
                ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0)
            } else {
                0.5
            };
            let proj = a + ab * t;
            let pos_err = p.distance(proj);
            let rad_err = (dedup_rad[i] - (r_a + (r_b - r_a) * t)).abs();
            if pos_err > pos_tol || rad_err > rad_tol {
                let score = (pos_err / pos_tol.max(1e-4)).max(rad_err / rad_tol.max(1e-4));
                if score > worst_score {
                    worst_score = score;
                    worst_idx = i;
                }
            }
        }
        if worst_score > 1.0 && worst_idx > start && worst_idx < end {
            keep[worst_idx] = true;
            stack.push((start, worst_idx));
            stack.push((worst_idx, end));
        }
    }
    let mut out_pts = Vec::new();
    let mut out_rad = Vec::new();
    for i in 0..n {
        if keep[i] {
            out_pts.push(dedup_pts[i]);
            out_rad.push(dedup_rad[i]);
        }
    }
    (out_pts, out_rad)
}

#[derive(Default)]
struct RectSweepCounts {
    tree: Vec<usize>,
}

impl RectSweepCounts {
    fn with_len(len: usize) -> Self {
        Self {
            tree: vec![0; len + 1],
        }
    }

    fn update(&mut self, index: usize, add: bool) {
        let mut index = index + 1;
        while index < self.tree.len() {
            if add {
                self.tree[index] += 1;
            } else {
                self.tree[index] -= 1;
            }
            index += index.isolate_lowest_one();
        }
    }

    /// Number of entries whose compressed coordinate is below `end`.
    fn prefix(&self, end: usize) -> usize {
        let mut index = end;
        let mut count = 0;
        while index > 0 {
            count += self.tree[index];
            index &= index - 1;
        }
        count
    }
}

fn nonadjacent_rects_overlap_bruteforce(boxes: &[Rect]) -> bool {
    (0..boxes.len()).any(|i| ((i + 2)..boxes.len()).any(|j| boxes[i].intersects(boxes[j])))
}

/// Detect the same non-adjacent expanded-box intersections as the quadratic scan, using an X
/// sweep and two Fenwick indexes over active Y interval endpoints. Touching bounds still count.
fn nonadjacent_rects_overlap(boxes: &[Rect]) -> bool {
    if boxes.len() < 3 {
        return false;
    }
    if boxes.iter().any(|rect| {
        [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
            .into_iter()
            .any(|value| !value.is_finite())
            || rect.min.x > rect.max.x
            || rect.min.y > rect.max.y
    }) {
        return nonadjacent_rects_overlap_bruteforce(boxes);
    }

    let mut y_values: Vec<f32> = boxes
        .iter()
        .flat_map(|rect| [rect.min.y, rect.max.y])
        .collect();
    y_values.sort_by(f32::total_cmp);
    y_values.dedup_by(|a, b| a.total_cmp(b).is_eq());

    let coordinate = |value: f32| {
        y_values
            .binary_search_by(|candidate| candidate.total_cmp(&value))
            .expect("rectangle endpoint was collected before the sweep")
    };
    let mut starts: Vec<usize> = (0..boxes.len()).collect();
    starts.sort_by(|&a, &b| boxes[a].min.x.total_cmp(&boxes[b].min.x).then(a.cmp(&b)));
    let mut ends: Vec<usize> = (0..boxes.len()).collect();
    ends.sort_by(|&a, &b| boxes[a].max.x.total_cmp(&boxes[b].max.x).then(a.cmp(&b)));

    let mut active = vec![false; boxes.len()];
    let mut min_y = RectSweepCounts::with_len(y_values.len());
    let mut max_y = RectSweepCounts::with_len(y_values.len());
    let mut end_cursor = 0;
    for index in starts {
        let current = boxes[index];
        while end_cursor < ends.len() && boxes[ends[end_cursor]].max.x < current.min.x {
            let expired = ends[end_cursor];
            if active[expired] {
                min_y.update(coordinate(boxes[expired].min.y), false);
                max_y.update(coordinate(boxes[expired].max.y), false);
                active[expired] = false;
            }
            end_cursor += 1;
        }

        // Active X intervals already intersect `current`. Among them, Y intervals overlap when
        // min_y <= current.max_y and max_y >= current.min_y. The two excluded sets are disjoint.
        let mins_at_or_below = y_values.partition_point(|value| *value <= current.max.y);
        let maxes_below = y_values.partition_point(|value| *value < current.min.y);
        let mut overlaps = min_y.prefix(mins_at_or_below) - max_y.prefix(maxes_below);

        // Consecutive spine segments share an endpoint and were deliberately excluded by the old
        // `j = i + 2` scan. At most two such intervals can be active, so remove them from the count.
        for neighbour in [
            index.checked_sub(1),
            (index + 1 < boxes.len()).then_some(index + 1),
        ]
        .into_iter()
        .flatten()
        {
            if active[neighbour] && current.intersects(boxes[neighbour]) {
                overlaps -= 1;
            }
        }
        if overlaps > 0 {
            return true;
        }

        min_y.update(coordinate(current.min.y), true);
        max_y.update(coordinate(current.max.y), true);
        active[index] = true;
    }
    false
}

fn round_brush_envelope(points: &[Pos2], pressures: &[f32], width: f32) -> Vec<PathCommand> {
    round_brush_envelope_with_overlap_check(points, pressures, width, nonadjacent_rects_overlap)
}

fn round_brush_envelope_with_overlap_check(
    points: &[Pos2],
    pressures: &[f32],
    width: f32,
    overlaps: fn(&[Rect]) -> bool,
) -> Vec<PathCommand> {
    if points.is_empty() || !width.is_finite() || width <= 0. {
        return vec![];
    }
    let raw_radii: Vec<f32> = if pressures.len() == points.len() {
        pressures
            .iter()
            .map(|&p| width * brush_force(Some(p)) * 0.5)
            .collect()
    } else {
        vec![width * 0.5; points.len()]
    };
    let (pts, radii) = simplify_round_spine(points, &raw_radii, 0.22, (width * 0.015).max(0.08));
    if pts.len() == 1 {
        let r = radii[0];
        return polyline(
            &(0..24)
                .map(|n| pts[0] + Vec2::angled(n as f32 * std::f32::consts::TAU / 24.) * r)
                .collect::<Vec<_>>(),
            true,
        );
    }

    // Check if the simplified spine can be emitted as a single non-self-folding perimeter contour.
    let m = pts.len();
    let mut dirs = Vec::with_capacity(m - 1);
    let mut clean_single_contour = true;
    for i in 0..(m - 1) {
        let d = pts[i + 1] - pts[i];
        let len = d.length();
        let max_r = radii[i].max(radii[i + 1]);
        if len <= 0.25
            || (radii[i + 1] - radii[i]).abs() >= len * 0.85
            || (m > 2 && len < max_r * 0.45)
        {
            clean_single_contour = false;
            break;
        }
        dirs.push(d / len);
    }
    if clean_single_contour {
        for i in 1..(m - 1) {
            if dirs[i - 1].dot(dirs[i]) < 0.35 {
                clean_single_contour = false;
                break;
            }
        }
    }
    if clean_single_contour && m > 3 {
        // Reject self-intersecting simplified spines so they take the disk+quad union path.
        let boxes: Vec<_> = (0..(m - 1))
            .map(|i| Rect::from_two_pos(pts[i], pts[i + 1]).expand(radii[i].max(radii[i + 1])))
            .collect();
        if overlaps(&boxes) {
            clean_single_contour = false;
        }
    }

    if clean_single_contour {
        let perp = |v: Vec2| Vec2::new(-v.y, v.x);
        let mut normals = Vec::with_capacity(m);
        normals.push(perp(dirs[0]));
        for i in 1..(m - 1) {
            let n0 = perp(dirs[i - 1]);
            let n1 = perp(dirs[i]);
            let bis = (n0 + n1).normalized();
            let scale = (1.0 / bis.dot(n1).max(0.75)).min(1.35);
            normals.push(bis * scale);
        }
        normals.push(perp(dirs[m - 2]));

        let cap_steps = 12usize;
        let mut boundary = Vec::with_capacity(m * 2 + cap_steps * 2 + 2);
        for i in 0..m {
            boundary.push(pts[i] + normals[i] * radii[i]);
        }
        let end_n = perp(dirs[m - 2]);
        let end_angle0 = end_n.angle();
        for step in 1..cap_steps {
            let t = step as f32 / cap_steps as f32;
            let ang = end_angle0 - t * std::f32::consts::PI;
            boundary.push(pts[m - 1] + Vec2::angled(ang) * radii[m - 1]);
        }
        for i in (0..m).rev() {
            boundary.push(pts[i] - normals[i] * radii[i]);
        }
        let start_n = -perp(dirs[0]);
        let start_angle0 = start_n.angle();
        for step in 1..cap_steps {
            let t = step as f32 / cap_steps as f32;
            let ang = start_angle0 - t * std::f32::consts::PI;
            boundary.push(pts[0] + Vec2::angled(ang) * radii[0]);
        }
        boundary.reverse();
        return polyline(&boundary, true);
    }

    // Fallback for sharp reversals or self-overlapping hooks: emit disks + quads on the simplified spine.
    let mut commands = Vec::with_capacity(pts.len() * 23);
    for (i, &point) in pts.iter().enumerate() {
        commands.extend(polyline(
            &(0..16)
                .map(|n| point + Vec2::angled(n as f32 * std::f32::consts::TAU / 16.) * radii[i])
                .collect::<Vec<_>>(),
            true,
        ));
        if i == 0 {
            continue;
        }
        let delta = point - pts[i - 1];
        if delta.length_sq() <= 1e-8 {
            continue;
        }
        let normal = Vec2::new(-delta.y, delta.x).normalized();
        commands.extend(polyline(
            &[
                pts[i - 1] - normal * radii[i - 1],
                point - normal * radii[i],
                point + normal * radii[i],
                pts[i - 1] + normal * radii[i - 1],
            ],
            true,
        ));
    }
    commands
}

#[cfg(test)]
fn brush_outline(points: &[Pos2], width: f32) -> Vec<PathCommand> {
    round_brush_envelope(points, &[], width)
}

/// Round, variable-width vector ink. Overlapping disks and connecting quads have
/// the same winding and are tessellated as one fill, including sharp reversals.
#[cfg(test)]
pub(crate) fn brush_outline_with_pressure(
    points: &[Pos2],
    pressures: &[f32],
    width: f32,
) -> Vec<PathCommand> {
    if points.is_empty() || !width.is_finite() || width <= 0. {
        return vec![];
    }
    if pressures.len() != points.len() || pressures.iter().all(|&p| brush_force(Some(p)) == 1.) {
        return brush_outline(points, width);
    }
    let radii: Vec<_> = pressures
        .iter()
        .map(|&p| width * brush_force(Some(p)) * 0.5)
        .collect();
    let mut commands = Vec::with_capacity(points.len() * 23);
    for (i, &point) in points.iter().enumerate() {
        commands.extend(polyline(
            &(0..16)
                .map(|n| point + Vec2::angled(n as f32 * std::f32::consts::TAU / 16.) * radii[i])
                .collect::<Vec<_>>(),
            true,
        ));
        if i == 0 {
            continue;
        }
        let delta = point - points[i - 1];
        if delta.length_sq() <= 1e-8 {
            continue;
        }
        let normal = Vec2::new(-delta.y, delta.x).normalized();
        commands.extend(polyline(
            &[
                points[i - 1] - normal * radii[i - 1],
                point - normal * radii[i],
                point + normal * radii[i],
                points[i - 1] + normal * radii[i - 1],
            ],
            true,
        ));
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
fn gradient_focal_ratio(g: &Gesture, gradient: &GradientGesture) -> Option<f64> {
    let elem_local = inverse(gradient.element_matrix, g.local(g.latest))?;
    let grad_local = inverse(gradient.matrix, elem_local)?;
    Some((f64::from(grad_local.x) / 819.2).clamp(-0.98, 0.98))
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
    radial_focal_ratio: Option<f64>,
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
    if let Some(focal_ratio) = radial_focal_ratio {
        let focal = screen(Pos2::new((focal_ratio * 819.2) as f32, 0.));
        let tri = vec![
            focal + Vec2::new(0., -5.),
            focal + Vec2::new(5., 4.),
            focal + Vec2::new(-5., 4.),
        ];
        p.add(Shape::convex_polygon(
            tri,
            palette.handle_fill,
            Stroke::new(1.5_f32, palette.overlay),
        ));
    }
    p.rect(
        Rect::from_center_size(edge, Vec2::splat(9.)),
        0,
        palette.handle_fill,
        Stroke::new(1.5_f32, palette.overlay),
        StrokeKind::Inside,
    );
}

impl Editor {
    /// Slice Merge-mode fills on the active editable layer along `points` (`T-128`,
    /// `editor-advanced-drawing :: lasso-partial-fills`), selecting the enclosed fill piece(s)
    /// in one `"Lasso split fill"` Undo step.
    pub fn lasso_select_partial_fill(&mut self, points: &[[f64; 2]]) -> Vec<Id> {
        self.finish_edits();
        if points.len() < 3 || !self.layer_editable() {
            return Vec::new();
        }
        let mut lasso_cmds = Vec::with_capacity(points.len() + 1);
        lasso_cmds.push(PathCommand::MoveTo {
            x: points[0][0],
            y: points[0][1],
        });
        for &[x, y] in &points[1..] {
            lasso_cmds.push(PathCommand::LineTo { x, y });
        }
        lasso_cmds.push(PathCommand::Close);

        let layer_idx = self.layer;
        let frame = self.frame;
        let mut candidate = self.project.clone();
        let sym_id = self.symbol_id().to_owned();
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return Vec::new();
        };
        let Some(layer) = sym.timeline.layers.get_mut(layer_idx) else {
            return Vec::new();
        };
        let layer_id = layer.id.clone();
        let Some(selected_ids) =
            crate::stage_selection::edit_ungrouped_merge_shapes(layer, frame, |elements| {
                lasso_split_merge_fills(elements, &lasso_cmds)
            })
        else {
            return Vec::new();
        };
        if selected_ids.is_empty() {
            return Vec::new();
        }
        candidate.upgrade_format_for_features();
        if candidate.validate().is_err() {
            return Vec::new();
        }
        if candidate != self.project {
            self.transact_named("Lasso split fill", |s| {
                s.project = candidate;
            });
        }
        let targets = selected_ids
            .iter()
            .cloned()
            .map(|element| SelectionTarget {
                layer: layer_id.clone(),
                element,
            })
            .collect();
        self.set_selection(targets);
        selected_ids
    }

    /// Update the radial gradient `focal_point_ratio` (`-0.98..=0.98`) on the selected gradient
    /// fill in one `"Gradient focal point"` Undo step (`T-128`, `editor-paint-effects :: focal-handles`).
    pub fn set_selected_gradient_focal_point(&mut self, focal_ratio: f64) -> bool {
        self.finish_edits();
        let Some(target) = self
            .selection
            .single()
            .filter(|t| self.target_editable(t))
            .cloned()
        else {
            return false;
        };
        let Some(element) = self.selected_sampled() else {
            return false;
        };
        let mut kind = element.kind;
        let ElementKind::PaintedPath {
            fill_gradient: Some(fill),
            ..
        } = &mut kind
        else {
            return false;
        };
        if fill.kind != GradientKind::Radial {
            return false;
        }
        fill.focal_point_ratio = focal_ratio.clamp(-0.98, 0.98);
        self.change_tool_elements(vec![(target, Some(kind))]);
        true
    }
    /// Options of the active tool (NF2 fields), shared by the Tools popover and Properties › Tool.
    pub(crate) fn authoring_tool_options(&mut self, ui: &mut egui::Ui) {
        use neoflash_ui::components as nf;
        let p = neoflash_ui::palette(ui.ctx());
        let note = |ui: &mut egui::Ui, text: &str, hint: Option<&str>| {
            let response = ui.label(
                neoflash_ui::typography::rich(text, neoflash_ui::TextStyle::Small)
                    .color(p.text_tertiary),
            );
            if let Some(hint) = hint {
                nf::tooltip(response, hint, None);
            }
        };
        let width = ui.available_width();
        match self.tool {
            Tool::Brush => self.brush_tool_contents(ui),
            Tool::Pencil => self.pencil_tool_contents(ui),
            Tool::Eraser => {
                nf::Select::options(
                    ui,
                    "eraser_mode",
                    &mut self.authoring_tools.eraser_mode,
                    &[
                        (EraserMode::Objects, "Whole objects"),
                        (EraserMode::Fills, "Whole fills"),
                        (EraserMode::Strokes, "Whole strokes"),
                        (EraserMode::StrokeSegments, "Cut strokes"),
                    ],
                    width,
                );
                let mut size = f64::from(self.authoring_tools.eraser_size);
                ui.add(
                    nf::Field::new(&mut size, "Eraser size")
                        .prefix("Size")
                        .unit("px")
                        .range(1. ..=128.)
                        .stepper()
                        .width(width),
                );
                self.authoring_tools.eraser_size = size as f32;
                let (text, hint) = match self.authoring_tools.eraser_mode {
                    EraserMode::Objects => (
                        "Remove touched objects",
                        "Removes each touched object completely, including its fill and stroke.",
                    ),
                    EraserMode::Fills => (
                        "Remove whole fills · keep strokes",
                        "Removes the entire fill of touched artwork. This mode does not cut holes into fills.",
                    ),
                    EraserMode::Strokes => (
                        "Remove whole strokes · keep fills",
                        "Removes the entire stroke of touched artwork, keeping its fill intact.",
                    ),
                    EraserMode::StrokeSegments => (
                        "Cut touched strokes · keep fills",
                        "Cuts the touched parts of vector strokes and keeps fills intact; touched curves are approximated. Masked artwork, combined fill/stroke effects and guided tweens needing new keys cannot be cut. A refused gesture changes no artwork.",
                    ),
                };
                note(ui, text, Some(hint));
            }
            Tool::Lasso => {
                let response = ui.add(nf::Checkbox::new(
                    &mut self.authoring_tools.lasso_contact,
                    "Touch selects",
                ));
                nf::tooltip(
                    response,
                    "Select whole placements whose bounds touch the freehand loop; turn off to require enclosure. Hold Shift to add.",
                    None,
                );
                let partial_resp = ui.add(nf::Checkbox::new(
                    &mut self.authoring_tools.lasso_partial_fills,
                    "Slice merge fills",
                ));
                nf::tooltip(
                    partial_resp,
                    "Slice Merge-mode fills along the freehand lasso contour (or hold Option while dragging) to select only the enclosed fill region.",
                    None,
                );
            }
            Tool::Zoom => {
                nf::Segmented::show(
                    ui,
                    &mut self.authoring_tools.zoom_out,
                    &[(false, "Zoom in"), (true, "Zoom out")],
                    true,
                );
                note(
                    ui,
                    "Click or drag area",
                    Some("Option temporarily zooms out. Drag a rectangle to fit that area."),
                );
            }
            Tool::Gradient => {
                note(ui, "Existing fill", None);
                note(ui, "Center: move", None);
                note(ui, "Square: scale / rotate", None);
            }
            Tool::Bucket => {
                let label = format!("Gap size: {}", self.authoring_tools.gap_close.label());
                let (response, _) = nf::Select::new("bucket_gap_size", &label)
                    .width(width)
                    .show(ui, |ui| {
                        for mode in [
                            GapCloseMode::None,
                            GapCloseMode::Small,
                            GapCloseMode::Medium,
                            GapCloseMode::Large,
                        ] {
                            if ui
                                .add(
                                    nf::MenuItem::new(mode.label())
                                        .checked(mode == self.authoring_tools.gap_close),
                                )
                                .clicked()
                            {
                                self.authoring_tools.gap_close = mode;
                            }
                        }
                    });
                nf::tooltip(
                    response,
                    "Closes each contour only when its end-to-start gap fits the chosen limit on screen: Small 6 px, Medium 14 px, Large 28 px. Fills authored vector shapes; bitmap flood fill is unsupported.",
                    None,
                );
                let text = match self.authoring_tools.gap_close {
                    GapCloseMode::None => "Fill closed vector shapes",
                    GapCloseMode::Small => "Close gaps up to 6 px on screen",
                    GapCloseMode::Medium => "Close gaps up to 14 px on screen",
                    GapCloseMode::Large => "Close gaps up to 28 px on screen",
                };
                note(ui, text, None);
            }
            Tool::Ink => note(
                ui,
                "Apply stroke",
                Some("Click vector artwork to apply the current stroke color and width."),
            ),
            Tool::Eyedropper => note(
                ui,
                "Sample color",
                Some(
                    "Click a stroke or fill, preserving alpha. Switches to Ink Bottle or Paint Bucket. Option chooses the stroke.",
                ),
            ),
            Tool::Line => note(ui, "Shift: 45°", None),
            Tool::PolyStar => {
                let kind = if self.authoring_tools.polystar_star {
                    "Star"
                } else {
                    "Polygon"
                };
                note(
                    ui,
                    &format!("{kind} · {} sides", self.authoring_tools.polystar_sides),
                    Some("Drag from centre to outer vertex. Hold Shift to snap to 45°."),
                );
                if ui.add(nf::Button::ghost("PolyStar settings…")).clicked() {
                    self.open_polystar_settings();
                }
            }
            Tool::Rectangle => {
                self.live_rectangle_tool_options(ui);
                if ui.add(nf::Button::ghost("PolyStar settings…")).clicked() {
                    self.open_polystar_settings();
                }
            }
            Tool::Ellipse => {
                self.live_oval_tool_options(ui);
            }
            Tool::Hand => {
                note(ui, "Drag to pan", None);
                note(ui, "Space: temporary", None);
            }
            _ => {}
        }
        if matches!(
            self.tool,
            Tool::Rectangle
                | Tool::Ellipse
                | Tool::PolyStar
                | Tool::Line
                | Tool::Pen
                | Tool::Pencil
                | Tool::Brush
        ) {
            nf::Segmented::show(
                ui,
                &mut self.workspace.drawing.object_drawing,
                &[(false, "Merge drawing"), (true, "Object drawing")],
                true,
            );
        }
        if matches!(
            self.tool,
            Tool::Select
                | Tool::Transform
                | Tool::Rectangle
                | Tool::Ellipse
                | Tool::PolyStar
                | Tool::Line
                | Tool::Pen
        ) {
            let snap_on = self.stage_view.prefs.objects_snap;
            let btn = ui.add(
                nf::IconButton::new(neoflash_ui::icons::i20::TOOL_SNAP, "Snap to objects")
                    .selected(snap_on),
            );
            if btn.clicked() {
                self.toggle_object_snap();
            }
        }
    }

    /// Open the NF2 `PolyStar tool settings` modal seeded from current PolyStar tool state.
    pub(crate) fn open_polystar_settings(&mut self) {
        self.authoring_tools.polystar_dialog = Some((
            self.authoring_tools.polystar_star,
            self.authoring_tools.polystar_sides.clamp(3, 32),
            self.authoring_tools.polystar_point_size.clamp(0.05, 0.95),
        ));
    }

    /// Render the NF2 `PolyStar tool settings` modal dialog when open.
    pub(crate) fn polystar_settings_modal(&mut self, ctx: &egui::Context) {
        let Some((mut star, mut sides, mut point_size)) = self.authoring_tools.polystar_dialog
        else {
            return;
        };
        use neoflash_ui::{
            components as c,
            tokens::{TextStyle, radius, space},
            typography::rich,
        };
        let mut accept = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("nf2-polystar-settings"))
            .frame(c::Surface::Dialog.frame(ctx))
            .show(ctx, |ui| {
                ui.set_width(400.0);
                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                cancel |= c::DialogHeader {
                    title: "PolyStar tool settings",
                }
                .show(ui);
                let nf = neoflash_ui::palette(ui.ctx());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 20.0;
                    let (frame, _) =
                        ui.allocate_exact_size(egui::vec2(110.0, 110.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        frame,
                        egui::CornerRadius::same(radius::LG as u8),
                        nf.bg_canvas,
                    );
                    let center = frame.center();
                    let count = if star { sides * 2 } else { sides } as usize;
                    let inner_r = 45.0 * (point_size.clamp(0.05, 0.95) as f32);
                    let points: Vec<Pos2> = (0..count)
                        .map(|i| {
                            let a = -std::f32::consts::FRAC_PI_2
                                + (i as f32) * std::f32::consts::TAU / (count as f32);
                            let r = if star && i % 2 == 1 { inner_r } else { 45.0 };
                            center + egui::vec2(a.cos(), a.sin()) * r
                        })
                        .collect();
                    let fill = if self.fill[3] > 0 {
                        Color32::from_rgba_unmultiplied(
                            self.fill[0],
                            self.fill[1],
                            self.fill[2],
                            self.fill[3],
                        )
                    } else {
                        nf.accent_default
                    };
                    let stroke = if self.stroke[3] > 0 {
                        Color32::from_rgba_unmultiplied(
                            self.stroke[0],
                            self.stroke[1],
                            self.stroke[2],
                            self.stroke[3],
                        )
                    } else {
                        nf.text_primary
                    };
                    for i in 0..points.len() {
                        ui.painter().add(Shape::convex_polygon(
                            vec![center, points[i], points[(i + 1) % points.len()]],
                            fill,
                            Stroke::NONE,
                        ));
                    }
                    ui.painter()
                        .add(Shape::closed_line(points, Stroke::new(2.0_f32, stroke)));

                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 10.0;
                        let w = ui.available_width();
                        c::Select::options(
                            ui,
                            "polystar-style",
                            &mut star,
                            &[(false, "Style: Polygon"), (true, "Style: Star")],
                            w,
                        );
                        let mut sides_f = f64::from(sides);
                        if ui
                            .add(
                                c::Field::new(&mut sides_f, "Sides")
                                    .prefix("Sides")
                                    .range(3.0..=32.0)
                                    .decimals(0)
                                    .stepper()
                                    .width(w),
                            )
                            .changed()
                        {
                            sides = (sides_f.round() as u32).clamp(3, 32);
                        }
                        ui.add_enabled(
                            star,
                            c::Field::new(&mut point_size, "Point size")
                                .prefix("Point size")
                                .range(0.05..=0.95)
                                .decimals(2)
                                .stepper()
                                .width(w),
                        );
                        ui.label(
                            rich("Point size 0–1: smaller = sharper points", TextStyle::Small)
                                .color(nf.text_tertiary),
                        );
                    });
                });
                ui.add_space(space::S2);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(c::Button::primary("OK")).clicked() {
                        accept = true;
                    }
                    if ui.add(c::Button::new("Cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        if modal.should_close() {
            cancel = true;
        }
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            accept = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if accept {
            self.authoring_tools.polystar_star = star;
            self.authoring_tools.polystar_sides = sides.clamp(3, 32);
            self.authoring_tools.polystar_point_size = point_size.clamp(0.05, 0.95);
            self.authoring_tools.polystar_dialog = None;
            self.set_tool(Tool::PolyStar);
        } else if cancel {
            self.authoring_tools.polystar_dialog = None;
        } else {
            self.authoring_tools.polystar_dialog = Some((star, sides, point_size));
        }
    }

    /// Renders NF2 `Rectangle options` or `Oval options` in Properties for a selected
    /// `Rectangle` or `Ellipse`, returning `Some(control_id)` when `element` changed.
    pub(crate) fn primitive_options_properties_ui(
        &mut self,
        ui: &mut egui::Ui,
        element: &mut Element,
    ) -> Option<egui::Id> {
        use neoflash_ui::components as c;
        match &element.kind {
            ElementKind::Rectangle { .. } => {
                let mut opts = element.rectangle_options();
                let mut changed = None;
                c::SectionHeader::new("Rectangle options")
                    .id_salt(("primitive-rect-options", &element.id))
                    .default_open(true)
                    .show(ui, |ui| {
                        changed = rectangle_options_widgets(ui, &mut opts);
                    });
                if changed.is_some() {
                    element.set_rectangle_options(opts);
                    self.authoring_tools.rect_options = opts;
                }
                changed
            }
            ElementKind::Ellipse { .. } => {
                let mut opts = element.oval_options();
                let mut changed = None;
                c::SectionHeader::new("Oval options")
                    .id_salt(("primitive-oval-options", &element.id))
                    .default_open(true)
                    .show(ui, |ui| {
                        changed = oval_options_widgets(ui, &mut opts);
                    });
                if changed.is_some() {
                    element.set_oval_options(opts);
                    self.authoring_tools.oval_options = opts;
                }
                changed
            }
            _ => None,
        }
    }

    fn live_rectangle_tool_options(&mut self, ui: &mut egui::Ui) {
        let selected_rect = self
            .selection
            .single()
            .filter(|t| self.target_editable(t))
            .cloned()
            .and_then(|target| {
                let el = self.selected_sampled()?;
                matches!(el.kind, ElementKind::Rectangle { .. })
                    .then(|| (target, el.rectangle_options()))
            });
        let mut opts = selected_rect
            .as_ref()
            .map(|(_, o)| *o)
            .unwrap_or(self.authoring_tools.rect_options);
        if let Some(control) = rectangle_options_widgets(ui, &mut opts) {
            self.authoring_tools.rect_options = opts;
            if let Some((target, _)) = selected_rect {
                let scope = egui::Id::new("tool-rect-primitive-options");
                self.begin_property_edit(scope, control);
                self.ensure_key_for(&target.layer, false);
                if let Some(layer_idx) = self.target_layer_index(&target) {
                    let frame = self.frame;
                    if let Some(el) = self.symbol_mut().timeline.layers[layer_idx]
                        .key_at_mut(frame)
                        .and_then(|k| k.elements.iter_mut().find(|e| e.id == target.element))
                    {
                        el.set_rectangle_options(opts);
                    }
                    self.project.upgrade_format_for_features();
                }
            }
        }
    }

    fn live_oval_tool_options(&mut self, ui: &mut egui::Ui) {
        let selected_oval = self
            .selection
            .single()
            .filter(|t| self.target_editable(t))
            .cloned()
            .and_then(|target| {
                let el = self.selected_sampled()?;
                matches!(el.kind, ElementKind::Ellipse { .. }).then(|| (target, el.oval_options()))
            });
        let mut opts = selected_oval
            .as_ref()
            .map(|(_, o)| *o)
            .unwrap_or(self.authoring_tools.oval_options);
        if let Some(control) = oval_options_widgets(ui, &mut opts) {
            self.authoring_tools.oval_options = opts;
            if let Some((target, _)) = selected_oval {
                let scope = egui::Id::new("tool-oval-primitive-options");
                self.begin_property_edit(scope, control);
                self.ensure_key_for(&target.layer, false);
                if let Some(layer_idx) = self.target_layer_index(&target) {
                    let frame = self.frame;
                    if let Some(el) = self.symbol_mut().timeline.layers[layer_idx]
                        .key_at_mut(frame)
                        .and_then(|k| k.elements.iter_mut().find(|e| e.id == target.element))
                    {
                        el.set_oval_options(opts);
                    }
                    self.project.upgrade_format_for_features();
                }
            }
        }
    }
}

fn rectangle_options_widgets(ui: &mut egui::Ui, opts: &mut RectangleOptions) -> Option<egui::Id> {
    use neoflash_ui::{components as c, icons::i16, tokens::space};
    let mut changed = None;
    let w = ((ui.available_width() - 32.0 - 2.0 * space::S2) / 2.0).max(56.0);
    let mut r = opts.corner_radii;
    let mut edited_corner = None;
    ui.horizontal(|ui| {
        let tl = ui.add(
            c::Field::new(&mut r[0], "Top-left radius")
                .prefix("↖")
                .range(0.0..=500.0)
                .decimals(0)
                .stepper()
                .width(w),
        );
        if tl.changed() {
            edited_corner = Some((0, tl.id));
        }
        let tr = ui.add(
            c::Field::new(&mut r[1], "Top-right radius")
                .prefix("↗")
                .range(0.0..=500.0)
                .decimals(0)
                .stepper()
                .width(w),
        );
        if tr.changed() {
            edited_corner = Some((1, tr.id));
        }
        let link =
            ui.add(c::IconButton::new(i16::LINK, "Lock corners").selected(opts.corners_locked));
        if link.clicked() {
            opts.corners_locked = !opts.corners_locked;
            if opts.corners_locked {
                r = [r[0]; 4];
            }
            changed = Some(link.id);
        }
    });
    ui.horizontal(|ui| {
        let bl = ui.add(
            c::Field::new(&mut r[3], "Bottom-left radius")
                .prefix("↙")
                .range(0.0..=500.0)
                .decimals(0)
                .stepper()
                .width(w),
        );
        if bl.changed() {
            edited_corner = Some((3, bl.id));
        }
        let br = ui.add(
            c::Field::new(&mut r[2], "Bottom-right radius")
                .prefix("↘")
                .range(0.0..=500.0)
                .decimals(0)
                .stepper()
                .width(w),
        );
        if br.changed() {
            edited_corner = Some((2, br.id));
        }
    });
    if let Some((idx, id)) = edited_corner {
        let v = r[idx].clamp(0.0, 500.0);
        if opts.corners_locked {
            r = [v; 4];
        } else {
            r[idx] = v;
        }
        changed = Some(id);
    }
    let mut slider_val = r[0].clamp(0.0, 100.0);
    let slider = ui.add(
        c::Slider::new(&mut slider_val, 0.0..=100.0, "Corner radius").width(2.0 * w + space::S2),
    );
    if slider.changed() {
        r = [slider_val; 4];
        changed = Some(slider.id);
    }
    opts.corner_radii = r;
    let reset = ui.add(c::Button::ghost("Reset"));
    if reset.clicked() && !opts.is_default() {
        *opts = RectangleOptions::default();
        changed = Some(reset.id);
    }
    changed
}

fn oval_options_widgets(ui: &mut egui::Ui, opts: &mut OvalOptions) -> Option<egui::Id> {
    use neoflash_ui::{components as c, icons::i16, tokens::space};
    let mut changed = None;
    let w = ((ui.available_width() - space::S2) / 2.0).max(64.0);
    ui.horizontal(|ui| {
        let s = ui.add(
            c::Field::new(&mut opts.start_angle, "Start")
                .prefix("Start")
                .unit("°")
                .range(0.0..=360.0)
                .decimals(0)
                .stepper()
                .width(w),
        );
        if s.changed() {
            opts.start_angle = opts.start_angle.clamp(0.0, 360.0);
            changed = Some(s.id);
        }
        let e = ui.add(
            c::Field::new(&mut opts.end_angle, "End")
                .prefix("End")
                .unit("°")
                .range(0.0..=360.0)
                .decimals(0)
                .stepper()
                .width(w),
        );
        if e.changed() {
            opts.end_angle = opts.end_angle.clamp(0.0, 360.0);
            changed = Some(e.id);
        }
    });
    ui.horizontal(|ui| {
        let inner = ui.add(
            c::Field::new(&mut opts.inner_radius, "Inner")
                .prefix("Inner")
                .unit("%")
                .range(0.0..=99.0)
                .decimals(0)
                .stepper()
                .width(w),
        );
        if inner.changed() {
            opts.inner_radius = opts.inner_radius.clamp(0.0, 99.0);
            changed = Some(inner.id);
        }
        let close = ui.add(c::Checkbox::new(&mut opts.close_path, "Close path"));
        if close.changed() {
            changed = Some(close.id);
        }
    });
    let reset = ui.add(c::Button::ghost("Reset").icon(i16::RESET_TRANSFORM));
    if reset.clicked() && !opts.is_default() {
        *opts = OvalOptions::default();
        changed = Some(reset.id);
    }
    changed
}

impl Editor {
    fn brush_gesture_commands(&self, gesture: &Gesture) -> Vec<PathCommand> {
        let mut points = gesture.points.clone();
        let smoothing = (self.authoring_tools.brush_smoothing / 100.) as f32;
        if smoothing > 0. && points.len() > 2 {
            for (index, triple) in gesture.points.windows(3).enumerate() {
                points[index + 1] =
                    triple[1].lerp(triple[0].lerp(triple[2], 0.5), smoothing * 0.75);
            }
        }
        let mut pressure = if self.authoring_tools.brush_pressure {
            gesture.pressures.clone()
        } else {
            vec![1.; points.len()]
        };
        if smoothing > 0. && points.len() > 3 && pressure.len() == points.len() {
            let pos_tol = (0.15 + smoothing * 0.65) / gesture.zoom.max(0.1);
            (points, pressure) = simplify_round_spine(&points, &pressure, pos_tol, 0.025);
        }
        shaped_brush_outline(
            &points,
            &pressure,
            self.authoring_tools.brush_size,
            self.authoring_tools.brush_shape,
        )
    }
}

/// Sweep the exact same convex tip used by the shared cursor. Adjacent hulls overlap, so
/// nonzero fill produces a continuous stroke even at corners and with stationary pressure.
pub(crate) fn shaped_brush_outline(
    points: &[Pos2],
    pressures: &[f32],
    size: f32,
    shape: neoflash_ui::components::BrushShapeKind,
) -> Vec<PathCommand> {
    use neoflash_ui::components::BrushShapeKind;
    if shape == BrushShapeKind::Round {
        return round_brush_envelope(points, pressures, size);
    }
    if points.is_empty() || !size.is_finite() || size <= 0. {
        return vec![];
    }
    let raw_forces: Vec<f32> = (0..points.len())
        .map(|i| brush_force(pressures.get(i).copied()))
        .collect();
    let (pts, forces) = simplify_round_spine(points, &raw_forces, 0.22, 0.02);
    let tips: Vec<_> = pts
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let tip = shape.footprint(size * forces[i]);
            if tip.ellipse {
                let rotation = egui::emath::Rot2::from_angle(tip.angle);
                (0..24)
                    .map(|n| {
                        let angle = n as f32 * std::f32::consts::TAU / 24.;
                        p + rotation * Vec2::new(tip.half.x * angle.cos(), tip.half.y * angle.sin())
                    })
                    .collect()
            } else {
                tip.outline(p)
            }
        })
        .collect();
    if tips.len() == 1 {
        return polyline(&tips[0], true);
    }
    tips.windows(2)
        .flat_map(|pair| {
            polyline(
                &convex_tip_hull(pair.iter().flatten().copied().collect()),
                true,
            )
        })
        .collect()
}
fn convex_tip_hull(mut points: Vec<Pos2>) -> Vec<Pos2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let cross = |a: Pos2, b: Pos2, c: Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let mut hull = Vec::new();
    for &p in &points {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0. {
            hull.pop();
        }
        hull.push(p);
    }
    let lower = hull.len();
    for &p in points.iter().rev().skip(1) {
        while hull.len() > lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0. {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    hull
}

#[cfg(test)]
mod round_brush_overlap_tests {
    use super::*;

    fn random_unit(state: &mut u64) -> f32 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        (*state as u32) as f32 / u32::MAX as f32
    }

    #[test]
    fn indexed_overlap_matches_the_previous_scan_including_touching_and_adjacency() {
        let explicit = [
            vec![],
            vec![Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.))],
            vec![
                Rect::from_min_max(Pos2::new(0., 0.), Pos2::new(1., 1.)),
                Rect::from_min_max(Pos2::new(1., 0.), Pos2::new(2., 1.)),
                Rect::from_min_max(Pos2::new(2., 0.), Pos2::new(3., 1.)),
            ],
            vec![
                Rect::from_min_max(Pos2::new(0., 0.), Pos2::new(1., 1.)),
                Rect::from_min_max(Pos2::new(4., 4.), Pos2::new(5., 5.)),
                Rect::from_min_max(Pos2::new(1., 1.), Pos2::new(2., 2.)),
            ],
        ];
        for boxes in explicit {
            assert_eq!(
                nonadjacent_rects_overlap(&boxes),
                nonadjacent_rects_overlap_bruteforce(&boxes)
            );
        }

        let mut state = 0x8f37_14c2_9d05_6ab1_u64;
        for len in 0..96 {
            let boxes: Vec<_> = (0..len)
                .map(|_| {
                    // Quantised coordinates deliberately create equal and just-touching bounds.
                    let x = (random_unit(&mut state) * 48.).floor() - 24.;
                    let y = (random_unit(&mut state) * 48.).floor() - 24.;
                    let width = (random_unit(&mut state) * 9.).floor();
                    let height = (random_unit(&mut state) * 9.).floor();
                    Rect::from_min_max(Pos2::new(x, y), Pos2::new(x + width, y + height))
                })
                .collect();
            assert_eq!(
                nonadjacent_rects_overlap(&boxes),
                nonadjacent_rects_overlap_bruteforce(&boxes),
                "rectangle count {len}"
            );
        }
    }

    #[test]
    fn indexed_overlap_keeps_round_brush_commands_byte_for_byte() {
        let mut state = 0x51a9_b20d_78e4_c633_u64;
        for case in 0..64 {
            let count = 4 + case % 61;
            let mut points = Vec::with_capacity(count);
            let mut pressures = Vec::with_capacity(count);
            let mut point = Pos2::ZERO;
            for index in 0..count {
                if index > 0 {
                    let angle = (random_unit(&mut state) - 0.5) * std::f32::consts::PI;
                    let distance = 0.3 + random_unit(&mut state) * 8.;
                    point += Vec2::angled(angle) * distance;
                    if index % 13 == 0 {
                        point = points[index / 3];
                    }
                }
                points.push(point);
                pressures.push(0.1 + random_unit(&mut state) * 0.9);
            }
            let width = 1. + random_unit(&mut state) * 60.;
            let indexed = round_brush_envelope_with_overlap_check(
                &points,
                &pressures,
                width,
                nonadjacent_rects_overlap,
            );
            let previous = round_brush_envelope_with_overlap_check(
                &points,
                &pressures,
                width,
                nonadjacent_rects_overlap_bruteforce,
            );
            assert_eq!(indexed, previous, "round Brush case {case}");
        }
    }

    #[test]
    fn indexed_overlap_handles_the_maximum_disjoint_sweep_without_pair_scanning() {
        // All X intervals overlap, but all Y intervals are disjoint. The previous scan considers
        // 2,092,035 candidate pairs at the Brush's 2,048-point limit; the indexed result is false.
        let count = 2047_usize;
        let mut boxes: Vec<_> = (0..count)
            .map(|index| {
                let y = index as f32 * 3.;
                Rect::from_min_max(Pos2::new(0., y), Pos2::new(10_000., y + 1.))
            })
            .collect();
        let old_candidate_pairs = (count - 1) * (count - 2) / 2;
        assert!(old_candidate_pairs > 2_000_000);
        assert!(!nonadjacent_rects_overlap(&boxes));

        // Touching is inclusive in egui::Rect::intersects and must still select the fallback.
        boxes[count - 1] = Rect::from_min_max(Pos2::new(10_000., 1.), Pos2::new(10_001., 2.));
        assert!(nonadjacent_rects_overlap(&boxes));
    }
}
