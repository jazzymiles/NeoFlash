//! Intersect painter meshes with filled vector unions. Vertex interpolation
//! preserves bitmap UVs and text colors; overlapping mask pieces paint once.
use eframe::egui::{self, Color32, Pos2, Shape};
use egui::epaint::{Mesh, Vertex};
use neoflash_core::{DrawItem, Element, ElementKind, FillRule, PathCommand, RuntimeClip};
use std::{cell::RefCell, collections::HashMap, sync::Arc};

type Triangle = [Pos2; 3];
const MAX_CLIP_WORK: usize = 4_000_000;
const COMPLEXITY_ERROR: &str = "Mask preview is too complex; simplify vector mask contours";

struct ClipTriangle {
    points: Triangle,
    bounds: egui::Rect,
}

/// A disjoint triangulation of the complete mask union, indexed in screen-space cells.
/// Overlapping strokes/components are united once, rather than subtracting them from
/// every painter triangle separately (which fragments long hatch strips quadratically).
struct MaskMesh {
    triangles: Vec<ClipTriangle>,
    bounds: egui::Rect,
    columns: usize,
    rows: usize,
    cells: Vec<Vec<usize>>,
}
impl MaskMesh {
    fn from_disjoint(raw_triangles: Vec<Triangle>) -> Result<Self, String> {
        if raw_triangles.len() > MAX_CLIP_WORK / 4 {
            return Err(COMPLEXITY_ERROR.into());
        }
        let mut triangles = Vec::with_capacity(raw_triangles.len());
        for mut points in raw_triangles {
            if points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
                return Err("Mask preview has non-finite geometry".into());
            }
            let area = cross(points[0], points[1], points[2]);
            if !area.is_finite() {
                return Err("Mask preview has non-finite geometry".into());
            }
            if area.abs() < 1e-8 {
                continue;
            }
            if area < 0. {
                points.swap(1, 2);
            }
            let mut bounds = egui::Rect::NOTHING;
            for point in points {
                bounds.extend_with(point);
            }
            triangles.push(ClipTriangle { points, bounds });
        }
        Self::build_grid(triangles)
    }

    fn new(triangles: Vec<Triangle>) -> Result<Self, String> {
        if triangles.len() <= 1 {
            return Self::from_disjoint(triangles);
        }
        if triangles.len() > MAX_CLIP_WORK / 4 {
            return Err(COMPLEXITY_ERROR.into());
        }
        let mut contours = Vec::with_capacity(triangles.len() * 4);
        for mut triangle in triangles {
            let area = cross(triangle[0], triangle[1], triangle[2]);
            if area.abs() < 1e-8 {
                continue;
            }
            if !area.is_finite() {
                return Err("Mask preview has non-finite geometry".into());
            }
            if area < 0. {
                triangle.swap(1, 2);
            }
            contours.push(PathCommand::MoveTo {
                x: triangle[0].x as f64,
                y: triangle[0].y as f64,
            });
            for point in &triangle[1..] {
                contours.push(PathCommand::LineTo {
                    x: point.x as f64,
                    y: point.y as f64,
                });
            }
            contours.push(PathCommand::Close);
        }
        let union = crate::vector::tessellate_cached(&contours, FillRule::NonZero, None, 0.01)?;
        if union.indices.len() / 3 > MAX_CLIP_WORK / 4 {
            return Err(COMPLEXITY_ERROR.into());
        }
        if union
            .vertices
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            return Err("Mask preview has non-finite geometry".into());
        }
        let triangles: Vec<_> = union
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|indices| {
                let mut points = indices.map(|i| {
                    let p = union.vertices[i as usize];
                    Pos2::new(p.x, p.y)
                });
                let area = cross(points[0], points[1], points[2]);
                if area.abs() < 1e-8 {
                    return None;
                }
                if area < 0. {
                    points.swap(1, 2);
                }
                let mut bounds = egui::Rect::NOTHING;
                for point in points {
                    bounds.extend_with(point);
                }
                Some(ClipTriangle { points, bounds })
            })
            .collect();
        Self::build_grid(triangles)
    }

    fn build_grid(triangles: Vec<ClipTriangle>) -> Result<Self, String> {
        if triangles.is_empty() {
            return Ok(Self {
                triangles,
                bounds: egui::Rect::ZERO,
                columns: 1,
                rows: 1,
                cells: vec![Vec::new()],
            });
        }
        let bounds = triangles
            .iter()
            .fold(egui::Rect::NOTHING, |bounds, triangle| {
                bounds.union(triangle.bounds)
            });
        let columns = (bounds.width() / 32.).ceil().clamp(1., 64.) as usize;
        let rows = (bounds.height() / 32.).ceil().clamp(1., 64.) as usize;
        let mut mesh = Self {
            triangles,
            bounds,
            columns,
            rows,
            cells: vec![Vec::new(); columns * rows],
        };
        let mut entries = 0;
        for (index, triangle) in mesh.triangles.iter().enumerate() {
            let (left, top, right, bottom) = mesh.cell_range(triangle.bounds);
            for y in top..=bottom {
                for x in left..=right {
                    entries += 1;
                    if entries > MAX_CLIP_WORK {
                        return Err(COMPLEXITY_ERROR.into());
                    }
                    mesh.cells[y * columns + x].push(index);
                }
            }
        }
        Ok(mesh)
    }

    fn cell_range(&self, bounds: egui::Rect) -> (usize, usize, usize, usize) {
        let x = |value: f32| {
            (((value - self.bounds.min.x) / self.bounds.width()) * self.columns as f32)
                .floor()
                .clamp(0., (self.columns - 1) as f32) as usize
        };
        let y = |value: f32| {
            (((value - self.bounds.min.y) / self.bounds.height()) * self.rows as f32)
                .floor()
                .clamp(0., (self.rows - 1) as f32) as usize
        };
        (
            x(bounds.min.x),
            y(bounds.min.y),
            x(bounds.max.x),
            y(bounds.max.y),
        )
    }

    fn cell_bounds(&self, x: usize, y: usize) -> egui::Rect {
        let step = self.bounds.size() / egui::vec2(self.columns as f32, self.rows as f32);
        egui::Rect::from_min_max(
            self.bounds.min + egui::vec2(x as f32 * step.x, y as f32 * step.y),
            self.bounds.min + egui::vec2((x + 1) as f32 * step.x, (y + 1) as f32 * step.y),
        )
    }
}

pub(crate) struct ClipMeshes(HashMap<String, (MaskMesh, bool)>);
impl ClipMeshes {
    /// The same fill/stroke silhouette used to paint vector artwork, including holes.
    /// This clips the existing NF2 hatch; it does not define another selection style.
    pub(crate) fn for_shape(item: &DrawItem, origin: Pos2, zoom: f32) -> Result<Self, String> {
        let single_fill_pass = is_fill_only_vector_silhouette(item);
        let triangles = vector_mesh(item, origin, zoom)?
            .map(|mesh| {
                mesh.indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|indices| indices.map(|i| mesh.vertices[i as usize].pos))
                    .collect()
            })
            .unwrap_or_default();
        let mask_mesh = if single_fill_pass {
            MaskMesh::from_disjoint(triangles)?
        } else {
            MaskMesh::new(triangles)?
        };
        Ok(Self(HashMap::from([(
            item.element_id.clone(),
            (mask_mesh, false),
        )])))
    }

    pub(crate) fn new(clips: &[RuntimeClip], origin: Pos2, zoom: f32) -> Result<Self, String> {
        let mut map = HashMap::new();
        for clip in clips {
            prepare_mask(&clip.id, clips, origin, zoom, &mut map, &mut Vec::new())?;
        }
        Ok(Self(map))
    }
    pub(crate) fn paint(
        &self,
        painter: &egui::Painter,
        ids: &[String],
        paint: impl FnOnce(&egui::Painter),
    ) -> Result<(), String> {
        if ids.is_empty() {
            paint(painter);
            return Ok(());
        }
        paint_clipped(painter, |mesh| self.clip_mesh(mesh, ids), paint)
    }

    pub(crate) fn clip_mesh(&self, mesh: &Mesh, ids: &[String]) -> Result<Mesh, String> {
        let mut result = mesh.clone();
        for id in ids {
            let (mask, invert) = self
                .0
                .get(id)
                .ok_or_else(|| format!("Missing authored mask {id}"))?;
            if *invert {
                continue;
            }
            result = intersect_mesh(&result, mask)?;
        }
        Ok(result)
    }
}

fn prepare_mask(
    id: &str,
    clips: &[RuntimeClip],
    origin: Pos2,
    zoom: f32,
    map: &mut HashMap<String, (MaskMesh, bool)>,
    active: &mut Vec<String>,
) -> Result<(), String> {
    if map.contains_key(id) {
        return Ok(());
    }
    if active.len() >= 64 || active.iter().any(|current| current == id) {
        return Err("Mask preview contains a cycle or excessive nesting".into());
    }
    let clip = clips
        .iter()
        .find(|clip| clip.id == id)
        .ok_or_else(|| format!("Missing authored mask {id}"))?;
    active.push(id.into());
    let mut triangles = Vec::new();
    for component in &clip.components {
        let tess = crate::vector::tessellate_cached(
            &component.commands,
            component.fill_rule,
            None,
            (0.25 / zoom.max(0.01)).clamp(0.001, 1.),
        )?;
        let mut mesh = Mesh::default();
        mesh.vertices.reserve(tess.vertices.len());
        for point in &tess.vertices {
            mesh.colored_vertex(
                origin
                    + crate::stage::affine(component.matrix, Pos2::new(point.x, point.y)).to_vec2()
                        * zoom,
                Color32::WHITE,
            );
        }
        mesh.indices.clone_from(&tess.indices);
        for dependency in &component.clip_ids {
            prepare_mask(dependency, clips, origin, zoom, map, active)?;
            let (dep_mask, dep_invert) = &map[dependency];
            if !*dep_invert {
                mesh = intersect_mesh(&mesh, dep_mask)?;
            }
        }
        triangles.extend(
            mesh.indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|indices| indices.map(|i| mesh.vertices[i as usize].pos)),
        );
    }
    active.pop();
    map.insert(id.into(), (MaskMesh::new(triangles)?, clip.invert));
    Ok(())
}

fn paint_clipped(
    painter: &egui::Painter,
    clip: impl Fn(&Mesh) -> Result<Mesh, String>,
    paint: impl FnOnce(&egui::Painter),
) -> Result<(), String> {
    let primitives = capture(painter, paint);
    // Complete every clip before emitting any mesh, so failures cannot leave half an overlay.
    let mut clipped = Vec::new();
    for primitive in primitives {
        if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
            clipped.push((primitive.clip_rect, clip(&mesh)?));
        }
    }
    for (rect, mesh) in clipped {
        painter
            .with_clip_rect(painter.clip_rect().intersect(rect))
            .add(Shape::mesh(mesh));
    }
    Ok(())
}

pub(crate) fn capture(
    painter: &egui::Painter,
    paint: impl FnOnce(&egui::Painter),
) -> Vec<egui::ClippedPrimitive> {
    let layer = egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("neoflash-mask-scratch"),
    );
    let scratch = egui::Painter::new(painter.ctx().clone(), layer, painter.clip_rect());
    paint(&scratch);
    let shapes = painter.ctx().graphics_mut(|graphics| {
        let list = graphics.entry(layer);
        let shapes = list.all_entries().cloned().collect();
        *list = Default::default();
        shapes
    });
    painter
        .ctx()
        .tessellate(shapes, painter.ctx().pixels_per_point())
}

fn is_fill_only_vector_silhouette(item: &DrawItem) -> bool {
    crate::vector::vector_paint_visibility(&item.kind)
        .is_some_and(|visibility| visibility.fill && !visibility.stroke)
}

/// Geometry only: the union of visible vector fills and strokes in the requested space.
/// Non-vector placements return None; their bounds are not a vector silhouette.
pub(crate) fn vector_mesh(
    item: &DrawItem,
    origin: Pos2,
    zoom: f32,
) -> Result<Option<Mesh>, String> {
    let Some(visibility) = crate::vector::vector_paint_visibility(&item.kind) else {
        return Ok(None);
    };
    let (width, fingerprint) = match &item.kind {
        ElementKind::Rectangle { stroke_width, .. } | ElementKind::Ellipse { stroke_width, .. } => {
            (*stroke_width, None)
        }
        ElementKind::Path { stroke_width, .. } => (*stroke_width, item.path_fingerprint),
        ElementKind::PaintedPath { stroke_width, .. } => (*stroke_width, item.path_fingerprint),
        _ => unreachable!("vector paint visibility only accepts vector elements"),
    };
    // Evaluated paths already contain renderer-ready commands. Borrow them instead of cloning a
    // potentially huge vector merely to pass it back through `Element::to_path_commands`.
    let generated_commands;
    let (commands, rule): (&[PathCommand], FillRule) = match &item.kind {
        ElementKind::Path {
            commands,
            fill_rule,
            ..
        }
        | ElementKind::PaintedPath {
            commands,
            fill_rule,
            ..
        } => (commands, *fill_rule),
        _ => {
            let element = Element {
                kind: item.kind.clone(),
                ..Default::default()
            };
            let Some((commands, rule)) = element.to_path_commands() else {
                return Ok(None);
            };
            generated_commands = commands;
            (&generated_commands, rule)
        }
    };
    let m = item.transform;
    let scale = m[0].hypot(m[1]).max(m[2].hypot(m[3])) as f32 * zoom;
    let tolerance = (0.25 / scale.max(0.01)).clamp(0.001, 1.);
    let mut mesh = Mesh::default();
    for width in [
        visibility.fill.then_some(None),
        visibility.stroke.then_some(Some(width)),
    ]
    .into_iter()
    .flatten()
    {
        let geometry = crate::vector::tessellate_cached_with_fingerprint(
            commands,
            fingerprint,
            rule,
            width,
            tolerance,
        )?;
        let base = mesh.vertices.len() as u32;
        mesh.vertices.reserve(geometry.vertices.len());
        for vertex in &geometry.vertices {
            mesh.colored_vertex(
                origin + crate::stage::affine(m, Pos2::new(vertex.x, vertex.y)).to_vec2() * zoom,
                Color32::WHITE,
            );
        }
        mesh.indices
            .extend(geometry.indices.iter().map(|&i| base + i));
    }
    Ok(Some(mesh))
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ShapeSelectionCacheKey {
    hash_a: u64,
    hash_b: u64,
    command_count: usize,
    linear_transform_bits: [u64; 4],
    /// Screen-space translation of this draw relative to the owning selection rectangle.
    relative_translation_bits: [u32; 2],
    zoom_bits: u32,
    rect_size_bits: [u32; 2],
    visible_bits: [u32; 4],
    hatch_bits: [u32; 4],
    ppp_bits: u32,
    dark_mode: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ShapeSelectionCacheSlot {
    layer_id: String,
    instance_path: Vec<String>,
    element_id: String,
}

impl ShapeSelectionCacheSlot {
    fn for_item(item: &DrawItem) -> Self {
        Self {
            layer_id: item.layer_id.clone(),
            instance_path: item.instance_path.clone(),
            element_id: item.element_id.clone(),
        }
    }
}

type ClippedHatchList = Vec<(egui::Rect, Arc<Mesh>)>;

struct CachedShapeSelection {
    anchor: Pos2,
    clipped: ClippedHatchList,
}

impl CachedShapeSelection {
    fn mesh_weight(&self) -> usize {
        self.clipped.iter().fold(0, |weight, (_, mesh)| {
            weight
                .saturating_add(mesh.vertices.len())
                .saturating_add(mesh.indices.len())
        })
    }
}

struct ShapeSelectionCacheEntry {
    key: ShapeSelectionCacheKey,
    selection: Arc<CachedShapeSelection>,
    weight: usize,
}

struct ShapeSelectionCache {
    entries: HashMap<ShapeSelectionCacheSlot, ShapeSelectionCacheEntry>,
    total_weight: usize,
    max_entries: usize,
    max_weight: usize,
}

impl ShapeSelectionCache {
    fn with_limits(max_entries: usize, max_weight: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries.min(512)),
            total_weight: 0,
            max_entries,
            max_weight,
        }
    }

    fn get(
        &self,
        slot: &ShapeSelectionCacheSlot,
        key: &ShapeSelectionCacheKey,
    ) -> Option<Arc<CachedShapeSelection>> {
        let entry = self.entries.get(slot)?;
        (entry.key == *key).then(|| entry.selection.clone())
    }

    fn insert(
        &mut self,
        slot: ShapeSelectionCacheSlot,
        key: ShapeSelectionCacheKey,
        selection: Arc<CachedShapeSelection>,
    ) {
        if let Some(previous) = self.entries.remove(&slot) {
            self.total_weight = self.total_weight.saturating_sub(previous.weight);
        }

        let weight = selection.mesh_weight();
        if self.max_entries == 0 || self.max_weight == 0 || weight > self.max_weight {
            return;
        }
        if self.entries.len() >= self.max_entries
            || self.total_weight.saturating_add(weight) > self.max_weight
        {
            self.clear();
        }
        self.total_weight = self.total_weight.saturating_add(weight);
        self.entries.insert(
            slot,
            ShapeSelectionCacheEntry {
                key,
                selection,
                weight,
            },
        );
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.total_weight = 0;
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

const MAX_SHAPE_SELECTION_CACHE_ENTRIES: usize = 16_384;
const MAX_SHAPE_SELECTION_CACHE_WEIGHT: usize = 4_000_000;

thread_local! {
    static SHAPE_SELECTION_CACHE: RefCell<ShapeSelectionCache> = RefCell::new(
        ShapeSelectionCache::with_limits(
            MAX_SHAPE_SELECTION_CACHE_ENTRIES,
            MAX_SHAPE_SELECTION_CACHE_WEIGHT,
        )
    );
}

#[cfg(test)]
thread_local! {
    static SHAPE_SELECTION_CACHE_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn item_silhouette_fingerprint(item: &DrawItem) -> Option<(u64, u64, usize)> {
    let visibility = crate::vector::vector_paint_visibility(&item.kind)?;
    match &item.kind {
        ElementKind::Path {
            commands,
            fill_rule,
            stroke_width,
            ..
        } => {
            let fingerprint = item
                .path_fingerprint
                .unwrap_or_else(|| neoflash_core::path_commands_fingerprint(commands));
            let (ha, hb) = (fingerprint.hash_a, fingerprint.hash_b);
            let meta = (visibility.fill as u64)
                | ((visibility.stroke as u64) << 1)
                | ((*fill_rule as u64) << 2)
                | stroke_width.to_bits().rotate_left(7);
            Some((ha ^ meta, hb.wrapping_add(meta), commands.len()))
        }
        ElementKind::PaintedPath {
            commands,
            fill_rule,
            stroke_width,
            ..
        } => {
            let fingerprint = item
                .path_fingerprint
                .unwrap_or_else(|| neoflash_core::path_commands_fingerprint(commands));
            let (ha, hb) = (fingerprint.hash_a, fingerprint.hash_b);
            let meta = (visibility.fill as u64)
                | ((visibility.stroke as u64) << 1)
                | ((*fill_rule as u64) << 2)
                | stroke_width.to_bits().rotate_left(7);
            Some((ha ^ meta, hb.wrapping_add(meta), commands.len()))
        }
        ElementKind::Rectangle {
            width,
            height,
            stroke_width,
            ..
        } => Some((
            width.to_bits() ^ height.to_bits().rotate_left(17),
            stroke_width.to_bits()
                ^ (visibility.fill as u64)
                ^ ((visibility.stroke as u64) << 1)
                ^ 0x5245_4354,
            0,
        )),
        ElementKind::Ellipse {
            width,
            height,
            stroke_width,
            ..
        } => Some((
            width.to_bits() ^ height.to_bits().rotate_left(17),
            stroke_width.to_bits()
                ^ (visibility.fill as u64)
                ^ ((visibility.stroke as u64) << 1)
                ^ 0x454c_4c50,
            0,
        )),
        _ => None,
    }
}

fn relative_rect_bits(rect: egui::Rect, anchor: Pos2) -> [u32; 4] {
    [
        (rect.min.x - anchor.x).to_bits(),
        (rect.min.y - anchor.y).to_bits(),
        (rect.max.x - anchor.x).to_bits(),
        (rect.max.y - anchor.y).to_bits(),
    ]
}

fn add_cached_shape_selection(
    painter: &egui::Painter,
    visible: egui::Rect,
    rect: egui::Rect,
    cached: &CachedShapeSelection,
) {
    let delta = rect.min - cached.anchor;
    let target = painter.with_clip_rect(visible);
    for (clip_rect, mesh) in &cached.clipped {
        let clip_rect = clip_rect.translate(delta);
        let shape = if delta == egui::Vec2::ZERO {
            Shape::mesh(mesh.clone())
        } else {
            let mut translated = (**mesh).clone();
            for vertex in &mut translated.vertices {
                vertex.pos += delta;
            }
            Shape::mesh(translated)
        };
        target
            .with_clip_rect(target.clip_rect().intersect(clip_rect))
            .add(shape);
    }
}

pub(crate) fn paint_shape_selection(
    painter: &egui::Painter,
    item: &DrawItem,
    origin: Pos2,
    zoom: f32,
    rect: egui::Rect,
    masks: Option<(&ClipMeshes, &[String])>,
) -> Result<(), String> {
    let Some((visible, hatch)) = bounded_hatch_rect(rect, painter.clip_rect()) else {
        return Ok(());
    };
    let unmasked = masks.is_none_or(|(_, ids)| ids.is_empty());
    let cache_entry = if unmasked {
        item_silhouette_fingerprint(item).map(|(hash_a, hash_b, command_count)| {
            let at_origin =
                origin + crate::stage::affine(item.transform, Pos2::ZERO).to_vec2() * zoom;
            (
                ShapeSelectionCacheSlot::for_item(item),
                ShapeSelectionCacheKey {
                    hash_a,
                    hash_b,
                    command_count,
                    linear_transform_bits: [
                        item.transform[0].to_bits(),
                        item.transform[1].to_bits(),
                        item.transform[2].to_bits(),
                        item.transform[3].to_bits(),
                    ],
                    relative_translation_bits: [
                        (at_origin.x - rect.min.x).to_bits(),
                        (at_origin.y - rect.min.y).to_bits(),
                    ],
                    zoom_bits: zoom.to_bits(),
                    rect_size_bits: [rect.width().to_bits(), rect.height().to_bits()],
                    visible_bits: relative_rect_bits(visible, rect.min),
                    hatch_bits: relative_rect_bits(hatch, rect.min),
                    ppp_bits: painter.ctx().pixels_per_point().to_bits(),
                    dark_mode: painter.ctx().style().visuals.dark_mode,
                },
            )
        })
    } else {
        None
    };

    if let Some((slot, key)) = &cache_entry
        && let Some(cached) = SHAPE_SELECTION_CACHE.with(|cell| cell.borrow().get(slot, key))
    {
        add_cached_shape_selection(painter, visible, rect, &cached);
        return Ok(());
    }

    #[cfg(test)]
    SHAPE_SELECTION_CACHE_BUILDS.with(|count| count.set(count.get() + 1));
    let shape = ClipMeshes::for_shape(item, origin, zoom)?;
    let target = painter.with_clip_rect(visible);
    let primitives = capture(&target, |p| {
        neoflash_ui::components::canvas::paint_shape_selection(p, p.ctx(), hatch)
    });
    let mut clipped = Vec::with_capacity(primitives.len());
    for primitive in primitives {
        if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
            let mesh = shape.clip_mesh(&mesh, std::slice::from_ref(&item.element_id))?;
            let mesh = if let Some((clips, ids)) = masks {
                clips.clip_mesh(&mesh, ids)?
            } else {
                mesh
            };
            clipped.push((primitive.clip_rect, Arc::new(mesh)));
        }
    }
    let cached = Arc::new(CachedShapeSelection {
        anchor: rect.min,
        clipped,
    });
    add_cached_shape_selection(painter, visible, rect, &cached);
    if let Some((slot, key)) = cache_entry {
        SHAPE_SELECTION_CACHE.with(|cell| {
            cell.borrow_mut().insert(slot, key, cached);
        });
    }
    Ok(())
}
/// Restrict NF2's hatch-generation loop to the visible viewport. Its diagonals have
/// constant x+y modulo `SHAPE_SELECTION_PERIOD`; extending the bounded left edge by less than one period
/// preserves that phase without iterating across distant/offscreen selection bounds.
const HATCH_PERIOD: f64 = neoflash_ui::components::canvas::SHAPE_SELECTION_PERIOD as f64;
fn bounded_hatch_rect(rect: egui::Rect, clip: egui::Rect) -> Option<(egui::Rect, egui::Rect)> {
    let visible = rect.intersect(clip);
    if !rect.is_finite() || !visible.is_finite() || !visible.is_positive() {
        return None;
    }
    let shift = ((visible.left() as f64 + visible.top() as f64)
        - (rect.left() as f64 + rect.top() as f64))
        .rem_euclid(HATCH_PERIOD);
    let mut hatch = visible;
    hatch.min.x -= shift as f32;
    Some((visible, hatch))
}

fn cross(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}
fn interpolate(a: Vertex, b: Vertex, t: f32) -> Vertex {
    let channels: [u8; 4] = std::array::from_fn(|i| {
        (a.color[i] as f32 + (b.color[i] as f32 - a.color[i] as f32) * t)
            .round()
            .clamp(0., 255.) as u8
    });
    Vertex {
        pos: a.pos + (b.pos - a.pos) * t,
        uv: a.uv + (b.uv - a.uv) * t,
        color: Color32::from_rgba_premultiplied(channels[0], channels[1], channels[2], channels[3]),
    }
}
/// Clip one source triangle to one CCW mask triangle. A triangle intersection has
/// at most six vertices; fixed buffers avoid allocating every rejected candidate.
fn triangle_intersection(
    source: [Vertex; 3],
    mask: &Triangle,
    work: &mut usize,
) -> Option<([Vertex; 8], usize)> {
    let mut polygon = [source[0]; 8];
    polygon[..3].copy_from_slice(&source);
    let mut len = 3;
    for edge in 0..3 {
        *work += len;
        let a = mask[edge];
        let b = mask[(edge + 1) % 3];
        let mut clipped = [source[0]; 8];
        let mut count = 0;
        let mut previous = polygon[len - 1];
        let mut pd = cross(a, b, previous.pos);
        for current in polygon[..len].iter().copied() {
            let cd = cross(a, b, current.pos);
            if (pd >= 0.) != (cd >= 0.) {
                clipped[count] = interpolate(previous, current, (pd / (pd - cd)).clamp(0., 1.));
                count += 1;
            }
            if cd >= 0. {
                clipped[count] = current;
                count += 1;
            }
            previous = current;
            pd = cd;
        }
        if count < 3 {
            return None;
        }
        polygon = clipped;
        len = count;
    }
    Some((polygon, len))
}

/// Triangle axes supplement the rectangle bounds test. This rejects cells beside a
/// diagonal hatch strip without walking the triangles stored inside those cells.
fn triangle_intersects_rect(source: &[Vertex; 3], bounds: egui::Rect, direction: f32) -> bool {
    for edge in 0..3 {
        let a = source[edge].pos;
        let b = source[(edge + 1) % 3].pos;
        let dx = (b.x - a.x) * direction;
        let dy = (b.y - a.y) * direction;
        let x = if dy <= 0. { bounds.max.x } else { bounds.min.x };
        let y = if dx >= 0. { bounds.max.y } else { bounds.min.y };
        if dx * (y - a.y) - dy * (x - a.x) < 0. {
            return false;
        }
    }
    true
}

fn intersect_mesh(mesh: &Mesh, mask: &MaskMesh) -> Result<Mesh, String> {
    let mut out = Mesh::with_texture(mesh.texture_id);
    if mask.triangles.is_empty() {
        return Ok(out);
    }
    let mut work = 0usize;
    let mut visited = vec![usize::MAX; mask.triangles.len()];
    for (stamp, indices) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
        let source = indices.map(|i| mesh.vertices[i as usize]);
        let area = cross(source[0].pos, source[1].pos, source[2].pos);
        if area.abs() < 1e-8 {
            continue;
        }
        let mut bounds = egui::Rect::NOTHING;
        for vertex in source {
            bounds.extend_with(vertex.pos);
        }
        if !bounds.intersects(mask.bounds) {
            continue;
        }
        let (left, top, right, bottom) = mask.cell_range(bounds.intersect(mask.bounds));
        for y in top..=bottom {
            for x in left..=right {
                work += 1;
                if work > MAX_CLIP_WORK {
                    return Err(COMPLEXITY_ERROR.into());
                }
                if !triangle_intersects_rect(&source, mask.cell_bounds(x, y), area.signum()) {
                    continue;
                }
                for &index in &mask.cells[y * mask.columns + x] {
                    // Count every scanned reference, including duplicates and rejected bounds.
                    work += 1;
                    if work > MAX_CLIP_WORK {
                        return Err(COMPLEXITY_ERROR.into());
                    }
                    if visited[index] == stamp {
                        continue;
                    }
                    visited[index] = stamp;
                    let triangle = &mask.triangles[index];
                    if !bounds.intersects(triangle.bounds)
                        || !triangle_intersects_rect(&source, triangle.bounds, area.signum())
                    {
                        continue;
                    }
                    let polygon = triangle_intersection(source, &triangle.points, &mut work);
                    if work > MAX_CLIP_WORK {
                        return Err(COMPLEXITY_ERROR.into());
                    }
                    let Some((polygon, len)) = polygon else {
                        continue;
                    };
                    let base = out.vertices.len() as u32;
                    for i in 1..len - 1 {
                        if cross(polygon[0].pos, polygon[i].pos, polygon[i + 1].pos).abs() >= 1e-8 {
                            out.indices.extend_from_slice(&[
                                base,
                                base + i as u32,
                                base + i as u32 + 1,
                            ]);
                        }
                    }
                    out.vertices.extend_from_slice(&polygon[..len]);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo { x, y },
            PathCommand::LineTo { x: x + width, y },
            PathCommand::LineTo {
                x: x + width,
                y: y + height,
            },
            PathCommand::LineTo { x, y: y + height },
            PathCommand::Close,
        ]
    }

    fn gradient(alpha: u8) -> neoflash_core::GradientPaint {
        neoflash_core::GradientPaint {
            kind: neoflash_core::GradientKind::Linear,
            matrix: [1., 0., 0., 1., 0., 0.],
            stops: vec![neoflash_core::GradientStop {
                ratio: 0,
                color: [20, 40, 60, alpha],
            }],
            focal_point_ratio: 0.,
        }
    }

    fn path_item(id: &str, kind: ElementKind) -> DrawItem {
        DrawItem {
            element_id: id.into(),
            name: String::new(),
            instance_path: vec![id.into()],
            layer_id: "layer".into(),
            transform: [1., 0., 0., 1., 0., 0.],
            alpha: 1.,
            color_transform: Default::default(),
            path_fingerprint: None,
            kind,
        }
    }

    #[test]
    fn vector_visibility_drives_clipping_and_selection_silhouettes() {
        let commands = rectangle(0., 0., 20., 12.);
        let cases = [
            (
                "painted solid fallback",
                ElementKind::PaintedPath {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [20, 40, 60, 255],
                    stroke: [0; 4],
                    stroke_width: 3.,
                    fill_gradient: None,
                    stroke_gradient: None,
                },
                crate::vector::VectorPaintVisibility {
                    fill: true,
                    stroke: false,
                },
            ),
            (
                "opaque gradient",
                ElementKind::PaintedPath {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [0; 4],
                    stroke_width: 3.,
                    fill_gradient: Some(gradient(255)),
                    stroke_gradient: None,
                },
                crate::vector::VectorPaintVisibility {
                    fill: true,
                    stroke: false,
                },
            ),
            (
                "transparent gradient over opaque fallback",
                ElementKind::PaintedPath {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [20, 40, 60, 255],
                    stroke: [0; 4],
                    stroke_width: 3.,
                    fill_gradient: Some(gradient(0)),
                    stroke_gradient: None,
                },
                crate::vector::VectorPaintVisibility {
                    fill: false,
                    stroke: false,
                },
            ),
            (
                "zero-width stroke",
                ElementKind::PaintedPath {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [20, 40, 60, 255],
                    stroke_width: 0.,
                    fill_gradient: None,
                    stroke_gradient: None,
                },
                crate::vector::VectorPaintVisibility {
                    fill: false,
                    stroke: false,
                },
            ),
            (
                "visible fill and stroke",
                ElementKind::PaintedPath {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: [20, 40, 60, 255],
                    stroke: [80, 100, 120, 255],
                    stroke_width: 3.,
                    fill_gradient: None,
                    stroke_gradient: None,
                },
                crate::vector::VectorPaintVisibility {
                    fill: true,
                    stroke: true,
                },
            ),
        ];

        for (name, kind, expected) in cases {
            let width = match &kind {
                ElementKind::PaintedPath { stroke_width, .. } => *stroke_width,
                _ => unreachable!(),
            };
            let item = path_item(name, kind);
            let mesh = vector_mesh(&item, Pos2::ZERO, 1.).unwrap().unwrap();
            assert_eq!(
                !mesh.indices.is_empty(),
                expected.fill || expected.stroke,
                "clipping mesh: {name}"
            );
            assert_eq!(
                is_fill_only_vector_silhouette(&item),
                expected.fill && !expected.stroke,
                "fill-only selection fast path: {name}"
            );

            let reference = path_item(
                &format!("{name}-reference"),
                ElementKind::Path {
                    commands: commands.clone(),
                    fill_rule: FillRule::NonZero,
                    fill: if expected.fill {
                        [1, 2, 3, 255]
                    } else {
                        [0; 4]
                    },
                    stroke: if expected.stroke {
                        [4, 5, 6, 255]
                    } else {
                        [0; 4]
                    },
                    stroke_width: width,
                },
            );
            assert_eq!(
                item_silhouette_fingerprint(&item),
                item_silhouette_fingerprint(&reference),
                "selection cache visibility: {name}"
            );
        }
    }

    #[test]
    fn overlapping_mask_components_paint_once_and_preserve_interpolated_attributes() {
        let clip = RuntimeClip {
            alpha_components: None,
            invert: false,
            id: "overlap".into(),
            components: vec![
                neoflash_core::ClipComponent {
                    clip_ids: vec![],
                    commands: rectangle(0., 0., 60., 80.),
                    fill_rule: FillRule::NonZero,
                    matrix: [1., 0., 0., 1., 0., 0.],
                },
                // Reverse orientation through a reflection: the component still adds area.
                neoflash_core::ClipComponent {
                    clip_ids: vec![],
                    commands: rectangle(0., 0., 60., 80.),
                    fill_rule: FillRule::NonZero,
                    matrix: [-1., 0., 0., 1., 100., 20.],
                },
            ],
        };
        let clips = ClipMeshes::new(&[clip], Pos2::ZERO, 1.).unwrap();
        let mut source = Mesh::with_texture(egui::TextureId::Managed(42));
        for point in [
            Pos2::ZERO,
            Pos2::new(100., 0.),
            Pos2::new(100., 100.),
            Pos2::new(0., 100.),
        ] {
            source.vertices.push(Vertex {
                pos: point,
                uv: (point.to_vec2() / 100.).to_pos2(),
                color: Color32::from_rgba_premultiplied(point.x as u8, 0, 0, 128),
            });
        }
        source.indices = vec![0, 1, 2, 0, 2, 3];
        let clipped = clips.clip_mesh(&source, &["overlap".into()]).unwrap();
        assert_eq!(clipped.texture_id, source.texture_id);
        let area: f32 = clipped
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|tri| {
                let [a, b, c] = tri.map(|i| clipped.vertices[i as usize].pos);
                cross(a, b, c).abs() * 0.5
            })
            .sum();
        assert!(
            (area - 8400.).abs() < 0.05,
            "the overlap must not be rendered twice: {area}"
        );
        for vertex in clipped.vertices {
            assert!((vertex.uv.x - vertex.pos.x / 100.).abs() < 1e-5);
            assert!((vertex.uv.y - vertex.pos.y / 100.).abs() < 1e-5);
            assert_eq!(vertex.color.a(), 128);
            assert!((vertex.color.r() as f32 - vertex.pos.x).abs() <= 2.);
        }
    }

    #[test]
    fn selection_hatch_cache_reuses_clipping_across_pure_translation() {
        SHAPE_SELECTION_CACHE.with(|cache| cache.borrow_mut().clear());
        SHAPE_SELECTION_CACHE_BUILDS.with(|count| count.set(0));

        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900., 700.));
        let origin = Pos2::new(31., 47.);
        let zoom = 1.25;
        let size = egui::vec2(90., 54.);
        let mut item = DrawItem {
            element_id: "translated-selection".into(),
            name: String::new(),
            instance_path: vec!["translated-selection".into()],
            layer_id: "layer".into(),
            transform: [1., 0., 0., 1., 80., 70.],
            alpha: 1.,
            color_transform: Default::default(),
            path_fingerprint: None,
            kind: ElementKind::Rectangle {
                width: size.x as f64,
                height: size.y as f64,
                fill: [20, 40, 60, 255],
                stroke: [0; 4],
                stroke_width: 0.,
            },
        };
        let screen_rect = |item: &DrawItem| {
            let min = origin + crate::stage::affine(item.transform, Pos2::ZERO).to_vec2() * zoom;
            egui::Rect::from_min_size(min, size * zoom)
        };
        let render = |item: &DrawItem, rect: egui::Rect| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    ..Default::default()
                },
                |ctx| {
                    let painter = ctx
                        .layer_painter(egui::LayerId::background())
                        .with_clip_rect(viewport);
                    paint_shape_selection(&painter, item, origin, zoom, rect, None).unwrap();
                },
            )
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                Shape::Mesh(mesh) => Some((clipped.clip_rect, mesh)),
                _ => None,
            })
            .collect::<Vec<_>>()
        };

        let first_rect = screen_rect(&item);
        let first = render(&item, first_rect);
        item.transform[4] += 37.;
        item.transform[5] += 19.;
        let second_rect = screen_rect(&item);
        let delta = second_rect.min - first_rect.min;
        let second = render(&item, second_rect);

        assert!(!first.is_empty());
        assert_eq!(first.len(), second.len());
        assert_eq!(
            SHAPE_SELECTION_CACHE_BUILDS.with(std::cell::Cell::get),
            1,
            "pure translation must not rebuild hatch clipping"
        );
        assert_eq!(
            SHAPE_SELECTION_CACHE.with(|cache| cache.borrow().len()),
            1,
            "moving a selected shape must not accumulate one cache entry per pointer frame"
        );
        for ((first_clip, first_mesh), (second_clip, second_mesh)) in first.iter().zip(&second) {
            assert!((second_clip.min - (first_clip.min + delta)).length_sq() < 1e-6);
            assert!((second_clip.max - (first_clip.max + delta)).length_sq() < 1e-6);
            assert_eq!(first_mesh.indices, second_mesh.indices);
            assert_eq!(first_mesh.vertices.len(), second_mesh.vertices.len());
            for (first_vertex, second_vertex) in
                first_mesh.vertices.iter().zip(&second_mesh.vertices)
            {
                assert!((second_vertex.pos - (first_vertex.pos + delta)).length_sq() < 1e-6);
                assert_eq!(first_vertex.uv, second_vertex.uv);
                assert_eq!(first_vertex.color, second_vertex.color);
            }
        }

        // A cold rebuild at the translated position must be pixel-for-pixel equivalent to the
        // translated cache hit, including the NF2 hatch phase (which is anchored to the shape).
        SHAPE_SELECTION_CACHE.with(|cache| cache.borrow_mut().clear());
        let rebuilt = render(&item, second_rect);
        assert_eq!(second.len(), rebuilt.len());
        for ((cached_clip, cached_mesh), (rebuilt_clip, rebuilt_mesh)) in
            second.iter().zip(&rebuilt)
        {
            assert_eq!(cached_clip, rebuilt_clip);
            assert_eq!(cached_mesh.indices, rebuilt_mesh.indices);
            assert_eq!(cached_mesh.vertices.len(), rebuilt_mesh.vertices.len());
            for (cached_vertex, rebuilt_vertex) in
                cached_mesh.vertices.iter().zip(&rebuilt_mesh.vertices)
            {
                assert!((cached_vertex.pos - rebuilt_vertex.pos).length_sq() < 1e-6);
                assert_eq!(cached_vertex.uv, rebuilt_vertex.uv);
                assert_eq!(cached_vertex.color, rebuilt_vertex.color);
            }
        }
    }

    #[test]
    fn selection_hatch_cache_replaces_transform_and_zoom_variants_in_one_authored_slot() {
        SHAPE_SELECTION_CACHE.with(|cache| cache.borrow_mut().clear());
        SHAPE_SELECTION_CACHE_BUILDS.with(|count| count.set(0));

        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(700., 500.));
        let selection_rect = egui::Rect::from_min_size(Pos2::new(40., 30.), egui::vec2(360., 300.));
        let mut item = DrawItem {
            element_id: "variant-selection".into(),
            name: String::new(),
            instance_path: vec!["symbol".into(), "variant-selection".into()],
            layer_id: "layer".into(),
            transform: [1., 0., 0., 1., 100., 90.],
            alpha: 1.,
            color_transform: Default::default(),
            path_fingerprint: None,
            kind: ElementKind::Rectangle {
                width: 80.,
                height: 50.,
                fill: [20, 40, 60, 255],
                stroke: [0; 4],
                stroke_width: 0.,
            },
        };

        for step in 0..24 {
            let angle = step as f64 * 0.07;
            let scale = 0.8 + step as f64 * 0.015;
            let (sin, cos) = angle.sin_cos();
            item.transform = [
                cos * scale,
                sin * scale,
                -sin * scale,
                cos * scale,
                100.,
                90.,
            ];
            let zoom = 1. + step as f32 * 0.01;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    ..Default::default()
                },
                |ctx| {
                    let painter = ctx
                        .layer_painter(egui::LayerId::background())
                        .with_clip_rect(viewport);
                    paint_shape_selection(&painter, &item, Pos2::ZERO, zoom, selection_rect, None)
                        .unwrap();
                },
            );
            assert!(!output.shapes.is_empty());
            SHAPE_SELECTION_CACHE.with(|cache| {
                let cache = cache.borrow();
                assert_eq!(cache.len(), 1);
                assert_eq!(
                    cache.total_weight,
                    cache
                        .entries
                        .values()
                        .map(|entry| entry.weight)
                        .sum::<usize>()
                );
            });
        }

        assert_eq!(
            SHAPE_SELECTION_CACHE_BUILDS.with(std::cell::Cell::get),
            24,
            "each distinct scale/rotation/zoom variant must rebuild but replace its authored slot"
        );
    }

    #[test]
    fn selection_hatch_cache_enforces_entry_and_mesh_weight_limits() {
        fn slot(id: usize) -> ShapeSelectionCacheSlot {
            ShapeSelectionCacheSlot {
                layer_id: format!("layer-{id}"),
                instance_path: vec!["symbol".into(), format!("instance-{id}")],
                element_id: format!("element-{id}"),
            }
        }
        fn key(seed: usize) -> ShapeSelectionCacheKey {
            ShapeSelectionCacheKey {
                hash_a: seed as u64,
                hash_b: !(seed as u64),
                command_count: seed,
                linear_transform_bits: [0; 4],
                relative_translation_bits: [0; 2],
                zoom_bits: 0,
                rect_size_bits: [0; 2],
                visible_bits: [0; 4],
                hatch_bits: [0; 4],
                ppp_bits: 0,
                dark_mode: false,
            }
        }
        fn selection(vertices: usize, indices: usize) -> Arc<CachedShapeSelection> {
            let mut mesh = Mesh::default();
            mesh.vertices.resize(
                vertices,
                Vertex {
                    pos: Pos2::ZERO,
                    uv: Pos2::ZERO,
                    color: Color32::WHITE,
                },
            );
            mesh.indices.resize(indices, 0);
            Arc::new(CachedShapeSelection {
                anchor: Pos2::ZERO,
                clipped: vec![(egui::Rect::ZERO, Arc::new(mesh))],
            })
        }

        let mut entry_bounded = ShapeSelectionCache::with_limits(2, 1_000);
        for id in 0..7 {
            entry_bounded.insert(slot(id), key(id), selection(2, 4));
            assert!(entry_bounded.len() <= 2);
            assert!(entry_bounded.total_weight <= 1_000);
        }

        let mut weight_bounded = ShapeSelectionCache::with_limits(10, 12);
        for id in 0..7 {
            weight_bounded.insert(slot(id), key(id), selection(2, 2));
            assert!(weight_bounded.len() <= 10);
            assert!(weight_bounded.total_weight <= 12);
            assert_eq!(
                weight_bounded.total_weight,
                weight_bounded
                    .entries
                    .values()
                    .map(|entry| entry.weight)
                    .sum::<usize>()
            );
        }

        let mut oversized = ShapeSelectionCache::with_limits(10, 5);
        oversized.insert(slot(0), key(0), selection(3, 3));
        assert_eq!(oversized.len(), 0);
        assert_eq!(oversized.total_weight, 0);
    }

    #[test]
    fn selection_hatch_generation_is_viewport_bounded_and_preserves_phase() {
        let viewport = egui::Rect::from_min_max(Pos2::new(11., 17.), Pos2::new(311., 217.));
        let huge = egui::Rect::from_min_max(
            Pos2::new(-100_000_000., -100_000_000.),
            Pos2::new(100_000_000., 100_000_000.),
        );
        let (visible, hatch) = bounded_hatch_rect(huge, viewport).unwrap();
        assert_eq!(visible, viewport);
        assert!(hatch.width() <= viewport.width() + HATCH_PERIOD as f32);
        assert_eq!(
            ((hatch.left() as f64 + hatch.top() as f64) - (huge.left() as f64 + huge.top() as f64))
                .rem_euclid(HATCH_PERIOD),
            0.
        );
        assert!(bounded_hatch_rect(egui::Rect::NOTHING, viewport).is_none());
        assert!(
            bounded_hatch_rect(
                egui::Rect::from_min_size(Pos2::new(2000., 2000.), egui::vec2(20., 20.)),
                viewport
            )
            .is_none()
        );
        let empty = MaskMesh::new(Vec::new()).unwrap();
        assert!(
            intersect_mesh(&Mesh::default(), &empty)
                .unwrap()
                .indices
                .is_empty()
        );

        let ctx = egui::Context::default();
        let item = DrawItem {
            element_id: "huge".into(),
            name: String::new(),
            instance_path: vec!["huge".into()],
            layer_id: "layer".into(),
            transform: [1., 0., 0., 1., -100_000_000., -100_000_000.],
            alpha: 1.,
            color_transform: Default::default(),
            path_fingerprint: None,
            kind: ElementKind::Rectangle {
                width: 200_000_000.,
                height: 200_000_000.,
                fill: [0, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.,
            },
        };
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(viewport),
                ..Default::default()
            },
            |ctx| {
                let painter = ctx
                    .layer_painter(egui::LayerId::background())
                    .with_clip_rect(viewport);
                paint_shape_selection(&painter, &item, Pos2::ZERO, 1., huge, None).unwrap();
            },
        );
        assert!(!output.shapes.is_empty());
        assert!(
            output.shapes.len() < 1000,
            "hatches must be generated only across the viewport"
        );
    }
}
