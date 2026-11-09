//! Direct authoring tools. Gestures are drafts; a completed edit is one undo step.
//! Interaction follows the user's Flash CS4 guide (drawing, color, selection, view).
use crate::{
    app::{Editor, Tool},
    selection::SelectionTarget,
    stage::{affine, inverse},
    stage_mapping::StageMapping,
    theme::UiPalette,
};
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};
use neoflash_core::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum FreeTransformMode {
    #[default]
    FreeTransform,
    RotateAndSkew,
    Scale,
    Distort,
    Envelope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum EraserMode {
    Objects,
    Fills,
    Strokes,
    #[default]
    StrokeSegments,
    SelectedFills,
    Inside,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum EraserShape {
    #[default]
    Round,
    Square,
}

impl EraserShape {
    pub(crate) fn brush_shape(self) -> neoflash_ui::components::BrushShapeKind {
        match self {
            Self::Round => neoflash_ui::components::BrushShapeKind::Round,
            Self::Square => neoflash_ui::components::BrushShapeKind::Square,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum MagicWandSmoothing {
    Pixels,
    Rough,
    #[default]
    Normal,
    Smooth,
}

impl MagicWandSmoothing {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pixels => "Pixels",
            Self::Rough => "Rough",
            Self::Normal => "Normal",
            Self::Smooth => "Smooth",
        }
    }
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
    pub eraser_shape: EraserShape,
    pub gap_close: GapCloseMode,
    pub lasso_contact: bool,
    pub lasso_partial_fills: bool,
    pub lasso_polygon_mode: bool,
    pub lasso_polygon: Vec<Pos2>,
    pub lasso_magic_wand: bool,
    pub magic_wand_threshold: u8,
    pub magic_wand_smoothing: MagicWandSmoothing,
    pub magic_wand_dialog: Option<(u8, MagicWandSmoothing)>,
    pub zoom_out: bool,
    pub polystar_star: bool,
    pub polystar_sides: u32,
    pub polystar_point_size: f64,
    pub polystar_dialog: Option<(bool, u32, f64)>,
    pub rect_options: RectangleOptions,
    pub oval_options: OvalOptions,
    pub transform_mode: FreeTransformMode,
    pub info_anchor: neoflash_ui::components::RegistrationPoint,
    pub primitive_drawn_selection: bool,
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
            eraser_mode: EraserMode::StrokeSegments,
            eraser_shape: EraserShape::Round,
            gap_close: GapCloseMode::None,
            lasso_contact: true,
            lasso_partial_fills: true,
            lasso_polygon_mode: false,
            lasso_polygon: Vec::new(),
            lasso_magic_wand: false,
            magic_wand_threshold: 20,
            magic_wand_smoothing: MagicWandSmoothing::Normal,
            magic_wand_dialog: None,
            zoom_out: false,
            polystar_star: false,
            polystar_sides: 5,
            polystar_point_size: 0.5,
            polystar_dialog: None,
            rect_options: RectangleOptions::default(),
            oval_options: OvalOptions::default(),
            transform_mode: FreeTransformMode::FreeTransform,
            info_anchor: neoflash_ui::components::RegistrationPoint::TopLeft,
            primitive_drawn_selection: false,
        }
    }
}
impl AuthoringToolsState {
    pub fn cancel(&mut self) {
        self.gesture = None;
        self.pressure_contact = None;
        self.lasso_polygon.clear();
        if let Some((_, size)) = self.brush_resize.take() {
            self.brush_size = size;
        }
    }
    pub fn is_drawing(&self) -> bool {
        self.gesture.is_some() || !self.lasso_polygon.is_empty()
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

fn multiply_matrices(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

struct Gesture {
    tool: Tool,
    symbol: Id,
    frame: u32,
    layer: Id,
    origin: Pos2,
    zoom: f32,
    mapping: StageMapping,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GradientHandleKind {
    Center,
    Width,
    Radius,
    Rotate,
    Focal,
}

struct GradientGesture {
    target: SelectionTarget,
    kind: ElementKind,
    matrix: [f64; 6],
    element_matrix: [f64; 6],
    handle_kind: GradientHandleKind,
    radial: bool,
    focal_point_ratio: f64,
}
impl Gesture {
    fn local(&self, point: Pos2) -> Pos2 {
        self.mapping.screen_to_edit(self.origin, self.zoom, point)
    }
}
const POLYSTAR_MIN_DRAG_PX: f32 = 2.0;
fn polystar_drag_is_large_enough(
    mapping: StageMapping,
    origin: Pos2,
    zoom: f32,
    center: Pos2,
    outer: Pos2,
) -> bool {
    mapping
        .edit_to_screen(origin, zoom, center)
        .distance(mapping.edit_to_screen(origin, zoom, outer))
        > POLYSTAR_MIN_DRAG_PX
}
fn local_position_tolerance(mapping: StageMapping, zoom: f32, screen_tolerance: f32) -> f32 {
    screen_tolerance / (zoom.max(0.1) * mapping.scale_bounds().1.max(1e-6) as f32)
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

fn transformed_geometry_contains(
    commands: &[PathCommand],
    rule: FillRule,
    width: f64,
    local_to_metric: [f64; 6],
    point: Pos2,
) -> bool {
    if width <= 0. {
        return false;
    }
    let Ok(mesh) = crate::vector::tessellate(commands, rule, Some(width), 0.2) else {
        return false;
    };
    mesh.indices.as_chunks::<3>().0.iter().any(|indices| {
        let triangle = indices.map(|index| {
            affine(
                local_to_metric,
                Pos2::new(
                    mesh.vertices[index as usize].x,
                    mesh.vertices[index as usize].y,
                ),
            )
        });
        let cross = |a: usize, b: usize| {
            (triangle[b].x - triangle[a].x) * (point.y - triangle[a].y)
                - (triangle[b].y - triangle[a].y) * (point.x - triangle[a].x)
        };
        let (a, b, c) = (cross(0, 1), cross(1, 2), cross(2, 0));
        (a >= -0.001 && b >= -0.001 && c >= -0.001) || (a <= 0.001 && b <= 0.001 && c <= 0.001)
    })
}

fn point_segment_distance(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let edge = b - a;
    let t = if edge.length_sq() > 0. {
        ((point - a).dot(edge) / edge.length_sq()).clamp(0., 1.)
    } else {
        0.
    };
    point.distance(a + edge * t)
}

/// Test the actual transformed stroke plus a constant 2.5 px centreline slop. Unlike a scalar
/// inverse scale, this stays local to the pointer under arbitrary affine edit-in-place mapping.
fn contour_hit_in_screen_metric(
    commands: &[PathCommand],
    rule: FillRule,
    width: f64,
    local_to_stage: [f64; 6],
    point_stage: Pos2,
    zoom: f32,
) -> bool {
    if transformed_geometry_contains(commands, rule, width, local_to_stage, point_stage) {
        return true;
    }
    let Ok(metric) = StageMapping::try_new(local_to_stage) else {
        return false;
    };
    let flatten_tolerance =
        (0.25 / (f64::from(zoom) * metric.scale_bounds().1.max(1e-6))).clamp(1e-5, 1.0);
    let Ok(contours) = flatten_path(
        commands,
        FlattenOptions {
            tolerance: flatten_tolerance,
            ..FlattenOptions::default()
        },
    ) else {
        return false;
    };
    let tolerance = 2.5 / zoom.max(1e-6);
    contours.iter().any(|contour| {
        contour.points.windows(2).any(|edge| {
            point_segment_distance(
                point_stage,
                affine(
                    local_to_stage,
                    Pos2::new(edge[0][0] as f32, edge[0][1] as f32),
                ),
                affine(
                    local_to_stage,
                    Pos2::new(edge[1][0] as f32, edge[1][1] as f32),
                ),
            ) <= tolerance
        }) || (contour.closed
            && contour.points.len() > 1
            && point_segment_distance(
                point_stage,
                affine(
                    local_to_stage,
                    Pos2::new(
                        contour.points.last().unwrap()[0] as f32,
                        contour.points.last().unwrap()[1] as f32,
                    ),
                ),
                affine(
                    local_to_stage,
                    Pos2::new(contour.points[0][0] as f32, contour.points[0][1] as f32),
                ),
            ) <= tolerance)
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
        mapping: StageMapping,
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
            let metric = mapping.map_matrix(item.transform);
            let contour_hit = contour_hit_in_screen_metric(
                &commands,
                rule,
                width,
                metric,
                mapping.edit_to_stage_pos(point),
                self.zoom,
            );
            let outline = (stroke[3] > 0 && width > 0. && contour_hit)
                || (self.tool == Tool::Ink && contour_hit);
            let mut metric = metric;
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

    fn paint_bucket_planar_face_at(
        &mut self,
        painter: &egui::Painter,
        point: Pos2,
        drag_end: Option<Pos2>,
        mapping: StageMapping,
    ) -> bool {
        if !self.layer_editable() {
            return false;
        }
        let layer = &self.symbol().timeline.layers[self.layer];
        let elements = sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps));
        let boundary_ids: std::collections::HashSet<_> =
            elements.iter().map(|element| element.id.as_str()).collect();
        let authored =
            self.presented_interactive_authored_scene(self.show_layer_masking(painter.ctx()));
        if let Some(masked_boundary) = authored.draws.iter().find(|draw| {
            draw.item.instance_path.len() == 1
                && draw
                    .item
                    .instance_path
                    .first()
                    .is_some_and(|id| boundary_ids.contains(id.as_str()))
                && !draw.clip_ids.is_empty()
        }) && !authored.contains_point(
            &masked_boundary.item.instance_path,
            [f64::from(point.x), f64::from(point.y)],
        ) {
            return false;
        }
        let mut screen_metric = mapping.matrix();
        for value in &mut screen_metric {
            *value *= f64::from(self.zoom);
        }
        let commands = match paint_bucket_planar_face_in_metric(
            &elements,
            [f64::from(point.x), f64::from(point.y)],
            screen_metric,
            self.authoring_tools.gap_close.max_gap_px(),
        ) {
            Ok(Some(commands)) => commands,
            Ok(None) => return false,
            Err(error) => {
                self.notify(
                    neoflash_ui::components::ToastType::Warning,
                    format!(
                        "Paint Bucket stopped before editing: {error}. Simplify the active layer or isolate a smaller region."
                    ),
                );
                return true;
            }
        };
        let id = new_id("object");
        let mut element = Element {
            id: id.clone(),
            name: format!("fill_{}", id.rsplit('-').next().unwrap_or("1")),
            merge: true,
            kind: ElementKind::Path {
                commands,
                fill_rule: FillRule::NonZero,
                fill: self.fill,
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        self.apply_active_tool_paint_with_bucket(
            &mut element,
            false,
            Some(point),
            drag_end.map(|end| (point, end)),
        );
        self.transact_named("Paint Bucket face", |editor| {
            editor.ensure_key(false);
            let layer = editor.layer;
            let frame = editor.frame;
            let created = crate::stage_selection::edit_ungrouped_merge_shapes(
                &mut editor.symbol_mut().timeline.layers[layer],
                frame,
                |elements| apply_merge_shape_to_keyframe(elements, element),
            )
            .unwrap_or_default();
            let layer_id = editor.symbol().timeline.layers[layer].id.clone();
            editor.set_selection(
                created
                    .into_iter()
                    .map(|element| SelectionTarget {
                        layer: layer_id.clone(),
                        element,
                    })
                    .collect(),
            );
        });
        true
    }

    fn paint_tool_click(
        &mut self,
        scene: &[DrawItem],
        p: &egui::Painter,
        point: Pos2,
        mapping: StageMapping,
        alt: bool,
    ) {
        self.paint_tool_click_or_bucket(scene, p, point, None, mapping, alt);
    }

    fn paint_tool_click_or_bucket(
        &mut self,
        scene: &[DrawItem],
        p: &egui::Painter,
        point: Pos2,
        drag_end: Option<Pos2>,
        mapping: StageMapping,
        alt: bool,
    ) {
        let tool = self.tool;
        let Some(hit) = self.tool_paint_hit(scene, p, point, mapping, tool == Tool::Bucket) else {
            if tool == Tool::Bucket && self.paint_bucket_planar_face_at(p, point, drag_end, mapping)
            {
                return;
            }
            self.message(if tool==Tool::Bucket {"Paint Bucket fills existing closed vector shapes. Close an open path first; enter a symbol to edit its artwork."} else {"Choose vector artwork on an unlocked layer; enter a symbol to edit its artwork."});
            return;
        };
        let (fill, stroke, width) = paint_parts(&hit.kind).unwrap();
        if tool == Tool::Eyedropper {
            if hit.stroke || alt {
                if let ElementKind::PaintedPath {
                    stroke_gradient: Some(gradient),
                    ..
                } = &hit.kind
                {
                    self.stroke = gradient.sample(hit.local.x as f64, hit.local.y as f64);
                    self.authoring_modify.drawing_stroke_gradient = Some(gradient.clone());
                } else {
                    self.stroke = stroke;
                    self.authoring_modify.drawing_stroke_gradient = None;
                }
                self.stroke_width = width;
            } else if let Some(gradient) = paint_gradient(&hit.kind) {
                self.fill = gradient.sample(hit.local.x as f64, hit.local.y as f64);
                self.authoring_modify.drawing_fill_gradient = Some(gradient.clone());
                self.authoring_tools.brush_lock_fill = true;
            } else {
                self.fill = fill;
                self.authoring_modify.drawing_fill_gradient = None;
            }
            return;
        }
        let mut element = self
            .target_layer_index(&hit.target)
            .and_then(|idx| {
                sample_layer_at_fps(
                    &self.symbol().timeline.layers[idx],
                    self.frame,
                    f64::from(self.project.fps),
                )
                .into_iter()
                .find(|e| e.id == hit.target.element)
            })
            .unwrap_or_else(|| Element {
                kind: hit.kind.clone(),
                ..Default::default()
            });
        if tool == Tool::Ink
            && is_merge_fill(&element)
            && self.grouped_targets(&hit.target).len() == 1
            && let Some((commands, fill_rule)) = element.to_path_commands()
        {
            let id = new_id("object");
            let mut outline = Element {
                id: id.clone(),
                name: format!("stroke_{}", id.rsplit('-').next().unwrap_or("1")),
                transform: element.transform,
                merge: true,
                visible: element.visible,
                kind: ElementKind::Path {
                    commands,
                    fill_rule,
                    fill: [0; 4],
                    stroke: self.stroke,
                    stroke_width: self.stroke_width,
                },
                ..Element::default()
            };
            self.apply_active_tool_paint(&mut outline, true);
            let layer_id = hit.target.layer.clone();
            self.transact_named("Ink Bottle contour", |editor| {
                editor.ensure_key_for(&layer_id, false);
                let Some(index) = editor
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .position(|layer| layer.id == layer_id)
                else {
                    return;
                };
                let frame = editor.frame;
                let created = crate::stage_selection::edit_ungrouped_merge_shapes(
                    &mut editor.symbol_mut().timeline.layers[index],
                    frame,
                    |elements| apply_merge_shape_to_keyframe(elements, outline),
                )
                .unwrap_or_default();
                editor.set_selection(
                    created
                        .into_iter()
                        .map(|element| SelectionTarget {
                            layer: layer_id.clone(),
                            element,
                        })
                        .collect(),
                );
            });
            return;
        }
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
        let local_end = drag_end.and_then(|end| inverse(hit.matrix, end));
        self.apply_active_tool_paint_with_bucket(
            &mut element,
            tool == Tool::Ink,
            (tool == Tool::Bucket).then_some(hit.local),
            (tool == Tool::Bucket)
                .then_some(local_end)
                .flatten()
                .map(|end| (hit.local, end)),
        );
        self.change_tool_elements(vec![(hit.target.clone(), Some(element.kind))]);
        self.set_selection(vec![hit.target]);
    }

    /// Cut through raw Merge fills and strokes with the swept eraser. Object Drawing paths retain
    /// the established stroke-segment behavior so filters, groups and instance identity are never
    /// silently flattened.
    fn erase_stroke_parts(&mut self, gesture: &Gesture, painter: &egui::Painter) {
        let mode = self.authoring_tools.eraser_mode;
        let selected_ids: std::collections::HashSet<Id> = self
            .selection
            .targets
            .iter()
            .map(|t| t.element.clone())
            .collect();
        let inside_target_id: Option<Id> = (mode == EraserMode::Inside)
            .then(|| gesture.targets.first().map(|t| t.element.clone()))
            .flatten();
        let local_points: Vec<_> = gesture
            .points
            .iter()
            .map(|p| [f64::from(p.x), f64::from(p.y)])
            .collect();
        let points: Vec<_> = local_points
            .iter()
            .map(|point| gesture.mapping.edit_to_stage_point(*point))
            .collect();
        let affected_layers: std::collections::HashSet<_> = gesture
            .targets
            .iter()
            .map(|target| target.layer.clone())
            .collect();
        let authored =
            self.presented_interactive_authored_scene(self.show_layer_masking(painter.ctx()));
        let mut changes = Vec::new();
        for target in &gesture.targets {
            if mode == EraserMode::SelectedFills && !selected_ids.contains(&target.element) {
                continue;
            }
            if mode == EraserMode::Inside
                && inside_target_id.as_deref() != Some(target.element.as_str())
            {
                continue;
            }
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
            let Some(element) = sample_layer_at_fps(
                &self.symbol().timeline.layers[index],
                self.frame,
                f64::from(self.project.fps),
            )
            .into_iter()
            .find(|element| element.id == target.element) else {
                continue;
            };
            if element.merge {
                // Raw Merge geometry is handled as one planar layer operation below, including
                // fill subtraction and exact stroke fragments.
                continue;
            }
            if matches!(mode, EraserMode::SelectedFills | EraserMode::Inside) {
                if paint_parts(&element.kind).is_some() {
                    let mut retained = element.clone();
                    apply_paint(&mut retained.kind, Some([0; 4]), None);
                    changes.push((target.clone(), vec![retained]));
                }
                continue;
            }
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
                gesture.mapping.map_matrix(draw.item.transform),
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
        let mut candidate = self.project.clone();
        let mut merge_changed = false;
        let mut merge_error = None;
        let symbol_id = self.symbol_id().to_owned();
        if let Some(symbol) = candidate.symbol_mut(&symbol_id) {
            for layer in &mut symbol.timeline.layers {
                if !affected_layers.contains(&layer.id) {
                    continue;
                }
                let radius = f64::from(self.authoring_tools.eraser_size / 2. / gesture.zoom);
                let tol = 0.2 / f64::from(gesture.zoom);
                let matrix = gesture.mapping.matrix();
                let result = crate::stage_selection::edit_ungrouped_merge_shapes(
                    layer,
                    self.frame,
                    |elements| {
                        if mode == EraserMode::StrokeSegments {
                            erase_merge_geometry_with_metric(
                                elements,
                                &local_points,
                                matrix,
                                radius,
                                tol,
                            )
                        } else {
                            let mut any_changed = false;
                            let mut next = Vec::with_capacity(elements.len());
                            for el in std::mem::take(elements) {
                                let eligible = is_merge_fill(&el)
                                    && match mode {
                                        EraserMode::SelectedFills => selected_ids.contains(&el.id),
                                        EraserMode::Inside => {
                                            inside_target_id.as_deref() == Some(el.id.as_str())
                                        }
                                        _ => true,
                                    };
                                if eligible {
                                    let mut single = vec![el];
                                    match erase_merge_geometry_with_metric(
                                        &mut single,
                                        &local_points,
                                        matrix,
                                        radius,
                                        tol,
                                    ) {
                                        Ok(changed) => {
                                            any_changed |= changed;
                                            next.extend(single);
                                        }
                                        Err(err) => return Err(err),
                                    }
                                } else {
                                    next.push(el);
                                }
                            }
                            *elements = next;
                            Ok(any_changed)
                        }
                    },
                );
                match result {
                    Some(Ok(changed)) => merge_changed |= changed,
                    Some(Err(error)) => {
                        merge_error = Some(error.to_string());
                        break;
                    }
                    None => {}
                }
            }
        }
        if let Some(error) = merge_error {
            self.notify(neoflash_ui::components::ToastType::Warning, error);
            return;
        }
        if changes.is_empty() && !merge_changed {
            return;
        }
        self.transact(|editor| {
            let before = editor.project.clone();
            editor.project = candidate;
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
        self.apply_active_tool_paint_with_bucket(element, stroke_well, None, None);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn apply_bucket_gradient_to_element(
        &self,
        element: &mut Element,
        start: Pos2,
        end: Option<Pos2>,
    ) {
        self.apply_active_tool_paint_with_bucket(
            element,
            false,
            Some(start),
            end.map(|to| (start, to)),
        );
    }

    fn apply_active_tool_paint_with_bucket(
        &self,
        element: &mut Element,
        stroke_well: bool,
        click_local: Option<Pos2>,
        drag_local: Option<(Pos2, Pos2)>,
    ) {
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
        if !stroke_well && !self.authoring_tools.brush_lock_fill {
            if let Some((from, to)) = drag_local
                && (to - from).length() > 1e-3
            {
                if gradient.kind == GradientKind::Linear {
                    let dx = f64::from(to.x - from.x) * 0.5 / 819.2;
                    let dy = f64::from(to.y - from.y) * 0.5 / 819.2;
                    gradient.matrix = [
                        dx,
                        dy,
                        -dy,
                        dx,
                        f64::from(from.x + to.x) * 0.5,
                        f64::from(from.y + to.y) * 0.5,
                    ];
                } else {
                    let dx = f64::from(to.x - from.x) / 819.2;
                    let dy = f64::from(to.y - from.y) / 819.2;
                    gradient.matrix = [dx, dy, -dy, dx, f64::from(from.x), f64::from(from.y)];
                }
            } else if let Some(pt) = click_local
                && gradient.kind == GradientKind::Radial
            {
                gradient.matrix[4] = f64::from(pt.x);
                gradient.matrix[5] = f64::from(pt.y);
            }
        }
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
        mapping: StageMapping,
    ) -> Result<Vec<(SelectionTarget, egui::Mesh)>, String> {
        let eligible: HashMap<_, _> = self
            .editable_targets()
            .into_iter()
            .map(|target| (target.element.clone(), target))
            .collect();
        let authored = mapping.map_scene(
            &self.presented_interactive_authored_scene(self.show_layer_masking(p.ctx())),
        );
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
            if self.authoring_tools.eraser_mode == EraserMode::SelectedFills
                && !self.selection.targets.contains(target)
            {
                continue;
            }
            let mut item = draw.item.clone();
            let vector = paint_parts(&item.kind).is_some();
            if self.authoring_tools.eraser_mode != EraserMode::Objects {
                // Whole-fill/stroke operations edit raw vector artwork, never a
                // containing symbol merely because a child was touched.
                if !vector || item.instance_path.len() != 1 {
                    continue;
                }
                match self.authoring_tools.eraser_mode {
                    EraserMode::Fills | EraserMode::SelectedFills | EraserMode::Inside => {
                        apply_paint(&mut item.kind, None, Some(([0; 4], 0.)))
                    }
                    EraserMode::Strokes => apply_paint(&mut item.kind, Some([0; 4]), None),
                    // The default partial mode cuts through both raw Merge fills and strokes.
                    // Keep the full visible mesh so a fill-only shape becomes a gesture target.
                    EraserMode::StrokeSegments => {}
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
        mapping: StageMapping,
        view: Rect,
        scene: &[DrawItem],
    ) -> bool {
        let space = ui.input(|i| i.key_down(egui::Key::Space)) && !ui.ctx().wants_keyboard_input();
        let owned = extended(self.tool)
            || space
            || self.authoring_tools.gesture.is_some()
            || !self.authoring_tools.lasso_polygon.is_empty();
        if !owned {
            return false;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape) || !i.focused) {
            self.authoring_tools.cancel();
            return true;
        }
        if self.tool == Tool::Lasso && !space && !self.authoring_tools.lasso_polygon.is_empty() {
            let (enter, shift, alt) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Enter),
                    i.modifiers.shift,
                    i.modifiers.alt,
                )
            });
            let finish_alt_release = !self.authoring_tools.lasso_polygon_mode
                && !alt
                && self.authoring_tools.lasso_polygon.len() >= 3;
            if enter || finish_alt_release {
                let pts = std::mem::take(&mut self.authoring_tools.lasso_polygon);
                self.finish_lasso_points(&pts, shift, alt, scene, p);
                return true;
            }
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
                    self.authoring_tools.brush_size = (size
                        + (pos.x - start.x)
                            / (self.zoom.max(0.001) * mapping.scale_bounds().1.max(1e-6) as f32))
                        .clamp(1., 200.);
                    neoflash_ui::components::brush_cursor(
                        p,
                        pos,
                        self.authoring_tools.brush_shape,
                        self.authoring_tools.brush_size
                            * self.zoom
                            * mapping.scale_bounds().1 as f32,
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
                || (g.tool != Tool::Hand
                    && (g.origin != origin || g.zoom != self.zoom || g.mapping != mapping))
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
                    } else if tool != Tool::Lasso || self.authoring_tools.lasso_polygon.is_empty() {
                        // finish_edits cancels the previous draft, not this pen-down sample.
                        let contact = self.authoring_tools.pressure_contact.take();
                        self.finish_edits();
                        self.authoring_tools.pressure_contact = contact;
                    }
                    self.playing = false;
                    let local = mapping.screen_to_edit(origin, self.zoom, pos);
                    // Line ends and PolyStar centres snap to guides, objects, grid and pixels (ST-04).
                    let local = if matches!(tool, Tool::Line | Tool::PolyStar) {
                        mapping.screen_to_edit(
                            origin,
                            self.zoom,
                            self.snap_screen(mapping, origin, pos),
                        )
                    } else {
                        local
                    };
                    if tool == Tool::Lasso {
                        if self.authoring_tools.lasso_magic_wand {
                            self.magic_wand_select_at(local, modifiers.shift);
                            continue;
                        }
                        if self.authoring_tools.lasso_polygon_mode
                            || modifiers.alt
                            || !self.authoring_tools.lasso_polygon.is_empty()
                        {
                            let double_clicked = ui.input(|i| {
                                i.pointer
                                    .button_double_clicked(egui::PointerButton::Primary)
                            });
                            let close_to_start = self
                                .authoring_tools
                                .lasso_polygon
                                .first()
                                .is_some_and(|first| {
                                    self.authoring_tools.lasso_polygon.len() >= 3
                                        && mapping
                                            .edit_to_screen(origin, self.zoom, *first)
                                            .distance(pos)
                                            <= 8.0
                                });
                            if (double_clicked && self.authoring_tools.lasso_polygon.len() >= 3)
                                || close_to_start
                            {
                                let pts = std::mem::take(&mut self.authoring_tools.lasso_polygon);
                                self.finish_lasso_points(
                                    &pts,
                                    modifiers.shift,
                                    modifiers.alt,
                                    scene,
                                    p,
                                );
                            } else if self.authoring_tools.lasso_polygon.len() < 2048 {
                                self.authoring_tools.lasso_polygon.push(local);
                            }
                            continue;
                        }
                    }
                    if matches!(tool, Tool::Ink | Tool::Eyedropper) {
                        self.paint_tool_click(scene, p, local, mapping, modifiers.alt);
                        continue;
                    }
                    let gradient = if tool == Tool::Gradient {
                        // A fill's transform handle may lie outside its artwork.
                        let selected_handle = self.selected_sampled().and_then(|element| {
                            let fill = paint_gradient(&element.kind)?;
                            let matrix = transform_matrix(element.transform);
                            let handles = gradient_handle_positions(
                                origin,
                                self.zoom,
                                mapping,
                                matrix,
                                fill.matrix,
                                (fill.kind == GradientKind::Radial)
                                    .then_some(fill.focal_point_ratio),
                            );
                            handles.hit(pos, modifiers.alt)?;
                            Some(PaintHit {
                                target: self.selection.single()?.clone(),
                                kind: element.kind,
                                local: inverse(matrix, local)?,
                                stroke: false,
                                matrix,
                            })
                        });
                        let Some(hit) = selected_handle
                            .or_else(|| self.tool_paint_hit(scene, p, local, mapping, true))
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
                        let handles = gradient_handle_positions(
                            origin,
                            self.zoom,
                            mapping,
                            hit.matrix,
                            matrix,
                            radial.then_some(focal_point_ratio),
                        );
                        let handle_kind = handles
                            .hit(pos, modifiers.alt)
                            .unwrap_or(GradientHandleKind::Center);
                        self.set_selection(vec![hit.target.clone()]);
                        Some(GradientGesture {
                            target: hit.target,
                            kind: hit.kind,
                            matrix,
                            element_matrix: hit.matrix,
                            handle_kind,
                            radial,
                            focal_point_ratio,
                        })
                    } else {
                        None
                    };
                    let eraser_meshes = if tool == Tool::Eraser {
                        match self.eraser_meshes(p, mapping) {
                            Ok(meshes) => meshes,
                            Err(error) => {
                                self.message(error);
                                continue;
                            }
                        }
                    } else {
                        Vec::new()
                    };
                    if matches!(tool, Tool::PolyStar | Tool::Line)
                        || (tool == Tool::Brush
                            && self.authoring_tools.brush_paint_mode
                                != BrushPaintMode::PaintSelection)
                    {
                        self.clear_selection();
                    }
                    self.authoring_tools.gesture = Some(Gesture {
                        tool,
                        symbol: self.symbol_id().to_owned(),
                        frame: self.frame,
                        layer: self.symbol().timeline.layers[self.layer].id.clone(),
                        origin,
                        zoom: self.zoom,
                        mapping,
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
        self.paint_authoring_gesture(p, origin, mapping, view);
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
                Tool::Gradient => self
                    .gradient_hover_cursor(origin, mapping, pos, ui.input(|i| i.modifiers.alt))
                    .unwrap_or_else(|| {
                        crate::stage_cursor::tool_cursor_precise(
                            tool,
                            self.workspace.drawing.precise_cursors,
                        )
                    }),
                _ => crate::stage_cursor::tool_cursor_precise(
                    tool,
                    self.workspace.drawing.precise_cursors,
                ),
            });
            if matches!(tool, Tool::Brush | Tool::Eraser) {
                let (size, shape, state) = if tool == Tool::Brush {
                    (
                        self.authoring_tools.brush_size
                            * self.zoom
                            * mapping.scale_bounds().1 as f32,
                        self.authoring_tools.brush_shape,
                        neoflash_ui::components::BrushCursorState::Idle,
                    )
                } else {
                    (
                        self.authoring_tools.eraser_size,
                        self.authoring_tools.eraser_shape.brush_shape(),
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
        let (mod_shift, mod_alt) = p.ctx().input(|i| (i.modifiers.shift, i.modifiers.alt));
        g.shift = mod_shift;
        if matches!(g.tool, Tool::Line | Tool::PolyStar | Tool::Bucket) {
            g.alt = mod_alt;
        }
        let mut point = g.local(pos);
        if matches!(g.tool, Tool::Line | Tool::PolyStar) && !g.shift {
            point = g.local(self.snap_screen(g.mapping, g.origin, pos));
        }
        if matches!(g.tool, Tool::Line | Tool::PolyStar | Tool::Bucket) && g.shift {
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
            Tool::Line | Tool::PolyStar | Tool::Bucket => {
                g.points.truncate(1);
                g.points.push(point);
            }
            Tool::Brush | Tool::Lasso => {
                if g.points.last().is_none_or(|last| {
                    g.mapping
                        .edit_to_screen(g.origin, g.zoom, *last)
                        .distance(pos)
                        >= 1.5
                }) {
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
                let previous_metric = g.mapping.edit_to_stage_pos(previous);
                let point_metric = g.mapping.edit_to_stage_pos(point);
                for (target, mesh) in &g.eraser_meshes {
                    if !g.targets.contains(target)
                        && swept_disk_hits_mesh(mesh, previous_metric, point_metric, radius)
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
                let line_pts = line_gesture_points(&g);
                let commands = if g.tool == Tool::Brush {
                    self.brush_gesture_commands(&g)
                } else {
                    polyline(&line_pts, false)
                };
                if commands.len() < 2
                    || (g.tool == Tool::Line
                        && g.mapping
                            .edit_to_screen(g.origin, g.zoom, line_pts[0])
                            .distance(g.mapping.edit_to_screen(
                                g.origin,
                                g.zoom,
                                *line_pts.last().unwrap(),
                            ))
                            < 0.1)
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
                self.authoring_tools.primitive_drawn_selection = true;
            }
            Tool::Bucket => {
                let start = g.points[0];
                let end = *g.points.last().unwrap_or(&start);
                let dragged = g
                    .mapping
                    .edit_to_screen(g.origin, g.zoom, start)
                    .distance(g.mapping.edit_to_screen(g.origin, g.zoom, end))
                    >= 4.0;
                self.paint_tool_click_or_bucket(
                    scene,
                    p,
                    start,
                    dragged.then_some(end),
                    g.mapping,
                    g.alt,
                );
            }
            Tool::Eraser => {
                let mode = self.authoring_tools.eraser_mode;
                if matches!(
                    mode,
                    EraserMode::StrokeSegments | EraserMode::SelectedFills | EraserMode::Inside
                ) {
                    self.erase_stroke_parts(&g, p);
                    return;
                }
                let mut changes = vec![];
                for target in g.targets {
                    let Some(index) = self.target_layer_index(&target) else {
                        continue;
                    };
                    let Some(element) = sample_layer_at_fps(
                        &self.symbol().timeline.layers[index],
                        self.frame,
                        f64::from(self.project.fps),
                    )
                    .into_iter()
                    .find(|e| e.id == target.element) else {
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
                self.finish_lasso_points(&g.points, g.shift, g.alt, scene, p);
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
                    if gradient.handle_kind == GradientHandleKind::Focal {
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
                if !polystar_drag_is_large_enough(
                    g.mapping,
                    g.origin,
                    g.zoom,
                    g.points[0],
                    g.points[1],
                ) {
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
                self.authoring_tools.primitive_drawn_selection = true;
            }
            _ => {}
        }
    }

    pub(crate) fn finish_lasso_points(
        &mut self,
        points: &[Pos2],
        shift: bool,
        alt: bool,
        scene: &[DrawItem],
        p: &egui::Painter,
    ) {
        if points.len() < 3 {
            return;
        }
        if (self.authoring_tools.lasso_partial_fills || alt) && self.layer_editable() {
            let pts: Vec<[f64; 2]> = points
                .iter()
                .map(|pt| [f64::from(pt.x), f64::from(pt.y)])
                .collect();
            let previous = shift.then(|| self.selection.targets.clone());
            if !self.lasso_select_partial_fill(&pts).is_empty() {
                if let Some(mut previous) = previous {
                    previous.extend(self.selection.targets.clone());
                    self.set_selection(previous);
                }
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
        let mut targets = if shift {
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
                        lasso_rect(points, *bounds, self.authoring_tools.lasso_contact)
                    })
                })
                .filter_map(|id| eligible.get(id).cloned()),
        );
        self.set_selection(targets);
    }

    fn paint_authoring_gesture(
        &self,
        p: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        view: Rect,
    ) {
        let palette = UiPalette::current(p.ctx());
        if !self.authoring_tools.lasso_polygon.is_empty() {
            let screen = |point: Pos2| mapping.edit_to_screen(origin, self.zoom, point);
            let mut pts: Vec<Pos2> = self
                .authoring_tools
                .lasso_polygon
                .iter()
                .map(|pt| screen(*pt))
                .collect();
            if let Some(latest) = p.ctx().input(|i| i.pointer.latest_pos()) {
                pts.push(latest);
            }
            if pts.len() >= 2 {
                p.add(Shape::line(
                    pts.clone(),
                    Stroke::new(1.5_f32, palette.overlay),
                ));
            }
            for &pt in &self.authoring_tools.lasso_polygon {
                let s = screen(pt);
                p.rect(
                    Rect::from_center_size(s, Vec2::splat(5.0)),
                    0.0,
                    palette.handle_fill,
                    Stroke::new(1.25_f32, palette.overlay),
                    StrokeKind::Inside,
                );
            }
        }
        if let Some(g) = &self.authoring_tools.gesture {
            let screen = |point: Pos2| g.mapping.edit_to_screen(g.origin, g.zoom, point);
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
                    let line_pts = line_gesture_points(g);
                    let commands = polyline(&line_pts, false);
                    if self.stroke_width > 0.
                        && let Ok(geometry) = crate::vector::tessellate_cached(
                            &commands,
                            FillRule::NonZero,
                            Some(self.stroke_width),
                            0.2,
                        )
                    {
                        let mut mesh = egui::Mesh::default();
                        mesh.vertices.reserve(geometry.vertices.len());
                        let color = Color32::from_rgba_unmultiplied(
                            self.stroke[0],
                            self.stroke[1],
                            self.stroke[2],
                            self.stroke[3],
                        );
                        for point in &geometry.vertices {
                            mesh.colored_vertex(screen(Pos2::new(point.x, point.y)), color);
                        }
                        mesh.indices.clone_from(&geometry.indices);
                        p.add(Shape::mesh(mesh));
                    }
                }
                Tool::Bucket => {
                    if g.points.len() >= 2 && self.authoring_modify.drawing_fill_gradient.is_some()
                    {
                        let a = screen(g.points[0]);
                        let b = screen(*g.points.last().unwrap());
                        if a.distance(b) >= 4.0 {
                            p.line_segment([a, b], Stroke::new(3.0_f32, palette.overlay_halo));
                            p.line_segment([a, b], Stroke::new(1.5_f32, palette.overlay));
                            p.circle(
                                a,
                                4.5,
                                palette.handle_fill,
                                Stroke::new(1.5_f32, palette.overlay),
                            );
                            p.rect(
                                Rect::from_center_size(b, Vec2::splat(8.0)),
                                0.0,
                                palette.handle_fill,
                                Stroke::new(1.5_f32, palette.overlay),
                                StrokeKind::Inside,
                            );
                        }
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
                // The cursor is the complete Eraser preview. Hit targets stay internal instead
                // of flashing whole-object boxes while the cursor crosses artwork.
                Tool::Eraser => {}
                Tool::Gradient => {
                    if let Some(gradient) = &g.gradient {
                        let is_focal = gradient.handle_kind == GradientHandleKind::Focal;
                        let matrix = if is_focal {
                            Some(gradient.matrix)
                        } else {
                            gradient_matrix(g, gradient)
                        };
                        let focal_ratio = if is_focal {
                            gradient_focal_ratio(g, gradient).unwrap_or(gradient.focal_point_ratio)
                        } else {
                            gradient.focal_point_ratio
                        };
                        if let Some(matrix) = matrix {
                            paint_gradient_handles(
                                p,
                                g.origin,
                                g.zoom,
                                g.mapping,
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
                    if polystar_drag_is_large_enough(
                        g.mapping,
                        g.origin,
                        g.zoom,
                        g.points[0],
                        g.points[1],
                    ) {
                        let (commands, origin_xy) = polystar_path_commands(
                            center,
                            radius,
                            dy.atan2(dx),
                            self.authoring_tools.polystar_sides,
                            self.authoring_tools.polystar_star,
                            self.authoring_tools.polystar_point_size,
                        );
                        let stroke_color = Color32::from_rgba_unmultiplied(
                            self.stroke[0],
                            self.stroke[1],
                            self.stroke[2],
                            self.stroke[3],
                        );
                        if self.stroke[3] > 0
                            && self.stroke_width > 0.
                            && let Ok(geometry) = crate::vector::tessellate_cached(
                                &commands,
                                FillRule::NonZero,
                                Some(self.stroke_width),
                                0.2,
                            )
                        {
                            let mut mesh = egui::Mesh::default();
                            mesh.vertices.reserve(geometry.vertices.len());
                            for point in &geometry.vertices {
                                mesh.colored_vertex(
                                    screen(Pos2::new(
                                        (origin_xy[0] + f64::from(point.x)) as f32,
                                        (origin_xy[1] + f64::from(point.y)) as f32,
                                    )),
                                    stroke_color,
                                );
                            }
                            mesh.indices.clone_from(&geometry.indices);
                            p.add(Shape::mesh(mesh));
                        } else {
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
                                p.add(Shape::closed_line(
                                    pts,
                                    Stroke::new(1.5_f32, palette.overlay),
                                ));
                            }
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
                mapping,
                transform_matrix(element.transform),
                fill.matrix,
                (fill.kind == GradientKind::Radial).then_some(fill.focal_point_ratio),
            );
        }
    }
}

fn line_gesture_points(g: &Gesture) -> Vec<Pos2> {
    if g.points.len() >= 2 && g.alt {
        let center = g.points[0];
        let end = *g.points.last().unwrap();
        let delta = end - center;
        vec![center - delta, center + delta]
    } else {
        g.points.clone()
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
    // Preserve the established 0.2-local-unit cap while allowing an affine screen tolerance to
    // make de-duplication stricter for visibly separated samples.
    let dedup_tolerance_sq = pos_tol.clamp(1e-4, 0.2).powi(2);
    for (&pt, &r) in points.iter().zip(radii.iter()) {
        if let Some(last_pt) = dedup_pts.last().copied()
            && pt.distance_sq(last_pt) < dedup_tolerance_sq
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

#[cfg(test)]
fn round_brush_envelope(points: &[Pos2], pressures: &[f32], width: f32) -> Vec<PathCommand> {
    round_brush_envelope_with_position_tolerance(points, pressures, width, 0.22)
}

fn round_brush_envelope_with_position_tolerance(
    points: &[Pos2],
    pressures: &[f32],
    width: f32,
    position_tolerance: f32,
) -> Vec<PathCommand> {
    round_brush_envelope_with_overlap_check(
        points,
        pressures,
        width,
        position_tolerance,
        nonadjacent_rects_overlap,
    )
}

fn round_brush_envelope_with_overlap_check(
    points: &[Pos2],
    pressures: &[f32],
    width: f32,
    position_tolerance: f32,
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
    let (pts, radii) = simplify_round_spine(
        points,
        &raw_radii,
        position_tolerance,
        (width * 0.015).max(0.08),
    );
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
struct GradientHandlePositions {
    center: Pos2,
    width: Pos2,
    rotate: Pos2,
    radius: Option<Pos2>,
    focal: Option<(Pos2, f64)>,
    left_line: [Pos2; 2],
    right_line: [Pos2; 2],
}

impl GradientHandlePositions {
    fn hit(&self, pos: Pos2, alt: bool) -> Option<GradientHandleKind> {
        const HIT_R: f32 = 12.0;
        if self.rotate.distance(pos) < HIT_R {
            return Some(GradientHandleKind::Rotate);
        }
        if let Some(rad_pos) = self.radius
            && rad_pos.distance(pos) < HIT_R
        {
            return Some(GradientHandleKind::Radius);
        }
        if self.width.distance(pos) < HIT_R {
            return Some(GradientHandleKind::Width);
        }
        if let Some((focal_pos, focal_ratio)) = self.focal {
            let tri_center = focal_pos + Vec2::new(0.0, -5.0);
            let near_focal = focal_pos.distance(pos) < HIT_R || tri_center.distance(pos) < HIT_R;
            if near_focal
                && (alt
                    || pos.y < self.center.y - 2.0
                    || (focal_ratio.abs() > 0.03
                        && focal_pos.distance(pos) < self.center.distance(pos)))
            {
                return Some(GradientHandleKind::Focal);
            }
        }
        if self.center.distance(pos) < HIT_R {
            return Some(GradientHandleKind::Center);
        }
        None
    }
}

fn gradient_handle_positions(
    origin: Pos2,
    zoom: f32,
    mapping: StageMapping,
    element: [f64; 6],
    gradient: [f64; 6],
    radial_focal_ratio: Option<f64>,
) -> GradientHandlePositions {
    let screen =
        |local| mapping.edit_to_screen(origin, zoom, affine(element, affine(gradient, local)));
    let center = screen(Pos2::ZERO);
    let left_mid = screen(Pos2::new(-819.2, 0.0));
    let right_mid = screen(Pos2::new(819.2, 0.0));
    let raw_perp = screen(Pos2::new(0.0, 819.2)) - center;
    let axis = right_mid - center;
    let perp = if raw_perp.length() >= 48.0 {
        raw_perp
    } else if axis.length() > 1e-3 {
        let dir = axis.normalized();
        Vec2::new(-dir.y, dir.x) * 64.0
    } else {
        Vec2::new(0.0, 64.0)
    };
    let left_line = [left_mid - perp, left_mid + perp];
    let right_line = [right_mid - perp, right_mid + perp];
    if let Some(focal_ratio) = radial_focal_ratio {
        let theta_rad = 0.35_f32;
        let theta_rot = 0.70_f32;
        let radius_pos = screen(Pos2::new(819.2 * theta_rad.cos(), 819.2 * theta_rad.sin()));
        let rotate_pos = screen(Pos2::new(819.2 * theta_rot.cos(), 819.2 * theta_rot.sin()));
        let focal_pos = screen(Pos2::new((focal_ratio * 819.2) as f32, 0.0));
        GradientHandlePositions {
            center,
            width: right_mid,
            rotate: rotate_pos,
            radius: Some(radius_pos),
            focal: Some((focal_pos, focal_ratio)),
            left_line,
            right_line,
        }
    } else {
        GradientHandlePositions {
            center,
            width: right_mid,
            rotate: right_line[0],
            radius: None,
            focal: None,
            left_line,
            right_line,
        }
    }
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
    let center = Pos2::new(matrix[4] as f32, matrix[5] as f32);
    match gradient.handle_kind {
        GradientHandleKind::Center => {
            matrix[4] += (current.x - start.x) as f64;
            matrix[5] += (current.y - start.y) as f64;
        }
        GradientHandleKind::Width => {
            let grad_local = inverse(gradient.matrix, current)?;
            let scale_x = (f64::from(grad_local.x).abs() / 819.2).clamp(0.01, 100.0);
            matrix[0] = gradient.matrix[0] * scale_x;
            matrix[1] = gradient.matrix[1] * scale_x;
        }
        GradientHandleKind::Radius => {
            let from = start - center;
            let to = current - center;
            if from.length() < 0.001 || to.length() < 0.001 {
                return None;
            }
            let scale = (to.length() / from.length()).clamp(0.01, 100.) as f64;
            for v in &mut matrix[0..4] {
                *v *= scale;
            }
        }
        GradientHandleKind::Rotate => {
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
            let (sin, cos) = (angle as f64).sin_cos();
            for offset in [0, 2] {
                let x = gradient.matrix[offset];
                let y = gradient.matrix[offset + 1];
                matrix[offset] = x * cos - y * sin;
                matrix[offset + 1] = x * sin + y * cos;
            }
        }
        GradientHandleKind::Focal => {}
    }
    Some(matrix)
}

fn paint_gradient_handles(
    p: &egui::Painter,
    origin: Pos2,
    zoom: f32,
    mapping: StageMapping,
    element: [f64; 6],
    gradient: [f64; 6],
    radial_focal_ratio: Option<f64>,
) {
    let screen =
        |local| mapping.edit_to_screen(origin, zoom, affine(element, affine(gradient, local)));
    let handles =
        gradient_handle_positions(origin, zoom, mapping, element, gradient, radial_focal_ratio);
    let palette = UiPalette::current(p.ctx());
    let halo = Stroke::new(3.0_f32, palette.overlay_halo);
    let line = Stroke::new(1.25_f32, palette.overlay);
    let handle_stroke = Stroke::new(1.5_f32, palette.overlay);

    if radial_focal_ratio.is_some() {
        // Radial gradient: elliptical boundary ring + horizontal diameter axis line.
        const SEGMENTS: usize = 64;
        let ring: Vec<Pos2> = (0..SEGMENTS)
            .map(|i| {
                let theta = (i as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
                screen(Pos2::new(819.2 * theta.cos(), 819.2 * theta.sin()))
            })
            .collect();
        p.add(Shape::closed_line(ring.clone(), halo));
        p.add(Shape::closed_line(ring, line));
        let left_edge = screen(Pos2::new(-819.2, 0.0));
        p.line_segment([left_edge, handles.width], halo);
        p.line_segment([left_edge, handles.width], line);
    } else {
        // Linear gradient: two parallel perpendicular boundary lines at x = -819.2 and x = +819.2.
        p.line_segment(handles.left_line, halo);
        p.line_segment(handles.left_line, line);
        p.line_segment(handles.right_line, halo);
        p.line_segment(handles.right_line, line);
    }

    // A. Center point handle (circle with center dot).
    p.circle(handles.center, 5.5, palette.handle_fill, handle_stroke);
    p.circle_filled(handles.center, 1.5, palette.overlay);

    // E. Focal point handle (inverted triangle `▽` on the axis line for Radial gradients).
    if let Some((focal, _)) = handles.focal {
        p.circle(focal, 3.5, palette.handle_fill, handle_stroke);
        let tri = vec![
            focal + Vec2::new(0., -2.),
            focal + Vec2::new(5., -10.),
            focal + Vec2::new(-5., -10.),
        ];
        p.add(Shape::convex_polygon(
            tri,
            palette.handle_fill,
            handle_stroke,
        ));
    }

    // B. Width adjustment handle (square with `[→]` arrow).
    let width_rect = Rect::from_center_size(handles.width, Vec2::splat(10.0));
    p.rect(
        width_rect,
        0,
        palette.handle_fill,
        handle_stroke,
        StrokeKind::Inside,
    );
    let axis_vec = handles.width - handles.center;
    let axis_dir = if axis_vec.length() > 1e-3 {
        axis_vec.normalized()
    } else {
        Vec2::RIGHT
    };
    let axis_perp = Vec2::new(-axis_dir.y, axis_dir.x);
    let a0 = handles.width - axis_dir * 2.8;
    let a1 = handles.width + axis_dir * 2.8;
    p.line_segment([a0, a1], Stroke::new(1.2_f32, palette.overlay));
    p.line_segment(
        [a1, a1 - axis_dir * 2.2 + axis_perp * 1.8],
        Stroke::new(1.2_f32, palette.overlay),
    );
    p.line_segment(
        [a1, a1 - axis_dir * 2.2 - axis_perp * 1.8],
        Stroke::new(1.2_f32, palette.overlay),
    );

    // D. Size / Radius handle (Radial only: circle with inward arrow toward center).
    if let Some(rad_pos) = handles.radius {
        p.circle(rad_pos, 5.5, palette.handle_fill, handle_stroke);
        let in_vec = handles.center - rad_pos;
        let inward = if in_vec.length() > 1e-3 {
            in_vec.normalized()
        } else {
            Vec2::LEFT
        };
        let in_perp = Vec2::new(-inward.y, inward.x);
        let r0 = rad_pos - inward * 2.5;
        let r1 = rad_pos + inward * 2.5;
        p.line_segment([r0, r1], Stroke::new(1.2_f32, palette.overlay));
        p.line_segment(
            [r1, r1 - inward * 2.0 + in_perp * 1.6],
            Stroke::new(1.2_f32, palette.overlay),
        );
        p.line_segment(
            [r1, r1 - inward * 2.0 - in_perp * 1.6],
            Stroke::new(1.2_f32, palette.overlay),
        );
    }

    // C. Rotation handle (circle with curved rotation arrows).
    p.circle(handles.rotate, 5.5, palette.handle_fill, handle_stroke);
    for base_ang in [0.2_f32, 2.3_f32, 4.4_f32] {
        let p0 = handles.rotate + Vec2::angled(base_ang) * 3.0;
        let p1 = handles.rotate + Vec2::angled(base_ang + 1.1) * 3.0;
        p.line_segment([p0, p1], Stroke::new(1.1_f32, palette.overlay));
    }
}

impl Editor {
    fn gradient_hover_cursor(
        &self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
        alt: bool,
    ) -> Option<neoflash_ui::cursors::Cursor> {
        use neoflash_ui::cursors as nf;
        if let Some(g) = &self.authoring_tools.gesture
            && let Some(grad) = &g.gradient
        {
            return Some(match grad.handle_kind {
                GradientHandleKind::Center => nf::MOVE,
                GradientHandleKind::Rotate => nf::ROTATE,
                GradientHandleKind::Focal => nf::RESIZE_H,
                GradientHandleKind::Width => nf::RESIZE_H,
                GradientHandleKind::Radius => nf::RESIZE_D1,
            });
        }
        let element = self.selected_sampled()?;
        let fill = paint_gradient(&element.kind)?;
        let handles = gradient_handle_positions(
            origin,
            self.zoom,
            mapping,
            transform_matrix(element.transform),
            fill.matrix,
            (fill.kind == GradientKind::Radial).then_some(fill.focal_point_ratio),
        );
        match handles.hit(pos, alt)? {
            GradientHandleKind::Center => Some(nf::MOVE),
            GradientHandleKind::Rotate => Some(nf::ROTATE),
            GradientHandleKind::Focal => Some(nf::RESIZE_H),
            GradientHandleKind::Width => Some(nf::RESIZE_H),
            GradientHandleKind::Radius => Some(nf::RESIZE_D1),
        }
    }

    /// Slice Merge-mode fills and strokes on the active editable layer along `points`, selecting
    /// only the enclosed fragments in one Undo step. Object Drawing and grouped artwork remain
    /// whole objects.
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
        let prior_bitmap_fills: Vec<(Id, [f64; 6], AuthoredBitmapFill)> = layer
            .key_at(frame)
            .map(|key| {
                key.bitmap_fills
                    .iter()
                    .filter_map(|bf| {
                        let el = key.elements.iter().find(|e| e.id == bf.element_id)?;
                        Some((
                            bf.element_id.clone(),
                            multiply_matrices(transform_matrix(el.transform), bf.matrix),
                            bf.clone(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let prior_ids: std::collections::HashSet<Id> = layer
            .key_at(frame)
            .map(|key| key.elements.iter().map(|e| e.id.clone()).collect())
            .unwrap_or_default();
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
        if !prior_bitmap_fills.is_empty()
            && let Some(key) = layer.key_at_mut(frame)
        {
            let current_ids: std::collections::HashSet<Id> =
                key.elements.iter().map(|e| e.id.clone()).collect();
            if let Some((_, world_matrix, template_bf)) = prior_bitmap_fills
                .iter()
                .find(|(old_id, _, _)| !current_ids.contains(old_id))
                .cloned()
            {
                let new_elem_ids: Vec<Id> = key
                    .elements
                    .iter()
                    .filter(|e| {
                        e.merge
                            && !prior_ids.contains(&e.id)
                            && !matches!(e.kind, ElementKind::Path { fill, .. } | ElementKind::PaintedPath { fill, .. } if fill[3] == 0)
                    })
                    .map(|e| e.id.clone())
                    .collect();
                key.bitmap_fills
                    .retain(|bf| current_ids.contains(&bf.element_id));
                for new_id in new_elem_ids {
                    let mut bf = template_bf.clone();
                    bf.element_id = new_id;
                    bf.matrix = world_matrix;
                    key.set_bitmap_fill(bf);
                }
            }
        }
        candidate.upgrade_format_for_features();
        if candidate.validate().is_err() {
            return Vec::new();
        }
        if candidate != self.project {
            self.transact_named("Lasso split geometry", |s| {
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

    /// Classic Flash 8 Magic Wand selection at `stage_pt`:
    /// 1. When clicking a bitmap (`ElementKind::Bitmap` or a broken-apart Merge shape with an
    ///    `AuthoredBitmapFill`), flood-fills contiguous pixels within `magic_wand_threshold`
    ///    (`0..=200`), traces the contour using `magic_wand_smoothing`, and slices/selects the
    ///    matching region in one Undo step.
    /// 2. When clicking a solid vector fill, selects editable vector fills whose color lies within
    ///    `magic_wand_threshold`.
    pub fn magic_wand_select_at(&mut self, stage_pt: Pos2, shift: bool) -> Vec<Id> {
        self.finish_edits();
        if !self.layer_editable() {
            return Vec::new();
        }
        let threshold = self.authoring_tools.magic_wand_threshold.clamp(0, 200);
        let smoothing = self.authoring_tools.magic_wand_smoothing;
        let layer_idx = self.layer;
        let frame = self.frame;

        // 1. Check if `stage_pt` hits a Bitmap or a broken-apart Bitmap fill on the active layer.
        let bitmap_hit: Option<(Id, Id, [f64; 6], bool, bool)> = {
            let sym = self.symbol();
            let layer = &sym.timeline.layers[layer_idx];
            let key = layer.key_at(frame);
            let sampled = sample_layer_at_fps(layer, frame, f64::from(self.project.fps));
            let mut hit = None;
            for element in sampled.iter().rev() {
                if !element.visible {
                    continue;
                }
                match &element.kind {
                    ElementKind::Bitmap {
                        resource_id,
                        width,
                        height,
                        smoothing: bmp_smooth,
                    } => {
                        let elem_m = transform_matrix(element.transform);
                        if let Some(local) = inverse(elem_m, stage_pt)
                            && local.x >= 0.0
                            && local.y >= 0.0
                            && local.x < *width as f32
                            && local.y < *height as f32
                        {
                            hit = Some((
                                element.id.clone(),
                                resource_id.clone(),
                                elem_m,
                                true,
                                *bmp_smooth,
                            ));
                            break;
                        }
                    }
                    ElementKind::Path {
                        commands,
                        fill_rule,
                        fill,
                        ..
                    }
                    | ElementKind::PaintedPath {
                        commands,
                        fill_rule,
                        fill,
                        ..
                    } if fill[3] > 0 => {
                        let Some(bf) = key.and_then(|k| {
                            k.bitmap_fills.iter().find(|bf| bf.element_id == element.id)
                        }) else {
                            continue;
                        };
                        let elem_m = transform_matrix(element.transform);
                        if let Some(local) = inverse(elem_m, stage_pt)
                            && geometry_contains(commands, *fill_rule, None, local)
                        {
                            let world_bmp_m = multiply_matrices(elem_m, bf.matrix);
                            hit = Some((
                                element.id.clone(),
                                bf.resource_id.clone(),
                                world_bmp_m,
                                false,
                                bf.smoothing,
                            ));
                            break;
                        }
                    }
                    _ => {}
                }
            }
            hit
        };

        if let Some((elem_id, resource_id, world_bmp_m, needs_break_apart, bmp_smoothing)) =
            bitmap_hit
            && let Some(bmp_pt) = inverse(world_bmp_m, stage_pt)
            && let Some(resource) = self
                .project
                .resources
                .iter()
                .find(|r| r.id == resource_id)
                .cloned()
            && let Ok(decoded) = resource.decode_image()
        {
            let w = decoded.width as usize;
            let h = decoded.height as usize;
            if w > 0 && h > 0 {
                let sx = (bmp_pt.x.floor() as isize).rem_euclid(w as isize) as usize;
                let sy = (bmp_pt.y.floor() as isize).rem_euclid(h as isize) as usize;
                let contour_bmp =
                    magic_wand_bitmap_contour(&decoded.rgba, w, h, sx, sy, threshold, smoothing);
                if contour_bmp.len() >= 3 {
                    let stage_poly: Vec<[f64; 2]> = contour_bmp
                        .into_iter()
                        .map(|[bx, by]| {
                            let p = affine(world_bmp_m, Pos2::new(bx as f32, by as f32));
                            [f64::from(p.x), f64::from(p.y)]
                        })
                        .collect();
                    if needs_break_apart {
                        let layer_id = self.symbol().timeline.layers[layer_idx].id.clone();
                        self.ensure_key_for(&layer_id, false);
                        if let Some(key) =
                            self.symbol_mut().timeline.layers[layer_idx].key_at_mut(frame)
                            && let Some(el) = key.elements.iter_mut().find(|e| e.id == elem_id)
                        {
                            el.scripts.clear();
                            el.accessibility = None;
                            el.transform.primitive = None;
                            el.merge = true;
                            el.kind = ElementKind::Path {
                                commands: rectangle_path_commands(
                                    decoded.width as f64,
                                    decoded.height as f64,
                                    RectangleOptions::default(),
                                ),
                                fill_rule: FillRule::NonZero,
                                fill: [255, 255, 255, 255],
                                stroke: [0; 4],
                                stroke_width: 0.0,
                            };
                            key.set_bitmap_fill(AuthoredBitmapFill {
                                element_id: elem_id.clone(),
                                resource_id,
                                matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                                repeat: false,
                                smoothing: bmp_smoothing,
                            });
                        }
                    }
                    return self.lasso_select_partial_fill(&stage_poly);
                }
            }
        }

        // 2. Fallback for vector fills: select editable vector shapes whose fill color is within
        // `magic_wand_threshold` of the clicked shape's fill color.
        let sym = self.symbol();
        let layer = &sym.timeline.layers[layer_idx];
        let layer_id = layer.id.clone();
        let sampled = sample_layer_at_fps(layer, frame, f64::from(self.project.fps));
        let mut seed_color: Option<[u8; 4]> = None;
        for element in sampled.iter().rev() {
            if !element.visible {
                continue;
            }
            let Some((commands, fill_rule, _)) = path_geometry(&element.kind) else {
                continue;
            };
            let fill = match &element.kind {
                ElementKind::Rectangle { fill, .. }
                | ElementKind::Ellipse { fill, .. }
                | ElementKind::Path { fill, .. }
                | ElementKind::PaintedPath { fill, .. } => *fill,
                _ => continue,
            };
            if fill[3] == 0 {
                continue;
            }
            if let Some(local) = inverse(transform_matrix(element.transform), stage_pt)
                && geometry_contains(&commands, fill_rule, None, local)
            {
                seed_color = Some(fill);
                break;
            }
        }
        let Some(seed) = seed_color else {
            if !shift {
                self.set_selection(Vec::new());
            }
            return Vec::new();
        };
        let mut matched_ids = Vec::new();
        let mut targets = if shift {
            self.selection.targets.clone()
        } else {
            Vec::new()
        };
        for element in &sampled {
            if !element.visible {
                continue;
            }
            let fill = match &element.kind {
                ElementKind::Rectangle { fill, .. }
                | ElementKind::Ellipse { fill, .. }
                | ElementKind::Path { fill, .. }
                | ElementKind::PaintedPath { fill, .. } => *fill,
                _ => continue,
            };
            if fill[3] > 0 && color_within_threshold(fill, seed, threshold) {
                matched_ids.push(element.id.clone());
                let target = SelectionTarget {
                    layer: layer_id.clone(),
                    element: element.id.clone(),
                };
                if !targets.contains(&target) {
                    targets.push(target);
                }
            }
        }
        self.set_selection(targets);
        matched_ids
    }

    /// Update the radial gradient `focal_point_ratio` (`-0.98..=0.98`) on the selected gradient
    /// fill in one `"Gradient focal point"` Undo step (`T-128`, `editor-paint-effects :: focal-handles`).
    #[cfg_attr(not(test), allow(dead_code))]
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
                        (EraserMode::StrokeSegments, "Erase normal"),
                        (EraserMode::Fills, "Erase fills"),
                        (EraserMode::Strokes, "Erase lines"),
                        (EraserMode::SelectedFills, "Erase selected fills"),
                        (EraserMode::Inside, "Erase inside"),
                        (EraserMode::Objects, "Erase whole objects"),
                    ],
                    width,
                );
                nf::Segmented::show(
                    ui,
                    &mut self.authoring_tools.eraser_shape,
                    &[
                        (EraserShape::Round, "Round"),
                        (EraserShape::Square, "Square"),
                    ],
                    true,
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
                    EraserMode::SelectedFills => (
                        "Remove selected fills · keep strokes",
                        "Erases only currently selected fills on editable layers, leaving unselected fills and all strokes intact.",
                    ),
                    EraserMode::Inside => (
                        "Remove fill where stroke began",
                        "Erases only the single fill inside which the eraser gesture started, leaving other fills and all strokes untouched.",
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
                let poly_resp = ui.add(nf::Checkbox::new(
                    &mut self.authoring_tools.lasso_polygon_mode,
                    "Polygon mode",
                ));
                if poly_resp.changed() && self.authoring_tools.lasso_polygon_mode {
                    self.authoring_tools.lasso_magic_wand = false;
                }
                nf::tooltip(
                    poly_resp,
                    "Click to add straight-sided polygon vertices; double-click (or press Enter) to close the selection loop.",
                    None,
                );
                let wand_resp = ui.add(nf::Checkbox::new(
                    &mut self.authoring_tools.lasso_magic_wand,
                    "Magic wand",
                ));
                if wand_resp.changed() && self.authoring_tools.lasso_magic_wand {
                    self.authoring_tools.lasso_polygon_mode = false;
                    self.authoring_tools.lasso_polygon.clear();
                }
                nf::tooltip(
                    wand_resp,
                    "Click a bitmap or fill to select contiguous pixels or fills of matching color within the Magic Wand threshold.",
                    None,
                );
                if ui.add(nf::Button::ghost("Magic wand settings…")).clicked() {
                    self.open_magic_wand_settings();
                }
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
        if self.tool == Tool::Transform {
            self.free_transform_modifier_grid(ui);
        }
    }

    pub(crate) fn set_free_transform_mode(&mut self, mode: FreeTransformMode) {
        self.set_tool(Tool::Transform);
        self.authoring_tools.transform_mode = mode;
        self.status = match mode {
            FreeTransformMode::FreeTransform => "Free Transform".to_owned(),
            FreeTransformMode::RotateAndSkew => "Free Transform: Rotate and Skew".to_owned(),
            FreeTransformMode::Scale => "Free Transform: Scale".to_owned(),
            FreeTransformMode::Distort => "Free Transform: Distort".to_owned(),
            FreeTransformMode::Envelope => "Free Transform: Envelope".to_owned(),
        };
    }

    pub(crate) fn active_free_transform_mode(&self) -> FreeTransformMode {
        let mode = self.authoring_tools.transform_mode;
        if matches!(
            mode,
            FreeTransformMode::Distort | FreeTransformMode::Envelope
        ) && !self.can_distort_or_envelope_selection()
        {
            FreeTransformMode::FreeTransform
        } else {
            mode
        }
    }

    /// 2×2 Classic Flash Free Transform sub-mode modifier buttons (`Rotate and Skew`, `Scale`,
    /// `Distort`, `Envelope`) shown under `Options` when the Free Transform tool (`Q`) is active.
    pub(crate) fn free_transform_modifier_grid(&mut self, ui: &mut egui::Ui) {
        use crate::ui_icons::{TransformModifierIcon, transform_modifier_button};
        let active = self.active_free_transform_mode();
        let can_deform = self.can_distort_or_envelope_selection();
        let gap = neoflash_ui::tokens::space::S1 / 2.0;
        let row_w = 2.0 * neoflash_ui::metrics::tool_button() + gap;
        let row_h = neoflash_ui::metrics::tool_button();
        ui.allocate_ui_with_layout(
            Vec2::new(row_w, row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = gap;
                if transform_modifier_button(
                    ui,
                    TransformModifierIcon::RotateAndSkew,
                    active == FreeTransformMode::RotateAndSkew,
                    true,
                    "Rotate and Skew",
                )
                .clicked()
                {
                    let next = if active == FreeTransformMode::RotateAndSkew {
                        FreeTransformMode::FreeTransform
                    } else {
                        FreeTransformMode::RotateAndSkew
                    };
                    self.set_free_transform_mode(next);
                }
                if transform_modifier_button(
                    ui,
                    TransformModifierIcon::Scale,
                    active == FreeTransformMode::Scale,
                    true,
                    "Scale",
                )
                .clicked()
                {
                    let next = if active == FreeTransformMode::Scale {
                        FreeTransformMode::FreeTransform
                    } else {
                        FreeTransformMode::Scale
                    };
                    self.set_free_transform_mode(next);
                }
            },
        );
        ui.allocate_ui_with_layout(
            Vec2::new(row_w, row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = gap;
                if transform_modifier_button(
                    ui,
                    TransformModifierIcon::Distort,
                    active == FreeTransformMode::Distort,
                    can_deform,
                    "Distort",
                )
                .clicked()
                {
                    let next = if active == FreeTransformMode::Distort {
                        FreeTransformMode::FreeTransform
                    } else {
                        FreeTransformMode::Distort
                    };
                    self.set_free_transform_mode(next);
                }
                if transform_modifier_button(
                    ui,
                    TransformModifierIcon::Envelope,
                    active == FreeTransformMode::Envelope,
                    can_deform,
                    "Envelope",
                )
                .clicked()
                {
                    let next = if active == FreeTransformMode::Envelope {
                        FreeTransformMode::FreeTransform
                    } else {
                        FreeTransformMode::Envelope
                    };
                    self.set_free_transform_mode(next);
                }
            },
        );
    }

    /// Open the Classic Flash 8 `Magic wand settings` modal seeded from current Lasso state.
    pub(crate) fn open_magic_wand_settings(&mut self) {
        self.authoring_tools.magic_wand_dialog = Some((
            self.authoring_tools.magic_wand_threshold.clamp(0, 200),
            self.authoring_tools.magic_wand_smoothing,
        ));
    }

    /// Render the `Magic wand settings` modal dialog (`Threshold: 0..=200`,
    /// `Smoothing: Pixels | Rough | Normal | Smooth`) when open.
    pub(crate) fn magic_wand_settings_modal(&mut self, ctx: &egui::Context) {
        let Some((mut threshold, mut smoothing)) = self.authoring_tools.magic_wand_dialog else {
            return;
        };
        use neoflash_ui::{
            components as c,
            tokens::{TextStyle, space},
            typography::rich,
        };
        let mut accept = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("nf2-magic-wand-settings"))
            .frame(c::Surface::Dialog.frame(ctx))
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                cancel |= c::DialogHeader {
                    title: "Magic wand settings",
                }
                .show(ui);
                let nf = neoflash_ui::palette(ui.ctx());
                let w = ui.available_width();
                let mut threshold_f = f64::from(threshold);
                if ui
                    .add(
                        c::Field::new(&mut threshold_f, "Threshold")
                            .prefix("Threshold")
                            .range(0.0..=200.0)
                            .decimals(0)
                            .stepper()
                            .width(w),
                    )
                    .changed()
                {
                    threshold = (threshold_f.round() as u32).clamp(0, 200) as u8;
                }
                ui.label(
                    rich("Enter a value between 0 and 200", TextStyle::Small)
                        .color(nf.text_tertiary),
                );
                c::Select::options(
                    ui,
                    "magic-wand-smoothing",
                    &mut smoothing,
                    &[
                        (MagicWandSmoothing::Pixels, "Smoothing: Pixels"),
                        (MagicWandSmoothing::Rough, "Smoothing: Rough"),
                        (MagicWandSmoothing::Normal, "Smoothing: Normal"),
                        (MagicWandSmoothing::Smooth, "Smoothing: Smooth"),
                    ],
                    w,
                );
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
            self.authoring_tools.magic_wand_threshold = threshold.clamp(0, 200);
            self.authoring_tools.magic_wand_smoothing = smoothing;
            self.authoring_tools.magic_wand_dialog = None;
            self.authoring_tools.lasso_magic_wand = true;
            self.authoring_tools.lasso_polygon_mode = false;
            self.authoring_tools.lasso_polygon.clear();
            self.set_tool(Tool::Lasso);
        } else if cancel {
            self.authoring_tools.magic_wand_dialog = None;
        } else {
            self.authoring_tools.magic_wand_dialog = Some((threshold, smoothing));
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
        self.magic_wand_settings_modal(ctx);
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
            let pos_tol =
                local_position_tolerance(gesture.mapping, gesture.zoom, 0.15 + smoothing * 0.65);
            (points, pressure) = simplify_round_spine(&points, &pressure, pos_tol, 0.025);
        }
        shaped_brush_outline_with_position_tolerance(
            &points,
            &pressure,
            self.authoring_tools.brush_size,
            self.authoring_tools.brush_shape,
            local_position_tolerance(gesture.mapping, gesture.zoom, 0.22),
        )
    }
}

/// Sweep the exact same convex tip used by the shared cursor. Adjacent hulls overlap, so
/// nonzero fill produces a continuous stroke even at corners and with stationary pressure.
#[cfg(test)]
pub(crate) fn shaped_brush_outline(
    points: &[Pos2],
    pressures: &[f32],
    size: f32,
    shape: neoflash_ui::components::BrushShapeKind,
) -> Vec<PathCommand> {
    shaped_brush_outline_with_position_tolerance(points, pressures, size, shape, 0.22)
}

fn shaped_brush_outline_with_position_tolerance(
    points: &[Pos2],
    pressures: &[f32],
    size: f32,
    shape: neoflash_ui::components::BrushShapeKind,
    position_tolerance: f32,
) -> Vec<PathCommand> {
    use neoflash_ui::components::BrushShapeKind;
    if shape == BrushShapeKind::Round {
        return round_brush_envelope_with_position_tolerance(
            points,
            pressures,
            size,
            position_tolerance,
        );
    }
    if points.is_empty() || !size.is_finite() || size <= 0. {
        return vec![];
    }
    let raw_forces: Vec<f32> = (0..points.len())
        .map(|i| brush_force(pressures.get(i).copied()))
        .collect();
    let (pts, forces) = simplify_round_spine(points, &raw_forces, position_tolerance, 0.02);
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

fn color_within_threshold(a: [u8; 4], b: [u8; 4], threshold: u8) -> bool {
    let dr = a[0].abs_diff(b[0]);
    let dg = a[1].abs_diff(b[1]);
    let db = a[2].abs_diff(b[2]);
    let da = a[3].abs_diff(b[3]);
    dr.max(dg).max(db).max(da) <= threshold
}

fn magic_wand_bitmap_contour(
    rgba: &[u8],
    w: usize,
    h: usize,
    sx: usize,
    sy: usize,
    threshold: u8,
    smoothing: MagicWandSmoothing,
) -> Vec<[f64; 2]> {
    if w == 0 || h == 0 || sx >= w || sy >= h || rgba.len() < w * h * 4 {
        return Vec::new();
    }
    let pixel = |x: usize, y: usize| -> [u8; 4] {
        let idx = (y * w + x) * 4;
        [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]]
    };
    let seed = pixel(sx, sy);
    let mut mask = vec![false; w * h];
    let mut queue = std::collections::VecDeque::new();
    mask[sy * w + sx] = true;
    queue.push_back((sx, sy));
    let mut top_left = (sx, sy);
    while let Some((x, y)) = queue.pop_front() {
        if (y, x) < (top_left.1, top_left.0) {
            top_left = (x, y);
        }
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if nx < w
                && ny < h
                && !mask[ny * w + nx]
                && color_within_threshold(pixel(nx, ny), seed, threshold)
            {
                mask[ny * w + nx] = true;
                queue.push_back((nx, ny));
            }
        }
    }

    let is_filled = |px: isize, py: isize| -> bool {
        px >= 0
            && py >= 0
            && (px as usize) < w
            && (py as usize) < h
            && mask[(py as usize) * w + (px as usize)]
    };

    // Trace outer boundary of the 4-connected component starting at the top edge of `top_left`.
    // Directions: 0 = East (+1, 0), 1 = South (0, +1), 2 = West (-1, 0), 3 = North (0, -1).
    let step_delta = [(1isize, 0isize), (0, 1), (-1, 0), (0, -1)];
    let edge_has_boundary = |vx: isize, vy: isize, d: usize| -> bool {
        let (rx, ry, lx, ly) = match d {
            0 => (vx, vy, vx, vy - 1),
            1 => (vx - 1, vy, vx, vy),
            2 => (vx - 1, vy - 1, vx - 1, vy),
            _ => (vx, vy - 1, vx - 1, vy - 1),
        };
        is_filled(rx, ry) && !is_filled(lx, ly)
    };

    let start_vx = top_left.0 as isize;
    let start_vy = top_left.1 as isize;
    let mut vx = start_vx + 1;
    let mut vy = start_vy;
    let mut dir = 0usize;
    let mut raw_vertices = vec![[start_vx as f64, start_vy as f64]];
    let max_steps = 4 * (w * h + 2);
    for _ in 0..max_steps {
        if vx == start_vx && vy == start_vy {
            break;
        }
        raw_vertices.push([vx as f64, vy as f64]);
        let mut next_dir = None;
        for cand in [(dir + 3) % 4, dir, (dir + 1) % 4, (dir + 2) % 4] {
            if edge_has_boundary(vx, vy, cand) {
                next_dir = Some(cand);
                break;
            }
        }
        let Some(d) = next_dir else {
            break;
        };
        dir = d;
        vx += step_delta[d].0;
        vy += step_delta[d].1;
    }

    // Drop collinear intermediate grid points along horizontal/vertical runs.
    let mut corners = Vec::with_capacity(raw_vertices.len());
    let n = raw_vertices.len();
    for i in 0..n {
        let prev = raw_vertices[(i + n - 1) % n];
        let curr = raw_vertices[i];
        let next = raw_vertices[(i + 1) % n];
        let dx1 = curr[0] - prev[0];
        let dy1 = curr[1] - prev[1];
        let dx2 = next[0] - curr[0];
        let dy2 = next[1] - curr[1];
        if (dx1 * dy2 - dy1 * dx2).abs() > 1e-6 {
            corners.push(curr);
        }
    }
    if corners.len() < 3 {
        return raw_vertices;
    }

    match smoothing {
        MagicWandSmoothing::Pixels => corners,
        MagicWandSmoothing::Rough => simplify_closed_polygon_rdp(&corners, 1.4),
        MagicWandSmoothing::Normal => {
            let simplified = simplify_closed_polygon_rdp(&corners, 1.0);
            chaikin_smooth_closed(&simplified, 1)
        }
        MagicWandSmoothing::Smooth => {
            let simplified = simplify_closed_polygon_rdp(&corners, 1.6);
            chaikin_smooth_closed(&simplified, 2)
        }
    }
}

fn simplify_closed_polygon_rdp(poly: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if poly.len() <= 4 || tolerance <= 0.0 {
        return poly.to_vec();
    }
    let pts: Vec<Pos2> = poly
        .iter()
        .chain(std::iter::once(&poly[0]))
        .map(|&[x, y]| Pos2::new(x as f32, y as f32))
        .collect();
    let radii = vec![1.0_f32; pts.len()];
    let (mut simplified, _) = simplify_round_spine(&pts, &radii, tolerance as f32, 1.0);
    if simplified.len() > 1 && simplified.first() == simplified.last() {
        simplified.pop();
    }
    if simplified.len() < 3 {
        return poly.to_vec();
    }
    simplified
        .into_iter()
        .map(|p| [f64::from(p.x), f64::from(p.y)])
        .collect()
}

fn chaikin_smooth_closed(poly: &[[f64; 2]], passes: usize) -> Vec<[f64; 2]> {
    let mut current = poly.to_vec();
    for _ in 0..passes {
        if current.len() < 3 {
            break;
        }
        let mut next = Vec::with_capacity(current.len() * 2);
        let n = current.len();
        for i in 0..n {
            let a = current[i];
            let b = current[(i + 1) % n];
            next.push([0.75 * a[0] + 0.25 * b[0], 0.75 * a[1] + 0.25 * b[1]]);
            next.push([0.25 * a[0] + 0.75 * b[0], 0.25 * a[1] + 0.75 * b[1]]);
        }
        current = next;
    }
    current
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
    fn contour_hit_slop_stays_constant_in_screen_space_under_anisotropic_mapping() {
        let commands = vec![
            PathCommand::MoveTo { x: 0., y: 0. },
            PathCommand::LineTo { x: 0., y: 100. },
        ];
        let mapping = [100., 0., 0., 1., 0., 0.];
        assert!(contour_hit_in_screen_metric(
            &commands,
            FillRule::NonZero,
            0.,
            mapping,
            Pos2::new(2., 50.),
            1.,
        ));
        assert!(!contour_hit_in_screen_metric(
            &commands,
            FillRule::NonZero,
            0.,
            mapping,
            Pos2::new(100., 50.),
            1.,
        ));
    }

    #[test]
    fn polystar_preview_and_commit_share_the_screen_space_creation_threshold() {
        let mapping = StageMapping::try_new([100., 0., 0., 1., 40., -20.]).unwrap();
        let origin = Pos2::new(80., 120.);
        let center = Pos2::new(1., 1.);
        assert!(!polystar_drag_is_large_enough(
            mapping,
            origin,
            1.,
            center,
            Pos2::new(1.02, 1.),
        ));
        assert!(polystar_drag_is_large_enough(
            mapping,
            origin,
            1.,
            center,
            Pos2::new(1.021, 1.),
        ));
    }

    #[test]
    fn brush_spine_simplification_preserves_screen_visible_affine_zigzags() {
        let mapping = StageMapping::try_new([100., 0., 0., 1., 0., 0.]).unwrap();
        let tolerance = local_position_tolerance(mapping, 1., 0.8);
        let points = [Pos2::new(0., 0.), Pos2::new(0.1, 0.1), Pos2::new(0., 0.2)];
        let (simplified, _) = simplify_round_spine(&points, &[1.; 3], tolerance, 0.025);
        assert_eq!(simplified, points);
        assert!(
            mapping
                .edit_to_stage_pos(points[1])
                .distance(mapping.edit_to_stage_pos(Pos2::new(0., 0.1)))
                > 0.8,
            "the retained local bend must be visibly larger than the screen-space tolerance"
        );
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
                0.22,
                nonadjacent_rects_overlap,
            );
            let previous = round_brush_envelope_with_overlap_check(
                &points,
                &pressures,
                width,
                0.22,
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

    #[test]
    fn classic_flash_8_tool_and_info_panel_mechanics_work() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let layer_id = editor.symbol().timeline.layers[0].id.clone();

        // 1. Accessibility panel is closed by default in PanelVisibility::default().
        assert!(
            !crate::workspace::PanelVisibility::default().accessibility,
            "Accessibility panel must be closed by default"
        );
        assert!(!editor.workspace.panels.accessibility);

        // 2. Selection Tool double-click on a Merge stroke selects only connected segments of the
        // same stroke color, stopping at segments with a different stroke color.
        let seg_a = Element {
            id: "seg-a".into(),
            merge: true,
            kind: ElementKind::Path {
                commands: vec![
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::LineTo { x: 50.0, y: 0.0 },
                ],
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [255, 0, 0, 255],
                stroke_width: 2.0,
            },
            ..Element::default()
        };
        let seg_b = Element {
            id: "seg-b".into(),
            merge: true,
            kind: ElementKind::Path {
                commands: vec![
                    PathCommand::MoveTo { x: 50.0, y: 0.0 },
                    PathCommand::LineTo { x: 50.0, y: 50.0 },
                ],
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [255, 0, 0, 255],
                stroke_width: 2.0,
            },
            ..Element::default()
        };
        let seg_diff_color = Element {
            id: "seg-c".into(),
            merge: true,
            kind: ElementKind::Path {
                commands: vec![
                    PathCommand::MoveTo { x: 50.0, y: 50.0 },
                    PathCommand::LineTo { x: 100.0, y: 50.0 },
                ],
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [0, 0, 255, 255],
                stroke_width: 2.0,
            },
            ..Element::default()
        };
        let connected = merge_connected_stroke_ids(&[seg_a, seg_b, seg_diff_color], "seg-a");
        assert_eq!(connected, vec!["seg-a".to_owned(), "seg-b".to_owned()]);

        // 3. Alt-Line draws symmetrically from the center out.
        let mut line_g = Gesture {
            tool: Tool::Line,
            symbol: "root".into(),
            frame: 0,
            layer: layer_id.clone(),
            origin: Pos2::ZERO,
            zoom: 1.0,
            mapping: StageMapping::identity(),
            pan: Vec2::ZERO,
            start: Pos2::new(100.0, 100.0),
            latest: Pos2::new(130.0, 110.0),
            points: vec![Pos2::new(100.0, 100.0), Pos2::new(130.0, 110.0)],
            pressures: vec![1.0, 1.0],
            shift: false,
            alt: true,
            targets: Vec::new(),
            eraser_meshes: Vec::new(),
            gradient: None,
        };
        assert_eq!(
            line_gesture_points(&line_g),
            vec![Pos2::new(70.0, 90.0), Pos2::new(130.0, 110.0)]
        );
        line_g.alt = false;
        assert_eq!(
            line_gesture_points(&line_g),
            vec![Pos2::new(100.0, 100.0), Pos2::new(130.0, 110.0)]
        );

        // 4. Paint Bucket click-and-drag defines linear and radial gradient matrices.
        editor.authoring_modify.drawing_fill_gradient = Some(GradientPaint {
            kind: GradientKind::Linear,
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    ratio: 255,
                    color: [0, 0, 255, 255],
                },
            ],
            matrix: [0.1, 0.0, 0.0, 0.1, 0.0, 0.0],
            spread: GradientSpread::Extend,
            linear_rgb: false,
            focal_point_ratio: 0.0,
        });
        editor.authoring_tools.brush_lock_fill = false;
        let mut rect_el = Element {
            id: "bucket-rect".into(),
            kind: ElementKind::Path {
                commands: rectangle_path_commands(100.0, 100.0, RectangleOptions::default()),
                fill_rule: FillRule::NonZero,
                fill: [255, 255, 255, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        editor.apply_bucket_gradient_to_element(
            &mut rect_el,
            Pos2::new(10.0, 50.0),
            Some(Pos2::new(90.0, 50.0)),
        );
        let ElementKind::PaintedPath {
            fill_gradient: Some(grad),
            ..
        } = &rect_el.kind
        else {
            panic!("expected gradient fill from bucket drag");
        };
        assert!((grad.matrix[4] - 50.0).abs() < 1e-3);
        assert!((grad.matrix[5] - 50.0).abs() < 1e-3);

        // 5. Eraser shape (Round vs Square) and Magic Wand smoothing labels/contours.
        assert_eq!(MagicWandSmoothing::Pixels.label(), "Pixels");
        assert_eq!(MagicWandSmoothing::Rough.label(), "Rough");
        assert_eq!(MagicWandSmoothing::Normal.label(), "Normal");
        assert_eq!(MagicWandSmoothing::Smooth.label(), "Smooth");
        assert_eq!(
            EraserShape::Round.brush_shape(),
            neoflash_ui::components::BrushShapeKind::Round
        );
        assert_eq!(
            EraserShape::Square.brush_shape(),
            neoflash_ui::components::BrushShapeKind::Square
        );

        // 6. Magic Wand bitmap contour extraction + vector fill threshold selection.
        let mut bmp_rgba = vec![0u8; 4 * 4 * 4];
        for y in 1..3 {
            for x in 1..3 {
                let i = (y * 4 + x) * 4;
                bmp_rgba[i..i + 4].copy_from_slice(&[200, 50, 50, 255]);
            }
        }
        let contour =
            magic_wand_bitmap_contour(&bmp_rgba, 4, 4, 1, 1, 10, MagicWandSmoothing::Pixels);
        assert_eq!(contour.len(), 4);

        // 7. Info panel 9-point registration anchor (`TopLeft` vs `Center` vs `BottomRight`).
        editor.symbol_mut().timeline.layers[0].keyframes[0].elements = vec![Element {
            id: "info-rect".into(),
            transform: Transform {
                x: 20.0,
                y: 30.0,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 40.0,
                height: 20.0,
                fill: [255, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        }];
        editor.set_selection(vec![SelectionTarget {
            layer: layer_id.clone(),
            element: "info-rect".into(),
        }]);
        let old_bounds = Rect::from_min_size(Pos2::new(20.0, 30.0), Vec2::new(40.0, 20.0));
        // Resizing W from 40 -> 80 with Center anchor shifts top-left X from 20 -> 0 so center stays at 40.
        editor.apply_info_bounds_edit_with_anchor(
            old_bounds,
            40.0,
            40.0,
            80.0,
            20.0,
            neoflash_ui::components::RegistrationPoint::Center,
        );
        let updated = &editor.symbol().timeline.layers[0].keyframes[0].elements[0];
        assert!((updated.transform.x - 0.0).abs() < 1e-3);
        editor.project.validate().unwrap();
    }
}
