//! Intersect painter meshes with filled vector unions. Vertex interpolation
//! preserves bitmap UVs and text colors; overlapping mask pieces paint once.
use eframe::egui::{self, Color32, Pos2, Shape};
use egui::epaint::{Mesh, Vertex};
use neoflash_core::RuntimeClip;
use std::collections::HashMap;

type Triangle = [Pos2; 3];
pub(crate) struct ClipMeshes(HashMap<String, Vec<Triangle>>);
impl ClipMeshes {
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
            map.insert(clip.id.clone(), triangles);
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
        for primitive in primitives {
            let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else {
                continue;
            };
            let mut result = mesh;
            for id in ids {
                let mask = self
                    .0
                    .get(id)
                    .ok_or_else(|| format!("Missing authored mask {id}"))?;
                result = intersect_mesh(&result, mask)?;
            }
            painter
                .with_clip_rect(painter.clip_rect().intersect(primitive.clip_rect))
                .add(Shape::mesh(result));
        }
        Ok(())
    }
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
fn half_plane(poly: &[Vertex], a: Pos2, b: Pos2, direction: f32) -> (Vec<Vertex>, Vec<Vertex>) {
    let mut inside = vec![];
    let mut outside = vec![];
    if poly.is_empty() {
        return (inside, outside);
    }
    let mut previous = *poly.last().unwrap();
    let mut pd = cross(a, b, previous.pos) * direction;
    for &current in poly {
        let cd = cross(a, b, current.pos) * direction;
        if (pd >= 0.) != (cd >= 0.) {
            let vertex = interpolate(previous, current, (pd / (pd - cd)).clamp(0., 1.));
            inside.push(vertex);
            outside.push(vertex);
        }
        if cd >= 0. {
            inside.push(current);
        } else {
            outside.push(current);
        }
        previous = current;
        pd = cd;
    }
    (inside, outside)
}
fn intersect_mesh(mesh: &Mesh, mask: &[Triangle]) -> Result<Mesh, String> {
    let mut out = Mesh::with_texture(mesh.texture_id);
    let mut operations = 0usize;
    for indices in mesh.indices.as_chunks::<3>().0 {
        let source: Vec<_> = indices.iter().map(|i| mesh.vertices[*i as usize]).collect();
        let mut remaining = vec![source];
        let mut accepted = vec![];
        for triangle in mask {
            let orientation = cross(triangle[0], triangle[1], triangle[2]);
            if orientation.abs() < 1e-8 {
                continue;
            }
            let direction = orientation.signum();
            let mut next = vec![];
            for mut polygon in remaining {
                for edge in 0..3 {
                    operations += polygon.len();
                    if operations > 4_000_000 {
                        return Err(
                            "Mask preview is too complex; simplify vector mask contours".into()
                        );
                    }
                    let (inside, outside) = half_plane(
                        &polygon,
                        triangle[edge],
                        triangle[(edge + 1) % 3],
                        direction,
                    );
                    if outside.len() >= 3 {
                        next.push(outside);
                    }
                    polygon = inside;
                    if polygon.len() < 3 {
                        break;
                    }
                }
                if polygon.len() >= 3 {
                    accepted.push(polygon);
                }
            }
            remaining = next;
            if remaining.is_empty() {
                break;
            }
        }
        for polygon in accepted {
            let base = out.vertices.len() as u32;
            for i in 1..polygon.len() - 1 {
                out.indices
                    .extend_from_slice(&[base, base + i as u32, base + i as u32 + 1]);
            }
            out.vertices.extend(polygon);
        }
    }
    Ok(out)
}
