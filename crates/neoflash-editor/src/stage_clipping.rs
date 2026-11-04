//! Intersect painter meshes with filled vector unions. Vertex interpolation
//! preserves bitmap UVs and text colors; overlapping mask pieces paint once.
use eframe::egui::{self, Color32, Pos2, Shape};
use egui::epaint::{Mesh, Vertex};
use neoflash_core::{DrawItem, Element, ElementKind, FillRule, PathCommand, RuntimeClip};
use std::collections::HashMap;

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
    fn new(triangles: Vec<Triangle>) -> Result<Self, String> {
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
        let union = crate::vector::tessellate(&contours, FillRule::NonZero, None, 0.01)?;
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

pub(crate) struct ClipMeshes(HashMap<String, MaskMesh>);
impl ClipMeshes {
    /// The same fill/stroke silhouette used to paint vector artwork, including holes.
    /// This clips the existing NF2 hatch; it does not define another selection style.
    pub(crate) fn for_shape(item: &DrawItem, origin: Pos2, zoom: f32) -> Result<Self, String> {
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
        Ok(Self(HashMap::from([(
            item.element_id.clone(),
            MaskMesh::new(triangles)?,
        )])))
    }

    pub(crate) fn new(clips: &[RuntimeClip], origin: Pos2, zoom: f32) -> Result<Self, String> {
        let mut map = HashMap::new();
        for clip in clips {
            let mut triangles = vec![];
            for component in &clip.components {
                let tess = crate::vector::tessellate(
                    &component.commands,
                    component.fill_rule,
                    None,
                    (0.25 / zoom.max(0.01)).clamp(0.001, 1.),
                )?;
                for indices in tess.indices.as_chunks::<3>().0 {
                    triangles.push(std::array::from_fn(|i| {
                        let point = tess.vertices[indices[i] as usize];
                        origin
                            + crate::stage::affine(component.matrix, Pos2::new(point.x, point.y))
                                .to_vec2()
                                * zoom
                    }));
                }
            }
            map.insert(clip.id.clone(), MaskMesh::new(triangles)?);
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
            let mask = self
                .0
                .get(id)
                .ok_or_else(|| format!("Missing authored mask {id}"))?;
            result = intersect_mesh(&result, mask)?;
        }
        Ok(result)
    }
}

fn paint_clipped(
    painter: &egui::Painter,
    clip: impl Fn(&Mesh) -> Result<Mesh, String>,
    paint: impl FnOnce(&egui::Painter),
) -> Result<(), String> {
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
    let primitives = painter
        .ctx()
        .tessellate(shapes, painter.ctx().pixels_per_point());
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

/// Geometry only: the union of visible vector fills and strokes in the requested space.
/// Non-vector placements return None; their bounds are not a vector silhouette.
pub(crate) fn vector_mesh(
    item: &DrawItem,
    origin: Pos2,
    zoom: f32,
) -> Result<Option<Mesh>, String> {
    let (fill, stroke, width) = match &item.kind {
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
        } => (fill[3] != 0, stroke[3] != 0, *stroke_width),
        ElementKind::PaintedPath {
            fill,
            stroke,
            stroke_width,
            fill_gradient,
            stroke_gradient,
            ..
        } => (
            fill_gradient
                .as_ref()
                .map_or(fill[3] != 0, |g| g.stops.iter().any(|s| s.color[3] != 0)),
            stroke_gradient
                .as_ref()
                .map_or(stroke[3] != 0, |g| g.stops.iter().any(|s| s.color[3] != 0)),
            *stroke_width,
        ),
        _ => return Ok(None),
    };
    let element = Element {
        kind: item.kind.clone(),
        ..Default::default()
    };
    let Some((commands, rule)) = element.to_path_commands() else {
        return Ok(None);
    };
    let m = item.transform;
    let scale = m[0].hypot(m[1]).max(m[2].hypot(m[3])) as f32 * zoom;
    let tolerance = (0.25 / scale.max(0.01)).clamp(0.001, 1.);
    let mut mesh = Mesh::default();
    for width in [
        fill.then_some(None),
        (stroke && width > 0.).then_some(Some(width)),
    ]
    .into_iter()
    .flatten()
    {
        let geometry = crate::vector::tessellate(&commands, rule, width, tolerance)?;
        let base = mesh.vertices.len() as u32;
        for vertex in geometry.vertices {
            mesh.colored_vertex(
                origin + crate::stage::affine(m, Pos2::new(vertex.x, vertex.y)).to_vec2() * zoom,
                Color32::WHITE,
            );
        }
        mesh.indices
            .extend(geometry.indices.into_iter().map(|i| base + i));
    }
    Ok(Some(mesh))
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
    let shape = ClipMeshes::for_shape(item, origin, zoom)?;
    paint_clipped(
        &painter.with_clip_rect(visible),
        |mesh| {
            let mesh = shape.clip_mesh(mesh, std::slice::from_ref(&item.element_id))?;
            if let Some((clips, ids)) = masks {
                clips.clip_mesh(&mesh, ids)
            } else {
                Ok(mesh)
            }
        },
        |p| neoflash_ui::components::canvas::paint_shape_selection(p, p.ctx(), hatch),
    )
}
/// Restrict NF2's hatch-generation loop to the visible viewport. Its diagonals have
/// constant x+y modulo 4; extending the bounded left edge by less than one period
/// preserves that phase without iterating across distant/offscreen selection bounds.
fn bounded_hatch_rect(rect: egui::Rect, clip: egui::Rect) -> Option<(egui::Rect, egui::Rect)> {
    let visible = rect.intersect(clip);
    if !rect.is_finite() || !visible.is_finite() || !visible.is_positive() {
        return None;
    }
    let shift = ((visible.left() as f64 + visible.top() as f64)
        - (rect.left() as f64 + rect.top() as f64))
        .rem_euclid(4.);
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

    #[test]
    fn overlapping_mask_components_paint_once_and_preserve_interpolated_attributes() {
        let clip = RuntimeClip {
            id: "overlap".into(),
            components: vec![
                neoflash_core::ClipComponent {
                    commands: rectangle(0., 0., 60., 80.),
                    fill_rule: FillRule::NonZero,
                    matrix: [1., 0., 0., 1., 0., 0.],
                },
                // Reverse orientation through a reflection: the component still adds area.
                neoflash_core::ClipComponent {
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
    fn selection_hatch_generation_is_viewport_bounded_and_preserves_phase() {
        let viewport = egui::Rect::from_min_max(Pos2::new(11., 17.), Pos2::new(311., 217.));
        let huge = egui::Rect::from_min_max(
            Pos2::new(-100_000_000., -100_000_000.),
            Pos2::new(100_000_000., 100_000_000.),
        );
        let (visible, hatch) = bounded_hatch_rect(huge, viewport).unwrap();
        assert_eq!(visible, viewport);
        assert!(hatch.width() <= viewport.width() + 4.);
        assert_eq!(
            ((hatch.left() as f64 + hatch.top() as f64) - (huge.left() as f64 + huge.top() as f64))
                .rem_euclid(4.),
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
