//! Native soft masks composite existing painter meshes, then apply shared core coverage.
//! This path changes artwork rendering only; the NF2 stage chrome is untouched.
use super::{DrawStyle, draw_item};
use crate::{app::Editor, stage_clipping::capture};
use eframe::egui::{
    self, Color32, ColorImage, Pos2, Rect, TextureHandle, TextureId, TextureOptions,
};
use egui::epaint::{ClippedPrimitive, Mesh, Primitive};
use neoflash_core::{AuthoredScene, MaskCoverage};
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::Arc,
};

const MAX_PIXELS: usize = 4_194_304;
const MAX_WORK: usize = 128_000_000;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone)]
struct Cached {
    key: u64,
    handle: TextureHandle,
    rect: Rect,
    bytes: usize,
    // Retaining resource identities prevents pointer reuse after Replace/Undo.
    _sources: Vec<Arc<str>>,
}
#[derive(Clone, Default)]
struct Cache(Vec<Cached>);
struct Captured {
    primitives: Vec<ClippedPrimitive>,
    clip_ids: Vec<String>,
    group: Option<String>,
    bounds: Rect,
}
#[derive(Clone)]
struct Surface {
    left: i32,
    top: i32,
    width: usize,
    height: usize,
    pixels: Vec<Color32>,
}
impl Surface {
    fn new(rect: Rect, scale: f32) -> Result<Self, String> {
        let left = (rect.left() * scale).floor() as i32;
        let top = (rect.top() * scale).floor() as i32;
        let width = ((rect.right() * scale).ceil() as i32 - left).max(0) as usize;
        let height = ((rect.bottom() * scale).ceil() as i32 - top).max(0) as usize;
        if width.saturating_mul(height) > MAX_PIXELS {
            return Err("Native mask raster exceeds four million pixels".into());
        }
        Ok(Self {
            left,
            top,
            width,
            height,
            pixels: vec![Color32::TRANSPARENT; width * height],
        })
    }
    fn composite(&mut self, source: &Self) {
        for y in 0..source.height {
            for x in 0..source.width {
                let dx = source.left + x as i32 - self.left;
                let dy = source.top + y as i32 - self.top;
                if dx >= 0 && dy >= 0 && dx < self.width as i32 && dy < self.height as i32 {
                    let target = &mut self.pixels[dy as usize * self.width + dx as usize];
                    *target = over(source.pixels[y * source.width + x], *target);
                }
            }
        }
    }
    fn mask(
        &mut self,
        coverage: &MaskCoverage,
        ids: &[String],
        origin: Pos2,
        zoom: f32,
        scale: f32,
    ) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }
        let start = [
            (self.left as f64 / f64::from(scale) - f64::from(origin.x)) / f64::from(zoom),
            (self.top as f64 / f64::from(scale) - f64::from(origin.y)) / f64::from(zoom),
        ];
        let alpha = coverage.raster(
            ids,
            start,
            [1. / f64::from(scale * zoom); 2],
            self.width,
            self.height,
        )?;
        for (pixel, alpha) in self.pixels.iter_mut().zip(alpha) {
            *pixel = pixel.gamma_multiply(f32::from(alpha) / 255.);
        }
        Ok(())
    }
}

pub(super) fn paint(
    editor: &mut Editor,
    painter: &egui::Painter,
    scene: &AuthoredScene,
    style: DrawStyle,
) -> Result<(), String> {
    let mut draws = Vec::with_capacity(scene.draws.len());
    let mut images = HashMap::new();
    for draw in &scene.draws {
        let texture = editor.bitmap_textures.texture_transformed(
            painter.ctx(),
            &editor.project,
            &draw.item.kind,
            draw.item.color_transform,
        );
        if let Some(id) = texture
            && let Some(image) = editor.bitmap_textures.image(id)
        {
            images.insert(id, image);
        }
        let primitives = capture(painter, |target| {
            draw_item(
                target,
                &draw.item,
                DrawStyle {
                    bitmap_texture: texture,
                    tint: style.tint.or_else(|| {
                        draw.outline_color
                            .map(|c| Color32::from_rgb(c[0], c[1], c[2]))
                    }),
                    outlines: style.outlines || draw.outline_color.is_some(),
                    ..style
                },
            )
        });
        let bounds = primitives
            .iter()
            .filter_map(|p| match &p.primitive {
                Primitive::Mesh(mesh) => Some(mesh.calc_bounds().intersect(p.clip_rect)),
                _ => None,
            })
            .fold(Rect::NOTHING, Rect::union)
            .intersect(painter.clip_rect());
        draws.push(Captured {
            primitives,
            clip_ids: draw.clip_ids.clone(),
            group: draw.mask_group.clone(),
            bounds,
        });
    }
    let bounds = draws
        .iter()
        .map(|d| d.bounds)
        .fold(Rect::NOTHING, Rect::union);
    if !bounds.is_positive() || !bounds.is_finite() {
        return Ok(());
    }
    let font = Arc::new(painter.ctx().fonts(|fonts| fonts.image()));
    images.insert(TextureId::Managed(0), (font.clone(), true));
    // Limit offscreen resolution, not stage zoom, on high-DPI/large windows.
    let scale = painter
        .ctx()
        .pixels_per_point()
        .min((MAX_PIXELS as f32 / (bounds.width() + 2.) / (bounds.height() + 2.)).sqrt())
        .max(0.01);
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_vec(&scene.clips)
        .map_err(|e| e.to_string())?
        .hash(&mut hash);
    for group in &scene.mask_groups {
        group.id.hash(&mut hash);
        group.parent_id.hash(&mut hash);
        group.clip_ids.hash(&mut hash);
    }
    [scale, style.origin.x, style.origin.y, style.zoom]
        .map(f32::to_bits)
        .hash(&mut hash);
    font.size.hash(&mut hash);
    for draw in &draws {
        draw.clip_ids.hash(&mut hash);
        draw.group.hash(&mut hash);
        for primitive in &draw.primitives {
            if let Primitive::Mesh(mesh) = &primitive.primitive {
                mesh.texture_id.hash(&mut hash);
                mesh.indices.hash(&mut hash);
                for v in &mesh.vertices {
                    [v.pos.x, v.pos.y, v.uv.x, v.uv.y]
                        .map(f32::to_bits)
                        .hash(&mut hash);
                    v.color.hash(&mut hash);
                }
                [
                    primitive.clip_rect.min.x,
                    primitive.clip_rect.min.y,
                    primitive.clip_rect.max.x,
                    primitive.clip_rect.max.y,
                ]
                .map(f32::to_bits)
                .hash(&mut hash);
            }
        }
    }
    let mask_resources: HashSet<_> = scene
        .clips
        .iter()
        .flat_map(|clip| clip.alpha_components.iter().flatten())
        .filter_map(|component| {
            component
                .resource_bitmap
                .as_ref()
                .map(|paint| paint.resource_id.as_str())
        })
        .collect();
    let sources: Vec<_> = editor
        .project
        .resources
        .iter()
        .filter(|resource| mask_resources.contains(resource.id.as_str()))
        .map(|r| r.data_base64.clone())
        .collect();
    for source in &sources {
        (Arc::as_ptr(source) as *const () as usize).hash(&mut hash);
    }
    let key = hash.finish();
    let cache_id = egui::Id::new("neoflash-alpha-mask-rasters");
    let cached = painter.ctx().data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<Cache>(cache_id);
        cache.0.iter().position(|entry| entry.key == key).map(|i| {
            let entry = cache.0.remove(i);
            cache.0.push(entry.clone());
            entry
        })
    });
    if let Some(cached) = cached {
        painter.image(
            cached.handle.id(),
            cached.rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
        return Ok(());
    }
    let coverage = MaskCoverage::new(&editor.project, &scene.clips)?;
    let mut renderer = Renderer {
        scene,
        draws: &draws,
        images: &images,
        coverage: &coverage,
        origin: style.origin,
        zoom: style.zoom,
        scale,
        work: 0,
        active: HashSet::new(),
    };
    let surface = renderer.group(None, &[])?;
    let Some(surface) = surface else {
        return Ok(());
    };
    let rect = Rect::from_min_size(
        Pos2::new(surface.left as f32 / scale, surface.top as f32 / scale),
        egui::vec2(surface.width as f32 / scale, surface.height as f32 / scale),
    );
    let bytes = surface.pixels.len() * 4;
    let image = ColorImage::new([surface.width, surface.height], surface.pixels);
    let handle = painter
        .ctx()
        .load_texture("authored-alpha-mask", image, TextureOptions::LINEAR);
    painter.image(
        handle.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
        Color32::WHITE,
    );
    painter.ctx().data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<Cache>(cache_id);
        while !cache.0.is_empty()
            && (cache.0.len() >= 16
                || cache.0.iter().map(|c| c.bytes).sum::<usize>() + bytes > MAX_CACHE_BYTES)
        {
            cache.0.remove(0);
        }
        cache.0.push(Cached {
            key,
            handle,
            rect,
            bytes,
            _sources: sources,
        });
    });
    Ok(())
}
struct Renderer<'a> {
    scene: &'a AuthoredScene,
    draws: &'a [Captured],
    images: &'a HashMap<TextureId, (Arc<ColorImage>, bool)>,
    coverage: &'a MaskCoverage,
    origin: Pos2,
    zoom: f32,
    scale: f32,
    work: usize,
    active: HashSet<String>,
}
impl Renderer<'_> {
    fn belongs(&self, draw: &Captured, group: Option<&str>) -> bool {
        let mut current = draw.group.as_deref();
        for _ in 0..=self.scene.mask_groups.len() {
            if current == group {
                return true;
            }
            let Some(id) = current else {
                return false;
            };
            current = self
                .scene
                .mask_groups
                .iter()
                .find(|g| g.id == id)
                .and_then(|g| g.parent_id.as_deref());
        }
        false
    }
    fn group(&mut self, id: Option<&str>, inherited: &[String]) -> Result<Option<Surface>, String> {
        if let Some(id) = id
            && !self.active.insert(id.into())
        {
            return Err("Native mask group cycle".into());
        }
        let bounds = self
            .draws
            .iter()
            .filter(|d| self.belongs(d, id))
            .map(|d| d.bounds)
            .fold(Rect::NOTHING, Rect::union);
        if !bounds.is_positive() {
            if let Some(id) = id {
                self.active.remove(id);
            }
            return Ok(None);
        }
        let group = id.and_then(|id| self.scene.mask_groups.iter().find(|g| g.id == id));
        let own = group.map(|g| g.clip_ids.clone()).unwrap_or_default();
        let mut deferred = inherited.to_vec();
        deferred.extend(own.iter().cloned());
        let mut surface = Surface::new(bounds, self.scale)?;
        self.work += surface.pixels.len();
        if self.work > MAX_WORK
            || self.active.len().saturating_mul(surface.pixels.len()) > MAX_PIXELS * 4
        {
            return Err("Native mask group work budget exceeded; simplify nested masks".into());
        }
        let mut emitted = HashSet::new();
        for index in 0..self.draws.len() {
            let draw = &self.draws[index];
            if !self.belongs(draw, id) || !draw.bounds.is_positive() {
                continue;
            }
            if draw.group.as_deref() == id {
                let extra: Vec<_> = draw
                    .clip_ids
                    .iter()
                    .filter(|c| !deferred.contains(c))
                    .cloned()
                    .collect();
                let mut part = Surface::new(draw.bounds, self.scale)?;
                for primitive in &draw.primitives {
                    if let Primitive::Mesh(mesh) = &primitive.primitive {
                        raster_mesh(
                            &mut part,
                            mesh,
                            primitive.clip_rect,
                            self.scale,
                            self.images,
                            &mut self.work,
                        )?;
                    }
                }
                part.mask(self.coverage, &extra, self.origin, self.zoom, self.scale)?;
                surface.composite(&part);
            } else {
                let mut child = draw
                    .group
                    .as_deref()
                    .ok_or("Native mask group is missing")?;
                for _ in 0..self.scene.mask_groups.len() {
                    let parent = self
                        .scene
                        .mask_groups
                        .iter()
                        .find(|g| g.id == child)
                        .ok_or("Native mask group is missing")?
                        .parent_id
                        .as_deref();
                    if parent == id {
                        break;
                    }
                    child = parent.ok_or("Native mask group ancestry is inconsistent")?;
                }
                let child = child.to_owned();
                if emitted.insert(child.clone())
                    && let Some(part) = self.group(Some(&child), &deferred)?
                {
                    surface.composite(&part);
                }
            }
        }
        surface.mask(self.coverage, &own, self.origin, self.zoom, self.scale)?;
        if let Some(id) = id {
            self.active.remove(id);
        }
        Ok(Some(surface))
    }
}
fn over(source: Color32, target: Color32) -> Color32 {
    let inv = 255 - u32::from(source.a());
    let s = source.to_array();
    let t = target.to_array();
    let c = std::array::from_fn::<_, 4, _>(|i| {
        (u32::from(s[i]) + (u32::from(t[i]) * inv + 127) / 255).min(255) as u8
    });
    Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3])
}
fn raster_mesh(
    surface: &mut Surface,
    mesh: &Mesh,
    clip: Rect,
    scale: f32,
    images: &HashMap<TextureId, (Arc<ColorImage>, bool)>,
    work: &mut usize,
) -> Result<(), String> {
    let (image, smooth) = images
        .get(&mesh.texture_id)
        .ok_or("Native mask cannot read the artwork texture")?;
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let vertices = triangle.map(|i| mesh.vertices[i as usize]);
        let mut points = vertices.map(|v| v.pos * scale);
        let mut verts = vertices;
        let mut area = edge(points[0], points[1], points[2]);
        if area == 0. || !area.is_finite() {
            continue;
        }
        if area < 0. {
            points.swap(1, 2);
            verts.swap(1, 2);
            area = -area;
        }
        let bounds = Rect::from_points(&points)
            .intersect(Rect::from_min_max(clip.min * scale, clip.max * scale));
        let x0 = (bounds.min.x.floor() as i32).max(surface.left);
        let y0 = (bounds.min.y.floor() as i32).max(surface.top);
        let x1 = (bounds.max.x.ceil() as i32).min(surface.left + surface.width as i32);
        let y1 = (bounds.max.y.ceil() as i32).min(surface.top + surface.height as i32);
        *work += (x1 - x0).max(0) as usize * (y1 - y0).max(0) as usize;
        if *work > MAX_WORK {
            return Err("Native mask mesh work budget exceeded".into());
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                let e = [
                    edge(points[1], points[2], p),
                    edge(points[2], points[0], p),
                    edge(points[0], points[1], p),
                ];
                if (0..3).any(|i| {
                    e[i] < 0. || (e[i] == 0. && !top_left(points[(i + 1) % 3], points[(i + 2) % 3]))
                }) {
                    continue;
                }
                let w = e.map(|v| v / area);
                let uv = Pos2::new(
                    (0..3).map(|i| verts[i].uv.x * w[i]).sum(),
                    (0..3).map(|i| verts[i].uv.y * w[i]).sum(),
                );
                let tex = texture(image, uv, *smooth);
                let rgba = std::array::from_fn::<_, 4, _>(|channel| {
                    let color: f32 = (0..3)
                        .map(|i| f32::from(verts[i].color[channel]) * w[i])
                        .sum();
                    (color * tex[channel] / 255.).round().clamp(0., 255.) as u8
                });
                let index =
                    (y - surface.top) as usize * surface.width + (x - surface.left) as usize;
                surface.pixels[index] = over(
                    Color32::from_rgba_premultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
                    surface.pixels[index],
                );
            }
        }
    }
    Ok(())
}
fn edge(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}
fn top_left(a: Pos2, b: Pos2) -> bool {
    b.y < a.y || (b.y == a.y && b.x > a.x)
}
fn texture(image: &ColorImage, uv: Pos2, smooth: bool) -> [f32; 4] {
    let pixel = |x: i32, y: i32| {
        image.pixels[y.clamp(0, image.size[1] as i32 - 1) as usize * image.size[0]
            + x.clamp(0, image.size[0] as i32 - 1) as usize]
            .to_array()
            .map(f32::from)
    };
    let x = uv.x * image.size[0] as f32;
    let y = uv.y * image.size[1] as f32;
    if !smooth {
        return pixel(x.floor() as i32, y.floor() as i32);
    }
    let (x, y) = (x - 0.5, y - 0.5);
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let a = pixel(ix as i32, iy as i32);
    let b = pixel(ix as i32 + 1, iy as i32);
    let c = pixel(ix as i32, iy as i32 + 1);
    let d = pixel(ix as i32 + 1, iy as i32 + 1);
    std::array::from_fn(|i| {
        (a[i] * (1. - fx) + b[i] * fx) * (1. - fy) + (c[i] * (1. - fx) + d[i] * fx) * fy
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::*;

    fn rect(id: &str, x: f64, fill: [u8; 4]) -> Element {
        Element {
            id: id.into(),
            name: id.into(),
            transform: Transform {
                x,
                ..Default::default()
            },
            kind: ElementKind::Rectangle {
                width: 20.,
                height: 10.,
                fill,
                stroke: [0; 4],
                stroke_width: 0.,
            },
            ..Default::default()
        }
    }
    #[test]
    fn native_soft_mask_flattens_overlap_once_and_replaces_cached_png_pixels() {
        use image::ImageEncoder;
        let ctx = egui::Context::default();
        let mut project = Project::blank();
        project.format_version = 9;
        let content = &mut project.symbols[0].timeline.layers[0];
        content.properties.mask = Some("soft-mask".into());
        content.keyframes[0].elements = vec![
            rect("red", 0., [255, 0, 0, 255]),
            rect("blue", 10., [0, 0, 255, 255]),
        ];
        let mut mask = content.clone();
        mask.id = "soft-mask".into();
        mask.name = "Soft mask".into();
        mask.properties.mask = None;
        mask.properties.role = LayerRole::Mask;
        mask.properties.mask_mode = MaskMode::Alpha;
        mask.keyframes[0].elements = vec![Element {
            kind: ElementKind::Bitmap {
                resource_id: "alpha-png".into(),
                width: 30,
                height: 10,
                smoothing: false,
            },
            ..rect("mask-source", 0., [0; 4])
        }];
        project.symbols[0].timeline.layers.push(mask);
        let resource = |alpha: u8| {
            let rgba: Vec<_> = (0..30 * 10).flat_map(|_| [0, 0, 0, alpha]).collect();
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png)
                .write_image(&rgba, 30, 10, image::ExtendedColorType::Rgba8)
                .unwrap();
            ResourceUnit::from_bytes("alpha-png", "alpha.png", "image/png", &png).unwrap()
        };
        project.resources.push(resource(128));
        project.validate().unwrap();
        let mut editor = Editor::from_project(&ctx, project);
        let render = |editor: &mut Editor, frame| {
            let scene = evaluate_scene(&editor.project, &editor.project.root, frame, false, true);
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(100., 100.))),
                    ..Default::default()
                },
                |ctx| {
                    let painter = ctx
                        .layer_painter(egui::LayerId::background())
                        .with_clip_rect(Rect::from_min_size(Pos2::ZERO, egui::vec2(100., 100.)));
                    editor.paint_scene_at(&painter, &scene, Pos2::ZERO, 1.);
                },
            );
            let handle = ctx.data(|data| {
                data.get_temp::<Cache>(egui::Id::new("neoflash-alpha-mask-rasters"))
                    .unwrap()
                    .0
                    .last()
                    .unwrap()
                    .handle
                    .id()
            });
            let image = &output
                .textures_delta
                .set
                .iter()
                .find(|(id, _)| *id == handle)
                .unwrap()
                .1
                .image;
            match image {
                egui::ImageData::Color(image) => image.clone(),
            }
        };
        let image = render(&mut editor, 0);
        assert_eq!(image.size, [30, 10]);
        assert_eq!(image.pixels[5 * 30 + 5].to_array(), [128, 0, 0, 128]);
        assert_eq!(
            image.pixels[5 * 30 + 15].to_array(),
            [0, 0, 128, 128],
            "overlap is flattened before half-alpha masking"
        );
        assert_eq!(image.pixels[5 * 30 + 25].to_array(), [0, 0, 128, 128]);
        editor.project.resources[0] = resource(64);
        let replaced = render(&mut editor, 0);
        assert_eq!(
            replaced.pixels[5 * 30 + 15].to_array(),
            [0, 0, 64, 64],
            "resource replacement invalidates the native raster cache"
        );
        let contour = |x: f64, width: f64| {
            vec![
                PathCommand::MoveTo { x, y: 0. },
                PathCommand::LineTo {
                    x: x + width,
                    y: 0.,
                },
                PathCommand::LineTo {
                    x: x + width,
                    y: 10.,
                },
                PathCommand::LineTo { x, y: 10. },
                PathCommand::Close,
            ]
        };
        let mut commands = contour(0., 30.);
        commands.extend(contour(5., 5.));
        let gradient = GradientPaint {
            kind: GradientKind::Linear,
            matrix: [30. / 1638.4, 0., 0., 1., 15., 0.],
            focal_point_ratio: 0.,
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [0; 4],
                },
                GradientStop {
                    ratio: 255,
                    color: [0, 0, 0, 255],
                },
            ],
        };
        let timeline = &mut editor.project.symbols[0].timeline;
        timeline.frame_count = 11;
        let mask = &mut timeline.layers[1];
        let source = &mut mask.keyframes[0].elements[0];
        source.kind = ElementKind::PaintedPath {
            commands,
            fill_rule: FillRule::EvenOdd,
            fill: [0, 0, 0, 255],
            stroke: [0; 4],
            stroke_width: 0.,
            fill_gradient: Some(gradient),
            stroke_gradient: None,
        };
        source.transform = Transform {
            x: 30.,
            scale_x: -1.,
            ..Default::default()
        };
        let mut end = mask.keyframes[0].clone();
        end.frame = 10;
        end.elements[0].transform.x = 40.;
        mask.keyframes[0].tween = Some(Easing::Linear);
        mask.keyframes.push(end);
        editor.project.validate().unwrap();
        let gradient = render(&mut editor, 0);
        assert!(
            (i32::from(gradient.pixels[5 * 30 + 15].a()) - 123).abs() <= 1,
            "mirrored gradient retains local paint coordinates"
        );
        assert_eq!(
            gradient.pixels[5 * 30 + 22].a(),
            0,
            "compound hole remains empty"
        );
        let moved = render(&mut editor, 10);
        assert!(
            (i32::from(moved.pixels[5 * 30 + 25].a()) - 123).abs() <= 1,
            "tweened mask follows its world transform"
        );
        let mut child = editor.project.symbols[0].clone();
        child.id = "nested-target".into();
        child.name = "Nested target".into();
        child.timeline.frame_count = 1;
        child.timeline.layers[0].id = "inner-content".into();
        child.timeline.layers[1].keyframes = vec![Keyframe {
            elements: vec![Element {
                kind: ElementKind::Rectangle {
                    width: 30.,
                    height: 10.,
                    fill: [0, 0, 0, 128],
                    stroke: [0; 4],
                    stroke_width: 0.,
                },
                ..rect("inner-mask-source", 0., [0; 4])
            }],
            ..Default::default()
        }];
        editor.project = Project::blank();
        editor.project.format_version = 9;
        let timeline = &mut editor.project.symbols[0].timeline;
        timeline.layers[0].properties.mask = Some("outer-mask".into());
        timeline.layers[0].keyframes[0].elements = vec![Element {
            kind: ElementKind::Instance {
                symbol_id: child.id.clone(),
                playback: InstancePlayback::Independent,
            },
            ..rect("nested-placement", 0., [0; 4])
        }];
        let mut outer = child.timeline.layers[1].clone();
        outer.id = "outer-mask".into();
        outer.name = "Outer mask".into();
        outer.keyframes[0].elements[0].id = "outer-mask-source".into();
        timeline.layers.push(outer);
        editor.project.symbols.push(child);
        editor.project.validate().unwrap();
        let nested = render(&mut editor, 0);
        assert_eq!(
            nested.pixels[5 * 30 + 15].to_array(),
            [0, 0, 64, 64],
            "nested target alpha applies each group once, without leaking outer coverage into the inner source"
        );
    }
}
