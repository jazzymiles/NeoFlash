use crate::theme::UiPalette;
use eframe::egui::{self, Color32, Pos2, Shape};
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
    math::{Point, point},
    path::Path,
};
use neoflash_core::*;
use std::{
    cell::RefCell,
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct TessCacheKey {
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
pub(super) const MAX_SCREEN_MESH_CACHE_ENTRIES: usize = 32_768;
pub(super) const MAX_SCREEN_MESH_CACHE_ELEMENTS: usize = 4_000_000;
pub(super) const PEN_CLOSE_RADIUS: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VectorPaintVisibility {
    pub(crate) fill: bool,
    pub(crate) stroke: bool,
}

/// Effective vector passes, matching the renderer: an explicit gradient replaces the solid
/// fallback, and a stroke also needs a positive authored width.
pub(crate) fn vector_paint_visibility(item: &DrawItem) -> Option<VectorPaintVisibility> {
    let visible_alpha = |solid: &Color, gradient: Option<&GradientPaint>| {
        gradient.map_or(solid[3] != 0, |paint| {
            paint.stops.iter().any(|stop| stop.color[3] != 0)
        })
    };
    match &item.kind {
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
            fill: item.bitmap_fill.is_some() || fill[3] != 0,
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
            fill: item.bitmap_fill.is_some() || visible_alpha(fill, fill_gradient.as_ref()),
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
pub(super) enum ScreenMeshPass {
    Fill,
    Stroke,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ScreenMeshSlot {
    pub(super) identity: u64,
    pub(super) pass: ScreenMeshPass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ScreenMeshKey {
    pub(super) tessellation: TessCacheKey,
    pub(super) transform_bits: [u64; 6],
    pub(super) origin_bits: [u32; 2],
    pub(super) zoom_bits: u32,
    pub(super) color: [u8; 4],
    pub(super) style: RetainedPathStyle,
}

pub(super) struct ScreenMeshEntry {
    key: ScreenMeshKey,
    mesh: Arc<egui::Mesh>,
    weight: usize,
}

pub(super) struct ScreenMeshCache {
    pub(super) entries: HashMap<ScreenMeshSlot, ScreenMeshEntry>,
    pub(super) total_elements: usize,
    max_entries: usize,
    max_elements: usize,
}

impl ScreenMeshCache {
    pub(super) fn new(max_entries: usize, max_elements: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries.min(1024)),
            total_elements: 0,
            max_entries,
            max_elements,
        }
    }

    pub(super) fn get(&self, slot: ScreenMeshSlot, key: ScreenMeshKey) -> Option<Arc<egui::Mesh>> {
        self.entries
            .get(&slot)
            .filter(|entry| entry.key == key)
            .map(|entry| entry.mesh.clone())
    }

    pub(super) fn insert(
        &mut self,
        slot: ScreenMeshSlot,
        key: ScreenMeshKey,
        mesh: Arc<egui::Mesh>,
    ) {
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
    pub(super) static SCREEN_MESH_CACHE: RefCell<ScreenMeshCache> = RefCell::new(ScreenMeshCache::new(
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

pub(super) fn bounds_cache_key(
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

pub(super) fn tess_cache_key(
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
pub(super) fn retained_screen_mesh(
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
