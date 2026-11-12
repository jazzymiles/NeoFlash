//! Vector geometry rendering and direct authoring. Draft gestures are not document state.
use crate::theme::UiPalette;
use crate::{
    app::{Editor, Tool},
    stage_mapping::StageMapping,
};
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2};
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
    math::{Point, point},
    path::Path,
};
use neoflash_core::*;
use neoflash_ui::components::canvas as cv;
use std::{
    cell::RefCell,
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TessCacheKey {
    hash_a: u64,
    hash_b: u64,
    len: usize,
    rule: u8,
    width_bits: Option<u64>,
    tol_bits: u32,
}

const MAX_TESS_CACHE_ENTRIES: usize = 65_536;
const MAX_TESS_CACHE_ELEMENTS: usize = 32_000_000;
const MAX_BOUNDS_CACHE_ENTRIES: usize = 65_536;
const MAX_SCREEN_MESH_CACHE_ENTRIES: usize = 32_768;
const MAX_SCREEN_MESH_CACHE_ELEMENTS: usize = 4_000_000;
const PEN_CLOSE_RADIUS: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VectorPaintVisibility {
    pub(crate) fill: bool,
    pub(crate) stroke: bool,
}

/// Effective vector passes, matching the renderer: an explicit gradient replaces the solid
/// fallback, and a stroke also needs a positive authored width.
pub(crate) fn vector_paint_visibility(kind: &ElementKind) -> Option<VectorPaintVisibility> {
    let visible_alpha = |solid: &Color, gradient: Option<&GradientPaint>| {
        gradient.map_or(solid[3] != 0, |paint| {
            paint.stops.iter().any(|stop| stop.color[3] != 0)
        })
    };
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
        } => Some(VectorPaintVisibility {
            fill: fill[3] != 0,
            stroke: stroke[3] != 0 && *stroke_width > 0.,
        }),
        ElementKind::PaintedPath {
            fill,
            stroke,
            stroke_width,
            fill_gradient,
            stroke_gradient,
            ..
        } => Some(VectorPaintVisibility {
            fill: visible_alpha(fill, fill_gradient.as_ref()),
            stroke: visible_alpha(stroke, stroke_gradient.as_ref()) && *stroke_width > 0.,
        }),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RetainedPathStyle {
    Artwork,
    Outline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ScreenMeshPass {
    Fill,
    Stroke,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ScreenMeshSlot {
    identity: u64,
    pass: ScreenMeshPass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ScreenMeshKey {
    tessellation: TessCacheKey,
    transform_bits: [u64; 6],
    origin_bits: [u32; 2],
    zoom_bits: u32,
    color: [u8; 4],
    style: RetainedPathStyle,
}

struct ScreenMeshEntry {
    key: ScreenMeshKey,
    mesh: Arc<egui::Mesh>,
    weight: usize,
}

struct ScreenMeshCache {
    entries: HashMap<ScreenMeshSlot, ScreenMeshEntry>,
    total_elements: usize,
    max_entries: usize,
    max_elements: usize,
}

impl ScreenMeshCache {
    fn new(max_entries: usize, max_elements: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries.min(1024)),
            total_elements: 0,
            max_entries,
            max_elements,
        }
    }

    fn get(&self, slot: ScreenMeshSlot, key: ScreenMeshKey) -> Option<Arc<egui::Mesh>> {
        self.entries
            .get(&slot)
            .filter(|entry| entry.key == key)
            .map(|entry| entry.mesh.clone())
    }

    fn insert(&mut self, slot: ScreenMeshSlot, key: ScreenMeshKey, mesh: Arc<egui::Mesh>) {
        if let Some(previous) = self.entries.remove(&slot) {
            self.total_elements = self.total_elements.saturating_sub(previous.weight);
        }
        let weight = mesh.vertices.len().saturating_add(mesh.indices.len());
        if self.max_entries == 0 || weight > self.max_elements {
            return;
        }
        if self.entries.len() >= self.max_entries
            || self.total_elements.saturating_add(weight) > self.max_elements
        {
            self.entries.clear();
            self.total_elements = 0;
        }
        self.total_elements += weight;
        self.entries
            .insert(slot, ScreenMeshEntry { key, mesh, weight });
    }
}

struct TessCache {
    entries: HashMap<TessCacheKey, Arc<VertexBuffers<Point, u32>>>,
    total_elements: usize,
}

impl Default for TessCache {
    fn default() -> Self {
        Self {
            entries: HashMap::with_capacity(1024),
            total_elements: 0,
        }
    }
}

type TransformedBoundsCacheKey = (u64, u64, usize, [u64; 4]);

thread_local! {
    static TESS_CACHE: RefCell<TessCache> = RefCell::new(TessCache::default());
    static BOUNDS_CACHE: RefCell<HashMap<(u64, u64, usize), Option<PathBounds>>> =
        RefCell::new(HashMap::with_capacity(1024));
    static TRANSFORMED_BOUNDS_CACHE: RefCell<HashMap<TransformedBoundsCacheKey, Option<PathBounds>>> =
        RefCell::new(HashMap::with_capacity(1024));
    static SCREEN_MESH_CACHE: RefCell<ScreenMeshCache> = RefCell::new(ScreenMeshCache::new(
        MAX_SCREEN_MESH_CACHE_ENTRIES,
        MAX_SCREEN_MESH_CACHE_ELEMENTS,
    ));
}

fn resolved_path_fingerprint(
    commands: &[PathCommand],
    supplied: Option<PathFingerprint>,
) -> PathFingerprint {
    supplied.unwrap_or_else(|| path_commands_fingerprint(commands))
}

fn bounds_cache_key(
    commands: &[PathCommand],
    fingerprint: Option<PathFingerprint>,
) -> (u64, u64, usize) {
    let fingerprint = resolved_path_fingerprint(commands, fingerprint);
    (fingerprint.hash_a, fingerprint.hash_b, commands.len())
}

pub(crate) fn path_bounds_cached(
    commands: &[PathCommand],
    fingerprint: Option<PathFingerprint>,
) -> Option<PathBounds> {
    let key = bounds_cache_key(commands, fingerprint);
    if let Some(hit) = BOUNDS_CACHE.with(|cell| cell.borrow().get(&key).copied()) {
        return hit;
    }
    let computed = path_bounds(commands).ok().flatten();
    BOUNDS_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if cache.len() >= MAX_BOUNDS_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(key, computed);
    });
    computed
}

fn transform_path_command_linear(cmd: PathCommand, m: [f64; 6]) -> PathCommand {
    let map = |x: f64, y: f64| -> (f64, f64) { (m[0] * x + m[2] * y, m[1] * x + m[3] * y) };
    match cmd {
        PathCommand::MoveTo { x, y } => {
            let (x, y) = map(x, y);
            PathCommand::MoveTo { x, y }
        }
        PathCommand::LineTo { x, y } => {
            let (x, y) = map(x, y);
            PathCommand::LineTo { x, y }
        }
        PathCommand::QuadraticTo { cx, cy, x, y } => {
            let (cx, cy) = map(cx, cy);
            let (x, y) = map(x, y);
            PathCommand::QuadraticTo { cx, cy, x, y }
        }
        PathCommand::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        } => {
            let (cx1, cy1) = map(cx1, cy1);
            let (cx2, cy2) = map(cx2, cy2);
            let (x, y) = map(x, y);
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            }
        }
        PathCommand::Close => PathCommand::Close,
    }
}

/// Exact tight axis-aligned bounding box of `commands` after applying the affine matrix `m`.
/// Translation `(m[4], m[5])` is applied after looking up the linear-part cache so moving an
/// object across the Stage reuses the cached curve extrema.
pub(crate) fn transformed_path_bounds_cached(
    commands: &[PathCommand],
    fingerprint: Option<PathFingerprint>,
    m: [f64; 6],
) -> Option<PathBounds> {
    if m.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let norm_bits = |v: f64| if v == 0.0 { 0 } else { v.to_bits() };
    let (ha, hb, len) = bounds_cache_key(commands, fingerprint);
    let key = (
        ha,
        hb,
        len,
        [
            norm_bits(m[0]),
            norm_bits(m[1]),
            norm_bits(m[2]),
            norm_bits(m[3]),
        ],
    );
    let linear_bounds =
        if let Some(hit) = TRANSFORMED_BOUNDS_CACHE.with(|cell| cell.borrow().get(&key).copied()) {
            hit
        } else {
            let transformed: Vec<PathCommand> = commands
                .iter()
                .copied()
                .map(|cmd| transform_path_command_linear(cmd, m))
                .collect();
            let computed = path_bounds(&transformed).ok().flatten();
            TRANSFORMED_BOUNDS_CACHE.with(|cell| {
                let mut cache = cell.borrow_mut();
                if cache.len() >= MAX_BOUNDS_CACHE_ENTRIES {
                    cache.clear();
                }
                cache.insert(key, computed);
            });
            computed
        }?;
    let tx = m[4];
    let ty = m[5];
    let shifted = PathBounds {
        x_min: linear_bounds.x_min + tx,
        y_min: linear_bounds.y_min + ty,
        x_max: linear_bounds.x_max + tx,
        y_max: linear_bounds.y_max + ty,
    };
    (shifted.x_min.is_finite()
        && shifted.y_min.is_finite()
        && shifted.x_max.is_finite()
        && shifted.y_max.is_finite())
    .then_some(shifted)
}

fn tess_cache_key(
    commands: &[PathCommand],
    fingerprint: Option<PathFingerprint>,
    rule: FillRule,
    width: Option<f64>,
    tolerance: f32,
) -> (TessCacheKey, f32) {
    let fingerprint = resolved_path_fingerprint(commands, fingerprint);
    let effective_tolerance = if width.is_none() && !fingerprint.has_curves {
        // Straight-line polygon fills do not flatten curves; reuse across all zoom levels.
        0.01_f32
    } else {
        let log_step = (tolerance.clamp(0.0005, 2.0).log2() * 4.0).round();
        2.0_f32.powf(log_step * 0.25).clamp(0.0005, 2.0)
    };
    (
        TessCacheKey {
            hash_a: fingerprint.hash_a,
            hash_b: fingerprint.hash_b,
            len: commands.len(),
            rule: match rule {
                FillRule::NonZero => 0,
                FillRule::EvenOdd => 1,
            },
            width_bits: width.map(|w| if w == 0.0 { 0 } else { w.to_bits() }),
            tol_bits: effective_tolerance.to_bits(),
        },
        effective_tolerance,
    )
}

pub(crate) fn tessellate_cached(
    commands: &[PathCommand],
    rule: FillRule,
    width: Option<f64>,
    tolerance: f32,
) -> Result<Arc<VertexBuffers<Point, u32>>, String> {
    tessellate_cached_with_fingerprint(commands, None, rule, width, tolerance)
}

pub(crate) fn tessellate_cached_with_fingerprint(
    commands: &[PathCommand],
    fingerprint: Option<PathFingerprint>,
    rule: FillRule,
    width: Option<f64>,
    tolerance: f32,
) -> Result<Arc<VertexBuffers<Point, u32>>, String> {
    let (key, effective_tolerance) = tess_cache_key(commands, fingerprint, rule, width, tolerance);
    if let Some(hit) = TESS_CACHE.with(|cell| cell.borrow().entries.get(&key).cloned()) {
        return Ok(hit);
    }

    let built = Arc::new(tessellate(commands, rule, width, effective_tolerance)?);
    let weight = built.vertices.len() + built.indices.len();
    if weight <= MAX_TESS_CACHE_ELEMENTS / 2 {
        TESS_CACHE.with(|cell| {
            let mut cache = cell.borrow_mut();
            if cache.entries.len() >= MAX_TESS_CACHE_ENTRIES
                || cache.total_elements + weight > MAX_TESS_CACHE_ELEMENTS
            {
                cache.entries.clear();
                cache.total_elements = 0;
            }
            cache.total_elements += weight;
            cache.entries.insert(key, built.clone());
        });
    }
    Ok(built)
}

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
    pub fingerprint: Option<PathFingerprint>,
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
        fingerprint,
    } = paint;
    for (c, width) in [(fill, None), (stroke, Some(width))] {
        if c.a() == 0 || width.is_some_and(|w| w <= 0.) {
            continue;
        }
        match tessellate_cached_with_fingerprint(commands, fingerprint, rule, width, tolerance) {
            Ok(geometry) => {
                let mut mesh = egui::epaint::Mesh::default();
                mesh.vertices.reserve(geometry.vertices.len());
                for vertex in &geometry.vertices {
                    mesh.colored_vertex(screen(Pos2::new(vertex.x, vertex.y)), c);
                }
                mesh.indices.clone_from(&geometry.indices);
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

fn screen_mesh_slot(item: &DrawItem, pass: ScreenMeshPass) -> ScreenMeshSlot {
    let mut identity = std::collections::hash_map::DefaultHasher::new();
    item.layer_id.hash(&mut identity);
    item.instance_path.hash(&mut identity);
    item.element_id.hash(&mut identity);
    ScreenMeshSlot {
        identity: identity.finish(),
        pass,
    }
}

#[allow(clippy::too_many_arguments)]
fn retained_screen_mesh(
    item: &DrawItem,
    commands: &[PathCommand],
    fingerprint: Option<PathFingerprint>,
    rule: FillRule,
    width: Option<f64>,
    tolerance: f32,
    color: Color32,
    origin: Pos2,
    zoom: f32,
    style: RetainedPathStyle,
    pass: ScreenMeshPass,
) -> Result<Arc<egui::Mesh>, String> {
    let (tessellation, _) = tess_cache_key(commands, fingerprint, rule, width, tolerance);
    let key = ScreenMeshKey {
        tessellation,
        transform_bits: item.transform.map(f64::to_bits),
        origin_bits: [origin.x.to_bits(), origin.y.to_bits()],
        zoom_bits: zoom.to_bits(),
        color: color.to_array(),
        style,
    };
    let slot = screen_mesh_slot(item, pass);
    if let Some(mesh) = SCREEN_MESH_CACHE.with(|cache| cache.borrow().get(slot, key)) {
        return Ok(mesh);
    }

    let geometry =
        tessellate_cached_with_fingerprint(commands, fingerprint, rule, width, tolerance)?;
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve(geometry.vertices.len());
    for vertex in &geometry.vertices {
        let local = Pos2::new(vertex.x, vertex.y);
        let screen = origin + crate::stage::affine(item.transform, local).to_vec2() * zoom;
        mesh.colored_vertex(screen, color);
    }
    mesh.indices.clone_from(&geometry.indices);
    let mesh = Arc::new(mesh);
    SCREEN_MESH_CACHE.with(|cache| cache.borrow_mut().insert(slot, key, mesh.clone()));
    Ok(mesh)
}

/// Paint an evaluated solid Stage path from a retained screen-space mesh. Drafts, gradients and
/// mask preparation deliberately keep their existing local-tessellation paths.
pub(crate) fn paint_retained_stage_path(
    painter: &egui::Painter,
    item: &DrawItem,
    commands: &[PathCommand],
    paint: PathPaint,
    origin: Pos2,
    zoom: f32,
    style: RetainedPathStyle,
) {
    let PathPaint {
        rule,
        fill,
        stroke,
        width,
        tolerance,
        fingerprint,
    } = paint;
    for (pass, color, width) in [
        (ScreenMeshPass::Fill, fill, None),
        (ScreenMeshPass::Stroke, stroke, Some(width)),
    ] {
        if color.a() == 0 || width.is_some_and(|value| value <= 0.) {
            continue;
        }
        match retained_screen_mesh(
            item,
            commands,
            fingerprint,
            rule,
            width,
            tolerance,
            color,
            origin,
            zoom,
            style,
            pass,
        ) {
            Ok(mesh) => {
                painter.add(Shape::mesh(mesh));
            }
            Err(message) => {
                let position =
                    origin + crate::stage::affine(item.transform, Pos2::ZERO).to_vec2() * zoom;
                painter.text(
                    position,
                    egui::Align2::LEFT_TOP,
                    message,
                    egui::FontId::monospace(crate::theme::PANEL_FONT),
                    UiPalette::current(painter.ctx()).error_text,
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
    pub pointer_mapping: StageMapping,
}
fn smooth_pencil_tangents(anchors: &mut [Anchor]) {
    let points: Vec<_> = anchors.iter().map(|a| a.point).collect();
    for (index, anchor) in anchors.iter_mut().enumerate() {
        let before = points[index.saturating_sub(1)];
        let after = points[(index + 1).min(points.len() - 1)];
        // Catmull–Rom converted to symmetric cubic handles, bounded by adjacent segments.
        let tangent = (after - before) / 6.;
        let limit = if index == 0 {
            anchor.point.distance(after)
        } else if index + 1 == points.len() {
            anchor.point.distance(before)
        } else {
            anchor
                .point
                .distance(before)
                .min(anchor.point.distance(after))
        } / 3.;
        anchor.tangent = tangent.normalized() * tangent.length().min(limit);
    }
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
    pub mapping: StageMapping,
    pub press: Pos2,
    pub began: bool,
    pub changed: bool,
    pub element: String,
    pub pull_tangent: bool,
}

/// Submode for `Tool::Transform` (`Tool::FreeTransform`) on the live Stage (`T-271`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FreeTransformSubmode {
    #[default]
    ScaleRotateSkew,
    Envelope,
    Perspective,
    Distort,
}

#[allow(non_upper_case_globals, dead_code)]
impl Tool {
    /// Alias for `Tool::Transform` matching the Free Transform tool name (`T-271`).
    pub const FreeTransform: Self = Self::Transform;
}

#[derive(Clone, Debug)]
pub(crate) struct EnvelopeLatticeSession {
    pub symbol_id: Id,
    pub layer_idx: usize,
    pub frame: u32,
    pub element_ids: Vec<Id>,
    pub baseline_bounds: [f64; 4],
    pub baseline_elements: Vec<(Id, Element)>,
    pub handles_by_hash: HashMap<u64, [[f64; 2]; 16]>,
}

#[derive(Clone, Debug)]
pub(crate) struct PerspectiveQuadSession {
    pub symbol_id: Id,
    pub layer_idx: usize,
    pub frame: u32,
    pub element_ids: Vec<Id>,
    pub baseline_bounds: [f64; 4],
    pub baseline_elements: Vec<(Id, Element)>,
    pub corners_by_hash: HashMap<u64, [[f64; 2]; 4]>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum StageLiveDeformKind {
    EnvelopeHandle {
        row: usize,
        col: usize,
    },
    PerspectiveCorner {
        corner: usize,
    },
    AssetWarpPin {
        pin_id: Id,
        element_id: Id,
        created_on_press: bool,
    },
    IkBoneEffector {
        armature_id: Id,
        bone_id: Id,
    },
    IkBoneCreate {
        start_stage: [f64; 2],
        current_stage: [f64; 2],
    },
    IkBindSweep {
        unbind: bool,
    },
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct StageLiveDeformDrag {
    pub kind: StageLiveDeformKind,
    pub before_project: Project,
    pub press_stage: [f64; 2],
    pub last_stage: [f64; 2],
    pub changed: bool,
}

#[derive(Default)]
pub(crate) struct VectorState {
    pub draft: Option<PathDraft>,
    pub point_drag: Option<PointDrag>,
    /// Selected anchor command indices for an editable path `(element_id, command_indices)`.
    pub selected_anchors: Option<(Id, Vec<usize>)>,
    /// Active Subselection (`Tool::Subselect`) marquee box `(press_screen, current_screen)`.
    pub subselect_marquee: Option<(Pos2, Pos2)>,
    /// Active Free Transform submode (`T-271`).
    pub free_transform_submode: FreeTransformSubmode,
    /// Cached 4x4 Envelope lattice session (`T-271`).
    pub envelope_lattice_session: Option<EnvelopeLatticeSession>,
    /// Cached 2x2 Perspective quad session (`T-271`).
    pub perspective_quad_session: Option<PerspectiveQuadSession>,
    /// Undeformed base shapes for Asset Warp pin deformation `(symbol_id, frame, element_id)` (`T-271`).
    pub asset_warp_base_elements: HashMap<(Id, u32, Id), Element>,
    /// Currently selected `WarpPin` ID on Stage (`T-271`).
    pub selected_warp_pin: Option<Id>,
    /// Currently selected `IkBone` ID on Stage (`T-271`).
    pub selected_ik_bone: Option<Id>,
    /// Undeformed base shapes and rest bone poses for IK deformation `(symbol_id, frame, element_id)` (`T-271`).
    #[allow(clippy::type_complexity)]
    pub ik_base_elements: HashMap<(Id, u32, Id), (Element, Vec<(Id, [f64; 2], [f64; 2], f64)>)>,
    /// Active live Stage drag transaction for `Envelope`, `Perspective`, `AssetWarp`, `Bone`, `Bind` (`T-271`).
    pub live_deform_drag: Option<StageLiveDeformDrag>,
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
    if handle.role == PointRole::Anchor {
        move_selected_anchors(commands, &[handle.command], to - handle.point);
        return;
    }
    let delta = to - handle.point;
    let dx = delta.x as f64;
    let dy = delta.y as f64;
    match (commands.get_mut(handle.command), handle.role) {
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

pub(crate) fn move_selected_anchors(
    commands: &mut [PathCommand],
    anchor_indices: &[usize],
    delta: Vec2,
) {
    let dx = delta.x as f64;
    let dy = delta.y as f64;
    let all_handles = handles(commands);
    let mut anchors: Vec<usize> = anchor_indices.to_vec();
    for &cmd_idx in anchor_indices {
        let Some(h) = all_handles
            .iter()
            .find(|h| h.command == cmd_idx && h.role == PointRole::Anchor)
        else {
            continue;
        };
        let start = (0..=cmd_idx)
            .rev()
            .find(|&i| matches!(commands.get(i), Some(PathCommand::MoveTo { .. })));
        let close = (cmd_idx..commands.len())
            .take_while(|&i| i == cmd_idx || !matches!(commands[i], PathCommand::MoveTo { .. }))
            .find(|&i| matches!(commands[i], PathCommand::Close));
        if let (Some(start), Some(close)) = (start, close)
            && close > start + 1
        {
            let last = close - 1;
            if cmd_idx == start || cmd_idx == last {
                let other = if cmd_idx == start { last } else { start };
                if all_handles.iter().any(|oh| {
                    oh.command == other
                        && oh.role == PointRole::Anchor
                        && oh.point.distance_sq(h.point) < 1e-8
                }) {
                    anchors.push(other);
                }
            }
        }
    }
    let mut targets = Vec::new();
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

type AnchorIncidentSegments = ([f64; 2], Option<(usize, [f64; 2])>, Option<usize>);

fn anchor_incident_segments(
    commands: &[PathCommand],
    anchor_cmd: usize,
) -> Option<AnchorIncidentSegments> {
    let anchor = anchor_endpoint(commands.get(anchor_cmd)?)?;
    for (start, body_end, closed) in contour_ranges(commands) {
        if anchor_cmd < start || anchor_cmd >= body_end {
            continue;
        }
        let coincident_seam = closed
            && body_end > start + 1
            && anchor_endpoint(&commands[start])
                .zip(anchor_endpoint(&commands[body_end - 1]))
                .is_some_and(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-4);
        let incoming = if anchor_cmd > start && anchor_cmd < body_end {
            anchor_endpoint(&commands[anchor_cmd - 1]).map(|prev| (anchor_cmd, prev))
        } else if anchor_cmd == start && coincident_seam && body_end >= start + 2 {
            anchor_endpoint(&commands[body_end - 2]).map(|prev| (body_end - 1, prev))
        } else {
            None
        };
        let outgoing = if anchor_cmd == body_end - 1 && coincident_seam {
            (start + 1 < body_end).then_some(start + 1)
        } else if anchor_cmd + 1 < body_end {
            Some(anchor_cmd + 1)
        } else {
            None
        };
        return Some((anchor, incoming, outgoing));
    }
    None
}

pub(crate) fn collapse_anchor_handles(
    commands: &[PathCommand],
    anchor_cmd: usize,
) -> Vec<PathCommand> {
    let mut out = commands.to_vec();
    let Some(([ax, ay], incoming, outgoing)) = anchor_incident_segments(commands, anchor_cmd)
    else {
        return out;
    };
    if let Some((in_idx, prev_pt)) = incoming {
        match out[in_idx] {
            PathCommand::QuadraticTo { x, y, .. } => {
                out[in_idx] = PathCommand::LineTo { x, y };
            }
            PathCommand::CubicTo { cx1, cy1, x, y, .. } => {
                if (cx1 - prev_pt[0]).hypot(cy1 - prev_pt[1]) <= 1e-4 {
                    out[in_idx] = PathCommand::LineTo { x, y };
                } else {
                    out[in_idx] = PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2: x,
                        cy2: y,
                        x,
                        y,
                    };
                }
            }
            _ => {}
        }
    }
    if let Some(out_idx) = outgoing {
        match out[out_idx] {
            PathCommand::QuadraticTo { x, y, .. } => {
                out[out_idx] = PathCommand::LineTo { x, y };
            }
            PathCommand::CubicTo { cx2, cy2, x, y, .. } => {
                if (cx2 - x).hypot(cy2 - y) <= 1e-4 {
                    out[out_idx] = PathCommand::LineTo { x, y };
                } else {
                    out[out_idx] = PathCommand::CubicTo {
                        cx1: ax,
                        cy1: ay,
                        cx2,
                        cy2,
                        x,
                        y,
                    };
                }
            }
            _ => {}
        }
    }
    out
}

pub(crate) fn pull_anchor_tangent(commands: &mut [PathCommand], anchor_cmd: usize, tangent: Vec2) {
    let Some(([ax, ay], incoming, outgoing)) = anchor_incident_segments(commands, anchor_cmd)
    else {
        return;
    };
    let tx = f64::from(tangent.x);
    let ty = f64::from(tangent.y);
    if let Some((in_idx, prev_pt)) = incoming {
        match commands[in_idx] {
            PathCommand::LineTo { x, y } => {
                commands[in_idx] = PathCommand::CubicTo {
                    cx1: prev_pt[0],
                    cy1: prev_pt[1],
                    cx2: ax - tx,
                    cy2: ay - ty,
                    x,
                    y,
                };
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                commands[in_idx] = PathCommand::CubicTo {
                    cx1: cx,
                    cy1: cy,
                    cx2: ax - tx,
                    cy2: ay - ty,
                    x,
                    y,
                };
            }
            PathCommand::CubicTo { cx1, cy1, x, y, .. } => {
                commands[in_idx] = PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2: ax - tx,
                    cy2: ay - ty,
                    x,
                    y,
                };
            }
            _ => {}
        }
    }
    if let Some(out_idx) = outgoing {
        match commands[out_idx] {
            PathCommand::LineTo { x, y } => {
                commands[out_idx] = PathCommand::CubicTo {
                    cx1: ax + tx,
                    cy1: ay + ty,
                    cx2: x,
                    cy2: y,
                    x,
                    y,
                };
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                commands[out_idx] = PathCommand::CubicTo {
                    cx1: ax + tx,
                    cy1: ay + ty,
                    cx2: cx,
                    cy2: cy,
                    x,
                    y,
                };
            }
            PathCommand::CubicTo { cx2, cy2, x, y, .. } => {
                commands[out_idx] = PathCommand::CubicTo {
                    cx1: ax + tx,
                    cy1: ay + ty,
                    cx2,
                    cy2,
                    x,
                    y,
                };
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

fn polyline_length(anchors: &[Anchor]) -> f32 {
    anchors
        .windows(2)
        .map(|w| w[0].point.distance(w[1].point))
        .sum()
}

/// Preferences ▸ Drawing ▸ Recognise lines: collapse a nearly straight open pencil stroke into a
/// single segment when every sample stays within `mode.recognise_lines_px()` of the start–end chord.
pub(crate) fn recognise_straight_line(
    raw_anchors: &[Anchor],
    zoom: f32,
    mode: crate::workspace::Tolerance,
) -> Option<[Pos2; 2]> {
    let tol_px = mode.recognise_lines_px()?;
    if raw_anchors.len() < 2 {
        return None;
    }
    let zoom = zoom.max(1e-3);
    let tol = tol_px / zoom;
    let a = raw_anchors[0].point;
    let b = raw_anchors[raw_anchors.len() - 1].point;
    let line = b - a;
    let chord_len = line.length();
    if chord_len < 4.0 / zoom {
        return None;
    }
    if polyline_length(raw_anchors) > chord_len * 1.15 + tol {
        return None;
    }
    let line_sq = line.length_sq();
    let max_sq = tol * tol;
    for anchor in raw_anchors {
        let t = ((anchor.point - a).dot(line) / line_sq).clamp(0.0, 1.0);
        if anchor.point.distance_sq(a + line * t) > max_sq {
            return None;
        }
    }
    Some([a, b])
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RecognisedShape {
    Rectangle {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    Ellipse {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}

fn sample_uniform_loop(points: &[Pos2], count: usize) -> Vec<Pos2> {
    if points.len() < 2 || count == 0 {
        return points.to_vec();
    }
    let mut cumulative = Vec::with_capacity(points.len());
    cumulative.push(0.0_f32);
    let mut total = 0.0_f32;
    for w in points.windows(2) {
        total += w[0].distance(w[1]);
        cumulative.push(total);
    }
    if total <= 1e-5 {
        return vec![points[0]; count];
    }
    let mut out = Vec::with_capacity(count);
    let mut seg = 0_usize;
    for i in 0..count {
        let target = total * (i as f32) / (count as f32);
        while seg + 2 < cumulative.len() && cumulative[seg + 1] < target {
            seg += 1;
        }
        let seg_start = cumulative[seg];
        let seg_len = (cumulative[seg + 1] - seg_start).max(1e-6);
        let t = ((target - seg_start) / seg_len).clamp(0.0, 1.0);
        out.push(points[seg].lerp(points[seg + 1], t));
    }
    out
}

/// Preferences ▸ Drawing ▸ Recognise shapes: classify a closed (or nearly closed) freehand pencil
/// loop as an axis-aligned `Rectangle` or `Ellipse` when the fit is clean and unambiguous.
pub(crate) fn recognise_primitive_shape(
    raw_anchors: &[Anchor],
    closed: bool,
    zoom: f32,
    mode: crate::workspace::Tolerance,
) -> Option<RecognisedShape> {
    use crate::workspace::Tolerance;
    if mode == Tolerance::Off || raw_anchors.len() < 5 {
        return None;
    }
    let zoom = zoom.max(1e-3);
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for anchor in raw_anchors {
        min_x = min_x.min(anchor.point.x);
        max_x = max_x.max(anchor.point.x);
        min_y = min_y.min(anchor.point.y);
        max_y = max_y.max(anchor.point.y);
    }
    let width = max_x - min_x;
    let height = max_y - min_y;
    if !width.is_finite() || !height.is_finite() || width * zoom < 12.0 || height * zoom < 12.0 {
        return None;
    }
    let min_dim = width.min(height);
    let max_dim = width.max(height);
    if min_dim / max_dim < 0.1 {
        return None;
    }

    let first = raw_anchors[0].point;
    let last = raw_anchors[raw_anchors.len() - 1].point;
    let gap = if closed { 0.0 } else { first.distance(last) };
    let max_gap = match mode {
        Tolerance::Off => return None,
        Tolerance::MustBeClose => (8.0 / zoom).max(0.12 * min_dim),
        Tolerance::Normal => (18.0 / zoom).max(0.25 * min_dim),
        Tolerance::CanBeDistant => (32.0 / zoom).max(0.38 * min_dim),
    };
    if gap > max_gap {
        return None;
    }

    let arc_len = polyline_length(raw_anchors);
    let loop_len = arc_len + gap;
    if arc_len <= 0.0 {
        return None;
    }

    let mut loop_pts: Vec<Pos2> = raw_anchors.iter().map(|a| a.point).collect();
    if first.distance(last) > 1e-4 {
        loop_pts.push(first);
    }
    let samples = sample_uniform_loop(&loop_pts, 64);

    // Candidate 1: Axis-aligned Rectangle.
    let rect_perim = 2.0 * (width + height);
    let px_floor = match mode {
        Tolerance::Off => 0.0,
        Tolerance::MustBeClose => 2.0,
        Tolerance::Normal => 4.0,
        Tolerance::CanBeDistant => 7.0,
    } / (zoom * min_dim);
    let (rect_rms_limit, rect_max_limit) = match mode {
        Tolerance::Off => return None,
        Tolerance::MustBeClose => (
            0.035_f32.max(px_floor * 0.6).min(0.06),
            0.085_f32.max(px_floor).min(0.13),
        ),
        Tolerance::Normal => (
            0.065_f32.max(px_floor * 0.6).min(0.08),
            0.145_f32.max(px_floor).min(0.18),
        ),
        Tolerance::CanBeDistant => (
            0.095_f32.max(px_floor * 0.6).min(0.11),
            0.21_f32.max(px_floor).min(0.23),
        ),
    };
    let edge_band = rect_max_limit * min_dim;
    let mut rect_sq_sum = 0.0_f32;
    let mut rect_max = 0.0_f32;
    let mut edge_hits = [0_u32; 4];
    for p in &samples {
        let dl = (p.x - min_x).abs();
        let dr = (max_x - p.x).abs();
        let dt = (p.y - min_y).abs();
        let db = (max_y - p.y).abs();
        if dl <= edge_band {
            edge_hits[0] += 1;
        }
        if dr <= edge_band {
            edge_hits[1] += 1;
        }
        if dt <= edge_band {
            edge_hits[2] += 1;
        }
        if db <= edge_band {
            edge_hits[3] += 1;
        }
        let err = dl.min(dr).min(dt).min(db) / min_dim;
        rect_sq_sum += err * err;
        rect_max = rect_max.max(err);
    }
    let rect_rms = (rect_sq_sum / samples.len() as f32).sqrt();
    let rect_ok = arc_len >= 0.72 * rect_perim
        && loop_len <= 1.35 * rect_perim
        && edge_hits.iter().all(|&c| c >= 4)
        && rect_rms <= rect_rms_limit
        && rect_max <= rect_max_limit;

    // Candidate 2: Axis-aligned Ellipse / Circle.
    let rx = width * 0.5;
    let ry = height * 0.5;
    let cx = min_x + rx;
    let cy = min_y + ry;
    let ell_perim = std::f32::consts::PI
        * (3.0 * (rx + ry) - ((3.0 * rx + ry) * (rx + 3.0 * ry)).max(0.0).sqrt());
    let (ell_rms_limit, ell_max_limit) = match mode {
        Tolerance::Off => return None,
        Tolerance::MustBeClose => (0.025_f32, 0.055_f32),
        Tolerance::Normal => (0.048_f32, 0.105_f32),
        Tolerance::CanBeDistant => (0.072_f32, 0.138_f32),
    };
    let mut ell_sq_sum = 0.0_f32;
    let mut ell_max = 0.0_f32;
    let mut sectors = 0_u8;
    for p in &samples {
        let u = (p.x - cx) / rx;
        let v = (p.y - cy) / ry;
        let angle = v.atan2(u).rem_euclid(std::f32::consts::TAU);
        let bucket = ((angle / std::f32::consts::TAU) * 8.0).floor() as u32;
        sectors |= 1_u8 << bucket.min(7);
        // Scale radial error by 0.5 so both candidates measure error in units of `min_dim`.
        let err = 0.5 * (u.hypot(v) - 1.0).abs();
        ell_sq_sum += err * err;
        ell_max = ell_max.max(err);
    }
    let ell_rms = (ell_sq_sum / samples.len() as f32).sqrt();
    let ell_ok = arc_len >= 0.72 * ell_perim
        && loop_len <= 1.35 * ell_perim
        && sectors.count_ones() >= 7
        && ell_rms <= ell_rms_limit
        && ell_max <= ell_max_limit;

    if rect_ok && rect_rms < 0.75 * ell_rms {
        Some(RecognisedShape::Rectangle {
            x: f64::from(min_x),
            y: f64::from(min_y),
            width: f64::from(width),
            height: f64::from(height),
        })
    } else if ell_ok && ell_rms < 0.75 * rect_rms {
        Some(RecognisedShape::Ellipse {
            x: f64::from(min_x),
            y: f64::from(min_y),
            width: f64::from(width),
            height: f64::from(height),
        })
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EndpointSide {
    Start,
    End,
}

#[derive(Clone, Debug)]
struct OpenEndpointMatch {
    element_id: Id,
    side: EndpointSide,
    world_point: Pos2,
    matrix: [f64; 6],
    same_style: bool,
}

fn open_contour_endpoints(commands: &[PathCommand]) -> Option<((f64, f64), (f64, f64))> {
    if commands.len() < 2 {
        return None;
    }
    let PathCommand::MoveTo { x: x0, y: y0 } = commands[0] else {
        return None;
    };
    if commands[1..]
        .iter()
        .any(|c| matches!(c, PathCommand::MoveTo { .. } | PathCommand::Close))
    {
        return None;
    }
    let (x1, y1) = match *commands.last()? {
        PathCommand::LineTo { x, y }
        | PathCommand::QuadraticTo { x, y, .. }
        | PathCommand::CubicTo { x, y, .. } => (x, y),
        PathCommand::MoveTo { .. } | PathCommand::Close => return None,
    };
    Some(((x0, y0), (x1, y1)))
}

fn inverse_f64(m: [f64; 6], px: f64, py: f64) -> Option<(f64, f64)> {
    let d = m[0] * m[3] - m[1] * m[2];
    if d.abs() < 1e-10 {
        return None;
    }
    let x = px - m[4];
    let y = py - m[5];
    Some(((m[3] * x - m[2] * y) / d, (-m[1] * x + m[0] * y) / d))
}

fn map_commands_inverse(commands: &[PathCommand], m: [f64; 6]) -> Option<Vec<PathCommand>> {
    let mut out = Vec::with_capacity(commands.len());
    for cmd in commands {
        let mapped = match *cmd {
            PathCommand::MoveTo { x, y } => {
                let (x, y) = inverse_f64(m, x, y)?;
                PathCommand::MoveTo { x, y }
            }
            PathCommand::LineTo { x, y } => {
                let (x, y) = inverse_f64(m, x, y)?;
                PathCommand::LineTo { x, y }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let (cx, cy) = inverse_f64(m, cx, cy)?;
                let (x, y) = inverse_f64(m, x, y)?;
                PathCommand::QuadraticTo { cx, cy, x, y }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let (cx1, cy1) = inverse_f64(m, cx1, cy1)?;
                let (cx2, cy2) = inverse_f64(m, cx2, cy2)?;
                let (x, y) = inverse_f64(m, x, y)?;
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                }
            }
            PathCommand::Close => PathCommand::Close,
        };
        out.push(mapped);
    }
    Some(out)
}

fn map_commands_forward(commands: &[PathCommand], matrix: [f64; 6]) -> Vec<PathCommand> {
    let map = |x: f64, y: f64| {
        (
            matrix[0] * x + matrix[2] * y + matrix[4],
            matrix[1] * x + matrix[3] * y + matrix[5],
        )
    };
    commands
        .iter()
        .map(|command| match *command {
            PathCommand::MoveTo { x, y } => {
                let (x, y) = map(x, y);
                PathCommand::MoveTo { x, y }
            }
            PathCommand::LineTo { x, y } => {
                let (x, y) = map(x, y);
                PathCommand::LineTo { x, y }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let (cx, cy) = map(cx, cy);
                let (x, y) = map(x, y);
                PathCommand::QuadraticTo { cx, cy, x, y }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let (cx1, cy1) = map(cx1, cy1);
                let (cx2, cy2) = map(cx2, cy2);
                let (x, y) = map(x, y);
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                }
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

fn set_contour_start(commands: &mut [PathCommand], pt: (f64, f64)) {
    if let Some(PathCommand::MoveTo { x, y }) = commands.first_mut() {
        *x = pt.0;
        *y = pt.1;
    }
}

fn set_contour_end(commands: &mut [PathCommand], pt: (f64, f64)) {
    if let Some(
        PathCommand::LineTo { x, y }
        | PathCommand::QuadraticTo { x, y, .. }
        | PathCommand::CubicTo { x, y, .. },
    ) = commands.last_mut()
    {
        *x = pt.0;
        *y = pt.1;
    }
}

fn reverse_open_contour(commands: &[PathCommand]) -> Option<Vec<PathCommand>> {
    let ((x0, y0), (xn, yn)) = open_contour_endpoints(commands)?;
    let mut prev = (x0, y0);
    let mut segments = Vec::with_capacity(commands.len() - 1);
    for cmd in &commands[1..] {
        match *cmd {
            PathCommand::LineTo { x, y } => {
                segments.push((prev, PathCommand::LineTo { x, y }));
                prev = (x, y);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                segments.push((prev, PathCommand::QuadraticTo { cx, cy, x, y }));
                prev = (x, y);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                segments.push((
                    prev,
                    PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x,
                        y,
                    },
                ));
                prev = (x, y);
            }
            PathCommand::MoveTo { .. } | PathCommand::Close => return None,
        }
    }
    let mut out = Vec::with_capacity(commands.len());
    out.push(PathCommand::MoveTo { x: xn, y: yn });
    for ((px, py), cmd) in segments.into_iter().rev() {
        out.push(match cmd {
            PathCommand::LineTo { .. } => PathCommand::LineTo { x: px, y: py },
            PathCommand::QuadraticTo { cx, cy, .. } => PathCommand::QuadraticTo {
                cx,
                cy,
                x: px,
                y: py,
            },
            PathCommand::CubicTo {
                cx1, cy1, cx2, cy2, ..
            } => PathCommand::CubicTo {
                cx1: cx2,
                cy1: cy2,
                cx2: cx1,
                cy2: cy1,
                x: px,
                y: py,
            },
            _ => return None,
        });
    }
    Some(out)
}

impl Editor {
    /// End only the active pointer phase before changing the viewport. Pen
    /// keeps its existing anchors/controls for the next press; Pencil commits
    /// samples already collected in the old view. A later move/release cannot
    /// append geometry until a new primary press starts another phase.
    pub(crate) fn end_vector_pointer_phase(&mut self) {
        let finish_pen_close = self
            .vector
            .draft
            .as_ref()
            .is_some_and(|draft| !draft.pencil && draft.closing && draft.anchors.len() >= 2);
        let finish_pencil = self.vector.draft.as_mut().is_some_and(|draft| {
            let active = draft.dragging || draft.closing;
            draft.dragging = false;
            draft.closing = false;
            draft.pencil && active
        });
        if finish_pen_close {
            self.finish_path(true);
        } else if finish_pencil {
            self.finish_path(false);
        }
    }

    pub(crate) fn finish_point_drag(&mut self) {
        if let Some(drag) = self.vector.point_drag.take()
            && drag.began
        {
            if drag.changed {
                self.commit_history();
            } else {
                self.history.cancel(&mut self.project);
            }
        }
    }
    pub(crate) fn finish_edits(&mut self) {
        self.finish_shape_hint_drag(false);
        self.finish_motion_curve_drag();
        self.finish_motion_key_drag();
        self.authoring_tools.cancel();
        self.finish_property_edit();
        self.finish_selection_gesture();
        self.finish_stage_text_typing();
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
        self.cancel_shape_hint_drag();
        self.cancel_motion_drags();
        self.authoring_tools.cancel();
        self.finish_property_edit();
        self.cancel_selection_gesture();
        self.exit_stage_text_edit();
        self.cancel_vector();
        self.shape_draft = None;
        self.timeline_key_drag = None;
        self.scale9_guide_drag = None;
        self.cancel_guide_drag();
    }
    pub(crate) fn set_playing(&mut self, playing: bool) {
        self.finish_edits();
        if playing && !self.playing {
            self.rewind_if_finished();
        }
        self.timeline_key_drag = None;
        self.playing = playing;
        self.accumulator = 0.;
        self.sync_authored_preview();
        self.last_tick = std::time::Instant::now();
    }
    pub(crate) fn set_tool(&mut self, tool: Tool) {
        if tool == self.tool
            && tool == Tool::Pen
            && self
                .vector
                .draft
                .as_ref()
                .is_some_and(|draft| draft.anchors.len() >= 2)
        {
            self.finish_path(false);
            return;
        }
        if tool != self.tool {
            self.finish_edits();
            if tool != Tool::Text {
                self.exit_stage_text_edit();
            }
            self.timeline_key_drag = None;
            self.vector = Default::default();
            if self.authoring_tools.primitive_drawn_selection && tool != Tool::Select {
                self.clear_selection();
            }
            self.authoring_tools.primitive_drawn_selection = false;
        }
        self.tool = tool;
        if self.selection.is_empty()
            && !matches!(tool, Tool::Select | Tool::Razor | Tool::Hand | Tool::Zoom)
        {
            self.timeline_edit.focused = false;
        }
        let sticky = match tool {
            Tool::Deco => crate::stage::properties_tool::StageStickyToolMode::Deco,
            Tool::SprayBrush => crate::stage::properties_tool::StageStickyToolMode::SprayBrush,
            Tool::Bone => crate::stage::properties_tool::StageStickyToolMode::Bone,
            Tool::Bind => crate::stage::properties_tool::StageStickyToolMode::Bind,
            Tool::AssetWarp => crate::stage::properties_tool::StageStickyToolMode::PuppetWarp,
            Tool::Translate3D => crate::stage::properties_tool::StageStickyToolMode::Translate3D,
            Tool::Rotate3D => crate::stage::properties_tool::StageStickyToolMode::Rotate3D,
            _ => crate::stage::properties_tool::StageStickyToolMode::None,
        };
        self.set_stage_sticky_tool_mode(sticky);
    }
    fn find_nearest_open_endpoint(&self, pt: Pos2, tol: f32) -> Option<OpenEndpointMatch> {
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let sampled = sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps));
        let no_gradients = self.authoring_modify.drawing_fill_gradient.is_none()
            && self.authoring_modify.drawing_stroke_gradient.is_none();
        let mut best: Option<(f32, OpenEndpointMatch)> = None;
        for el in sampled.iter().rev() {
            let ElementKind::Path {
                commands,
                fill,
                stroke,
                stroke_width,
                ..
            } = &el.kind
            else {
                continue;
            };
            let Some((local_start, local_end)) = open_contour_endpoints(commands) else {
                continue;
            };
            let matrix = transform_matrix(el.transform);
            let same_style = no_gradients
                && fill[3] == 0
                && *stroke == self.stroke
                && (*stroke_width - self.stroke_width).abs() < 1e-6;
            for (side, local_pt) in [
                (EndpointSide::Start, local_start),
                (EndpointSide::End, local_end),
            ] {
                let world_point =
                    crate::stage::affine(matrix, Pos2::new(local_pt.0 as f32, local_pt.1 as f32));
                let dist = world_point.distance(pt);
                if dist <= tol && best.as_ref().is_none_or(|(b, _)| dist < *b) {
                    best = Some((
                        dist,
                        OpenEndpointMatch {
                            element_id: el.id.clone(),
                            side,
                            world_point,
                            matrix,
                            same_style,
                        },
                    ));
                }
            }
        }
        best.map(|(_, m)| m)
    }

    fn build_joined_open_path(
        &self,
        new_world_commands: &[PathCommand],
        start_match: Option<&OpenEndpointMatch>,
        end_match: Option<&OpenEndpointMatch>,
    ) -> Option<(Id, Vec<PathCommand>, bool)> {
        let (primary, is_start_primary) = match (start_match, end_match) {
            (Some(s), _) if s.same_style => (s, true),
            (_, Some(e)) if e.same_style => (e, false),
            _ => return None,
        };
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let sampled = sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps));
        let existing = sampled.iter().find(|e| e.id == primary.element_id)?;
        let ElementKind::Path {
            commands: existing_cmds,
            ..
        } = &existing.kind
        else {
            return None;
        };
        let (existing_start, existing_end) = open_contour_endpoints(existing_cmds)?;
        let local_new = map_commands_inverse(new_world_commands, primary.matrix)?;
        if local_new.len() < 2 {
            return None;
        }
        let other_closes_same = if is_start_primary {
            end_match.is_some_and(|e| e.element_id == primary.element_id && e.side != primary.side)
        } else {
            start_match
                .is_some_and(|s| s.element_id == primary.element_id && s.side != primary.side)
        };
        let has_curve = existing_cmds.iter().chain(local_new.iter()).any(|c| {
            matches!(
                c,
                PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
            )
        });
        let can_close =
            other_closes_same && (has_curve || existing_cmds.len() + local_new.len() >= 5);

        let mut merged = Vec::with_capacity(existing_cmds.len() + local_new.len());
        let mut closed_loop = false;
        match (is_start_primary, primary.side) {
            (true, EndpointSide::End) => {
                // Existing start..end + new start..end
                let mut tail = local_new;
                set_contour_start(&mut tail, existing_end);
                if can_close {
                    set_contour_end(&mut tail, existing_start);
                }
                merged.extend_from_slice(existing_cmds);
                merged.extend_from_slice(&tail[1..]);
                if can_close {
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                }
            }
            (false, EndpointSide::End) => {
                // Existing start..end + reversed(new)
                let mut rev = reverse_open_contour(&local_new)?;
                set_contour_start(&mut rev, existing_end);
                if can_close {
                    set_contour_end(&mut rev, existing_start);
                }
                merged.extend_from_slice(existing_cmds);
                merged.extend_from_slice(&rev[1..]);
                if can_close {
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                }
            }
            (false, EndpointSide::Start) => {
                // New start..end + existing start..end
                let mut head = local_new;
                set_contour_end(&mut head, existing_start);
                if can_close {
                    let head_start = open_contour_endpoints(&head)?.0;
                    let mut rest = existing_cmds.clone();
                    set_contour_end(&mut rest, head_start);
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&rest[1..]);
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                } else {
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&existing_cmds[1..]);
                }
            }
            (true, EndpointSide::Start) => {
                // Reversed(new) + existing start..end
                let mut head = reverse_open_contour(&local_new)?;
                set_contour_end(&mut head, existing_start);
                if can_close {
                    let head_start = open_contour_endpoints(&head)?.0;
                    let mut rest = existing_cmds.clone();
                    set_contour_end(&mut rest, head_start);
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&rest[1..]);
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                } else {
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&existing_cmds[1..]);
                }
            }
        }
        validate_path_commands(&merged).ok()?;
        Some((primary.element_id.clone(), merged, closed_loop))
    }

    pub(crate) fn finish_path(&mut self, closed: bool) {
        let Some(mut draft) = self.vector.draft.take() else {
            return;
        };
        if draft.limit_reached {
            self.message("Pencil stroke exceeds 16000 samples. Draw shorter strokes; this draft was discarded.");
            return;
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
        // Recognition/smoothing tolerances are screen-point preferences. Under a
        // non-uniform parent transform, use the largest scale so accepted local
        // distances never exceed the requested screen tolerance.
        let zoom =
            (draft.pointer_zoom as f64 * draft.pointer_mapping.scale_bounds().1).max(1e-3) as f32;
        let mut drawing = self.workspace.drawing.clone();
        if draft.pencil {
            use crate::{tool_preferences::PencilMode, workspace::Tolerance};
            if self.workspace.tools.pencil_smoothing == 0. {
                drawing.smooth_curves = Tolerance::Off;
            }
            match self.workspace.tools.pencil_mode {
                PencilMode::Straighten => {}
                PencilMode::Smooth => {
                    drawing.recognise_lines = Tolerance::Off;
                    drawing.recognise_shapes = Tolerance::Off;
                }
                PencilMode::Ink => {
                    drawing.recognise_lines = Tolerance::Off;
                    drawing.recognise_shapes = Tolerance::Off;
                    drawing.smooth_curves = Tolerance::Off;
                    drawing.connect_lines = Tolerance::Off;
                }
            }
        }
        let raw_anchors = draft.anchors.clone();

        // 1. Preferences ▸ Drawing ▸ Recognise shapes (Pencil only).
        let recognised_shape = if draft.pencil {
            recognise_primitive_shape(&raw_anchors, closed, zoom, drawing.recognise_shapes)
        } else {
            None
        };

        // 2. Preferences ▸ Drawing ▸ Connect lines (self-close check).
        let connect_tol = drawing.connect_lines.connect_lines_px().map(|px| px / zoom);
        let mut effective_closed = closed;
        let mut self_closed = false;
        if !effective_closed
            && recognised_shape.is_none()
            && let Some(tol) = connect_tol
        {
            let first = raw_anchors[0].point;
            let last = raw_anchors[raw_anchors.len() - 1].point;
            if raw_anchors.len() >= 3
                && polyline_length(&raw_anchors) >= tol * 3.0
                && first.distance(last) <= tol
            {
                effective_closed = true;
                self_closed = true;
            }
        }

        // 3. Preferences ▸ Drawing ▸ Recognise lines & Smooth curves (Pencil only).
        if draft.pencil && recognised_shape.is_none() {
            if !effective_closed
                && let Some([a, b]) =
                    recognise_straight_line(&raw_anchors, zoom, drawing.recognise_lines)
            {
                draft.anchors = vec![
                    Anchor {
                        point: a,
                        tangent: Vec2::ZERO,
                    },
                    Anchor {
                        point: b,
                        tangent: Vec2::ZERO,
                    },
                ];
            } else if let Some(rdp_px) = drawing.smooth_curves.smooth_rdp_px() {
                draft.anchors = simplify(
                    &draft.anchors,
                    rdp_px / zoom * (self.workspace.tools.pencil_smoothing as f32 / 50.),
                );
            }
        }

        if draft.pencil
            && self.workspace.tools.pencil_mode == crate::tool_preferences::PencilMode::Smooth
            && drawing.smooth_curves != crate::workspace::Tolerance::Off
            && recognised_shape.is_none()
        {
            smooth_pencil_tangents(&mut draft.anchors);
        }

        // If self-closing left a trailing point right on top of the start anchor, drop it so
        // `PathDraft::commands(true)` closes cleanly without a tiny hook segment, or cancel
        // self-close if simplification reduced the stroke to a 2-point out-and-back line.
        if self_closed && let Some(tol) = connect_tol {
            if draft.anchors.len() > 3
                && draft.anchors.last().is_some_and(|last| {
                    last.tangent.length_sq() <= 0.01
                        && last.point.distance(draft.anchors[0].point) <= tol
                })
            {
                draft.anchors.pop();
            } else if draft.anchors.len() == 3
                && draft.anchors[2].tangent.length_sq() <= 0.01
                && draft.anchors[2].point.distance(draft.anchors[0].point) <= tol
            {
                effective_closed = false;
            }
        }
        if draft.anchors.len() < 2 && recognised_shape.is_none() {
            return;
        }

        if let Some(shape) = recognised_shape {
            let (kind, x, y, msg) = match shape {
                RecognisedShape::Rectangle {
                    x,
                    y,
                    width,
                    height,
                } => (
                    ElementKind::Rectangle {
                        width,
                        height,
                        fill: self.fill,
                        stroke: self.stroke,
                        stroke_width: self.stroke_width,
                    },
                    x,
                    y,
                    "Rectangle recognised.",
                ),
                RecognisedShape::Ellipse {
                    x,
                    y,
                    width,
                    height,
                } => (
                    ElementKind::Ellipse {
                        width,
                        height,
                        fill: self.fill,
                        stroke: self.stroke,
                        stroke_width: self.stroke_width,
                    },
                    x,
                    y,
                    "Oval recognised.",
                ),
            };
            if draft.symbol == self.symbol_id()
                && draft.frame == self.frame
                && target_layer == self.layer
            {
                self.add_element(kind, x, y);
            } else {
                self.with_temporary_symbol_target(
                    draft.symbol.clone(),
                    draft.frame,
                    target_layer,
                    |editor| editor.add_element(kind, x, y),
                );
            }
            self.message(msg);
            return;
        }

        let draft_symbol = draft.symbol.clone();
        let draft_frame = draft.frame;
        let live_target = draft_symbol == self.symbol_id()
            && draft_frame == self.frame
            && target_layer == self.layer;
        let mut finish = |editor: &mut Editor| {
            // 4. Preferences ▸ Drawing ▸ Connect lines (snap / join open path endpoints on
            // the original layer even if the playhead or edit session has since moved).
            let mut joined = None;
            if !effective_closed && let Some(tol) = connect_tol {
                let start_pt = draft.anchors[0].point;
                let end_pt = draft.anchors[draft.anchors.len() - 1].point;
                let start_match = editor.find_nearest_open_endpoint(start_pt, tol);
                let end_match = editor.find_nearest_open_endpoint(end_pt, tol).filter(|e| {
                    start_match
                        .as_ref()
                        .is_none_or(|s| s.element_id != e.element_id || s.side != e.side)
                });
                if let Some(ref s) = start_match
                    && (draft.anchors.len() > 2 || s.world_point.distance(end_pt) > 1e-3)
                {
                    draft.anchors[0].point = s.world_point;
                }
                if let Some(ref e) = end_match {
                    let current_start = draft.anchors[0].point;
                    if (draft.anchors.len() > 2 || e.world_point.distance(current_start) > 1e-3)
                        && let Some(last) = draft.anchors.last_mut()
                    {
                        last.point = e.world_point;
                    }
                }
                if drawing.object_drawing {
                    let world_cmds = draft.commands(false);
                    joined = editor.build_joined_open_path(
                        &world_cmds,
                        start_match.as_ref(),
                        end_match.as_ref(),
                    );
                }
            }

            if let Some((target_id, merged_commands, closed_loop)) = joined {
                let join_fill = editor.fill;
                editor.transact_named("Path", |s| {
                    s.ensure_key(false);
                    let layer = s.layer;
                    let frame = s.frame;
                    if let Some(el) = s.symbol_mut().timeline.layers[layer]
                        .key_at_mut(frame)
                        .and_then(|k| k.elements.iter_mut().find(|e| e.id == target_id))
                        && let ElementKind::Path { commands, fill, .. } = &mut el.kind
                    {
                        *commands = merged_commands.clone();
                        if closed_loop {
                            *fill = join_fill;
                        }
                    }
                    s.select_single_id(target_id.clone());
                });
                closed_loop
            } else {
                let commands = draft.commands(effective_closed);
                editor.add_element(
                    ElementKind::Path {
                        commands,
                        fill_rule: FillRule::NonZero,
                        fill: if effective_closed {
                            editor.fill
                        } else {
                            [0, 0, 0, 0]
                        },
                        stroke: editor.stroke,
                        stroke_width: editor.stroke_width,
                    },
                    0.,
                    0.,
                );
                effective_closed
            }
        };
        let final_closed = if live_target {
            finish(self)
        } else {
            self.with_temporary_symbol_target(draft_symbol, draft_frame, target_layer, finish)
        };
        self.message(if final_closed {
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
        mapping: StageMapping,
        _page: Rect,
        response: &egui::Response,
    ) {
        let zoom = self.zoom;
        let local = |pos: Pos2| mapping.screen_to_edit(origin, zoom, pos);
        let screen = |pos: Pos2| mapping.edit_to_screen(origin, zoom, pos);
        if self.vector.point_drag.as_ref().is_some_and(|d| {
            !self.point_drag_matches(d)
                || d.origin != origin
                || d.zoom != self.zoom
                || d.mapping != mapping
        }) {
            self.cancel_vector();
        }
        if self.vector.draft.as_ref().is_some_and(|draft| {
            (draft.dragging || draft.closing)
                && (draft.pointer_origin != origin
                    || draft.pointer_zoom != zoom
                    || draft.pointer_mapping != mapping)
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
        let canvas_hover = ui.input(|i| i.pointer.hover_pos()).filter(|&pos| {
            response.rect.contains(pos)
                && ui
                    .ctx()
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer == response.layer_id)
        });
        if self.tool == Tool::Pen
            && self.vector.draft.is_none()
            && self.layer_editable()
            && let Some(hover_pos) = canvas_hover
        {
            let alt = ui.input(|i| i.modifiers.alt);
            if self
                .pen_anchor_candidate(origin, mapping, hover_pos, alt)
                .is_some()
            {
                self.stage_cursor = Some(if alt {
                    neoflash_ui::cursors::PEN_CONVERT
                } else {
                    neoflash_ui::cursors::PEN_REMOVE
                });
            } else if self
                .pen_subdivide_candidate(origin, mapping, hover_pos)
                .is_some()
            {
                self.stage_cursor = Some(neoflash_ui::cursors::PEN_ADD);
            }
        }
        if matches!(self.tool, Tool::Pen | Tool::Pencil) && self.layer_editable() {
            for event in &events {
                match *event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers,
                    } if response.rect.contains(pos)
                        && ui
                            .ctx()
                            .layer_id_at(pos)
                            .is_none_or(|layer| layer == response.layer_id) =>
                    {
                        self.playing = false;
                        if self.tool == Tool::Pen
                            && self.vector.draft.is_none()
                            && (self.try_pen_anchor_action(origin, mapping, pos, modifiers)
                                || self.try_pen_subdivide(origin, mapping, pos))
                        {
                            continue;
                        }
                        // Pen anchors snap to guides and grid (ST-04); pencil strokes are freehand.
                        let position = if self.tool == Tool::Pen {
                            local(self.snap_screen(mapping, origin, pos))
                        } else {
                            local(pos)
                        };
                        let closes = self.tool == Tool::Pen
                            && self.vector.draft.as_ref().is_some_and(|draft| {
                                draft.anchors.len() >= 2
                                    && draft.anchors.first().is_some_and(|first| {
                                        screen(first.point).distance(pos) < PEN_CLOSE_RADIUS
                                    })
                            });
                        if closes {
                            // Commit on press: a lost native release or viewport transition must
                            // not turn an explicit close click into another open anchor.
                            self.finish_path(true);
                            continue;
                        }
                        if self.tool == Tool::Pen
                            && (modifiers.command || modifiers.ctrl)
                            && self
                                .vector
                                .draft
                                .as_ref()
                                .is_some_and(|draft| draft.anchors.len() >= 2)
                        {
                            self.finish_path(false);
                            continue;
                        }
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
                            pointer_mapping: mapping,
                        });
                        draft.pointer_origin = origin;
                        draft.pointer_zoom = zoom;
                        draft.pointer_mapping = mapping;
                        draft.anchors.push(Anchor {
                            point: position,
                            tangent: Vec2::ZERO,
                        });
                        draft.dragging = true;
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
                                        .is_some_and(|a| screen(a.point).distance(pos) >= 1.5)
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
        if self.tool == Tool::Subselect
            || (self.tool == Tool::Pen && self.vector.point_drag.is_some())
        {
            self.edit_path_points(ui, p, origin, mapping, response);
        }
        if let Some(draft) = &self.vector.draft {
            let blue = UiPalette::current(p.ctx()).overlay;
            paint_path(
                p,
                &draft.commands(false),
                PathPaint {
                    rule: FillRule::NonZero,
                    fill: Color32::TRANSPARENT,
                    stroke: blue,
                    width: 1.5 / (zoom as f64 * mapping.scale_bounds().1.max(1e-6)),
                    tolerance: 0.2,
                    fingerprint: None,
                },
                screen,
            );
            // `NF2/Anchor Point` + `NF2/Bezier Handle`; the anchor being placed is the selected one,
            // the first anchor shows Hover while a click there would close the path.
            let hover = response.hover_pos();
            let closes = |at: Pos2| {
                !draft.pencil
                    && draft.anchors.len() >= 2
                    && hover.is_some_and(|pos| at.distance(pos) < PEN_CLOSE_RADIUS)
            };
            if !draft.pencil {
                let last = draft.anchors.len().saturating_sub(1);
                for (i, anchor) in draft.anchors.iter().enumerate() {
                    let at = screen(anchor.point);
                    if anchor.tangent.length_sq() > 0.1 {
                        for handle in [anchor.point + anchor.tangent, anchor.point - anchor.tangent]
                        {
                            cv::paint_bezier_handle(p, p.ctx(), at, screen(handle));
                        }
                    }
                    let state = if i == last {
                        cv::AnchorState::Selected
                    } else if i == 0 && closes(at) {
                        cv::AnchorState::Hover
                    } else {
                        cv::AnchorState::Unselected
                    };
                    cv::paint_anchor(p, p.ctx(), at, state);
                }
                if let Some(first) = draft.anchors.first()
                    && closes(screen(first.point))
                {
                    self.stage_cursor = Some(neoflash_ui::cursors::PEN_CLOSE);
                }
            }
            if !draft.pencil
                && self.workspace.drawing.pen_preview
                && let (Some(last), Some(hover)) = (draft.anchors.last(), response.hover_pos())
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
        mapping: StageMapping,
        response: &egui::Response,
    ) {
        let Some(element) = self.selected_sampled() else {
            return;
        };
        let Some(commands) = editable_element_commands(&element) else {
            return;
        };
        let matrix = transform_matrix(element.transform);
        let zoom = self.zoom;
        let screen =
            |pos: Pos2| mapping.edit_to_screen(origin, zoom, crate::stage::affine(matrix, pos));
        let handles = handles(&commands);
        // `NF2/Bezier Handle` from each anchor to its control points, then `NF2/Anchor Point`s.
        let mut previous = Pos2::ZERO;
        for command in &commands {
            match *command {
                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                    previous = Pos2::new(x as f32, y as f32)
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let control = screen(Pos2::new(cx as f32, cy as f32));
                    let end = Pos2::new(x as f32, y as f32);
                    cv::paint_bezier_handle(p, p.ctx(), screen(previous), control);
                    cv::paint_bezier_handle(p, p.ctx(), screen(end), control);
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
                    let first = screen(Pos2::new(cx1 as f32, cy1 as f32));
                    let second = screen(Pos2::new(cx2 as f32, cy2 as f32));
                    cv::paint_bezier_handle(p, p.ctx(), screen(previous), first);
                    cv::paint_bezier_handle(p, p.ctx(), screen(end), second);
                    previous = end;
                }
                PathCommand::Close => {}
            }
        }
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|&pos| {
            self.vector.point_drag.is_none()
                && response.rect.contains(pos)
                && ui
                    .ctx()
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer == response.layer_id)
        });
        let dragged = self
            .vector
            .point_drag
            .as_ref()
            .map(|drag| (drag.handle.command, drag.handle.role));
        let near = |at: Pos2| hover.is_some_and(|pos| at.distance(pos) < 8.);
        for handle in &handles {
            if handle.role == PointRole::Anchor {
                let at = screen(handle.point);
                let is_selected = dragged == Some((handle.command, handle.role))
                    || self.is_anchor_selected(&element.id, handle.command);
                let state = if is_selected {
                    cv::AnchorState::Selected
                } else if near(at) {
                    cv::AnchorState::Hover
                } else {
                    cv::AnchorState::Unselected
                };
                cv::paint_anchor(p, p.ctx(), at, state);
            }
        }
        let near_any_handle = handles.iter().any(|h| near(screen(h.point)));
        let alt_hover = ui.input(|i| i.modifiers.alt);
        if dragged.is_some() || near_any_handle {
            self.stage_cursor = Some(neoflash_ui::cursors::SUBSELECT_POINT);
        } else if alt_hover
            && let Some(hover_pos) = hover
            && path_segment_hit_local(&commands, matrix, mapping, origin, zoom, hover_pos).is_some()
        {
            self.stage_cursor = Some(neoflash_ui::cursors::PEN_ADD);
        }
        let double_clicked = response.double_clicked() && !self.stage_view.guide_pointer;
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
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
                        if handle.role == PointRole::Anchor {
                            if modifiers.shift {
                                let selected =
                                    self.toggle_selected_anchor(&element.id, handle.command);
                                if !selected {
                                    continue;
                                }
                            } else if !self.is_anchor_selected(&element.id, handle.command) {
                                self.vector.selected_anchors =
                                    Some((element.id.clone(), vec![handle.command]));
                            }
                        }
                        self.vector.point_drag = Some(PointDrag {
                            handle,
                            original: commands.clone(),
                            inverse: matrix,
                            symbol: self.symbol_id().to_owned(),
                            frame: self.frame,
                            layer: self.selection.single().unwrap().layer.clone(),
                            origin,
                            zoom,
                            mapping,
                            press: pos,
                            began: false,
                            changed: false,
                            element: element.id.clone(),
                            pull_tangent: handle.role == PointRole::Anchor && modifiers.alt,
                        });
                    } else if (modifiers.alt || double_clicked)
                        && let Some((cmd_idx, t)) =
                            path_segment_hit_local(&commands, matrix, mapping, origin, zoom, pos)
                        && let Some((new_commands, inserted)) =
                            subdivide_path_at(&commands, cmd_idx, t)
                        && validate_path_commands(&new_commands).is_ok()
                    {
                        let element_id = element.id.clone();
                        self.finish_edits();
                        self.playing = false;
                        self.transact(|s| {
                            s.ensure_selected_key();
                            if let Some(el) = s.selected_element_mut() {
                                write_editable_element_commands(el, new_commands);
                            }
                        });
                        self.vector.selected_anchors = Some((element_id, vec![inserted]));
                        return;
                    } else if self.tool == Tool::Subselect {
                        self.finish_edits();
                        self.playing = false;
                        self.vector.subselect_marquee = Some((pos, pos));
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    if let Some((_, current)) = &mut self.vector.subselect_marquee {
                        *current = pos;
                    } else {
                        self.update_path_drag(pos, origin, mapping);
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers,
                } => {
                    if let Some((start, _)) = self.vector.subselect_marquee.take() {
                        if start.distance(pos) >= 3.0 {
                            let marquee = Rect::from_two_pos(start, pos);
                            let mut hit_indices: Vec<usize> = handles
                                .iter()
                                .filter(|h| {
                                    h.role == PointRole::Anchor && marquee.contains(screen(h.point))
                                })
                                .map(|h| h.command)
                                .collect();
                            hit_indices.sort_unstable();
                            hit_indices.dedup();
                            if modifiers.shift
                                && let Some((sel_id, existing)) = &mut self.vector.selected_anchors
                                && *sel_id == element.id
                            {
                                for idx in hit_indices {
                                    if !existing.contains(&idx) {
                                        existing.push(idx);
                                    }
                                }
                            } else {
                                self.vector.selected_anchors =
                                    Some((element.id.clone(), hit_indices));
                            }
                        }
                    } else {
                        self.update_path_drag(pos, origin, mapping);
                        self.finish_point_drag();
                    }
                }
                _ => {}
            }
        }
        if let Some((start, current)) = self.vector.subselect_marquee
            && start.distance(current) >= 3.0
        {
            cv::paint_marquee(p, p.ctx(), Rect::from_two_pos(start, current));
        }
    }
    fn update_path_drag(&mut self, pos: Pos2, origin: Pos2, mapping: StageMapping) {
        let Some(mut drag) = self.vector.point_drag.take() else {
            return;
        };
        if !self.point_drag_matches(&drag)
            || drag.origin != origin
            || drag.zoom != self.zoom
            || drag.mapping != mapping
        {
            if drag.began {
                self.history.cancel(&mut self.project);
            }
            return;
        }
        if !drag.began && pos.distance(drag.press) < 3. {
            self.vector.point_drag = Some(drag);
            return;
        }
        // The dragged anchor or control point snaps to guides and grid (ST-04).
        let anchor = crate::stage::affine(drag.inverse, drag.handle.point);
        let pos = self.snap_drag(drag.mapping, drag.origin, drag.press, pos, anchor);
        let local = |point| {
            crate::stage::inverse(
                drag.inverse,
                drag.mapping.screen_to_edit(drag.origin, drag.zoom, point),
            )
        };
        let (Some(current), Some(press)) = (local(pos), local(drag.press)) else {
            self.vector.point_drag = Some(drag);
            return;
        };
        let mut commands = drag.original.clone();
        if drag.pull_tangent && drag.handle.role == PointRole::Anchor {
            pull_anchor_tangent(
                &mut commands,
                drag.handle.command,
                current - drag.handle.point,
            );
        } else if drag.handle.role == PointRole::Anchor
            && let Some((sel_id, indices)) = &self.vector.selected_anchors
            && *sel_id == drag.element
            && indices.len() > 1
            && indices.contains(&drag.handle.command)
        {
            move_selected_anchors(&mut commands, indices, current - press);
        } else {
            move_handle(
                &mut commands,
                drag.handle,
                drag.handle.point + (current - press),
            );
        }
        drag.changed = commands != drag.original;
        if drag.changed && !drag.began {
            self.history.begin(&self.project);
            self.ensure_selected_key();
            drag.began = true;
        }
        if drag.began
            && let Some(el) = self.selected_element_mut()
        {
            write_editable_element_commands(el, commands);
        }
        self.vector.point_drag = Some(drag);
    }
    fn point_drag_matches(&self, drag: &PointDrag) -> bool {
        matches!(self.tool, Tool::Subselect | Tool::Pen)
            && self.symbol_id() == drag.symbol
            && self.frame == drag.frame
            && self
                .selection
                .single()
                .is_some_and(|target| target.layer == drag.layer && target.element == drag.element)
            && self.selected_editable()
    }

    pub(crate) fn is_anchor_selected(&self, element_id: &str, command: usize) -> bool {
        self.vector
            .selected_anchors
            .as_ref()
            .is_some_and(|(id, indices)| id == element_id && indices.contains(&command))
    }

    pub(crate) fn toggle_selected_anchor(&mut self, element_id: &str, command: usize) -> bool {
        match &mut self.vector.selected_anchors {
            Some((id, indices)) if id == element_id => {
                if let Some(pos) = indices.iter().position(|&i| i == command) {
                    indices.remove(pos);
                    false
                } else {
                    indices.push(command);
                    true
                }
            }
            _ => {
                self.vector.selected_anchors = Some((element_id.to_owned(), vec![command]));
                true
            }
        }
    }

    pub(crate) fn on_selected_path_segment(
        &self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
    ) -> bool {
        if !self.selected_editable() {
            return false;
        }
        let Some(element) = self.selected_sampled() else {
            return false;
        };
        let Some(commands) = editable_element_commands(&element) else {
            return false;
        };
        let matrix = transform_matrix(element.transform);
        let zoom = self.zoom;
        let screen =
            |pt: Pos2| mapping.edit_to_screen(origin, zoom, crate::stage::affine(matrix, pt));
        if handles(&commands)
            .iter()
            .any(|h| screen(h.point).distance(pos) < 8.)
        {
            return false;
        }
        path_segment_hit_local(&commands, matrix, mapping, origin, zoom, pos).is_some()
    }

    pub(crate) fn pen_anchor_candidate(
        &self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
        alt: bool,
    ) -> Option<(
        crate::selection::SelectionTarget,
        Element,
        Vec<PathCommand>,
        PathHandle,
    )> {
        let zoom = self.zoom;
        let try_element = |target: crate::selection::SelectionTarget, element: Element| {
            let commands = editable_element_commands(&element)?;
            let matrix = transform_matrix(element.transform);
            let screen =
                |pt: Pos2| mapping.edit_to_screen(origin, zoom, crate::stage::affine(matrix, pt));
            let handle = handles(&commands)
                .into_iter()
                .rev()
                .filter(|h| h.role == PointRole::Anchor && screen(h.point).distance(pos) < 8.)
                .min_by(|a, b| {
                    screen(a.point)
                        .distance_sq(pos)
                        .total_cmp(&screen(b.point).distance_sq(pos))
                })?;
            if !alt {
                let remaining = delete_anchors_from_path(&commands, &[handle.command]);
                if remaining.is_empty()
                    || remaining == commands
                    || validate_path_commands(&remaining).is_err()
                {
                    return None;
                }
            }
            Some((target, element, commands, handle))
        };

        if self.selected_editable()
            && let Some(target) = self.selection.single().cloned()
            && let Some(element) = self.selected_sampled()
            && let Some(found) = try_element(target, element)
        {
            return Some(found);
        }

        let timeline = &self.symbol().timeline;
        for layer in timeline.layers.iter().rev() {
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
            {
                continue;
            }
            for element in sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps))
                .into_iter()
                .rev()
            {
                let target = crate::selection::SelectionTarget {
                    layer: layer.id.clone(),
                    element: element.id.clone(),
                };
                if let Some(found) = try_element(target, element) {
                    return Some(found);
                }
            }
        }
        None
    }

    pub(crate) fn try_pen_anchor_action(
        &mut self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
        modifiers: egui::Modifiers,
    ) -> bool {
        let Some((target, element, commands, handle)) =
            self.pen_anchor_candidate(origin, mapping, pos, modifiers.alt)
        else {
            return false;
        };
        let element_id = target.element.clone();
        self.finish_edits();
        self.playing = false;
        if modifiers.alt {
            let collapsed = collapse_anchor_handles(&commands, handle.command);
            let base_commands =
                if collapsed != commands && validate_path_commands(&collapsed).is_ok() {
                    let updated = collapsed.clone();
                    let target_clone = target.clone();
                    self.transact(|s| {
                        s.ensure_key_for(&target_clone.layer, false);
                        if let Some(layer_idx) = s.target_layer_index(&target_clone) {
                            let frame = s.frame;
                            if let Some(key) =
                                s.symbol_mut().timeline.layers[layer_idx].key_at_mut(frame)
                                && let Some(el) = key
                                    .elements
                                    .iter_mut()
                                    .find(|e| e.id == target_clone.element)
                            {
                                write_editable_element_commands(el, updated);
                            }
                        }
                        s.set_selection(vec![target_clone]);
                    });
                    collapsed
                } else {
                    self.set_selection(vec![target.clone()]);
                    commands
                };
            self.vector.selected_anchors = Some((element_id.clone(), vec![handle.command]));
            self.vector.point_drag = Some(PointDrag {
                handle,
                original: base_commands,
                inverse: transform_matrix(element.transform),
                symbol: self.symbol_id().to_owned(),
                frame: self.frame,
                layer: target.layer,
                origin,
                zoom: self.zoom,
                mapping,
                press: pos,
                began: false,
                changed: false,
                element: element_id,
                pull_tangent: true,
            });
            return true;
        }
        let remaining = delete_anchors_from_path(&commands, &[handle.command]);
        if remaining.is_empty()
            || remaining == commands
            || validate_path_commands(&remaining).is_err()
        {
            return false;
        }
        self.transact(|s| {
            s.ensure_key_for(&target.layer, false);
            if let Some(layer_idx) = s.target_layer_index(&target) {
                let frame = s.frame;
                if let Some(key) = s.symbol_mut().timeline.layers[layer_idx].key_at_mut(frame)
                    && let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element)
                {
                    write_editable_element_commands(el, remaining);
                }
            }
            s.set_selection(vec![target]);
        });
        self.vector.selected_anchors = None;
        true
    }

    pub(crate) fn nudge_selected_vector_points(&mut self, dx: f64, dy: f64) -> bool {
        if self.tool != Tool::Subselect || !self.selected_editable() {
            return false;
        }
        let Some(element) = self.selected_sampled() else {
            return false;
        };
        let Some(mut commands) = editable_element_commands(&element) else {
            return false;
        };
        let Some((selected_id, indices)) = self.vector.selected_anchors.clone() else {
            return false;
        };
        if selected_id != element.id || indices.is_empty() {
            return false;
        }
        let m = transform_matrix(element.transform);
        let (Some(p0), Some(p1)) = (
            crate::stage::inverse(m, Pos2::ZERO),
            crate::stage::inverse(m, Pos2::new(dx as f32, dy as f32)),
        ) else {
            return false;
        };
        let local_delta = p1 - p0;
        let before = commands.clone();
        move_selected_anchors(&mut commands, &indices, local_delta);
        if commands == before || validate_path_commands(&commands).is_err() {
            return false;
        }
        self.playing = false;
        self.transact(|s| {
            s.ensure_selected_key();
            if let Some(el) = s.selected_element_mut() {
                write_editable_element_commands(el, commands);
            }
        });
        true
    }

    pub(crate) fn pen_subdivide_candidate(
        &self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
    ) -> Option<(crate::selection::SelectionTarget, Vec<PathCommand>, usize)> {
        let zoom = self.zoom;
        let try_element =
            |target: crate::selection::SelectionTarget,
             element: Element|
             -> Option<(crate::selection::SelectionTarget, Vec<PathCommand>, usize)> {
                let commands = editable_element_commands(&element)?;
                let matrix = transform_matrix(element.transform);
                let screen = |pt: Pos2| {
                    mapping.edit_to_screen(origin, zoom, crate::stage::affine(matrix, pt))
                };
                if handles(&commands)
                    .iter()
                    .any(|h| h.role == PointRole::Anchor && screen(h.point).distance(pos) < 8.)
                {
                    return None;
                }
                let (cmd_idx, t) =
                    path_segment_hit_local(&commands, matrix, mapping, origin, zoom, pos)?;
                let (new_commands, inserted) = subdivide_path_at(&commands, cmd_idx, t)?;
                validate_path_commands(&new_commands).ok()?;
                Some((target, new_commands, inserted))
            };

        if self.selected_editable()
            && let Some(target) = self.selection.single().cloned()
            && let Some(element) = self.selected_sampled()
            && let Some(found) = try_element(target, element)
        {
            return Some(found);
        }

        let timeline = &self.symbol().timeline;
        for layer in timeline.layers.iter().rev() {
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
            {
                continue;
            }
            for element in sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps))
                .into_iter()
                .rev()
            {
                let target = crate::selection::SelectionTarget {
                    layer: layer.id.clone(),
                    element: element.id.clone(),
                };
                if let Some(found) = try_element(target, element) {
                    return Some(found);
                }
            }
        }
        None
    }

    pub(crate) fn try_pen_subdivide(
        &mut self,
        origin: Pos2,
        mapping: StageMapping,
        pos: Pos2,
    ) -> bool {
        let Some((target, new_commands, inserted)) =
            self.pen_subdivide_candidate(origin, mapping, pos)
        else {
            return false;
        };
        let element_id = target.element.clone();
        self.finish_edits();
        self.playing = false;
        self.transact(|s| {
            s.ensure_key_for(&target.layer, false);
            if let Some(layer_idx) = s.target_layer_index(&target) {
                let frame = s.frame;
                if let Some(key) = s.symbol_mut().timeline.layers[layer_idx].key_at_mut(frame)
                    && let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element)
                {
                    write_editable_element_commands(el, new_commands);
                }
            }
            s.set_selection(vec![target]);
        });
        self.vector.selected_anchors = Some((element_id, vec![inserted]));
        true
    }

    pub(crate) fn delete_selected_vector_points(&mut self) -> bool {
        if !self.selected_editable() {
            return false;
        }
        let Some(target) = self.selection.single().cloned() else {
            return false;
        };
        let Some(element) = self.selected_sampled() else {
            return false;
        };
        let Some(commands) = editable_element_commands(&element) else {
            return false;
        };
        let Some((selected_id, indices)) = self.vector.selected_anchors.clone() else {
            return false;
        };
        if selected_id != element.id || indices.is_empty() {
            return false;
        }
        let remaining = delete_anchors_from_path(&commands, &indices);
        if remaining == commands {
            return false;
        }
        self.playing = false;
        if remaining.is_empty() {
            self.transact(|s| {
                s.ensure_key_for(&target.layer, false);
                if let Some(index) = s.target_layer_index(&target) {
                    let frame = s.frame;
                    if let Some(key) = s.symbol_mut().timeline.layers[index].key_at_mut(frame) {
                        key.elements.retain(|el| el.id != target.element);
                    }
                }
                s.prune_authoring_groups();
                s.clear_selection();
            });
            self.vector.selected_anchors = None;
            return true;
        }
        if validate_path_commands(&remaining).is_err() {
            return false;
        }
        self.transact(|s| {
            s.ensure_selected_key();
            if let Some(el) = s.selected_element_mut() {
                write_editable_element_commands(el, remaining);
            }
        });
        self.vector.selected_anchors = None;
        true
    }

    /// True when at least one selected element on an editable layer is a shape or path
    /// that can be smoothed or straightened with the Selection tool (`V`) modifiers.
    pub(crate) fn can_smooth_or_straighten_selection(&self) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        let timeline = &self.symbol().timeline;
        self.selection.targets.iter().any(|target| {
            let Some(layer_idx) = self.target_layer_index(target) else {
                return false;
            };
            let layer = &timeline.layers[layer_idx];
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
            {
                return false;
            }
            sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps))
                .into_iter()
                .find(|el| el.id == target.element)
                .is_some_and(|el| {
                    el.transform.primitive.is_none() && editable_element_commands(&el).is_some()
                })
        })
    }

    /// Smooths selected path/shape contours in one Undo step (`Selection` tool `Smooth` modifier).
    pub(crate) fn smooth_selection(&mut self) -> bool {
        self.apply_selection_contour_modifier(smooth_path_commands)
    }

    /// Straightens selected path/shape contours in one Undo step (`Selection` tool `Straighten` modifier).
    pub(crate) fn straighten_selection(&mut self) -> bool {
        self.apply_selection_contour_modifier(straighten_path_commands)
    }

    fn apply_selection_contour_modifier(
        &mut self,
        op: fn(&[PathCommand], f32) -> Vec<PathCommand>,
    ) -> bool {
        if !self.can_smooth_or_straighten_selection() {
            return false;
        }
        let zoom = self.zoom.max(0.1);
        let timeline = &self.symbol().timeline;
        let mut updates: Vec<(crate::selection::SelectionTarget, Vec<PathCommand>)> = Vec::new();
        for target in &self.selection.targets {
            let Some(layer_idx) = self.target_layer_index(target) else {
                continue;
            };
            let layer = &timeline.layers[layer_idx];
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
            {
                continue;
            }
            let Some(sampled) = sample_layer_at_fps(layer, self.frame, f64::from(self.project.fps))
                .into_iter()
                .find(|el| el.id == target.element)
            else {
                continue;
            };
            if sampled.transform.primitive.is_some() {
                continue;
            }
            let Some(commands) = editable_element_commands(&sampled) else {
                continue;
            };
            let next = op(&commands, zoom);
            if next != commands && validate_path_commands(&next).is_ok() {
                updates.push((target.clone(), next));
            }
        }
        if updates.is_empty() {
            return false;
        }
        self.finish_edits();
        self.playing = false;
        self.transact(|s| {
            let mut ensured_layers: Vec<Id> = Vec::new();
            for (target, next_commands) in updates {
                if !ensured_layers.contains(&target.layer) {
                    s.ensure_key_for(&target.layer, false);
                    ensured_layers.push(target.layer.clone());
                }
                if let Some(layer_idx) = s.target_layer_index(&target) {
                    let frame = s.frame;
                    if let Some(key) = s.symbol_mut().timeline.layers[layer_idx].key_at_mut(frame)
                        && let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element)
                    {
                        write_editable_element_commands(el, next_commands);
                    }
                }
            }
        });
        true
    }
}

fn contour_anchor_points(
    commands: &[PathCommand],
    start: usize,
    body_end: usize,
    closed: bool,
) -> Vec<Pos2> {
    let mut pts = Vec::new();
    let Some(PathCommand::MoveTo { x: x0, y: y0 }) = commands.get(start).copied() else {
        return pts;
    };
    pts.push(Pos2::new(x0 as f32, y0 as f32));
    for cmd in &commands[(start + 1)..body_end] {
        match *cmd {
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => {
                pts.push(Pos2::new(x as f32, y as f32));
            }
            PathCommand::MoveTo { .. } | PathCommand::Close => {}
        }
    }
    if closed && pts.len() >= 2 && pts.first().unwrap().distance(*pts.last().unwrap()) <= 1e-3 {
        pts.pop();
    }
    pts
}

fn contour_dense_samples(commands: &[PathCommand], start: usize, body_end: usize) -> Vec<Pos2> {
    let mut pts = Vec::new();
    let Some(PathCommand::MoveTo { x: x0, y: y0 }) = commands.get(start).copied() else {
        return pts;
    };
    let mut cur = [x0, y0];
    pts.push(Pos2::new(x0 as f32, y0 as f32));
    for cmd in &commands[(start + 1)..body_end] {
        match *cmd {
            PathCommand::LineTo { x, y } => {
                pts.push(Pos2::new(x as f32, y as f32));
                cur = [x, y];
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                for step in 1..=4 {
                    let t = f64::from(step) / 4.0;
                    let mt = 1.0 - t;
                    let px = mt * mt * cur[0] + 2.0 * mt * t * cx + t * t * x;
                    let py = mt * mt * cur[1] + 2.0 * mt * t * cy + t * t * y;
                    pts.push(Pos2::new(px as f32, py as f32));
                }
                cur = [x, y];
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                for step in 1..=4 {
                    let t = f64::from(step) / 4.0;
                    let mt = 1.0 - t;
                    let px = mt * mt * mt * cur[0]
                        + 3.0 * mt * mt * t * cx1
                        + 3.0 * mt * t * t * cx2
                        + t * t * t * x;
                    let py = mt * mt * mt * cur[1]
                        + 3.0 * mt * mt * t * cy1
                        + 3.0 * mt * t * t * cy2
                        + t * t * t * y;
                    pts.push(Pos2::new(px as f32, py as f32));
                }
                cur = [x, y];
            }
            PathCommand::MoveTo { .. } | PathCommand::Close => {}
        }
    }
    pts
}

fn rdp_simplify_points(points: &[Pos2], tolerance: f32) -> Vec<Pos2> {
    let anchors: Vec<Anchor> = points
        .iter()
        .map(|&point| Anchor {
            point,
            tangent: Vec2::ZERO,
        })
        .collect();
    simplify(&anchors, tolerance)
        .into_iter()
        .map(|a| a.point)
        .collect()
}

fn open_catmull_rom_to_cubic(pts: &[Pos2], tension: f32) -> Vec<PathCommand> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let k = tension / 6.0;
    let mut out = Vec::with_capacity(n);
    out.push(PathCommand::MoveTo {
        x: f64::from(pts[0].x),
        y: f64::from(pts[0].y),
    });
    for i in 0..(n - 1) {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = if i + 2 < n { pts[i + 2] } else { pts[n - 1] };
        let c1 = p1 + (p2 - p0) * k;
        let c2 = p2 - (p3 - p1) * k;
        out.push(PathCommand::CubicTo {
            cx1: f64::from(c1.x),
            cy1: f64::from(c1.y),
            cx2: f64::from(c2.x),
            cy2: f64::from(c2.y),
            x: f64::from(p2.x),
            y: f64::from(p2.y),
        });
    }
    out
}

fn closed_catmull_rom_to_cubic(pts: &[Pos2], tension: f32) -> Vec<PathCommand> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let k = tension / 6.0;
    let mut out = Vec::with_capacity(n + 2);
    out.push(PathCommand::MoveTo {
        x: f64::from(pts[0].x),
        y: f64::from(pts[0].y),
    });
    for i in 0..n {
        let p0 = pts[(i + n - 1) % n];
        let p1 = pts[i];
        let p2 = pts[(i + 1) % n];
        let p3 = pts[(i + 2) % n];
        let c1 = p1 + (p2 - p0) * k;
        let c2 = p2 - (p3 - p1) * k;
        out.push(PathCommand::CubicTo {
            cx1: f64::from(c1.x),
            cy1: f64::from(c1.y),
            cx2: f64::from(c2.x),
            cy2: f64::from(c2.y),
            x: f64::from(p2.x),
            y: f64::from(p2.y),
        });
    }
    out.push(PathCommand::Close);
    out
}

pub(crate) fn smooth_path_commands(commands: &[PathCommand], zoom: f32) -> Vec<PathCommand> {
    let ranges = contour_ranges(commands);
    if ranges.is_empty() {
        return commands.to_vec();
    }
    let rdp_eps = (1.2 / zoom.max(0.25)).clamp(0.35, 2.5);
    let mut out = Vec::new();
    for (start, body_end, closed) in ranges {
        let mut pts = contour_anchor_points(commands, start, body_end, closed);
        if closed {
            if pts.len() >= 3 {
                if pts.len() > 8 {
                    let mut loop_pts = pts.clone();
                    let mid = loop_pts.len() / 2;
                    let first_half = rdp_simplify_points(&loop_pts[..=mid], rdp_eps);
                    loop_pts.push(pts[0]);
                    let second_half = rdp_simplify_points(&loop_pts[mid..], rdp_eps);
                    let mut combined = first_half;
                    if second_half.len() > 2 {
                        combined.extend_from_slice(&second_half[1..(second_half.len() - 1)]);
                    }
                    if combined.len() >= 3 {
                        pts = combined;
                    }
                }
                let n = pts.len();
                let smoothed: Vec<Pos2> = (0..n)
                    .map(|i| {
                        let prev = pts[(i + n - 1) % n];
                        let cur = pts[i];
                        let next = pts[(i + 1) % n];
                        Pos2::new(
                            0.2 * prev.x + 0.6 * cur.x + 0.2 * next.x,
                            0.2 * prev.y + 0.6 * cur.y + 0.2 * next.y,
                        )
                    })
                    .collect();
                out.extend(closed_catmull_rom_to_cubic(&smoothed, 1.0));
            } else {
                out.extend_from_slice(&commands[start..body_end]);
                if closed {
                    out.push(PathCommand::Close);
                }
            }
        } else if pts.len() >= 3 {
            if pts.len() > 8 {
                let simplified = rdp_simplify_points(&pts, rdp_eps);
                if simplified.len() >= 3 {
                    pts = simplified;
                }
            }
            let n = pts.len();
            let mut smoothed = pts.clone();
            for i in 1..(n - 1) {
                let prev = pts[i - 1];
                let cur = pts[i];
                let next = pts[i + 1];
                smoothed[i] = Pos2::new(
                    0.22 * prev.x + 0.56 * cur.x + 0.22 * next.x,
                    0.22 * prev.y + 0.56 * cur.y + 0.22 * next.y,
                );
            }
            out.extend(open_catmull_rom_to_cubic(&smoothed, 1.0));
        } else if body_end == start + 2 {
            // Single 2-point curve: relax control points gently toward the chord.
            let p0 = [f64::from(pts[0].x), f64::from(pts[0].y)];
            out.push(commands[start]);
            match commands[start + 1] {
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let mid_x = 0.5 * (p0[0] + x);
                    let mid_y = 0.5 * (p0[1] + y);
                    out.push(PathCommand::QuadraticTo {
                        cx: 0.75 * cx + 0.25 * mid_x,
                        cy: 0.75 * cy + 0.25 * mid_y,
                        x,
                        y,
                    });
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let l1x = p0[0] + (x - p0[0]) / 3.0;
                    let l1y = p0[1] + (y - p0[1]) / 3.0;
                    let l2x = p0[0] + 2.0 * (x - p0[0]) / 3.0;
                    let l2y = p0[1] + 2.0 * (y - p0[1]) / 3.0;
                    out.push(PathCommand::CubicTo {
                        cx1: 0.75 * cx1 + 0.25 * l1x,
                        cy1: 0.75 * cy1 + 0.25 * l1y,
                        cx2: 0.75 * cx2 + 0.25 * l2x,
                        cy2: 0.75 * cy2 + 0.25 * l2y,
                        x,
                        y,
                    });
                }
                other => out.push(other),
            }
        } else {
            out.extend_from_slice(&commands[start..body_end]);
        }
    }
    out
}

fn snap_near_axis_endpoint(from: [f64; 2], to: [f64; 2]) -> [f64; 2] {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let len = dx.hypot(dy);
    if len <= 1e-5 {
        return to;
    }
    // Snap segments within ~4 degrees of horizontal or vertical.
    let tan_limit = 0.07;
    if (dy.abs() / len) <= tan_limit {
        [to[0], from[1]]
    } else if (dx.abs() / len) <= tan_limit {
        [from[0], to[1]]
    } else {
        to
    }
}

pub(crate) fn straighten_path_commands(commands: &[PathCommand], zoom: f32) -> Vec<PathCommand> {
    use crate::workspace::Tolerance;
    let ranges = contour_ranges(commands);
    if ranges.is_empty() {
        return commands.to_vec();
    }
    let rdp_eps = (2.4 / zoom.max(0.25)).clamp(0.8, 6.0);
    let curve_flat_tol = f64::from((5.0 / zoom.max(0.25)).clamp(1.5, 12.0));
    let mut out = Vec::new();
    for (start, body_end, closed) in ranges {
        let samples = contour_dense_samples(commands, start, body_end);
        let sample_anchors: Vec<Anchor> = samples
            .iter()
            .map(|&point| Anchor {
                point,
                tangent: Vec2::ZERO,
            })
            .collect();
        if closed {
            if let Some(recognised) =
                recognise_primitive_shape(&sample_anchors, true, zoom, Tolerance::Normal)
            {
                match recognised {
                    RecognisedShape::Rectangle {
                        x,
                        y,
                        width,
                        height,
                    } => {
                        out.push(PathCommand::MoveTo { x, y });
                        out.push(PathCommand::LineTo { x: x + width, y });
                        out.push(PathCommand::LineTo {
                            x: x + width,
                            y: y + height,
                        });
                        out.push(PathCommand::LineTo { x, y: y + height });
                        out.push(PathCommand::Close);
                        continue;
                    }
                    RecognisedShape::Ellipse {
                        x,
                        y,
                        width,
                        height,
                    } => {
                        let rx = width * 0.5;
                        let ry = height * 0.5;
                        let cx = x + rx;
                        let cy = y + ry;
                        let k = 0.552_284_749_830_793_6;
                        let kx = rx * k;
                        let ky = ry * k;
                        out.push(PathCommand::MoveTo { x: cx + rx, y: cy });
                        out.push(PathCommand::CubicTo {
                            cx1: cx + rx,
                            cy1: cy + ky,
                            cx2: cx + kx,
                            cy2: cy + ry,
                            x: cx,
                            y: cy + ry,
                        });
                        out.push(PathCommand::CubicTo {
                            cx1: cx - kx,
                            cy1: cy + ry,
                            cx2: cx - rx,
                            cy2: cy + ky,
                            x: cx - rx,
                            y: cy,
                        });
                        out.push(PathCommand::CubicTo {
                            cx1: cx - rx,
                            cy1: cy - ky,
                            cx2: cx - kx,
                            cy2: cy - ry,
                            x: cx,
                            y: cy - ry,
                        });
                        out.push(PathCommand::CubicTo {
                            cx1: cx + kx,
                            cy1: cy - ry,
                            cx2: cx + rx,
                            cy2: cy - ky,
                            x: cx + rx,
                            y: cy,
                        });
                        out.push(PathCommand::Close);
                        continue;
                    }
                }
            }
        } else if let Some([s_pt, e_pt]) =
            recognise_straight_line(&sample_anchors, zoom, Tolerance::Normal)
        {
            let snapped_end = snap_near_axis_endpoint(
                [f64::from(s_pt.x), f64::from(s_pt.y)],
                [f64::from(e_pt.x), f64::from(e_pt.y)],
            );
            out.push(PathCommand::MoveTo {
                x: f64::from(s_pt.x),
                y: f64::from(s_pt.y),
            });
            out.push(PathCommand::LineTo {
                x: snapped_end[0],
                y: snapped_end[1],
            });
            continue;
        }

        // Stepwise straightening: flatten shallow curves / pull deeper curve control points
        // toward their chord, then simplify nearly-collinear LineTo runs.
        let has_curves = commands[(start + 1)..body_end].iter().any(|c| {
            matches!(
                c,
                PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
            )
        });
        if has_curves {
            let Some(PathCommand::MoveTo { x: x0, y: y0 }) = commands.get(start).copied() else {
                continue;
            };
            let mut cur = [x0, y0];
            out.push(PathCommand::MoveTo { x: x0, y: y0 });
            for cmd in &commands[(start + 1)..body_end] {
                match *cmd {
                    PathCommand::LineTo { x, y } => {
                        let p = snap_near_axis_endpoint(cur, [x, y]);
                        out.push(PathCommand::LineTo { x: p[0], y: p[1] });
                        cur = p;
                    }
                    PathCommand::QuadraticTo { cx, cy, x, y } => {
                        let mid = [0.5 * (cur[0] + x), 0.5 * (cur[1] + y)];
                        let dev = (cx - mid[0]).hypot(cy - mid[1]);
                        if dev <= curve_flat_tol {
                            let p = snap_near_axis_endpoint(cur, [x, y]);
                            out.push(PathCommand::LineTo { x: p[0], y: p[1] });
                            cur = p;
                        } else {
                            out.push(PathCommand::QuadraticTo {
                                cx: 0.5 * cx + 0.5 * mid[0],
                                cy: 0.5 * cy + 0.5 * mid[1],
                                x,
                                y,
                            });
                            cur = [x, y];
                        }
                    }
                    PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x,
                        y,
                    } => {
                        let l1 = [cur[0] + (x - cur[0]) / 3.0, cur[1] + (y - cur[1]) / 3.0];
                        let l2 = [
                            cur[0] + 2.0 * (x - cur[0]) / 3.0,
                            cur[1] + 2.0 * (y - cur[1]) / 3.0,
                        ];
                        let dev = (cx1 - l1[0])
                            .hypot(cy1 - l1[1])
                            .max((cx2 - l2[0]).hypot(cy2 - l2[1]));
                        if dev <= curve_flat_tol {
                            let p = snap_near_axis_endpoint(cur, [x, y]);
                            out.push(PathCommand::LineTo { x: p[0], y: p[1] });
                            cur = p;
                        } else {
                            out.push(PathCommand::CubicTo {
                                cx1: 0.5 * cx1 + 0.5 * l1[0],
                                cy1: 0.5 * cy1 + 0.5 * l1[1],
                                cx2: 0.5 * cx2 + 0.5 * l2[0],
                                cy2: 0.5 * cy2 + 0.5 * l2[1],
                                x,
                                y,
                            });
                            cur = [x, y];
                        }
                    }
                    PathCommand::MoveTo { .. } | PathCommand::Close => {}
                }
            }
            if closed {
                out.push(PathCommand::Close);
            }
        } else {
            let anchors = contour_anchor_points(commands, start, body_end, closed);
            if closed && anchors.len() >= 3 {
                let mut loop_pts = anchors.clone();
                loop_pts.push(anchors[0]);
                let simplified = rdp_simplify_points(&loop_pts, rdp_eps);
                let mut clean: Vec<Pos2> = if simplified.len() >= 4 {
                    simplified[..(simplified.len() - 1)].to_vec()
                } else {
                    anchors
                };
                out.push(PathCommand::MoveTo {
                    x: f64::from(clean[0].x),
                    y: f64::from(clean[0].y),
                });
                let mut cur = [f64::from(clean[0].x), f64::from(clean[0].y)];
                for pt in clean.drain(1..) {
                    let p = snap_near_axis_endpoint(cur, [f64::from(pt.x), f64::from(pt.y)]);
                    out.push(PathCommand::LineTo { x: p[0], y: p[1] });
                    cur = p;
                }
                out.push(PathCommand::Close);
            } else if !closed && anchors.len() >= 2 {
                let simplified = rdp_simplify_points(&anchors, rdp_eps);
                let clean = if simplified.len() >= 2 {
                    simplified
                } else {
                    anchors
                };
                out.push(PathCommand::MoveTo {
                    x: f64::from(clean[0].x),
                    y: f64::from(clean[0].y),
                });
                let mut cur = [f64::from(clean[0].x), f64::from(clean[0].y)];
                for pt in &clean[1..] {
                    let p = snap_near_axis_endpoint(cur, [f64::from(pt.x), f64::from(pt.y)]);
                    out.push(PathCommand::LineTo { x: p[0], y: p[1] });
                    cur = p;
                }
            } else {
                out.extend_from_slice(&commands[start..body_end]);
                if closed {
                    out.push(PathCommand::Close);
                }
            }
        }
    }
    out
}

pub(crate) fn editable_element_commands(element: &Element) -> Option<Vec<PathCommand>> {
    element.to_path_commands().map(|(commands, _)| commands)
}

pub(crate) fn write_editable_element_commands(element: &mut Element, commands: Vec<PathCommand>) {
    match &mut element.kind {
        ElementKind::Path {
            commands: destination,
            ..
        }
        | ElementKind::PaintedPath {
            commands: destination,
            ..
        } => {
            *destination = commands;
        }
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
            element.transform.primitive = None;
            element.kind = ElementKind::Path {
                commands,
                fill_rule: FillRule::NonZero,
                fill: *fill,
                stroke: *stroke,
                stroke_width: *stroke_width,
            };
        }
        _ => {}
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReshapeVertexKind {
    Corner,
    SmoothCurve,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ReshapeAction {
    Corner {
        local_vertex: [f64; 2],
    },
    SmoothVertex {
        local_vertex: [f64; 2],
        break_corner: bool,
    },
    Segment {
        cmd_idx: usize,
        t: f64,
        local_point: [f64; 2],
        create_corner: bool,
    },
}

impl ReshapeAction {
    pub(crate) fn with_corner_modifier(self) -> Self {
        match self {
            Self::SmoothVertex { local_vertex, .. } => Self::SmoothVertex {
                local_vertex,
                break_corner: true,
            },
            Self::Segment {
                cmd_idx,
                t,
                local_point,
                ..
            } => Self::Segment {
                cmd_idx,
                t,
                local_point,
                create_corner: true,
            },
            other => other,
        }
    }
}

#[derive(Clone, Copy)]
struct ContourSegInfo {
    p0: [f64; 2],
    p1: [f64; 2],
    is_curve: bool,
    out_dir: [f64; 2],
    in_dir: [f64; 2],
}

fn contour_ranges(commands: &[PathCommand]) -> Vec<(usize, usize, bool)> {
    let mut ranges = Vec::new();
    let mut current_start = None;
    for (idx, cmd) in commands.iter().enumerate() {
        if matches!(cmd, PathCommand::MoveTo { .. }) {
            if let Some(start) = current_start {
                let closed = matches!(commands.get(idx - 1), Some(PathCommand::Close));
                let body_end = if closed { idx - 1 } else { idx };
                ranges.push((start, body_end, closed));
            }
            current_start = Some(idx);
        }
    }
    if let Some(start) = current_start {
        let end = commands.len();
        let closed = matches!(commands.get(end - 1), Some(PathCommand::Close));
        let body_end = if closed { end - 1 } else { end };
        ranges.push((start, body_end, closed));
    }
    ranges
}

pub(crate) fn classify_reshape_vertices(
    commands: &[PathCommand],
) -> Vec<([f64; 2], ReshapeVertexKind)> {
    let mut out = Vec::new();
    let junction_kind = |prev: &ContourSegInfo, next: &ContourSegInfo| -> ReshapeVertexKind {
        if !prev.is_curve || !next.is_curve {
            return ReshapeVertexKind::Corner;
        }
        let l1 = prev.in_dir[0].hypot(prev.in_dir[1]);
        let l2 = next.out_dir[0].hypot(next.out_dir[1]);
        if l1 <= 1e-9 || l2 <= 1e-9 {
            return ReshapeVertexKind::Corner;
        }
        let dot = (prev.in_dir[0] * next.out_dir[0] + prev.in_dir[1] * next.out_dir[1]) / (l1 * l2);
        if dot >= 0.965 {
            ReshapeVertexKind::SmoothCurve
        } else {
            ReshapeVertexKind::Corner
        }
    };

    for (start, body_end, closed) in contour_ranges(commands) {
        if body_end <= start + 1 {
            continue;
        }
        let Some(start_pt) = anchor_endpoint(&commands[start]) else {
            continue;
        };
        let mut p0 = start_pt;
        let mut segs = Vec::new();
        for cmd in &commands[(start + 1)..body_end] {
            match *cmd {
                PathCommand::LineTo { x, y } => {
                    let p1 = [x, y];
                    let d = [p1[0] - p0[0], p1[1] - p0[1]];
                    if d[0].hypot(d[1]) > 1e-6 {
                        segs.push(ContourSegInfo {
                            p0,
                            p1,
                            is_curve: false,
                            out_dir: d,
                            in_dir: d,
                        });
                    }
                    p0 = p1;
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let p1 = [x, y];
                    let out_dir = if (cx - p0[0]).hypot(cy - p0[1]) > 1e-6 {
                        [cx - p0[0], cy - p0[1]]
                    } else {
                        [x - p0[0], y - p0[1]]
                    };
                    let in_dir = if (x - cx).hypot(y - cy) > 1e-6 {
                        [x - cx, y - cy]
                    } else {
                        [x - p0[0], y - p0[1]]
                    };
                    segs.push(ContourSegInfo {
                        p0,
                        p1,
                        is_curve: true,
                        out_dir,
                        in_dir,
                    });
                    p0 = p1;
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let p1 = [x, y];
                    let out_dir = if (cx1 - p0[0]).hypot(cy1 - p0[1]) > 1e-6 {
                        [cx1 - p0[0], cy1 - p0[1]]
                    } else if (cx2 - p0[0]).hypot(cy2 - p0[1]) > 1e-6 {
                        [cx2 - p0[0], cy2 - p0[1]]
                    } else {
                        [x - p0[0], y - p0[1]]
                    };
                    let in_dir = if (x - cx2).hypot(y - cy2) > 1e-6 {
                        [x - cx2, y - cy2]
                    } else if (x - cx1).hypot(y - cy1) > 1e-6 {
                        [x - cx1, y - cy1]
                    } else {
                        [x - p0[0], y - p0[1]]
                    };
                    segs.push(ContourSegInfo {
                        p0,
                        p1,
                        is_curve: true,
                        out_dir,
                        in_dir,
                    });
                    p0 = p1;
                }
                _ => {}
            }
        }
        if closed && (p0[0] - start_pt[0]).hypot(p0[1] - start_pt[1]) > 1e-6 {
            let d = [start_pt[0] - p0[0], start_pt[1] - p0[1]];
            segs.push(ContourSegInfo {
                p0,
                p1: start_pt,
                is_curve: false,
                out_dir: d,
                in_dir: d,
            });
        }
        if segs.is_empty() {
            continue;
        }
        let effectively_closed = closed
            || (segs.len() >= 2
                && (segs.last().unwrap().p1[0] - start_pt[0])
                    .hypot(segs.last().unwrap().p1[1] - start_pt[1])
                    <= 1e-4);
        let first_kind = if effectively_closed {
            junction_kind(segs.last().unwrap(), &segs[0])
        } else {
            ReshapeVertexKind::Corner
        };
        out.push((segs[0].p0, first_kind));
        for i in 0..(segs.len() - 1) {
            out.push((segs[i].p1, junction_kind(&segs[i], &segs[i + 1])));
        }
        if !effectively_closed {
            out.push((segs.last().unwrap().p1, ReshapeVertexKind::Corner));
        }
    }
    out
}

fn eval_command_segment_at(commands: &[PathCommand], cmd_idx: usize, t: f64) -> Option<[f64; 2]> {
    let t = t.clamp(0.0, 1.0);
    let mut subpath_start: Option<[f64; 2]> = None;
    let mut cursor: Option<[f64; 2]> = None;
    for (idx, cmd) in commands.iter().enumerate() {
        if idx == cmd_idx {
            let p0 = cursor?;
            return match *cmd {
                PathCommand::MoveTo { .. } => None,
                PathCommand::LineTo { x, y } => {
                    Some([p0[0] + (x - p0[0]) * t, p0[1] + (y - p0[1]) * t])
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let u = 1.0 - t;
                    Some([
                        u * u * p0[0] + 2.0 * u * t * cx + t * t * x,
                        u * u * p0[1] + 2.0 * u * t * cy + t * t * y,
                    ])
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let u = 1.0 - t;
                    let (u2, t2) = (u * u, t * t);
                    Some([
                        u2 * u * p0[0] + 3.0 * u2 * t * cx1 + 3.0 * u * t2 * cx2 + t2 * t * x,
                        u2 * u * p0[1] + 3.0 * u2 * t * cy1 + 3.0 * u * t2 * cy2 + t2 * t * y,
                    ])
                }
                PathCommand::Close => {
                    let start = subpath_start?;
                    Some([
                        p0[0] + (start[0] - p0[0]) * t,
                        p0[1] + (start[1] - p0[1]) * t,
                    ])
                }
            };
        }
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                subpath_start = Some([x, y]);
                cursor = Some([x, y]);
            }
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => {
                cursor = Some([x, y]);
            }
            PathCommand::Close => {
                cursor = subpath_start;
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn hit_reshape_on_commands(
    commands: &[PathCommand],
    stroke_width: f64,
    matrix: [f64; 6],
    mapping: StageMapping,
    origin: Pos2,
    zoom: f32,
    pos: Pos2,
    alt: bool,
) -> Option<(ReshapeAction, neoflash_ui::cursors::Cursor, Pos2)> {
    let vertices = classify_reshape_vertices(commands);
    let mut best_vertex: Option<(f32, [f64; 2], ReshapeVertexKind, Pos2)> = None;
    for (local_v, kind) in vertices {
        let edit_pt = crate::stage::affine(matrix, Pos2::new(local_v[0] as f32, local_v[1] as f32));
        let screen_pt = mapping.edit_to_screen(origin, zoom, edit_pt);
        let dist = screen_pt.distance(pos);
        if dist <= 8.0 && best_vertex.is_none_or(|(best_d, ..)| dist < best_d) {
            best_vertex = Some((dist, local_v, kind, edit_pt));
        }
    }
    if let Some((_, local_vertex, kind, edit_pt)) = best_vertex {
        return match kind {
            ReshapeVertexKind::Corner => Some((
                ReshapeAction::Corner { local_vertex },
                neoflash_ui::cursors::ARROW_RESHAPE_CORNER,
                edit_pt,
            )),
            ReshapeVertexKind::SmoothCurve => {
                let cursor = if alt {
                    neoflash_ui::cursors::ARROW_RESHAPE_CORNER
                } else {
                    neoflash_ui::cursors::ARROW_RESHAPE_CURVE
                };
                Some((
                    ReshapeAction::SmoothVertex {
                        local_vertex,
                        break_corner: alt,
                    },
                    cursor,
                    edit_pt,
                ))
            }
        };
    }

    let stage_pt = Pos2::ZERO + (pos - origin) / zoom.max(1e-3);
    let composite = mapping.map_matrix(matrix);
    let stage_commands = map_commands_forward(commands, composite);
    let scale = composite[0]
        .hypot(composite[1])
        .max(composite[2].hypot(composite[3]));
    let stroke_tol = if stroke_width > 0.0 && scale.is_finite() {
        0.5 * stroke_width * scale + 2.0 / f64::from(zoom.max(1e-3))
    } else {
        0.0
    };
    let tolerance = (7.0 / f64::from(zoom.max(1e-3))).max(stroke_tol);
    let (cmd_idx, t, _closest, dist) = nearest_path_segment(
        &stage_commands,
        [f64::from(stage_pt.x), f64::from(stage_pt.y)],
    )?;
    if dist > tolerance {
        return None;
    }
    let local_point = eval_command_segment_at(commands, cmd_idx, t)?;
    let edit_pt = crate::stage::affine(
        matrix,
        Pos2::new(local_point[0] as f32, local_point[1] as f32),
    );
    let cursor = if alt {
        neoflash_ui::cursors::ARROW_RESHAPE_CORNER
    } else {
        neoflash_ui::cursors::ARROW_RESHAPE_CURVE
    };
    Some((
        ReshapeAction::Segment {
            cmd_idx,
            t,
            local_point,
            create_corner: alt,
        },
        cursor,
        edit_pt,
    ))
}

pub(crate) fn apply_reshape_action(
    commands: &[PathCommand],
    action: ReshapeAction,
    local_delta: [f64; 2],
) -> Option<Vec<PathCommand>> {
    if !local_delta[0].is_finite() || !local_delta[1].is_finite() {
        return None;
    }
    if local_delta[0].hypot(local_delta[1]) <= 1e-9 {
        return Some(commands.to_vec());
    }
    match action {
        ReshapeAction::Corner { local_vertex } => Some(move_reshape_vertex(
            commands,
            local_vertex,
            local_delta,
            false,
        )),
        ReshapeAction::SmoothVertex {
            local_vertex,
            break_corner,
        } => Some(move_reshape_vertex(
            commands,
            local_vertex,
            local_delta,
            break_corner,
        )),
        ReshapeAction::Segment {
            cmd_idx,
            t,
            local_point,
            create_corner,
        } => reshape_segment_at(
            commands,
            cmd_idx,
            t,
            local_point,
            local_delta,
            create_corner,
        ),
    }
}

fn move_reshape_vertex(
    commands: &[PathCommand],
    local_vertex: [f64; 2],
    delta: [f64; 2],
    break_corner: bool,
) -> Vec<PathCommand> {
    let [dx, dy] = delta;
    let moved_target = [local_vertex[0] + dx, local_vertex[1] + dy];
    let near_vertex =
        |pt: [f64; 2]| (pt[0] - local_vertex[0]).hypot(pt[1] - local_vertex[1]) <= 1e-4;
    let mut out = commands.to_vec();

    for (start, body_end, closed) in contour_ranges(commands) {
        if body_end <= start {
            continue;
        }
        let mut prev_moved = anchor_endpoint(&commands[start]).is_some_and(near_vertex);
        if prev_moved && let Some(PathCommand::MoveTo { x, y }) = out.get_mut(start) {
            *x += dx;
            *y += dy;
        }
        let first_moved = prev_moved;
        for idx in (start + 1)..body_end {
            let cur_moved = anchor_endpoint(&commands[idx]).is_some_and(near_vertex);
            let p0 = anchor_endpoint(&commands[idx - 1]).unwrap_or(local_vertex);
            let p1 = anchor_endpoint(&commands[idx]).unwrap_or(local_vertex);
            match out.get_mut(idx) {
                Some(PathCommand::LineTo { x, y }) => {
                    if cur_moved {
                        *x += dx;
                        *y += dy;
                    }
                }
                Some(PathCommand::QuadraticTo { cx, cy, x, y }) => {
                    if cur_moved {
                        *x += dx;
                        *y += dy;
                    }
                    if break_corner && (prev_moved || cur_moved) {
                        let a = if prev_moved { moved_target } else { p0 };
                        let b = if cur_moved { moved_target } else { p1 };
                        *cx = 0.5 * (a[0] + b[0]);
                        *cy = 0.5 * (a[1] + b[1]);
                    } else if prev_moved && cur_moved {
                        *cx += dx;
                        *cy += dy;
                    } else if prev_moved || cur_moved {
                        *cx += 0.5 * dx;
                        *cy += 0.5 * dy;
                    }
                }
                Some(PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                }) => {
                    if prev_moved {
                        if break_corner {
                            *cx1 = moved_target[0];
                            *cy1 = moved_target[1];
                        } else {
                            *cx1 += dx;
                            *cy1 += dy;
                        }
                    }
                    if cur_moved {
                        if break_corner {
                            *cx2 = moved_target[0];
                            *cy2 = moved_target[1];
                        } else {
                            *cx2 += dx;
                            *cy2 += dy;
                        }
                        *x += dx;
                        *y += dy;
                    }
                }
                _ => {}
            }
            prev_moved = cur_moved;
        }
        let _ = (closed, first_moved);
    }
    out
}

fn reshape_segment_at(
    commands: &[PathCommand],
    cmd_idx: usize,
    t: f64,
    local_point: [f64; 2],
    delta: [f64; 2],
    create_corner: bool,
) -> Option<Vec<PathCommand>> {
    if cmd_idx >= commands.len() {
        return None;
    }
    let mut subpath_start: Option<[f64; 2]> = None;
    let mut cursor: Option<[f64; 2]> = None;
    let mut p0_at_cmd = None;
    let mut start_at_cmd = None;
    for (idx, cmd) in commands.iter().enumerate() {
        if idx == cmd_idx {
            p0_at_cmd = cursor;
            start_at_cmd = subpath_start;
            break;
        }
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                subpath_start = Some([x, y]);
                cursor = Some([x, y]);
            }
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => {
                cursor = Some([x, y]);
            }
            PathCommand::Close => {
                cursor = subpath_start;
            }
        }
    }
    let p0 = p0_at_cmd?;
    let corner = [local_point[0] + delta[0], local_point[1] + delta[1]];

    if create_corner {
        match commands[cmd_idx] {
            PathCommand::MoveTo { .. } => return None,
            PathCommand::LineTo { x, y } => {
                let mut out = Vec::with_capacity(commands.len() + 1);
                out.extend_from_slice(&commands[..cmd_idx]);
                out.push(PathCommand::LineTo {
                    x: corner[0],
                    y: corner[1],
                });
                out.push(PathCommand::LineTo { x, y });
                out.extend_from_slice(&commands[(cmd_idx + 1)..]);
                return Some(out);
            }
            PathCommand::Close => {
                let mut out = Vec::with_capacity(commands.len() + 1);
                out.extend_from_slice(&commands[..cmd_idx]);
                out.push(PathCommand::LineTo {
                    x: corner[0],
                    y: corner[1],
                });
                out.push(PathCommand::Close);
                out.extend_from_slice(&commands[(cmd_idx + 1)..]);
                return Some(out);
            }
            PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. } => {
                let (mut split, inserted) = subdivide_path_at(commands, cmd_idx, t)?;
                let p_end = anchor_endpoint(split.get(inserted + 1)?)?;
                match split.get_mut(inserted)? {
                    PathCommand::QuadraticTo { cx, cy, x, y } => {
                        *x = corner[0];
                        *y = corner[1];
                        *cx = 0.5 * (p0[0] + corner[0]);
                        *cy = 0.5 * (p0[1] + corner[1]);
                    }
                    PathCommand::CubicTo { cx2, cy2, x, y, .. } => {
                        *x = corner[0];
                        *y = corner[1];
                        *cx2 = corner[0];
                        *cy2 = corner[1];
                    }
                    _ => {}
                }
                match split.get_mut(inserted + 1)? {
                    PathCommand::QuadraticTo { cx, cy, .. } => {
                        *cx = 0.5 * (corner[0] + p_end[0]);
                        *cy = 0.5 * (corner[1] + p_end[1]);
                    }
                    PathCommand::CubicTo { cx1, cy1, .. } => {
                        *cx1 = corner[0];
                        *cy1 = corner[1];
                    }
                    _ => {}
                }
                return Some(split);
            }
        }
    }

    let tau = t.clamp(0.15, 0.85);
    let quad_weight = (2.0 * (1.0 - tau) * tau).max(1e-4);
    let cubic_weight = (3.0 * (1.0 - tau) * tau).max(1e-4);
    let mut out = Vec::with_capacity(commands.len() + 1);
    out.extend_from_slice(&commands[..cmd_idx]);
    match commands[cmd_idx] {
        PathCommand::MoveTo { .. } => return None,
        PathCommand::LineTo { x, y } => {
            let cx = 0.5 * (p0[0] + x) + delta[0] / quad_weight;
            let cy = 0.5 * (p0[1] + y) + delta[1] / quad_weight;
            out.push(PathCommand::QuadraticTo { cx, cy, x, y });
        }
        PathCommand::Close => {
            let start = start_at_cmd?;
            let cx = 0.5 * (p0[0] + start[0]) + delta[0] / quad_weight;
            let cy = 0.5 * (p0[1] + start[1]) + delta[1] / quad_weight;
            out.push(PathCommand::QuadraticTo {
                cx,
                cy,
                x: start[0],
                y: start[1],
            });
            out.push(PathCommand::Close);
        }
        PathCommand::QuadraticTo { cx, cy, x, y } => {
            out.push(PathCommand::QuadraticTo {
                cx: cx + delta[0] / quad_weight,
                cy: cy + delta[1] / quad_weight,
                x,
                y,
            });
        }
        PathCommand::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        } => {
            out.push(PathCommand::CubicTo {
                cx1: cx1 + delta[0] / cubic_weight,
                cy1: cy1 + delta[1] / cubic_weight,
                cx2: cx2 + delta[0] / cubic_weight,
                cy2: cy2 + delta[1] / cubic_weight,
                x,
                y,
            });
        }
    }
    out.extend_from_slice(&commands[(cmd_idx + 1)..]);
    Some(out)
}

pub(crate) fn path_segment_hit_local(
    commands: &[PathCommand],
    matrix: [f64; 6],
    mapping: StageMapping,
    origin: Pos2,
    zoom: f32,
    pos: Pos2,
) -> Option<(usize, f64)> {
    let stage_pt = Pos2::ZERO + (pos - origin) / zoom.max(1e-3);
    let composite = mapping.map_matrix(matrix);
    let stage_commands = map_commands_forward(commands, composite);
    let tolerance = 7.0 / f64::from(zoom.max(1e-3));
    let (cmd_idx, t, _closest, dist) = nearest_path_segment(
        &stage_commands,
        [f64::from(stage_pt.x), f64::from(stage_pt.y)],
    )?;
    (dist <= tolerance).then_some((cmd_idx, t))
}

fn anchor_endpoint(cmd: &PathCommand) -> Option<[f64; 2]> {
    match *cmd {
        PathCommand::MoveTo { x, y }
        | PathCommand::LineTo { x, y }
        | PathCommand::QuadraticTo { x, y, .. }
        | PathCommand::CubicTo { x, y, .. } => Some([x, y]),
        PathCommand::Close => None,
    }
}

pub(crate) fn delete_anchors_from_path(
    commands: &[PathCommand],
    remove_indices: &[usize],
) -> Vec<PathCommand> {
    if remove_indices.is_empty() {
        return commands.to_vec();
    }
    let mut contours = Vec::new();
    let mut current_start = None;
    for (idx, cmd) in commands.iter().enumerate() {
        if matches!(cmd, PathCommand::MoveTo { .. }) {
            if let Some(start) = current_start {
                contours.push(start..idx);
            }
            current_start = Some(idx);
        }
    }
    if let Some(start) = current_start {
        contours.push(start..commands.len());
    }

    let mut out = Vec::new();
    for range in contours {
        let start = range.start;
        let end = range.end;
        if end <= start {
            continue;
        }
        let closed = matches!(commands.get(end - 1), Some(PathCommand::Close));
        let body_end = if closed { end - 1 } else { end };
        if body_end <= start {
            continue;
        }
        let coincident_seam = closed
            && body_end > start + 1
            && anchor_endpoint(&commands[start])
                .zip(anchor_endpoint(&commands[body_end - 1]))
                .is_some_and(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-4);

        if coincident_seam {
            let last = body_end - 1;
            let remove_seam = remove_indices.contains(&start) || remove_indices.contains(&last);
            let kept_interior: Vec<PathCommand> = ((start + 1)..last)
                .filter(|idx| !remove_indices.contains(idx))
                .map(|idx| commands[idx])
                .collect();
            if remove_seam {
                if kept_interior.len() < 2 {
                    continue;
                }
                if let Some([x, y]) = anchor_endpoint(&kept_interior[0]) {
                    out.push(PathCommand::MoveTo { x, y });
                    out.extend(kept_interior.into_iter().skip(1));
                    out.push(PathCommand::Close);
                }
            } else {
                if kept_interior.is_empty() {
                    continue;
                }
                out.push(commands[start]);
                out.extend(kept_interior);
                out.push(commands[last]);
                out.push(PathCommand::Close);
            }
        } else {
            let kept: Vec<PathCommand> = (start..body_end)
                .filter(|idx| !remove_indices.contains(idx))
                .map(|idx| commands[idx])
                .collect();
            if kept.len() < 2 {
                continue;
            }
            if let Some([x, y]) = anchor_endpoint(&kept[0]) {
                out.push(PathCommand::MoveTo { x, y });
                out.extend(kept.into_iter().skip(1));
                if closed {
                    out.push(PathCommand::Close);
                }
            }
        }
    }
    out
}

/// Gradient geometry stays authored; native preview samples the shared brush on
/// a bounded adaptive mesh. HTML5 uses Canvas gradients on the original path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_gradient_path(
    painter: &egui::Painter,
    commands: &[PathCommand],
    fingerprint: Option<PathFingerprint>,
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
        let geometry =
            match tessellate_cached_with_fingerprint(commands, fingerprint, rule, width, 0.1) {
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
                let ab = a + (b - a) * 0.5;
                let ac = a + (c - a) * 0.5;
                let bc = b + (c - b) * 0.5;
                let stop_or_edge_refine = if error <= 2.
                    && extent <= 24.
                    && extent > 3.5
                    && let Some(g) = gradient
                    && (g.stops.len() > 2 || !g.spread.is_extend() || g.linear_rgb)
                {
                    let crosses_stop = if let (Some(ua), Some(ub), Some(uc)) = (
                        g.raw_unit_at(a.x as f64, a.y as f64),
                        g.raw_unit_at(b.x as f64, b.y as f64),
                        g.raw_unit_at(c.x as f64, c.y as f64),
                    ) {
                        let u_min = ua.min(ub).min(uc);
                        let u_max = ua.max(ub).max(uc);
                        (u_min.floor() as i64 != u_max.floor() as i64)
                            || g.stops.iter().any(|s| {
                                let r = f64::from(s.ratio) / 255.0;
                                r > u_min + 1e-3 && r < u_max - 1e-3
                            })
                    } else {
                        false
                    };
                    crosses_stop || {
                        let edge_err = [
                            (sample(ab), colors[0], colors[1]),
                            (sample(bc), colors[1], colors[2]),
                            (sample(ac), colors[0], colors[2]),
                        ]
                        .into_iter()
                        .map(|(m, c0, c1)| {
                            (0..4)
                                .map(|i| {
                                    ((u16::from(c0[i]) + u16::from(c1[i])) as f32 * 0.5
                                        - f32::from(m[i]))
                                    .abs()
                                })
                                .fold(0f32, f32::max)
                        })
                        .fold(0f32, f32::max);
                        edge_err > 2.0
                    }
                } else {
                    false
                };
                let refine =
                    gradient.is_some() && (error > 2. || extent > 24. || stop_or_edge_refine);
                if refine && depth < 7 && mesh.vertices.len() + pending.len() * 3 < 150_000 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle_commands(width: f64) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo { x: 0., y: 0. },
            PathCommand::LineTo { x: width, y: 0. },
            PathCommand::LineTo { x: width, y: 12. },
            PathCommand::LineTo { x: 0., y: 12. },
            PathCommand::Close,
        ]
    }

    fn gradient(alpha: u8) -> GradientPaint {
        GradientPaint {
            kind: GradientKind::Linear,
            matrix: [1., 0., 0., 1., 0., 0.],
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [20, 40, 60, alpha],
                },
                GradientStop {
                    ratio: 255,
                    color: [80, 100, 120, alpha],
                },
            ],
            focal_point_ratio: 0.,
            ..Default::default()
        }
    }

    fn rendered_path_passes(kind: &ElementKind) -> usize {
        let ctx = egui::Context::default();
        ctx.run(Default::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            match kind {
                ElementKind::Path {
                    commands,
                    fill_rule,
                    fill,
                    stroke,
                    stroke_width,
                } => paint_path(
                    &painter,
                    commands,
                    PathPaint {
                        rule: *fill_rule,
                        fill: Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3]),
                        stroke: Color32::from_rgba_unmultiplied(
                            stroke[0], stroke[1], stroke[2], stroke[3],
                        ),
                        width: *stroke_width,
                        tolerance: 0.1,
                        fingerprint: None,
                    },
                    |point| point,
                ),
                ElementKind::PaintedPath {
                    commands,
                    fill_rule,
                    fill,
                    stroke,
                    stroke_width,
                    fill_gradient,
                    stroke_gradient,
                } => paint_gradient_path(
                    &painter,
                    commands,
                    None,
                    *fill_rule,
                    *stroke_width,
                    (fill, fill_gradient.as_ref()),
                    (stroke, stroke_gradient.as_ref()),
                    |point| point,
                    |color| Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
                ),
                _ => unreachable!("test cases are path renderers"),
            }
        })
        .shapes
        .iter()
        .filter(|clipped| matches!(clipped.shape, Shape::Mesh(_)))
        .count()
    }

    #[test]
    fn vector_visibility_matches_renderer_and_stage_hit_testing() {
        let commands = rectangle_commands(20.);
        let cases = [
            (
                "solid fill",
                ElementKind::Path {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [20, 40, 60, 255],
                    stroke: [0; 4],
                    stroke_width: 3.,
                },
                VectorPaintVisibility {
                    fill: true,
                    stroke: false,
                },
                Pos2::new(10., 6.),
            ),
            (
                "painted solid fallbacks",
                ElementKind::PaintedPath {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [20, 40, 60, 255],
                    stroke: [80, 100, 120, 255],
                    stroke_width: 3.,
                    fill_gradient: None,
                    stroke_gradient: None,
                },
                VectorPaintVisibility {
                    fill: true,
                    stroke: true,
                },
                Pos2::new(10., 6.),
            ),
            (
                "opaque fill gradient overrides transparent solid",
                ElementKind::PaintedPath {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [0; 4],
                    stroke_width: 3.,
                    fill_gradient: Some(gradient(255)),
                    stroke_gradient: None,
                },
                VectorPaintVisibility {
                    fill: true,
                    stroke: false,
                },
                Pos2::new(10., 6.),
            ),
            (
                "transparent fill gradient overrides opaque solid",
                ElementKind::PaintedPath {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [20, 40, 60, 255],
                    stroke: [0; 4],
                    stroke_width: 3.,
                    fill_gradient: Some(gradient(0)),
                    stroke_gradient: None,
                },
                VectorPaintVisibility {
                    fill: false,
                    stroke: false,
                },
                Pos2::new(10., 6.),
            ),
            (
                "opaque stroke gradient overrides transparent solid",
                ElementKind::PaintedPath {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [0; 4],
                    stroke_width: 3.,
                    fill_gradient: None,
                    stroke_gradient: Some(gradient(255)),
                },
                VectorPaintVisibility {
                    fill: false,
                    stroke: true,
                },
                Pos2::new(0., 6.),
            ),
            (
                "transparent stroke gradient overrides opaque solid",
                ElementKind::PaintedPath {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [20, 40, 60, 255],
                    stroke_width: 3.,
                    fill_gradient: None,
                    stroke_gradient: Some(gradient(0)),
                },
                VectorPaintVisibility {
                    fill: false,
                    stroke: false,
                },
                Pos2::new(0., 6.),
            ),
            (
                "zero-width stroke",
                ElementKind::PaintedPath {
                    commands,
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [20, 40, 60, 255],
                    stroke_width: 0.,
                    fill_gradient: None,
                    stroke_gradient: None,
                },
                VectorPaintVisibility {
                    fill: false,
                    stroke: false,
                },
                Pos2::new(0., 6.),
            ),
        ];
        let ctx = egui::Context::default();
        let painter = ctx.layer_painter(egui::LayerId::background());
        for (name, kind, expected, probe) in cases {
            assert_eq!(vector_paint_visibility(&kind), Some(expected), "{name}");
            assert_eq!(
                rendered_path_passes(&kind),
                usize::from(expected.fill) + usize::from(expected.stroke),
                "renderer passes: {name}"
            );
            let item = DrawItem {
                element_id: name.into(),
                name: String::new(),
                instance_path: vec![name.into()],
                layer_id: "layer".into(),
                transform: [1., 0., 0., 1., 0., 0.],
                alpha: 1.,
                color_transform: Default::default(),
                path_fingerprint: None,
                kind,
            };
            assert_eq!(
                crate::stage::hit_item_with_slop(&item, &painter, probe, 1., 3.),
                expected.fill || expected.stroke,
                "stage hit: {name}"
            );
        }
    }

    #[test]
    fn supplied_path_fingerprint_matches_fallback_bounds_and_tessellation_keys() {
        let commands = vec![
            PathCommand::MoveTo { x: 0., y: 0. },
            PathCommand::QuadraticTo {
                cx: 20.,
                cy: 30.,
                x: 40.,
                y: 0.,
            },
            PathCommand::LineTo { x: 0., y: 0. },
            PathCommand::Close,
        ];
        let fingerprint = path_commands_fingerprint(&commands);

        assert_eq!(
            bounds_cache_key(&commands, None),
            bounds_cache_key(&commands, Some(fingerprint))
        );
        assert_eq!(
            path_bounds_cached(&commands, None),
            path_bounds_cached(&commands, Some(fingerprint))
        );
        for width in [None, Some(3.5)] {
            assert_eq!(
                tess_cache_key(&commands, None, FillRule::EvenOdd, width, 0.123),
                tess_cache_key(
                    &commands,
                    Some(fingerprint),
                    FillRule::EvenOdd,
                    width,
                    0.123
                )
            );
        }

        let fallback = tessellate_cached(&commands, FillRule::EvenOdd, None, 0.123).unwrap();
        let supplied = tessellate_cached_with_fingerprint(
            &commands,
            Some(fingerprint),
            FillRule::EvenOdd,
            None,
            0.123,
        )
        .unwrap();
        assert!(Arc::ptr_eq(&fallback, &supplied));
    }

    #[test]
    fn retained_screen_mesh_reuses_arc_and_replaces_changed_draw_slot() {
        SCREEN_MESH_CACHE.with(|cache| {
            *cache.borrow_mut() = ScreenMeshCache::new(
                MAX_SCREEN_MESH_CACHE_ENTRIES,
                MAX_SCREEN_MESH_CACHE_ELEMENTS,
            );
        });
        let commands = rectangle_commands(20.);
        let fingerprint = path_commands_fingerprint(&commands);
        let mut item = DrawItem {
            element_id: "retained-path".into(),
            name: String::new(),
            instance_path: vec!["outer".into(), "retained-path".into()],
            layer_id: "layer".into(),
            transform: [1., 0., 0., 1., 5., 7.],
            alpha: 1.,
            color_transform: Default::default(),
            path_fingerprint: Some(fingerprint),
            kind: ElementKind::Path {
                commands: commands.clone(),
                fill_rule: FillRule::NonZero,
                fill: [220, 40, 20, 255],
                stroke: [0; 4],
                stroke_width: 0.,
            },
        };
        let origin = Pos2::new(30., 40.);
        let color = Color32::from_rgb(220, 40, 20);
        let build = |item: &DrawItem, commands: &[PathCommand], color, style| {
            retained_screen_mesh(
                item,
                commands,
                item.path_fingerprint,
                FillRule::NonZero,
                None,
                0.1,
                color,
                origin,
                1.5,
                style,
                ScreenMeshPass::Fill,
            )
            .unwrap()
        };

        let first = build(&item, &commands, color, RetainedPathStyle::Artwork);
        let warm = build(&item, &commands, color, RetainedPathStyle::Artwork);
        assert!(Arc::ptr_eq(&first, &warm));

        item.transform[4] += 10.;
        let moved = build(&item, &commands, color, RetainedPathStyle::Artwork);
        assert!(!Arc::ptr_eq(&warm, &moved));
        let recolored = build(
            &item,
            &commands,
            Color32::from_rgb(20, 80, 230),
            RetainedPathStyle::Artwork,
        );
        assert!(!Arc::ptr_eq(&moved, &recolored));

        let changed_commands = rectangle_commands(28.);
        item.path_fingerprint = Some(path_commands_fingerprint(&changed_commands));
        let reshaped = build(
            &item,
            &changed_commands,
            Color32::from_rgb(20, 80, 230),
            RetainedPathStyle::Artwork,
        );
        assert!(!Arc::ptr_eq(&recolored, &reshaped));
        let outlined = build(
            &item,
            &changed_commands,
            Color32::from_rgb(20, 80, 230),
            RetainedPathStyle::Outline,
        );
        assert!(!Arc::ptr_eq(&reshaped, &outlined));

        SCREEN_MESH_CACHE.with(|cache| {
            let cache = cache.borrow();
            assert_eq!(
                cache.entries.len(),
                1,
                "changed drag positions replace one slot"
            );
        });
    }

    #[test]
    fn retained_screen_mesh_cache_is_bounded_by_entries_and_elements() {
        let commands = rectangle_commands(20.);
        let fingerprint = path_commands_fingerprint(&commands);
        let (tessellation, _) =
            tess_cache_key(&commands, Some(fingerprint), FillRule::NonZero, None, 0.1);
        let key = ScreenMeshKey {
            tessellation,
            transform_bits: [0; 6],
            origin_bits: [0; 2],
            zoom_bits: 1.0_f32.to_bits(),
            color: [10, 20, 30, 255],
            style: RetainedPathStyle::Artwork,
        };
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(Pos2::ZERO, Color32::WHITE);
        let mesh = Arc::new(mesh);
        let slot = |identity| ScreenMeshSlot {
            identity,
            pass: ScreenMeshPass::Fill,
        };

        let mut by_entries = ScreenMeshCache::new(2, usize::MAX);
        for identity in 0..3 {
            by_entries.insert(slot(identity), key, mesh.clone());
        }
        assert!(by_entries.entries.len() <= 2);

        let mut by_elements = ScreenMeshCache::new(10, 2);
        for identity in 0..3 {
            by_elements.insert(slot(identity), key, mesh.clone());
        }
        assert!(by_elements.total_elements <= 2);
        assert!(by_elements.entries.len() <= 2);
    }
}

/// Interactive Stage handle kind for `PerspectiveGridSpec` (`T-233`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerspectiveGridHandle {
    Horizon,
    LeftVanishingPoint,
    RightVanishingPoint,
}

#[allow(dead_code)]
fn default_envelope_grid_points(rows: u32, cols: u32) -> Vec<[f64; 2]> {
    let rows = rows.clamp(1, 8);
    let cols = cols.clamp(1, 8);
    let mut pts = Vec::with_capacity(((rows + 1) * (cols + 1)) as usize);
    for r in 0..=rows {
        for c in 0..=cols {
            pts.push([c as f64 / cols as f64, r as f64 / rows as f64]);
        }
    }
    pts
}

#[allow(dead_code)]
impl Editor {
    /// Return the active `EnvelopeWarp` on the primary selected vector element, if any (`T-233`).
    pub fn selected_envelope_warp(&self) -> Option<EnvelopeWarp> {
        let fx = self.selected_element_vector_effects()?;
        fx.envelope_warp
    }

    /// Initialize or replace the `EnvelopeWarp` on the selected vector element in 1 Undo step (`T-233`).
    pub fn set_selected_envelope_warp(&mut self, warp: Option<EnvelopeWarp>) -> bool {
        let Some(element_id) = self
            .selected_element()
            .map(|e| e.id.clone())
            .or_else(|| self.selection.targets.first().map(|t| t.element.clone()))
        else {
            return false;
        };
        let layer_idx = self.layer;
        let frame = self.frame;
        let clean_warp = warp.map(|mut w| {
            w.rows = w.rows.clamp(1, 8);
            w.cols = w.cols.clamp(1, 8);
            w.bend = if w.bend.is_finite() {
                w.bend.clamp(-100.0, 100.0)
            } else {
                0.0
            };
            w.horizontal_distortion = if w.horizontal_distortion.is_finite() {
                w.horizontal_distortion.clamp(-100.0, 100.0)
            } else {
                0.0
            };
            w.vertical_distortion = if w.vertical_distortion.is_finite() {
                w.vertical_distortion.clamp(-100.0, 100.0)
            } else {
                0.0
            };
            let expected = ((w.rows + 1) * (w.cols + 1)) as usize;
            if w.control_points.len() != expected {
                w.control_points = default_envelope_grid_points(w.rows, w.cols);
            }
            w
        });
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        if !key.elements.iter().any(|e| e.id == element_id) {
            return false;
        }
        let existing_warp = key
            .vector_effects(&element_id)
            .and_then(|fx| fx.envelope_warp.clone());
        if existing_warp == clean_warp {
            return false;
        }
        self.transact_named("Set Envelope Warp", |editor| {
            let symbol = editor.symbol_mut();
            let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
                return;
            };
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
                return;
            };
            let mut fx = key
                .vector_effects(&element_id)
                .cloned()
                .unwrap_or(ElementVectorEffects {
                    element_id: element_id.clone(),
                    ..Default::default()
                });
            fx.envelope_warp = clean_warp;
            key.set_vector_effects(fx);
        });
        true
    }

    /// Compute the Stage-space handle positions `(row, col, [stage_x, stage_y])` for the selected
    /// element's `EnvelopeWarp` mesh (`T-233`).
    pub fn selected_envelope_stage_handles(&self) -> Vec<(u32, u32, [f64; 2])> {
        let Some(warp) = self.selected_envelope_warp() else {
            return Vec::new();
        };
        let Some(element_id) = self
            .selected_element()
            .map(|e| e.id.as_str())
            .or_else(|| self.selection.targets.first().map(|t| t.element.as_str()))
        else {
            return Vec::new();
        };
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return Vec::new();
        };
        let Some(key) = layer.key_at(self.frame) else {
            return Vec::new();
        };
        let Some(el) = key.elements.iter().find(|e| e.id == element_id) else {
            return Vec::new();
        };
        let Some((commands, _)) = el.to_path_commands() else {
            return Vec::new();
        };
        let Ok(Some(bounds)) = neoflash_core::path_bounds(&commands) else {
            return Vec::new();
        };
        let bw = (bounds.x_max - bounds.x_min).max(1.0);
        let bh = (bounds.y_max - bounds.y_min).max(1.0);
        let mat = transform_matrix(el.transform);
        let mut out = Vec::new();
        for r in 0..=warp.rows {
            for c in 0..=warp.cols {
                let idx = (r * (warp.cols + 1) + c) as usize;
                let uv = warp.control_points.get(idx).copied().unwrap_or([
                    c as f64 / warp.cols.max(1) as f64,
                    r as f64 / warp.rows.max(1) as f64,
                ]);
                let lx = bounds.x_min + uv[0] * bw;
                let ly = bounds.y_min + uv[1] * bh;
                let sx = mat[0] * lx + mat[2] * ly + mat[4];
                let sy = mat[1] * lx + mat[3] * ly + mat[5];
                out.push((r, c, [sx, sy]));
            }
        }
        out
    }

    /// Hit-test a Stage-space point against the selected element's `EnvelopeWarp` mesh handles (`T-233`).
    pub fn hit_test_envelope_stage_handle(
        &self,
        stage_point: [f64; 2],
        tolerance: f64,
    ) -> Option<(u32, u32)> {
        let tol_sq = tolerance.max(1.0).powi(2);
        self.selected_envelope_stage_handles()
            .into_iter()
            .filter_map(|(r, c, pos)| {
                let d2 = (pos[0] - stage_point[0]).powi(2) + (pos[1] - stage_point[1]).powi(2);
                (d2 <= tol_sq).then_some((r, c, d2))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(r, c, _)| (r, c))
    }

    /// Drag a control point `(row, col)` of the selected element's `EnvelopeWarp` mesh to a new
    /// normalized UV offset or Stage-space coordinate in 1 Undo step (`T-233`).
    pub fn drag_selected_envelope_handle_uv(
        &mut self,
        row: u32,
        col: u32,
        new_uv: [f64; 2],
    ) -> bool {
        if !new_uv[0].is_finite() || !new_uv[1].is_finite() {
            return false;
        }
        let clean_uv = [new_uv[0].clamp(-4.0, 5.0), new_uv[1].clamp(-4.0, 5.0)];
        let Some(mut warp) = self.selected_envelope_warp() else {
            return false;
        };
        if row > warp.rows || col > warp.cols {
            return false;
        }
        let expected = ((warp.rows + 1) * (warp.cols + 1)) as usize;
        if warp.control_points.len() != expected {
            warp.control_points = default_envelope_grid_points(warp.rows, warp.cols);
        }
        let idx = (row * (warp.cols + 1) + col) as usize;
        let Some(slot) = warp.control_points.get_mut(idx) else {
            return false;
        };
        if (slot[0] - clean_uv[0]).abs() <= 1e-6 && (slot[1] - clean_uv[1]).abs() <= 1e-6 {
            return false;
        }
        *slot = clean_uv;
        warp.preset = EnvelopeWarpPreset::CustomGrid;
        self.set_selected_envelope_warp(Some(warp))
    }

    /// Drag a control point `(row, col)` of the selected element's `EnvelopeWarp` mesh to a new
    /// Stage-space position `[stage_x, stage_y]` in 1 Undo step (`T-233`).
    pub fn drag_selected_envelope_handle_stage(
        &mut self,
        row: u32,
        col: u32,
        stage_pos: [f64; 2],
    ) -> bool {
        if !stage_pos[0].is_finite() || !stage_pos[1].is_finite() {
            return false;
        }
        let Some(element_id) = self
            .selected_element()
            .map(|e| e.id.as_str())
            .or_else(|| self.selection.targets.first().map(|t| t.element.as_str()))
        else {
            return false;
        };
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return false;
        };
        let Some(key) = layer.key_at(self.frame) else {
            return false;
        };
        let Some(el) = key.elements.iter().find(|e| e.id == element_id) else {
            return false;
        };
        let Some((commands, _)) = el.to_path_commands() else {
            return false;
        };
        let Ok(Some(bounds)) = neoflash_core::path_bounds(&commands) else {
            return false;
        };
        let bw = (bounds.x_max - bounds.x_min).max(1.0);
        let bh = (bounds.y_max - bounds.y_min).max(1.0);
        let m = transform_matrix(el.transform);
        let det = m[0] * m[3] - m[1] * m[2];
        let [lx, ly] = if det.abs() > 1e-9 {
            let dx = stage_pos[0] - m[4];
            let dy = stage_pos[1] - m[5];
            [
                (m[3] * dx - m[2] * dy) / det,
                (-m[1] * dx + m[0] * dy) / det,
            ]
        } else {
            [stage_pos[0] - el.transform.x, stage_pos[1] - el.transform.y]
        };
        let u = (lx - bounds.x_min) / bw;
        let v = (ly - bounds.y_min) / bh;
        self.drag_selected_envelope_handle_uv(row, col, [u, v])
    }

    /// Return the active `PerspectiveGridSpec` on the current timeline (`T-233`).
    pub fn active_perspective_grid(&self) -> Option<PerspectiveGridSpec> {
        self.symbol().timeline.perspective_grid()
    }

    /// Enable, update, or clear the `PerspectiveGridSpec` on the active timeline in 1 Undo step (`T-233`).
    pub fn apply_timeline_perspective_grid(&mut self, grid: Option<PerspectiveGridSpec>) -> bool {
        let clean = grid.map(|mut g| {
            if !g.horizon_y.is_finite() {
                g.horizon_y = 240.0;
            }
            if !g.left_vanishing_x.is_finite() {
                g.left_vanishing_x = -200.0;
            }
            if !g.right_vanishing_x.is_finite() {
                g.right_vanishing_x = 1000.0;
            }
            if (g.right_vanishing_x - g.left_vanishing_x).abs() < 10.0 {
                g.right_vanishing_x = g.left_vanishing_x + 100.0;
            }
            if !g.grid_cell_size.is_finite() || g.grid_cell_size <= 1.0 {
                g.grid_cell_size = 32.0;
            }
            if !g.grid_extent.is_finite() || g.grid_extent <= 10.0 {
                g.grid_extent = 480.0;
            }
            g
        });
        if self.active_perspective_grid() == clean {
            return false;
        }
        self.set_timeline_perspective_grid(clean);
        true
    }

    /// Toggle the active timeline's Perspective Grid on or off in 1 Undo step (`T-233`).
    pub fn toggle_perspective_grid(&mut self) -> bool {
        let mut grid = self.active_perspective_grid().unwrap_or_default();
        grid.enabled = !grid.enabled;
        self.apply_timeline_perspective_grid(Some(grid))
    }

    /// Switch the active `PerspectivePlane` (`LeftGrid`, `RightGrid`, `HorizontalGrid`) on the timeline's
    /// Perspective Grid in 1 Undo step (`T-233`).
    pub fn set_perspective_active_plane(&mut self, plane: PerspectivePlane) -> bool {
        let mut grid = self.active_perspective_grid().unwrap_or_default();
        grid.enabled = true;
        grid.active_plane = plane;
        self.apply_timeline_perspective_grid(Some(grid))
    }

    /// Drag a `PerspectiveGridHandle` (`Horizon`, `LeftVanishingPoint`, `RightVanishingPoint`) on
    /// Stage to `stage_pos` in 1 Undo step (`T-233`).
    pub fn drag_perspective_grid_handle(
        &mut self,
        handle: PerspectiveGridHandle,
        stage_pos: [f64; 2],
    ) -> bool {
        if !stage_pos[0].is_finite() || !stage_pos[1].is_finite() {
            return false;
        }
        let mut grid = self.active_perspective_grid().unwrap_or_default();
        grid.enabled = true;
        match handle {
            PerspectiveGridHandle::Horizon => {
                grid.horizon_y = stage_pos[1].clamp(-10_000.0, 10_000.0);
            }
            PerspectiveGridHandle::LeftVanishingPoint => {
                grid.left_vanishing_x = stage_pos[0]
                    .clamp(-20_000.0, 20_000.0)
                    .min(grid.right_vanishing_x - 20.0);
                grid.horizon_y = stage_pos[1].clamp(-10_000.0, 10_000.0);
            }
            PerspectiveGridHandle::RightVanishingPoint => {
                grid.right_vanishing_x = stage_pos[0]
                    .clamp(-20_000.0, 20_000.0)
                    .max(grid.left_vanishing_x + 20.0);
                grid.horizon_y = stage_pos[1].clamp(-10_000.0, 10_000.0);
            }
        }
        self.apply_timeline_perspective_grid(Some(grid))
    }

    /// Hit-test a Stage-space position against the active Perspective Grid vanishing/horizon
    /// handles (`T-233`).
    pub fn hit_test_perspective_grid_handle(
        &self,
        stage_pos: [f64; 2],
        tolerance: f64,
    ) -> Option<PerspectiveGridHandle> {
        let grid = self.active_perspective_grid()?;
        if !grid.enabled {
            return None;
        }
        let tol = tolerance.max(2.0);
        let left_d2 = (stage_pos[0] - grid.left_vanishing_x).powi(2)
            + (stage_pos[1] - grid.horizon_y).powi(2);
        if left_d2 <= tol * tol {
            return Some(PerspectiveGridHandle::LeftVanishingPoint);
        }
        let right_d2 = (stage_pos[0] - grid.right_vanishing_x).powi(2)
            + (stage_pos[1] - grid.horizon_y).powi(2);
        if right_d2 <= tol * tol {
            return Some(PerspectiveGridHandle::RightVanishingPoint);
        }
        if (stage_pos[1] - grid.horizon_y).abs() <= tol {
            return Some(PerspectiveGridHandle::Horizon);
        }
        None
    }
}

fn bernstein3(index: usize, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let u = 1.0 - t;
    match index {
        0 => u * u * u,
        1 => 3.0 * t * u * u,
        2 => 3.0 * t * t * u,
        3 => t * t * t,
        _ => 0.0,
    }
}

fn default_envelope_4x4_handles(bounds: [f64; 4]) -> [[f64; 2]; 16] {
    let [min_x, min_y, max_x, max_y] = bounds;
    let w = (max_x - min_x).max(1.0);
    let h = (max_y - min_y).max(1.0);
    let mut out = [[0.0; 2]; 16];
    for r in 0..4 {
        for c in 0..4 {
            out[r * 4 + c] = [min_x + (c as f64 / 3.0) * w, min_y + (r as f64 / 3.0) * h];
        }
    }
    out
}

fn default_perspective_2x2_corners(bounds: [f64; 4]) -> [[f64; 2]; 4] {
    let [min_x, min_y, max_x, max_y] = bounds;
    [
        [min_x, min_y],
        [max_x, min_y],
        [max_x, max_y],
        [min_x, max_y],
    ]
}

fn evaluate_envelope_4x4_ffd(
    pt: [f64; 2],
    baseline_bounds: [f64; 4],
    handles: &[[f64; 2]; 16],
) -> [f64; 2] {
    let [min_x, min_y, max_x, max_y] = baseline_bounds;
    let w = (max_x - min_x).max(1.0);
    let h = (max_y - min_y).max(1.0);
    let u = ((pt[0] - min_x) / w).clamp(0.0, 1.0);
    let v = ((pt[1] - min_y) / h).clamp(0.0, 1.0);
    let rest_handles = default_envelope_4x4_handles(baseline_bounds);
    let mut dx = 0.0;
    let mut dy = 0.0;
    for r in 0..4 {
        let bv = bernstein3(r, v);
        for c in 0..4 {
            let weight = bv * bernstein3(c, u);
            let idx = r * 4 + c;
            dx += weight * (handles[idx][0] - rest_handles[idx][0]);
            dy += weight * (handles[idx][1] - rest_handles[idx][1]);
        }
    }
    [pt[0] + dx, pt[1] + dy]
}

fn evaluate_perspective_quad_map(
    pt: [f64; 2],
    baseline_bounds: [f64; 4],
    corners: &[[f64; 2]; 4],
) -> [f64; 2] {
    let [min_x, min_y, max_x, max_y] = baseline_bounds;
    let w = (max_x - min_x).max(1.0);
    let h = (max_y - min_y).max(1.0);
    let u = (pt[0] - min_x) / w;
    let v = (pt[1] - min_y) / h;
    let top_x = corners[0][0] + (corners[1][0] - corners[0][0]) * u;
    let top_y = corners[0][1] + (corners[1][1] - corners[0][1]) * u;
    let bot_x = corners[3][0] + (corners[2][0] - corners[3][0]) * u;
    let bot_y = corners[3][1] + (corners[2][1] - corners[3][1]) * u;
    [top_x + (bot_x - top_x) * v, top_y + (bot_y - top_y) * v]
}

fn subdivide_cubic_commands(commands: &[PathCommand], splits: usize) -> Vec<PathCommand> {
    let splits = splits.max(1);
    let mut out = Vec::with_capacity(commands.len() * splits);
    let mut cur = [0.0_f64; 2];
    let mut start = [0.0_f64; 2];
    let split_once = |p0: [f64; 2],
                      p1: [f64; 2],
                      p2: [f64; 2],
                      p3: [f64; 2],
                      t: f64|
     -> ([[f64; 2]; 4], [[f64; 2]; 4]) {
        let lerp =
            |a: [f64; 2], b: [f64; 2], s: f64| [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s];
        let p01 = lerp(p0, p1, t);
        let p12 = lerp(p1, p2, t);
        let p23 = lerp(p2, p3, t);
        let p012 = lerp(p01, p12, t);
        let p123 = lerp(p12, p23, t);
        let p0123 = lerp(p012, p123, t);
        ([p0, p01, p012, p0123], [p0123, p123, p23, p3])
    };
    let append_subdivided = |out: &mut Vec<PathCommand>,
                             p0: [f64; 2],
                             p1: [f64; 2],
                             p2: [f64; 2],
                             p3: [f64; 2],
                             n: usize| {
        let mut rem_p0 = p0;
        let mut rem_p1 = p1;
        let mut rem_p2 = p2;
        let mut rem_p3 = p3;
        for i in 0..n {
            let rem = n - i;
            if rem <= 1 {
                out.push(PathCommand::CubicTo {
                    cx1: rem_p1[0],
                    cy1: rem_p1[1],
                    cx2: rem_p2[0],
                    cy2: rem_p2[1],
                    x: rem_p3[0],
                    y: rem_p3[1],
                });
                break;
            }
            let t = 1.0 / (rem as f64);
            let (left, right) = split_once(rem_p0, rem_p1, rem_p2, rem_p3, t);
            out.push(PathCommand::CubicTo {
                cx1: left[1][0],
                cy1: left[1][1],
                cx2: left[2][0],
                cy2: left[2][1],
                x: left[3][0],
                y: left[3][1],
            });
            rem_p0 = right[0];
            rem_p1 = right[1];
            rem_p2 = right[2];
            rem_p3 = right[3];
        }
    };
    for &cmd in commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                cur = [x, y];
                start = [x, y];
                out.push(cmd);
            }
            PathCommand::LineTo { x, y } => {
                let p0 = cur;
                let p3 = [x, y];
                let p1 = [p0[0] + (p3[0] - p0[0]) / 3.0, p0[1] + (p3[1] - p0[1]) / 3.0];
                let p2 = [
                    p0[0] + (p3[0] - p0[0]) * 2.0 / 3.0,
                    p0[1] + (p3[1] - p0[1]) * 2.0 / 3.0,
                ];
                append_subdivided(&mut out, p0, p1, p2, p3, splits);
                cur = p3;
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let p0 = cur;
                let p3 = [x, y];
                let p1 = [
                    p0[0] + (cx - p0[0]) * 2.0 / 3.0,
                    p0[1] + (cy - p0[1]) * 2.0 / 3.0,
                ];
                let p2 = [
                    p3[0] + (cx - p3[0]) * 2.0 / 3.0,
                    p3[1] + (cy - p3[1]) * 2.0 / 3.0,
                ];
                append_subdivided(&mut out, p0, p1, p2, p3, splits);
                cur = p3;
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                append_subdivided(&mut out, cur, [cx1, cy1], [cx2, cy2], [x, y], splits);
                cur = [x, y];
            }
            PathCommand::Close => {
                if (cur[0] - start[0]).hypot(cur[1] - start[1]) > 1e-4 {
                    let p0 = cur;
                    let p3 = start;
                    let p1 = [p0[0] + (p3[0] - p0[0]) / 3.0, p0[1] + (p3[1] - p0[1]) / 3.0];
                    let p2 = [
                        p0[0] + (p3[0] - p0[0]) * 2.0 / 3.0,
                        p0[1] + (p3[1] - p0[1]) * 2.0 / 3.0,
                    ];
                    append_subdivided(&mut out, p0, p1, p2, p3, splits);
                    cur = start;
                }
                out.push(PathCommand::Close);
            }
        }
    }
    out
}

fn map_element_stage_points(
    base_element: &Element,
    subdivide: usize,
    map_stage: impl Fn([f64; 2]) -> [f64; 2],
) -> Option<Element> {
    let raw_cmds = editable_element_commands(base_element)
        .or_else(|| base_element.to_path_commands().map(|(c, _)| c))?;
    let commands = if subdivide > 1 && raw_cmds.len() <= 16 {
        subdivide_cubic_commands(&raw_cmds, subdivide)
    } else {
        subdivide_cubic_commands(&raw_cmds, 1)
    };
    let mat = transform_matrix(base_element.transform);
    let to_local = |stage_pt: [f64; 2]| -> [f64; 2] {
        if let Some((lx, ly)) = inverse_f64(mat, stage_pt[0], stage_pt[1]) {
            [lx, ly]
        } else {
            [
                stage_pt[0] - base_element.transform.x,
                stage_pt[1] - base_element.transform.y,
            ]
        }
    };
    let map_local = |lx: f64, ly: f64| -> [f64; 2] {
        let sx = mat[0] * lx + mat[2] * ly + mat[4];
        let sy = mat[1] * lx + mat[3] * ly + mat[5];
        to_local(map_stage([sx, sy]))
    };
    let mut warped = Vec::with_capacity(commands.len());
    for cmd in commands {
        let mapped = match cmd {
            PathCommand::MoveTo { x, y } => {
                let [nx, ny] = map_local(x, y);
                PathCommand::MoveTo { x: nx, y: ny }
            }
            PathCommand::LineTo { x, y } => {
                let [nx, ny] = map_local(x, y);
                PathCommand::LineTo { x: nx, y: ny }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let [ncx, ncy] = map_local(cx, cy);
                let [nx, ny] = map_local(x, y);
                PathCommand::QuadraticTo {
                    cx: ncx,
                    cy: ncy,
                    x: nx,
                    y: ny,
                }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let [nc1x, nc1y] = map_local(cx1, cy1);
                let [nc2x, nc2y] = map_local(cx2, cy2);
                let [nx, ny] = map_local(x, y);
                PathCommand::CubicTo {
                    cx1: nc1x,
                    cy1: nc1y,
                    cx2: nc2x,
                    cy2: nc2y,
                    x: nx,
                    y: ny,
                }
            }
            PathCommand::Close => PathCommand::Close,
        };
        warped.push(mapped);
    }
    let mut updated = base_element.clone();
    write_editable_element_commands(&mut updated, warped);
    Some(updated)
}

fn element_stage_bounds(el: &Element) -> Option<[f64; 4]> {
    let (cmds, _) = el.to_path_commands()?;
    let mat = transform_matrix(el.transform);
    let stage_cmds = map_commands_forward(&cmds, mat);
    let b = neoflash_core::path_bounds(&stage_cmds).ok().flatten()?;
    Some([b.x_min, b.y_min, b.x_max, b.y_max])
}

fn elements_hash(elements: &[Element]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for el in elements {
        el.id.hash(&mut hasher);
        el.transform.x.to_bits().hash(&mut hasher);
        el.transform.y.to_bits().hash(&mut hasher);
        el.transform.scale_x.to_bits().hash(&mut hasher);
        el.transform.scale_y.to_bits().hash(&mut hasher);
        el.transform.rotation.to_bits().hash(&mut hasher);
        el.transform.skew.to_bits().hash(&mut hasher);
        if let Some((cmds, _)) = el.to_path_commands() {
            cmds.len().hash(&mut hasher);
            for cmd in cmds {
                match cmd {
                    PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                        x.to_bits().hash(&mut hasher);
                        y.to_bits().hash(&mut hasher);
                    }
                    PathCommand::QuadraticTo { cx, cy, x, y } => {
                        cx.to_bits().hash(&mut hasher);
                        cy.to_bits().hash(&mut hasher);
                        x.to_bits().hash(&mut hasher);
                        y.to_bits().hash(&mut hasher);
                    }
                    PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x,
                        y,
                    } => {
                        cx1.to_bits().hash(&mut hasher);
                        cy1.to_bits().hash(&mut hasher);
                        cx2.to_bits().hash(&mut hasher);
                        cy2.to_bits().hash(&mut hasher);
                        x.to_bits().hash(&mut hasher);
                        y.to_bits().hash(&mut hasher);
                    }
                    PathCommand::Close => 99_u8.hash(&mut hasher),
                }
            }
        }
    }
    hasher.finish()
}

fn deform_stage_point_with_asset_warp_pins(
    point: [f64; 2],
    pins: &[WarpPin],
    falloff_radius: f64,
) -> [f64; 2] {
    if pins.is_empty() {
        return point;
    }
    if pins.len() == 1 {
        let pin = &pins[0];
        let dx = point[0] - pin.rest_pos[0];
        let dy = point[1] - pin.rest_pos[1];
        let d2 = dx * dx + dy * dy;
        let r2 = falloff_radius.max(20.0).powi(2);
        let w = 1.0 / (1.0 + d2 / r2);
        let (rx, ry) = if pin.rotation.abs() > 1e-6 && pin.kind != WarpPinKind::Fixed {
            let rad = pin.rotation.to_radians();
            let c = rad.cos();
            let s = rad.sin();
            (dx * c - dy * s, dx * s + dy * c)
        } else {
            (dx, dy)
        };
        let target_x = pin.pos[0] + rx;
        let target_y = pin.pos[1] + ry;
        return [
            point[0] + (target_x - point[0]) * w,
            point[1] + (target_y - point[1]) * w,
        ];
    }
    let mut sum_w = 0.0;
    let mut acc_x = 0.0;
    let mut acc_y = 0.0;
    for pin in pins {
        let dx = point[0] - pin.rest_pos[0];
        let dy = point[1] - pin.rest_pos[1];
        let d2 = dx * dx + dy * dy;
        let w = pin.stiffness.clamp(0.05, 1.0) / (d2 + 16.0);
        let (rx, ry) = if pin.rotation.abs() > 1e-6 && pin.kind != WarpPinKind::Fixed {
            let rad = pin.rotation.to_radians();
            let c = rad.cos();
            let s = rad.sin();
            (dx * c - dy * s, dx * s + dy * c)
        } else {
            (dx, dy)
        };
        let target_pos = if pin.kind == WarpPinKind::Fixed {
            pin.rest_pos
        } else {
            pin.pos
        };
        acc_x += (target_pos[0] + rx) * w;
        acc_y += (target_pos[1] + ry) * w;
        sum_w += w;
    }
    if sum_w > 1e-9 {
        [acc_x / sum_w, acc_y / sum_w]
    } else {
        point
    }
}

impl Editor {
    /// Return the active Free Transform submode (`ScaleRotateSkew`, `Envelope`, `Perspective`, `Distort`) (`T-271`).
    pub fn free_transform_submode(&self) -> FreeTransformSubmode {
        self.vector.free_transform_submode
    }

    /// Switch the active Free Transform submode and synchronize `FreeTransformMode` (`T-271`).
    #[allow(dead_code)]
    pub fn set_free_transform_submode(&mut self, submode: FreeTransformSubmode) {
        self.vector.free_transform_submode = submode;
        self.authoring_tools.transform_mode = match submode {
            FreeTransformSubmode::ScaleRotateSkew => {
                crate::authoring_tools::FreeTransformMode::FreeTransform
            }
            FreeTransformSubmode::Envelope => crate::authoring_tools::FreeTransformMode::Envelope,
            FreeTransformSubmode::Perspective | FreeTransformSubmode::Distort => {
                crate::authoring_tools::FreeTransformMode::Distort
            }
        };
    }

    fn selected_vector_elements_on_active_key(&self) -> Vec<Element> {
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return Vec::new();
        };
        let Some(key) = layer.key_at(self.frame) else {
            return Vec::new();
        };
        let mut ids: Vec<Id> = self
            .selection
            .targets
            .iter()
            .filter(|t| t.layer == layer.id)
            .map(|t| t.element.clone())
            .collect();
        if ids.is_empty()
            && let Some(el) = self.selected_element()
        {
            ids.push(el.id.clone());
        }
        if ids.is_empty() && key.elements.len() == 1 {
            ids.push(key.elements[0].id.clone());
        }
        key.elements
            .iter()
            .filter(|el| {
                ids.contains(&el.id)
                    && !key.controller_elements.contains(&el.id)
                    && el.to_path_commands().is_some()
            })
            .cloned()
            .collect()
    }

    pub(crate) fn selected_vector_bounds_on_stage(&self) -> Option<[f64; 4]> {
        let els = self.selected_vector_elements_on_active_key();
        if els.is_empty() {
            return None;
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for el in &els {
            if let Some([x0, y0, x1, y1]) = element_stage_bounds(el) {
                min_x = min_x.min(x0);
                min_y = min_y.min(y0);
                max_x = max_x.max(x1);
                max_y = max_y.max(y1);
            }
        }
        (min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite())
            .then_some([min_x, min_y, max_x, max_y])
    }

    /// Return the 16 (`4x4`) interactive `Envelope` lattice handles `(row, col, [stage_x, stage_y])`
    /// for the selected vector shape(s) on Stage (`T-271`).
    pub fn envelope_warp_handles(&self) -> Vec<(usize, usize, [f64; 2])> {
        let els = self.selected_vector_elements_on_active_key();
        if els.is_empty() {
            return Vec::new();
        }
        let cur_hash = elements_hash(&els);
        let el_ids: Vec<Id> = els.iter().map(|e| e.id.clone()).collect();
        let handles_16 = if let Some(session) = &self.vector.envelope_lattice_session
            && session.symbol_id == self.symbol_id()
            && session.layer_idx == self.layer
            && session.frame == self.frame
            && session.element_ids == el_ids
            && let Some(saved) = session.handles_by_hash.get(&cur_hash)
        {
            *saved
        } else if let Some(bounds) = self.selected_vector_bounds_on_stage() {
            default_envelope_4x4_handles(bounds)
        } else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(16);
        for r in 0..4 {
            for c in 0..4 {
                out.push((r, c, handles_16[r * 4 + c]));
            }
        }
        out
    }

    /// Hit-test a Stage-space point against the 16 (`4x4`) `Envelope` lattice handles (`T-271`).
    pub fn hit_test_envelope_warp_handle(
        &self,
        stage_pt: [f64; 2],
        tolerance: f64,
    ) -> Option<(usize, usize)> {
        let tol_sq = tolerance.max(1.5).powi(2);
        self.envelope_warp_handles()
            .into_iter()
            .filter_map(|(r, c, pos)| {
                let d2 = (pos[0] - stage_pt[0]).powi(2) + (pos[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((r, c, d2))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(r, c, _)| (r, c))
    }

    pub(crate) fn apply_envelope_warp_handle_in_place(
        &mut self,
        row: usize,
        col: usize,
        target_stage: [f64; 2],
    ) -> bool {
        if row >= 4 || col >= 4 || !target_stage[0].is_finite() || !target_stage[1].is_finite() {
            return false;
        }
        let els = self.selected_vector_elements_on_active_key();
        if els.is_empty() {
            return false;
        }
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let el_ids: Vec<Id> = els.iter().map(|e| e.id.clone()).collect();
        let cur_hash = elements_hash(&els);

        let needs_init = self
            .vector
            .envelope_lattice_session
            .as_ref()
            .is_none_or(|s| {
                s.symbol_id != symbol_id
                    || s.layer_idx != layer_idx
                    || s.frame != frame
                    || s.element_ids != el_ids
                    || !s.handles_by_hash.contains_key(&cur_hash)
            });
        if needs_init {
            let Some(bounds) = self.selected_vector_bounds_on_stage() else {
                return false;
            };
            let initial_handles = default_envelope_4x4_handles(bounds);
            let mut handles_by_hash = HashMap::new();
            handles_by_hash.insert(cur_hash, initial_handles);
            self.vector.envelope_lattice_session = Some(EnvelopeLatticeSession {
                symbol_id: symbol_id.clone(),
                layer_idx,
                frame,
                element_ids: el_ids.clone(),
                baseline_bounds: bounds,
                baseline_elements: els.iter().map(|e| (e.id.clone(), e.clone())).collect(),
                handles_by_hash,
            });
        }

        let Some(session) = self.vector.envelope_lattice_session.as_ref() else {
            return false;
        };
        let baseline_bounds = session.baseline_bounds;
        let baseline_elements = session.baseline_elements.clone();
        let mut handles = session
            .handles_by_hash
            .get(&cur_hash)
            .copied()
            .unwrap_or_else(|| default_envelope_4x4_handles(baseline_bounds));
        let idx = row * 4 + col;
        if (handles[idx][0] - target_stage[0]).hypot(handles[idx][1] - target_stage[1]) <= 1e-6 {
            return false;
        }
        handles[idx] = target_stage;

        let bw = (baseline_bounds[2] - baseline_bounds[0]).max(1.0);
        let bh = (baseline_bounds[3] - baseline_bounds[1]).max(1.0);
        let uv_points: Vec<[f64; 2]> = handles
            .iter()
            .map(|[sx, sy]| {
                [
                    ((sx - baseline_bounds[0]) / bw).clamp(-4.0, 5.0),
                    ((sy - baseline_bounds[1]) / bh).clamp(-4.0, 5.0),
                ]
            })
            .collect();

        let symbol = self.symbol_mut();
        let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
            return false;
        };
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
            return false;
        };
        let mut updated_els = Vec::new();
        for (id, base_el) in &baseline_elements {
            let Some(warped_el) = map_element_stage_points(base_el, 4, |pt| {
                evaluate_envelope_4x4_ffd(pt, baseline_bounds, &handles)
            }) else {
                continue;
            };
            if let Some(slot) = key.elements.iter_mut().find(|e| &e.id == id) {
                *slot = warped_el.clone();
                updated_els.push(warped_el);
            }
            let mut fx = key
                .vector_effects(id)
                .cloned()
                .unwrap_or(ElementVectorEffects {
                    element_id: id.clone(),
                    ..Default::default()
                });
            fx.envelope_warp = Some(EnvelopeWarp {
                preset: EnvelopeWarpPreset::CustomGrid,
                rows: 3,
                cols: 3,
                control_points: uv_points.clone(),
                ..EnvelopeWarp::default()
            });
            key.set_vector_effects(fx);
        }
        if updated_els.is_empty() {
            return false;
        }
        let new_hash = elements_hash(&updated_els);
        if let Some(session) = self.vector.envelope_lattice_session.as_mut() {
            session.handles_by_hash.insert(new_hash, handles);
        }
        true
    }

    /// Move a `(row, col)` handle of the `4x4` `Envelope` lattice to `target_stage` in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn move_envelope_warp_handle(
        &mut self,
        row: usize,
        col: usize,
        target_stage: [f64; 2],
    ) -> bool {
        self.history.begin(&self.project);
        if !self.apply_envelope_warp_handle_in_place(row, col, target_stage) {
            return false;
        }
        self.commit_history();
        true
    }

    /// Return the 4 (`2x2`) interactive `Perspective` corner handles `[TL, TR, BR, BL]` in Stage
    /// coordinates for the selected vector shape(s) (`T-271`).
    pub fn perspective_corners(&self) -> Option<[[f64; 2]; 4]> {
        let els = self.selected_vector_elements_on_active_key();
        if els.is_empty() {
            return None;
        }
        let cur_hash = elements_hash(&els);
        let el_ids: Vec<Id> = els.iter().map(|e| e.id.clone()).collect();
        if let Some(session) = &self.vector.perspective_quad_session
            && session.symbol_id == self.symbol_id()
            && session.layer_idx == self.layer
            && session.frame == self.frame
            && session.element_ids == el_ids
            && let Some(saved) = session.corners_by_hash.get(&cur_hash)
        {
            return Some(*saved);
        }
        let bounds = self.selected_vector_bounds_on_stage()?;
        Some(default_perspective_2x2_corners(bounds))
    }

    /// Hit-test a Stage-space point against the 4 (`2x2`) `Perspective` corner handles (`0..4`) (`T-271`).
    pub fn hit_test_perspective_corner(&self, stage_pt: [f64; 2], tolerance: f64) -> Option<usize> {
        let corners = self.perspective_corners()?;
        let tol_sq = tolerance.max(1.5).powi(2);
        corners
            .iter()
            .enumerate()
            .filter_map(|(i, pos)| {
                let d2 = (pos[0] - stage_pt[0]).powi(2) + (pos[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((i, d2))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
    }

    pub(crate) fn apply_perspective_corner_in_place(
        &mut self,
        corner: usize,
        target_stage: [f64; 2],
        symmetric_taper: bool,
    ) -> bool {
        if corner >= 4 || !target_stage[0].is_finite() || !target_stage[1].is_finite() {
            return false;
        }
        let els = self.selected_vector_elements_on_active_key();
        if els.is_empty() {
            return false;
        }
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let el_ids: Vec<Id> = els.iter().map(|e| e.id.clone()).collect();
        let cur_hash = elements_hash(&els);

        let needs_init = self
            .vector
            .perspective_quad_session
            .as_ref()
            .is_none_or(|s| {
                s.symbol_id != symbol_id
                    || s.layer_idx != layer_idx
                    || s.frame != frame
                    || s.element_ids != el_ids
                    || !s.corners_by_hash.contains_key(&cur_hash)
            });
        if needs_init {
            let Some(bounds) = self.selected_vector_bounds_on_stage() else {
                return false;
            };
            let initial_corners = default_perspective_2x2_corners(bounds);
            let mut corners_by_hash = HashMap::new();
            corners_by_hash.insert(cur_hash, initial_corners);
            self.vector.perspective_quad_session = Some(PerspectiveQuadSession {
                symbol_id: symbol_id.clone(),
                layer_idx,
                frame,
                element_ids: el_ids.clone(),
                baseline_bounds: bounds,
                baseline_elements: els.iter().map(|e| (e.id.clone(), e.clone())).collect(),
                corners_by_hash,
            });
        }

        let Some(session) = self.vector.perspective_quad_session.as_ref() else {
            return false;
        };
        let baseline_bounds = session.baseline_bounds;
        let baseline_elements = session.baseline_elements.clone();
        let mut corners = session
            .corners_by_hash
            .get(&cur_hash)
            .copied()
            .unwrap_or_else(|| default_perspective_2x2_corners(baseline_bounds));
        if (corners[corner][0] - target_stage[0]).hypot(corners[corner][1] - target_stage[1])
            <= 1e-6
        {
            return false;
        }
        let prev = corners[corner];
        corners[corner] = target_stage;
        if symmetric_taper {
            let dx = target_stage[0] - prev[0];
            let dy = target_stage[1] - prev[1];
            let horiz_mirror = match corner {
                0 => 1,
                1 => 0,
                2 => 3,
                _ => 2,
            };
            let vert_mirror = match corner {
                0 => 3,
                1 => 2,
                2 => 1,
                _ => 0,
            };
            if dx.abs() >= dy.abs() {
                corners[horiz_mirror][0] -= dx;
                corners[horiz_mirror][1] += dy;
            } else {
                corners[vert_mirror][0] += dx;
                corners[vert_mirror][1] -= dy;
            }
        }

        let symbol = self.symbol_mut();
        let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
            return false;
        };
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
            return false;
        };
        let mut updated_els = Vec::new();
        for (id, base_el) in &baseline_elements {
            let Some(warped_el) = map_element_stage_points(base_el, 2, |pt| {
                evaluate_perspective_quad_map(pt, baseline_bounds, &corners)
            }) else {
                continue;
            };
            if let Some(slot) = key.elements.iter_mut().find(|e| &e.id == id) {
                *slot = warped_el.clone();
                updated_els.push(warped_el);
            }
        }
        if updated_els.is_empty() {
            return false;
        }
        let new_hash = elements_hash(&updated_els);
        if let Some(session) = self.vector.perspective_quad_session.as_mut() {
            session.corners_by_hash.insert(new_hash, corners);
        }
        true
    }

    /// Move corner `corner` (`0..4`: TL, TR, BR, BL) of the `2x2` `Perspective` quad to `target_stage`
    /// in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn move_perspective_corner(&mut self, corner: usize, target_stage: [f64; 2]) -> bool {
        self.history.begin(&self.project);
        if !self.apply_perspective_corner_in_place(corner, target_stage, false) {
            return false;
        }
        self.commit_history();
        true
    }

    fn active_asset_warp_element_id(&self) -> Option<Id> {
        if let Some(el) = self.selected_element() {
            return Some(el.id.clone());
        }
        if let Some(target) = self.selection.targets.first() {
            return Some(target.element.clone());
        }
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let key = layer.key_at(self.frame)?;
        key.elements
            .iter()
            .find(|e| !key.controller_elements.contains(&e.id))
            .map(|e| e.id.clone())
    }

    /// Return the Stage-space `WarpPin`s `(pin_id, [stage_x, stage_y], kind)` for the selected
    /// element (`T-271`).
    pub fn asset_warp_stage_pins(&self) -> Vec<(Id, [f64; 2], WarpPinKind)> {
        let Some(target_id) = self.active_asset_warp_element_id() else {
            return Vec::new();
        };
        if let Some(layer) = self.symbol().timeline.layers.get(self.layer)
            && let Some(key) = layer.key_at(self.frame)
            && let Some(rig) = key.asset_warp(&target_id)
        {
            return rig
                .pins
                .iter()
                .map(|p| (p.id.clone(), p.pos, p.kind))
                .collect();
        }
        Vec::new()
    }

    /// Hit-test a Stage-space point against the active `WarpPin` handles (`T-271`).
    pub fn hit_test_asset_warp_pin(&self, stage_pt: [f64; 2], tolerance: f64) -> Option<Id> {
        let tol_sq = tolerance.max(1.5).powi(2);
        self.asset_warp_stage_pins()
            .into_iter()
            .filter_map(|(id, pos, _)| {
                let d2 = (pos[0] - stage_pt[0]).powi(2) + (pos[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((id, d2))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, _)| id)
    }

    /// Re-evaluate the mesh deformation of `element_id` from its undeformed base shape and
    /// active `WarpPin`s (`T-271`).
    pub fn deform_shape_with_warp_pins(&mut self, element_id: &str) -> bool {
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let pins: Vec<WarpPin> = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .and_then(|k| k.asset_warp(element_id))
            .map(|r| r.pins.clone())
            .unwrap_or_default();

        let current_el = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .and_then(|k| k.elements.iter().find(|e| e.id == element_id))
            .cloned();
        let Some(current_el) = current_el else {
            return false;
        };
        let base_key = (symbol_id, frame, element_id.to_owned());
        let base_el = self
            .vector
            .asset_warp_base_elements
            .entry(base_key)
            .or_insert_with(|| current_el.clone())
            .clone();

        let warped_el = if pins.is_empty() {
            base_el
        } else {
            let radius = element_stage_bounds(&base_el)
                .map(|[x0, y0, x1, y1]| (x1 - x0).max(y1 - y0).max(40.0) * 0.75)
                .unwrap_or(100.0);
            let Some(el) = map_element_stage_points(&base_el, 4, |pt| {
                deform_stage_point_with_asset_warp_pins(pt, &pins, radius)
            }) else {
                return false;
            };
            el
        };

        let symbol = self.symbol_mut();
        let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
            return false;
        };
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
            return false;
        };
        if let Some(slot) = key.elements.iter_mut().find(|e| e.id == element_id) {
            *slot = warped_el;
            return true;
        }
        false
    }

    pub(crate) fn add_asset_warp_pin_in_place(&mut self, stage_pt: [f64; 2]) -> Option<Id> {
        if !stage_pt[0].is_finite() || !stage_pt[1].is_finite() {
            return None;
        }
        let target_id = self.active_asset_warp_element_id()?;
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        if let Some(el) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .and_then(|k| k.elements.iter().find(|e| e.id == target_id))
            .cloned()
        {
            let has_existing_pins = self
                .symbol()
                .timeline
                .layers
                .get(layer_idx)
                .and_then(|l| l.key_at(frame))
                .and_then(|k| k.asset_warp(&target_id))
                .is_some_and(|r| !r.pins.is_empty());
            if !has_existing_pins {
                self.vector
                    .asset_warp_base_elements
                    .insert((symbol_id, frame, target_id.clone()), el);
            } else {
                self.vector
                    .asset_warp_base_elements
                    .entry((symbol_id, frame, target_id.clone()))
                    .or_insert(el);
            }
        }

        let pin_id = new_id("wpin");
        let new_pin = WarpPin {
            id: pin_id.clone(),
            kind: WarpPinKind::Open,
            rest_pos: stage_pt,
            pos: stage_pt,
            rotation: 0.0,
            stiffness: 1.0,
        };
        let symbol = self.symbol_mut();
        let layer = symbol.timeline.layers.get_mut(layer_idx)?;
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let key = layer.keyframes.iter_mut().find(|k| k.frame == key_frame)?;
        if let Some(rig) = key.asset_warp_mut(&target_id) {
            if rig.pins.len() >= WarpPin::MAX_PINS {
                return None;
            }
            rig.pins.push(new_pin);
        } else {
            key.set_asset_warp(AssetWarpRig {
                element_id: target_id,
                pins: vec![new_pin],
                envelop_subdivisions: 4,
            });
        }
        self.vector.selected_warp_pin = Some(pin_id.clone());
        Some(pin_id)
    }

    /// Add a new `WarpPin` at `stage_pt` on the selected shape/element in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn add_asset_warp_pin(&mut self, stage_pt: [f64; 2]) -> Option<Id> {
        self.history.begin(&self.project);
        let pin_id = self.add_asset_warp_pin_in_place(stage_pt)?;
        self.commit_history();
        Some(pin_id)
    }

    pub(crate) fn apply_asset_warp_pin_in_place(
        &mut self,
        pin_id: &str,
        stage_pt: [f64; 2],
    ) -> bool {
        if !stage_pt[0].is_finite() || !stage_pt[1].is_finite() {
            return false;
        }
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let mut target_element_id: Option<Id> = None;

        if let Some(el_id) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .and_then(|k| {
                k.asset_warps
                    .iter()
                    .find(|r| r.pins.iter().any(|p| p.id == pin_id))
                    .map(|r| r.element_id.clone())
            })
            && let Some(el) = self
                .symbol()
                .timeline
                .layers
                .get(layer_idx)
                .and_then(|l| l.key_at(frame))
                .and_then(|k| k.elements.iter().find(|e| e.id == el_id))
                .cloned()
        {
            self.vector
                .asset_warp_base_elements
                .entry((symbol_id, frame, el_id))
                .or_insert(el);
        }

        let mut moved = false;
        {
            let symbol = self.symbol_mut();
            if let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) {
                let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
                if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                    for rig in &mut key.asset_warps {
                        if let Some(pin) = rig.pins.iter_mut().find(|p| p.id == pin_id) {
                            if (pin.pos[0] - stage_pt[0]).hypot(pin.pos[1] - stage_pt[1]) > 1e-6 {
                                pin.pos = stage_pt;
                                moved = true;
                            }
                            target_element_id = Some(rig.element_id.clone());
                        }
                    }
                }
            }
        }
        if !moved {
            return false;
        }
        self.vector.selected_warp_pin = Some(pin_id.to_owned());
        if let Some(el_id) = target_element_id {
            let _ = self.deform_shape_with_warp_pins(&el_id);
        }
        true
    }

    /// Move an existing `WarpPin` to `stage_pt` and deform the bound shape in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn move_asset_warp_pin(&mut self, pin_id: &str, stage_pt: [f64; 2]) -> bool {
        self.history.begin(&self.project);
        if !self.apply_asset_warp_pin_in_place(pin_id, stage_pt) {
            return false;
        }
        self.commit_history();
        true
    }

    /// Remove `pin_id` from the active `AssetWarpRig` and update the shape mesh in 1 Undo step (`T-271`).
    pub fn remove_asset_warp_pin(&mut self, pin_id: &str) -> bool {
        self.history.begin(&self.project);
        let layer_idx = self.layer;
        let frame = self.frame;
        let mut target_element_id: Option<Id> = None;
        let mut removed = false;
        {
            let symbol = self.symbol_mut();
            if let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) {
                let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
                if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                    for rig in &mut key.asset_warps {
                        let before_len = rig.pins.len();
                        rig.pins.retain(|p| p.id != pin_id);
                        if rig.pins.len() != before_len {
                            removed = true;
                            target_element_id = Some(rig.element_id.clone());
                        }
                    }
                    key.asset_warps.retain(|r| !r.pins.is_empty());
                }
            }
        }
        if !removed {
            return false;
        }
        if self.vector.selected_warp_pin.as_deref() == Some(pin_id) {
            self.vector.selected_warp_pin = None;
        }
        if let Some(el_id) = target_element_id {
            let _ = self.deform_shape_with_warp_pins(&el_id);
        }
        self.commit_history();
        true
    }

    /// Return `(armature_id, bone_id, head_stage, tail_stage, world_angle_deg)` for all `IkBone`s
    /// on the active keyframe (`T-271`).
    #[allow(clippy::type_complexity)]
    pub fn ik_bone_stage_poses(&self) -> Vec<(Id, Id, [f64; 2], [f64; 2], f64)> {
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return Vec::new();
        };
        let Some(key) = layer.key_at(self.frame) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for arm in &key.ik_armatures {
            for (bone_id, head, tail, world_angle) in compute_bone_world_poses(arm, &key.elements) {
                out.push((arm.id.clone(), bone_id, head, tail, world_angle));
            }
        }
        out
    }

    /// Hit-test a Stage-space point against `IkBone` gizmos, returning
    /// `Some((armature_id, bone_id, is_tail_effector))` (`T-271`).
    pub fn hit_test_ik_bone_gizmo(
        &self,
        stage_pt: [f64; 2],
        tolerance: f64,
    ) -> Option<(Id, Id, bool)> {
        let tol = tolerance.max(2.0);
        let tol_sq = tol * tol;
        let poses = self.ik_bone_stage_poses();
        // 1. Prioritize bone tails / effectors
        if let Some((arm_id, bone_id, _)) = poses
            .iter()
            .filter_map(|(arm_id, bone_id, _, tail, _)| {
                let d2 = (tail[0] - stage_pt[0]).powi(2) + (tail[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((arm_id.clone(), bone_id.clone(), d2))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
        {
            return Some((arm_id, bone_id, true));
        }
        // 2. Next check bone heads / joints
        if let Some((arm_id, bone_id, _)) = poses
            .iter()
            .filter_map(|(arm_id, bone_id, head, _, _)| {
                let d2 = (head[0] - stage_pt[0]).powi(2) + (head[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((arm_id.clone(), bone_id.clone(), d2))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
        {
            return Some((arm_id, bone_id, false));
        }
        // 3. Check bone body segment distance
        poses
            .into_iter()
            .filter_map(|(arm_id, bone_id, head, tail, _)| {
                let vx = tail[0] - head[0];
                let vy = tail[1] - head[1];
                let len_sq = vx * vx + vy * vy;
                let t = if len_sq > 1e-9 {
                    (((stage_pt[0] - head[0]) * vx + (stage_pt[1] - head[1]) * vy) / len_sq)
                        .clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let proj = [head[0] + vx * t, head[1] + vy * t];
                let d2 = (proj[0] - stage_pt[0]).powi(2) + (proj[1] - stage_pt[1]).powi(2);
                (d2 <= tol_sq).then_some((arm_id, bone_id, t >= 0.5, d2))
            })
            .min_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(arm_id, bone_id, is_tail, _)| (arm_id, bone_id, is_tail))
    }

    /// Create a new `IkBone` between `start_stage` and `end_stage` in 1 Undo step (`T-271`).
    pub fn create_ik_bone_on_stage(
        &mut self,
        start_stage: [f64; 2],
        end_stage: [f64; 2],
    ) -> Option<(Id, Id)> {
        if !start_stage[0].is_finite()
            || !start_stage[1].is_finite()
            || !end_stage[0].is_finite()
            || !end_stage[1].is_finite()
        {
            return None;
        }
        let dx = end_stage[0] - start_stage[0];
        let dy = end_stage[1] - start_stage[1];
        let length = dx.hypot(dy).max(16.0);
        let world_angle = dy.atan2(dx).to_degrees();

        self.history.begin(&self.project);
        let parent_hit = self
            .ik_bone_stage_poses()
            .into_iter()
            .filter_map(|(arm_id, bone_id, _head, tail, _angle)| {
                let d = (tail[0] - start_stage[0]).hypot(tail[1] - start_stage[1]);
                (d <= 28.0).then_some((arm_id, bone_id, d))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

        let layer_idx = self.layer;
        let frame = self.frame;
        let bone_id = new_id("ikbone");
        let bone_el_id = new_id("ikjoint");

        let arm_id = {
            let symbol = self.symbol_mut();
            let layer = symbol.timeline.layers.get_mut(layer_idx)?;
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let key = layer.keyframes.iter_mut().find(|k| k.frame == key_frame)?;

            key.elements.push(Element {
                id: bone_el_id.clone(),
                name: format!("Bone {}", key.controller_elements.len() + 1),
                transform: Transform {
                    x: start_stage[0],
                    y: start_stage[1],
                    rotation: world_angle,
                    alpha: 0.0,
                    ..Transform::default()
                },
                kind: ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                        PathCommand::LineTo { x: 1.0, y: 0.0 },
                    ],
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            });
            key.controller_elements.push(bone_el_id.clone());

            if let Some((arm_id, parent_id, _)) = parent_hit
                && let Some(arm) = key.ik_armatures.iter_mut().find(|a| a.id == arm_id)
            {
                arm.bones.push(IkBone {
                    id: bone_id.clone(),
                    element_id: bone_el_id,
                    parent_bone_id: Some(parent_id),
                    length,
                    min_angle: None,
                    max_angle: None,
                    stiffness: 1.0,
                });
                arm.effector_bone_id = bone_id.clone();
                arm.target_pos = end_stage;
                arm_id
            } else if let Some(arm) = key.ik_armatures.first_mut() {
                let last_bone_id = arm.bones.last().map(|b| b.id.clone());
                arm.bones.push(IkBone {
                    id: bone_id.clone(),
                    element_id: bone_el_id,
                    parent_bone_id: last_bone_id,
                    length,
                    min_angle: None,
                    max_angle: None,
                    stiffness: 1.0,
                });
                arm.effector_bone_id = bone_id.clone();
                arm.target_pos = end_stage;
                arm.id.clone()
            } else {
                let new_arm_id = new_id("arm");
                key.ik_armatures.push(IkArmature {
                    id: new_arm_id.clone(),
                    name: "Armature 1".into(),
                    bones: vec![IkBone {
                        id: bone_id.clone(),
                        element_id: bone_el_id,
                        parent_bone_id: None,
                        length,
                        min_angle: None,
                        max_angle: None,
                        stiffness: 1.0,
                    }],
                    effector_bone_id: bone_id.clone(),
                    target_pos: end_stage,
                    pole_pos: None,
                });
                new_arm_id
            }
        };

        let poses: Vec<(Id, [f64; 2], [f64; 2], f64)> = self
            .ik_bone_stage_poses()
            .into_iter()
            .filter(|(a_id, _, _, _, _)| a_id == &arm_id)
            .map(|(_, b_id, h, t, ang)| (b_id, h, t, ang))
            .collect();
        let symbol_id = self.symbol_id().to_owned();
        for el in self.selected_vector_elements_on_active_key() {
            self.vector.ik_base_elements.insert(
                (symbol_id.clone(), frame, el.id.clone()),
                (el, poses.clone()),
            );
        }

        self.vector.selected_ik_bone = Some(bone_id.clone());
        self.commit_history();
        Some((arm_id, bone_id))
    }

    fn parse_armature_bound_vertices(arm_name: &str) -> HashMap<Id, Vec<(usize, Id, f64, f64)>> {
        let mut out: HashMap<Id, Vec<(usize, Id, f64, f64)>> = HashMap::new();
        let Some((_, meta)) = arm_name.split_once("|bind:") else {
            return out;
        };
        for entry in meta.split(';') {
            let parts: Vec<&str> = entry.split(',').collect();
            if parts.len() == 5
                && let Ok(v_idx) = parts[1].parse::<usize>()
                && let Ok(ox) = parts[3].parse::<f64>()
                && let Ok(oy) = parts[4].parse::<f64>()
            {
                out.entry(parts[0].to_owned()).or_default().push((
                    v_idx,
                    parts[2].to_owned(),
                    ox,
                    oy,
                ));
            }
        }
        out
    }

    fn encode_armature_bound_vertices(
        base_name: &str,
        bindings: &HashMap<Id, Vec<(usize, Id, f64, f64)>>,
    ) -> String {
        let clean_base = base_name
            .split_once("|bind:")
            .map(|(b, _)| b)
            .unwrap_or(base_name);
        let mut entries = Vec::new();
        let mut shape_ids: Vec<&Id> = bindings.keys().collect();
        shape_ids.sort();
        for shape_id in shape_ids {
            if let Some(list) = bindings.get(shape_id) {
                for (v_idx, bone_id, ox, oy) in list {
                    entries.push(format!("{shape_id},{v_idx},{bone_id},{ox:.4},{oy:.4}"));
                }
            }
        }
        if entries.is_empty() {
            clean_base.to_owned()
        } else {
            format!("{clean_base}|bind:{}", entries.join(";"))
        }
    }

    fn apply_ik_pose_to_scene_in_place(&mut self, armature_id: &str) {
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let Some(key_snapshot) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .cloned()
        else {
            return;
        };
        let Some(arm) = key_snapshot
            .ik_armatures
            .iter()
            .find(|a| a.id == armature_id)
            .cloned()
        else {
            return;
        };
        let current_poses = compute_bone_world_poses(&arm, &key_snapshot.elements);
        if current_poses.is_empty() {
            return;
        }
        let explicit_vertex_bindings = Self::parse_armature_bound_vertices(&arm.name);

        let mut deformed_shapes: Vec<(Id, Element)> = Vec::new();
        for el in &key_snapshot.elements {
            if key_snapshot.controller_elements.contains(&el.id) || el.to_path_commands().is_none()
            {
                continue;
            }
            let base_key = (symbol_id.clone(), frame, el.id.clone());
            let (base_el, rest_poses) = self
                .vector
                .ik_base_elements
                .entry(base_key)
                .or_insert_with(|| (el.clone(), current_poses.clone()))
                .clone();

            if let Some(v_bindings) = explicit_vertex_bindings.get(&el.id)
                && !v_bindings.is_empty()
            {
                let Some(mut cmds) = editable_element_commands(&base_el)
                    .or_else(|| base_el.to_path_commands().map(|(c, _)| c))
                else {
                    continue;
                };
                let mat = transform_matrix(base_el.transform);
                let anchor_cmds: Vec<usize> = cmds
                    .iter()
                    .enumerate()
                    .filter_map(|(i, cmd)| (!matches!(cmd, PathCommand::Close)).then_some(i))
                    .collect();
                for &(v_idx, ref bone_id, rest_ox, rest_oy) in v_bindings {
                    let Some(&cmd_idx) = anchor_cmds.get(v_idx).or(Some(&v_idx)) else {
                        continue;
                    };
                    let Some((_, head, _, world_angle)) =
                        current_poses.iter().find(|(id, _, _, _)| id == bone_id)
                    else {
                        continue;
                    };
                    let rad = world_angle.to_radians();
                    let c = rad.cos();
                    let s = rad.sin();
                    let stage_x = head[0] + rest_ox * c - rest_oy * s;
                    let stage_y = head[1] + rest_ox * s + rest_oy * c;
                    let [lx, ly] = if let Some((lx, ly)) = inverse_f64(mat, stage_x, stage_y) {
                        [lx, ly]
                    } else {
                        [stage_x - base_el.transform.x, stage_y - base_el.transform.y]
                    };
                    if let Some(cur_anchor) = crate::vector::handles(&cmds)
                        .into_iter()
                        .find(|h| h.command == cmd_idx && h.role == PointRole::Anchor)
                    {
                        let delta = Pos2::new(lx as f32, ly as f32) - cur_anchor.point;
                        move_selected_anchors(&mut cmds, &[cmd_idx], delta);
                    }
                }
                let mut updated = base_el.clone();
                write_editable_element_commands(&mut updated, cmds);
                deformed_shapes.push((el.id.clone(), updated));
                continue;
            }

            if !rest_poses.is_empty() {
                let Some(warped) = map_element_stage_points(&base_el, 4, |pt| {
                    let mut w_sum = 0.0;
                    let mut out_x = 0.0;
                    let mut out_y = 0.0;
                    for (bone_id, cur_head, _, cur_angle) in &current_poses {
                        let Some((_, rest_head, rest_tail, rest_angle)) =
                            rest_poses.iter().find(|(id, _, _, _)| id == bone_id)
                        else {
                            continue;
                        };
                        let vx = rest_tail[0] - rest_head[0];
                        let vy = rest_tail[1] - rest_head[1];
                        let len_sq = vx * vx + vy * vy;
                        let t = if len_sq > 1e-9 {
                            (((pt[0] - rest_head[0]) * vx + (pt[1] - rest_head[1]) * vy) / len_sq)
                                .clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let proj = [rest_head[0] + vx * t, rest_head[1] + vy * t];
                        let d2 = (pt[0] - proj[0]).powi(2) + (pt[1] - proj[1]).powi(2);
                        let w = 1.0 / (d2 + 16.0);
                        let r_rad = rest_angle.to_radians();
                        let rc = r_rad.cos();
                        let rs = r_rad.sin();
                        let rel_x = pt[0] - rest_head[0];
                        let rel_y = pt[1] - rest_head[1];
                        let local_x = rel_x * rc + rel_y * rs;
                        let local_y = -rel_x * rs + rel_y * rc;
                        let c_rad = cur_angle.to_radians();
                        let cc = c_rad.cos();
                        let cs = c_rad.sin();
                        let mapped_x = cur_head[0] + local_x * cc - local_y * cs;
                        let mapped_y = cur_head[1] + local_x * cs + local_y * cc;
                        out_x += mapped_x * w;
                        out_y += mapped_y * w;
                        w_sum += w;
                    }
                    if w_sum > 1e-9 {
                        [out_x / w_sum, out_y / w_sum]
                    } else {
                        pt
                    }
                }) else {
                    continue;
                };
                deformed_shapes.push((el.id.clone(), warped));
            }
        }

        let symbol = self.symbol_mut();
        let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
            return;
        };
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
            return;
        };
        for (id, warped) in deformed_shapes {
            if let Some(slot) = key.elements.iter_mut().find(|e| e.id == id) {
                *slot = warped;
            }
        }
    }

    pub(crate) fn apply_ik_bone_effector_in_place(
        &mut self,
        armature_id: &str,
        bone_id: &str,
        target_stage: [f64; 2],
    ) -> bool {
        if !target_stage[0].is_finite() || !target_stage[1].is_finite() {
            return false;
        }
        let initial_poses: Vec<(Id, [f64; 2], [f64; 2], f64)> = self
            .ik_bone_stage_poses()
            .into_iter()
            .filter(|(a_id, _, _, _, _)| a_id == armature_id)
            .map(|(_, b_id, h, t, ang)| (b_id, h, t, ang))
            .collect();
        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        for el in self.selected_vector_elements_on_active_key() {
            self.vector
                .ik_base_elements
                .entry((symbol_id.clone(), frame, el.id.clone()))
                .or_insert_with(|| (el, initial_poses.clone()));
        }

        let solved = {
            let symbol = self.symbol_mut();
            let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
                return false;
            };
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
                return false;
            };
            let Some(arm) = key.ik_armatures.iter_mut().find(|a| a.id == armature_id) else {
                return false;
            };
            if !arm.bones.iter().any(|b| b.id == bone_id) {
                return false;
            }
            arm.effector_bone_id = bone_id.to_owned();
            arm.target_pos = target_stage;
            let arm_clone = arm.clone();
            solve_ik_armature_fabrik_ccd(&mut key.elements, &arm_clone, &[]);
            true
        };
        if !solved {
            return false;
        }
        self.vector.selected_ik_bone = Some(bone_id.to_owned());
        self.apply_ik_pose_to_scene_in_place(armature_id);
        true
    }

    /// Drag an `IkBone` joint/effector to `target_stage`, solve inverse kinematics, and update
    /// bound elements/shapes in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn move_ik_bone_effector(
        &mut self,
        armature_id: &str,
        bone_id: &str,
        target_stage: [f64; 2],
    ) -> bool {
        self.history.begin(&self.project);
        if !self.apply_ik_bone_effector_in_place(armature_id, bone_id, target_stage) {
            return false;
        }
        self.commit_history();
        true
    }

    /// Solve IK for `armature_id`'s active or leaf effector bone toward `target_stage` in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn pose_ik_armature(&mut self, armature_id: &str, target_stage: [f64; 2]) -> bool {
        let poses = self.ik_bone_stage_poses();
        let bone_id = self
            .vector
            .selected_ik_bone
            .clone()
            .filter(|sel| {
                poses
                    .iter()
                    .any(|(a_id, b_id, _, _, _)| a_id == armature_id && b_id == sel)
            })
            .or_else(|| {
                poses
                    .iter()
                    .rev()
                    .find(|(a_id, _, _, _, _)| a_id == armature_id)
                    .map(|(_, b_id, _, _, _)| b_id.clone())
            });
        let Some(bone_id) = bone_id else {
            return false;
        };
        self.move_ik_bone_effector(armature_id, &bone_id, target_stage)
    }

    /// Solve the active IK chain toward `target_stage` in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn solve_ik_chain(&mut self, target_stage: [f64; 2]) -> bool {
        let Some((arm_id, _, _, _, _)) = self.ik_bone_stage_poses().into_iter().next() else {
            return false;
        };
        self.pose_ik_armature(&arm_id, target_stage)
    }

    pub(crate) fn apply_bind_stage_point_in_place(
        &mut self,
        stage_pt: [f64; 2],
        unbind: bool,
    ) -> bool {
        if !stage_pt[0].is_finite() || !stage_pt[1].is_finite() {
            return false;
        }
        let poses = self.ik_bone_stage_poses();
        if poses.is_empty() {
            return false;
        }

        let chosen_bone = self
            .vector
            .selected_ik_bone
            .as_ref()
            .and_then(|sel| poses.iter().find(|(_, b_id, _, _, _)| b_id == sel).cloned())
            .or_else(|| {
                poses
                    .iter()
                    .map(|pose| {
                        let (_, _, head, tail, _) = pose;
                        let vx = tail[0] - head[0];
                        let vy = tail[1] - head[1];
                        let len_sq = vx * vx + vy * vy;
                        let t = if len_sq > 1e-9 {
                            (((stage_pt[0] - head[0]) * vx + (stage_pt[1] - head[1]) * vy) / len_sq)
                                .clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let proj = [head[0] + vx * t, head[1] + vy * t];
                        let d2 = (proj[0] - stage_pt[0]).powi(2) + (proj[1] - stage_pt[1]).powi(2);
                        (pose.clone(), d2)
                    })
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(p, _)| p)
            });
        let Some((arm_id, bone_id, bone_head, _bone_tail, bone_angle)) = chosen_bone else {
            return false;
        };
        self.vector.selected_ik_bone = Some(bone_id.clone());

        let els = self.selected_vector_elements_on_active_key();
        let Some(el) = els.first().cloned() else {
            return false;
        };
        let Some(cmds) =
            editable_element_commands(&el).or_else(|| el.to_path_commands().map(|(c, _)| c))
        else {
            return false;
        };
        let mat = transform_matrix(el.transform);
        let mut anchor_pts: Vec<(usize, [f64; 2])> = Vec::new();
        for cmd in &cmds {
            let pt = match *cmd {
                PathCommand::MoveTo { x, y }
                | PathCommand::LineTo { x, y }
                | PathCommand::QuadraticTo { x, y, .. }
                | PathCommand::CubicTo { x, y, .. } => {
                    let sx = mat[0] * x + mat[2] * y + mat[4];
                    let sy = mat[1] * x + mat[3] * y + mat[5];
                    Some([sx, sy])
                }
                PathCommand::Close => None,
            };
            if let Some(p) = pt {
                let idx = anchor_pts.len();
                anchor_pts.push((idx, p));
            }
        }
        let Some(&(v_idx, v_pos)) = anchor_pts.iter().min_by(|a, b| {
            let da = (a.1[0] - stage_pt[0]).powi(2) + (a.1[1] - stage_pt[1]).powi(2);
            let db = (b.1[0] - stage_pt[0]).powi(2) + (b.1[1] - stage_pt[1]).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        }) else {
            return false;
        };

        let symbol_id = self.symbol_id().to_owned();
        let layer_idx = self.layer;
        let frame = self.frame;
        let arm_poses: Vec<(Id, [f64; 2], [f64; 2], f64)> = poses
            .into_iter()
            .filter(|(a, _, _, _, _)| a == &arm_id)
            .map(|(_, b, h, t, ang)| (b, h, t, ang))
            .collect();
        self.vector
            .ik_base_elements
            .entry((symbol_id, frame, el.id.clone()))
            .or_insert_with(|| (el.clone(), arm_poses));

        let rad = bone_angle.to_radians();
        let c = rad.cos();
        let s = rad.sin();
        let rel_x = v_pos[0] - bone_head[0];
        let rel_y = v_pos[1] - bone_head[1];
        let rest_offset_x = rel_x * c + rel_y * s;
        let rest_offset_y = -rel_x * s + rel_y * c;

        let symbol = self.symbol_mut();
        let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
            return false;
        };
        let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
            return false;
        };
        let Some(arm) = key.ik_armatures.iter_mut().find(|a| a.id == arm_id) else {
            return false;
        };
        let mut map = Self::parse_armature_bound_vertices(&arm.name);
        let list = map.entry(el.id.clone()).or_default();
        let mut changed = false;
        if unbind {
            let before_len = list.len();
            list.retain(|(vi, bid, _, _)| !(*vi == v_idx && bid == &bone_id));
            changed = list.len() != before_len;
        } else if let Some(existing) = list.iter_mut().find(|(vi, _, _, _)| *vi == v_idx) {
            if existing.1 != bone_id {
                existing.1 = bone_id;
                existing.2 = rest_offset_x;
                existing.3 = rest_offset_y;
                changed = true;
            }
        } else {
            list.push((v_idx, bone_id, rest_offset_x, rest_offset_y));
            changed = true;
        }
        if changed {
            arm.name = Self::encode_armature_bound_vertices(&arm.name, &map);
        }
        changed
    }

    /// Bind or unbind the nearest contour vertex of the selected shape at `stage_pt` to the
    /// active/nearest `IkBone` in 1 Undo step (`T-271`).
    #[allow(dead_code)]
    pub fn bind_stage_point_to_bone(&mut self, stage_pt: [f64; 2], unbind: bool) -> bool {
        self.history.begin(&self.project);
        if !self.apply_bind_stage_point_in_place(stage_pt, unbind) {
            return false;
        }
        self.commit_history();
        true
    }

    /// Return the sorted list of contour vertex indices of `shape_id` bound to `bone_id` (`T-271`).
    pub fn bound_vertices_for_bone(&self, shape_id: &str, bone_id: &str) -> Vec<usize> {
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return Vec::new();
        };
        let Some(key) = layer.key_at(self.frame) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for arm in &key.ik_armatures {
            let map = Self::parse_armature_bound_vertices(&arm.name);
            if let Some(list) = map.get(shape_id) {
                for (v_idx, bid, _, _) in list {
                    if bid == bone_id {
                        out.push(*v_idx);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

#[cfg(test)]
mod vector_t233_tests {
    use super::*;

    #[test]
    fn envelope_mesh_handle_drag_and_bake_with_one_step_undo_and_round_trip() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.project.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "shape-env".into(),
                name: "Shape".into(),
                transform: Transform {
                    x: 100.0,
                    y: 50.0,
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 200.0,
                    height: 100.0,
                    fill: [60, 120, 240, 255],
                    stroke: [10, 20, 40, 255],
                    stroke_width: 2.0,
                },
                ..Element::default()
            });
        editor.select_single_id("shape-env".into());

        // Set a 2x2 EnvelopeWarp grid in 1 Undo step
        let grid = EnvelopeWarp {
            preset: EnvelopeWarpPreset::CustomGrid,
            rows: 2,
            cols: 2,
            control_points: default_envelope_grid_points(2, 2),
            ..EnvelopeWarp::default()
        };
        assert!(editor.set_selected_envelope_warp(Some(grid)));
        let handles = editor.selected_envelope_stage_handles();
        assert_eq!(handles.len(), 9);
        assert_eq!(
            editor.hit_test_envelope_stage_handle([100.0, 50.0], 8.0),
            Some((0, 0))
        );

        // Drag center handle (1, 1) in Stage space -> updates control_points[4] in 1 Undo step
        assert!(editor.drag_selected_envelope_handle_stage(1, 1, [220.0, 130.0]));
        let updated_warp = editor.selected_envelope_warp().expect("envelope warp");
        assert_eq!(updated_warp.preset, EnvelopeWarpPreset::CustomGrid);
        assert!((updated_warp.control_points[4][0] - 0.6).abs() < 1e-6);
        assert!((updated_warp.control_points[4][1] - 0.8).abs() < 1e-6);

        // Undo handle drag -> restores original (0.5, 0.5) center control point
        editor.undo();
        let restored_warp = editor.selected_envelope_warp().expect("restored warp");
        assert!((restored_warp.control_points[4][0] - 0.5).abs() < 1e-6);
        assert!((restored_warp.control_points[4][1] - 0.5).abs() < 1e-6);

        // Redo handle drag, save/reopen, and validate
        editor.redo();
        editor.project.validate().expect("valid project");
        let json = editor.project.to_json().expect("serialize");
        let reopened = Project::from_json(&json).expect("deserialize");
        reopened.validate().expect("reopened valid");
    }

    #[test]
    fn perspective_grid_handle_drag_plane_switch_snap_and_undo() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        assert!(editor.active_perspective_grid().is_none());

        // Enable perspective grid
        assert!(editor.toggle_perspective_grid());
        let g = editor.active_perspective_grid().expect("perspective grid");
        assert!(g.enabled);

        // Drag left vanishing point and horizon in 1 Undo step
        assert!(editor.drag_perspective_grid_handle(
            PerspectiveGridHandle::LeftVanishingPoint,
            [-320.0, 180.0],
        ));
        let g2 = editor.active_perspective_grid().expect("updated grid");
        assert!((g2.left_vanishing_x - (-320.0)).abs() < 1e-6);
        assert!((g2.horizon_y - 180.0).abs() < 1e-6);
        assert_eq!(
            editor.hit_test_perspective_grid_handle([-320.0, 180.0], 10.0),
            Some(PerspectiveGridHandle::LeftVanishingPoint)
        );

        // Switch active plane to RightGrid
        assert!(editor.set_perspective_active_plane(PerspectivePlane::RightGrid));
        let g3 = editor.active_perspective_grid().expect("plane switched");
        assert_eq!(g3.active_plane, PerspectivePlane::RightGrid);

        // Snap a Stage point onto the active perspective grid
        let snapped = g3.snap_point([415.0, 205.0], 24.0);
        assert!(snapped[0].is_finite() && snapped[1].is_finite());

        // 1-step Undo restores previous plane
        editor.undo();
        assert_eq!(
            editor.active_perspective_grid().expect("grid").active_plane,
            PerspectivePlane::LeftGrid
        );

        editor.project.validate().expect("valid");
        let json = editor.project.to_json().expect("serialize");
        let reopened = Project::from_json(&json).expect("deserialize");
        reopened.validate().expect("reopened valid");
        assert!(reopened.symbols[0].timeline.perspective_grid().is_some());
    }
}
