#[path = "stage_alpha_masks.rs"]
mod alpha_masks;
#[path = "panels/appearance.rs"]
pub(crate) mod appearance;
#[path = "../../neoflash-core/src/deco_spray.rs"]
pub(crate) mod deco_spray;
#[path = "../../neoflash-core/src/ik.rs"]
pub(crate) mod ik;
#[path = "panels/properties_movie_clip.rs"]
pub(crate) mod properties_movie_clip;
#[path = "panels/properties_tool.rs"]
pub(crate) mod properties_tool;

use crate::app::{Editor, Tool};
use crate::stage_mapping::StageMapping;
use crate::symbol_edit::SymbolEditEntry;
use crate::theme::UiPalette;
use eframe::egui::{
    self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
};
use neoflash_core::*;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Clone, Default)]
struct SymbolContextClips {
    ids: Vec<String>,
    clips: Vec<RuntimeClip>,
}

impl SymbolContextClips {
    fn attach(&self, scene: &mut AuthoredScene, group_suffix: &str) {
        if self.ids.is_empty() {
            return;
        }
        for clip in &self.clips {
            if !scene.clips.iter().any(|existing| existing.id == clip.id) {
                scene.clips.push(clip.clone());
            }
        }
        for draw in &mut scene.draws {
            for id in &self.ids {
                if !draw.clip_ids.contains(id) {
                    draw.clip_ids.push(id.clone());
                }
            }
        }

        // Alpha/luminance masks must apply after the edited instance has composited its own
        // children. Wrap existing root groups instead of multiplying mask coverage per draw.
        if self
            .clips
            .iter()
            .any(|clip| clip.alpha_components.is_some() || clip.invert)
        {
            let group_id = format!("@symbol-edit-context:{group_suffix}");
            for group in &mut scene.mask_groups {
                if group.parent_id.is_none() {
                    group.parent_id = Some(group_id.clone());
                }
            }
            for draw in &mut scene.draws {
                if draw.mask_group.is_none() {
                    draw.mask_group = Some(group_id.clone());
                }
            }
            scene.mask_groups.push(AuthoredMaskGroup {
                id: group_id,
                parent_id: None,
                clip_ids: self.ids.clone(),
                blend_mode: BlendMode::Normal,
            });
        }
    }

    fn extend_from_branch(
        &mut self,
        scene: &AuthoredScene,
        placement: &str,
        branch_ids: impl Iterator<Item = String>,
    ) {
        let mut needed: HashSet<String> = branch_ids
            .filter(|id| !clip_is_inside_placement(id, placement))
            .collect();
        loop {
            let before = needed.len();
            let mut dependencies = Vec::new();
            for clip in scene.clips.iter().filter(|clip| needed.contains(&clip.id)) {
                dependencies.extend(
                    clip.components
                        .iter()
                        .flat_map(|component| component.clip_ids.iter().cloned()),
                );
                dependencies.extend(
                    clip.alpha_components
                        .iter()
                        .flatten()
                        .flat_map(|component| component.clip_ids.iter().cloned()),
                );
            }
            needed.extend(dependencies);
            if needed.len() == before {
                break;
            }
        }
        for id in needed {
            if !self.ids.contains(&id) {
                self.ids.push(id.clone());
            }
            if let Some(clip) = scene.clips.iter().find(|clip| clip.id == id)
                && !self.clips.iter().any(|existing| existing.id == id)
            {
                self.clips.push(clip.clone());
            }
        }
    }
}

fn clip_is_inside_placement(id: &str, placement: &str) -> bool {
    let Some(start) = id.find('[') else {
        return false;
    };
    let mut deserializer = serde_json::Deserializer::from_str(&id[start..]);
    Vec::<Id>::deserialize(&mut deserializer)
        .ok()
        .and_then(|path| path.into_iter().next())
        .is_some_and(|first| first == placement)
}

pub(crate) fn affine(m: [f64; 6], p: Pos2) -> Pos2 {
    Pos2::new(
        (m[0] * p.x as f64 + m[2] * p.y as f64 + m[4]) as f32,
        (m[1] * p.x as f64 + m[3] * p.y as f64 + m[5]) as f32,
    )
}
pub(crate) fn inverse(m: [f64; 6], p: Pos2) -> Option<Pos2> {
    let d = m[0] * m[3] - m[1] * m[2];
    if d.abs() < 1e-10 {
        return None;
    }
    let x = p.x as f64 - m[4];
    let y = p.y as f64 - m[5];
    Some(Pos2::new(
        ((m[3] * x - m[2] * y) / d) as f32,
        ((-m[1] * x + m[0] * y) / d) as f32,
    ))
}
fn color(c: Color, alpha: f64, tint: Option<Color32>) -> Color32 {
    let a = (c[3] as f64 * alpha).clamp(0., 255.) as u8;
    if let Some(t) = tint {
        Color32::from_rgba_unmultiplied(t.r(), t.g(), t.b(), a)
    } else {
        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
    }
}
fn text_galley(
    p: &egui::Painter,
    text: &str,
    font: &str,
    size: f64,
    c: Color32,
) -> std::sync::Arc<egui::Galley> {
    let named = FontFamily::Name(font.to_owned().into());
    let family = if p.ctx().fonts(|f| f.families().contains(&named)) {
        named
    } else {
        FontFamily::Proportional
    };
    p.layout_no_wrap(text.to_owned(), FontId::new(size as f32, family), c)
}
pub(crate) fn local_bounds(item: &DrawItem, p: &egui::Painter) -> Rect {
    match &item.kind {
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. }
        | ElementKind::TextBox { width, height, .. }
        | ElementKind::Video { width, height } => {
            Rect::from_min_size(Pos2::ZERO, Vec2::new(*width as f32, *height as f32))
        }
        ElementKind::Bitmap { width, height, .. } => {
            Rect::from_min_size(Pos2::ZERO, Vec2::new(*width as f32, *height as f32))
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            ..
        } => Rect::from_min_size(
            Pos2::ZERO,
            text_galley(p, text, font_family, *font_size, Color32::BLACK).size(),
        ),
        ElementKind::Instance { .. } => Rect::NOTHING,
        ElementKind::Path {
            commands,
            stroke_width,
            ..
        }
        | ElementKind::PaintedPath {
            commands,
            stroke_width,
            ..
        } => crate::vector::path_bounds_cached(commands, item.path_fingerprint)
            .map(|b| {
                let pad = if crate::vector::vector_paint_visibility(&item.kind)
                    .is_some_and(|v| v.stroke || !v.fill)
                {
                    *stroke_width as f32 / 2.
                } else {
                    0.
                };
                Rect::from_min_max(
                    Pos2::new(b.x_min as f32, b.y_min as f32),
                    Pos2::new(b.x_max as f32, b.y_max as f32),
                )
                .expand(pad)
            })
            .unwrap_or(Rect::NOTHING),
    }
}
pub(crate) fn world_bounds(item: &DrawItem, p: &egui::Painter) -> Rect {
    let [a, b, c, d, tx, ty] = item.transform;
    if b.abs() > 1e-9 || c.abs() > 1e-9 {
        match &item.kind {
            ElementKind::Path {
                commands,
                stroke_width,
                ..
            }
            | ElementKind::PaintedPath {
                commands,
                stroke_width,
                ..
            } => {
                return crate::vector::transformed_path_bounds_cached(
                    commands,
                    item.path_fingerprint,
                    item.transform,
                )
                .map(|bounds| {
                    let pad = if crate::vector::vector_paint_visibility(&item.kind)
                        .is_some_and(|v| v.stroke || !v.fill)
                    {
                        let half = (*stroke_width * 0.5).max(0.0);
                        let sx = a.hypot(c);
                        let sy = b.hypot(d);
                        Vec2::new((half * sx) as f32, (half * sy) as f32)
                    } else {
                        Vec2::ZERO
                    };
                    Rect::from_min_max(
                        Pos2::new(bounds.x_min as f32, bounds.y_min as f32),
                        Pos2::new(bounds.x_max as f32, bounds.y_max as f32),
                    )
                    .expand2(pad)
                })
                .unwrap_or(Rect::NOTHING);
            }
            ElementKind::Ellipse { width, height, .. }
                if *width >= 0.0 && *height >= 0.0 && width.is_finite() && height.is_finite() =>
            {
                let rx = *width * 0.5;
                let ry = *height * 0.5;
                let wx = a * rx + c * ry + tx;
                let wy = b * rx + d * ry + ty;
                let ex = (a * rx).hypot(c * ry);
                let ey = (b * rx).hypot(d * ry);
                return Rect::from_min_max(
                    Pos2::new((wx - ex) as f32, (wy - ey) as f32),
                    Pos2::new((wx + ex) as f32, (wy + ey) as f32),
                );
            }
            _ => {}
        }
    }
    let r = local_bounds(item, p);
    let mut bounds = Rect::NOTHING;
    for pt in [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
    ] {
        bounds.extend_with(affine(item.transform, pt));
    }
    bounds
}
pub(crate) fn hit_item(item: &DrawItem, painter: &egui::Painter, point: Pos2, zoom: f32) -> bool {
    hit_item_with_slop(item, painter, point, zoom, 0.0)
}
pub(crate) fn hit_item_with_slop(
    item: &DrawItem,
    painter: &egui::Painter,
    point: Pos2,
    zoom: f32,
    slop_px: f32,
) -> bool {
    let Some(local) = inverse(item.transform, point) else {
        return false;
    };
    let bounds = local_bounds(item, painter);
    if bounds.is_negative() {
        return false;
    }
    let zoom = zoom.max(1e-3);
    let sx = item.transform[0].hypot(item.transform[1]).max(0.001);
    let sy = item.transform[2].hypot(item.transform[3]).max(0.001);
    let max_scale = sx.max(sy);
    let slop = slop_px.max(0.0);
    let slop_x = slop / (sx as f32 * zoom);
    let slop_y = slop / (sy as f32 * zoom);
    if !bounds.expand2(Vec2::new(slop_x, slop_y)).contains(local) {
        return false;
    }
    match &item.kind {
        ElementKind::Path {
            commands,
            fill_rule,
            stroke_width,
            ..
        }
        | ElementKind::PaintedPath {
            commands,
            fill_rule,
            stroke_width,
            ..
        } => {
            let Some(visibility) = crate::vector::vector_paint_visibility(&item.kind) else {
                return false;
            };
            let local_slop = f64::from(slop) / (max_scale * f64::from(zoom));
            let base_stroke = if visibility.stroke { *stroke_width } else { 0. };
            let effective_stroke = if visibility.stroke || (visibility.fill && local_slop > 0.) {
                base_stroke + 2. * local_slop
            } else {
                0.
            };
            hit_test_path(
                commands,
                *fill_rule,
                [local.x as f64, local.y as f64],
                visibility.fill,
                effective_stroke,
                FlattenOptions {
                    tolerance: 0.25 / (max_scale * zoom as f64),
                    ..Default::default()
                },
            )
            .unwrap_or(false)
        }
        ElementKind::Ellipse { .. } => {
            let rx = (bounds.width() / 2. + slop_x).max(1e-6);
            let ry = (bounds.height() / 2. + slop_y).max(1e-6);
            let nx = (local.x - bounds.center().x) / rx;
            let ny = (local.y - bounds.center().y) / ry;
            nx * nx + ny * ny <= 1.
        }
        _ => true,
    }
}
#[derive(Clone, Copy)]
struct DrawStyle {
    origin: Pos2,
    zoom: f32,
    opacity: f64,
    tint: Option<Color32>,
    outlines: bool,
    bitmap_texture: Option<egui::TextureId>,
}
fn draw_item(p: &egui::Painter, item: &DrawItem, style: DrawStyle, retain_solid_paths: bool) {
    let DrawStyle {
        origin,
        zoom,
        opacity,
        tint,
        outlines,
        bitmap_texture,
    } = style;
    let screen = |pt: Pos2| origin + affine(item.transform, pt).to_vec2() * zoom;
    let alpha = opacity;
    let color = |value: Color, alpha: f64, tint: Option<Color32>| {
        color(item.color_transform.apply(value), alpha, tint)
    };
    match &item.kind {
        ElementKind::Rectangle { fill, stroke, .. } | ElementKind::Ellipse { fill, stroke, .. } => {
            crate::stage_primitives::paint_primitive(
                p,
                item,
                origin,
                zoom,
                color(*fill, alpha, tint),
                color(
                    if outlines {
                        [90, 110, 130, 255]
                    } else {
                        *stroke
                    },
                    alpha,
                    tint,
                ),
                outlines,
            );
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color: c,
        } => {
            let c = color(*c, alpha, tint);
            let galley = text_galley(p, text, font_family, *font_size, c);
            if outlines {
                let r = Rect::from_min_size(Pos2::ZERO, galley.size());
                let points = vec![
                    screen(r.left_top()),
                    screen(r.right_top()),
                    screen(r.right_bottom()),
                    screen(r.left_bottom()),
                ];
                p.add(Shape::closed_line(points, Stroke::new(1.0_f32, c)));
            } else {
                let shape = Shape::Text(egui::epaint::TextShape::new(Pos2::ZERO, galley, c));
                let primitives = p.ctx().tessellate(
                    vec![egui::epaint::ClippedShape {
                        clip_rect: Rect::EVERYTHING,
                        shape,
                    }],
                    p.ctx().pixels_per_point(),
                );
                for primitive in primitives {
                    if let egui::epaint::Primitive::Mesh(mut mesh) = primitive.primitive {
                        for vertex in &mut mesh.vertices {
                            vertex.pos = screen(vertex.pos);
                        }
                        p.add(Shape::mesh(mesh));
                    }
                }
            }
        }
        ElementKind::TextBox {
            width,
            height,
            format,
            ..
        } => {
            let c = color(
                [
                    (format.color >> 16) as u8,
                    (format.color >> 8) as u8,
                    format.color as u8,
                    255,
                ],
                alpha,
                tint,
            );
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(*width as f32, *height as f32));
            let points = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ]
            .map(screen);
            if outlines {
                p.add(Shape::closed_line(points.to_vec(), Stroke::new(1.0_f32, c)));
            } else if let Err(error) = crate::text_boxes::paint(p, &item.kind, c, screen) {
                p.add(Shape::closed_line(
                    points.to_vec(),
                    Stroke::new(
                        1.0_f32,
                        crate::theme::UiPalette::current(p.ctx()).error_text,
                    ),
                ));
                p.text(
                    points[0],
                    egui::Align2::LEFT_TOP,
                    format!("Text preview: {error}"),
                    FontId::proportional(12.),
                    crate::theme::UiPalette::current(p.ctx()).error_text,
                );
            }
        }
        ElementKind::Video { width, height } => {
            if *width <= 0. || *height <= 0. {
                return;
            }
            let palette = UiPalette::current(p.ctx());
            let foreground = color(palette.muted_text.to_array(), alpha, tint);
            let background = color(palette.window_fill.to_array(), alpha, tint);
            let w = *width as f32;
            let h = *height as f32;
            let mut points = [
                Pos2::ZERO,
                Pos2::new(w, 0.),
                Pos2::new(w, h),
                Pos2::new(0., h),
            ]
            .map(screen);
            let ppp = p.ctx().pixels_per_point();
            let matrix = item.transform;
            let scale = zoom as f64 * ppp as f64;
            if (matrix[0] * scale - 1.).abs() <= 0.001
                && (matrix[3] * scale - 1.).abs() <= 0.001
                && (matrix[1] * scale).abs() <= 0.001
                && (matrix[2] * scale).abs() <= 0.001
            {
                let top_left = (points[0].to_vec2() * ppp).round() / ppp;
                points = [
                    top_left.to_pos2(),
                    (top_left + Vec2::new(w / ppp, 0.)).to_pos2(),
                    (top_left + Vec2::new(w / ppp, h / ppp)).to_pos2(),
                    (top_left + Vec2::new(0., h / ppp)).to_pos2(),
                ];
            }
            if outlines {
                p.add(Shape::closed_line(
                    points.to_vec(),
                    Stroke::new(1.0_f32, foreground),
                ));
            } else if let Some(texture) = bitmap_texture {
                let mut mesh = egui::Mesh::with_texture(texture);
                let uvs = [
                    Pos2::new(0., 0.),
                    Pos2::new(1., 0.),
                    Pos2::new(1., 1.),
                    Pos2::new(0., 1.),
                ];
                let c = self::color([255, 255, 255, 255], alpha, tint);
                for (pos, uv) in points.into_iter().zip(uvs) {
                    mesh.vertices
                        .push(egui::epaint::Vertex { pos, uv, color: c });
                }
                mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                p.add(Shape::mesh(mesh));
            } else {
                p.add(Shape::convex_polygon(
                    points.to_vec(),
                    background,
                    Stroke::new(1.0_f32, foreground),
                ));
                let r = w.min(h) * 0.12;
                let center = Pos2::new(w * 0.5, h * 0.4);
                p.add(Shape::convex_polygon(
                    [
                        Pos2::new(center.x - r * 0.6, center.y - r),
                        Pos2::new(center.x + r, center.y),
                        Pos2::new(center.x - r * 0.6, center.y + r),
                    ]
                    .map(screen)
                    .to_vec(),
                    foreground,
                    Stroke::NONE,
                ));
                let galley = p.layout_no_wrap(
                    "VIDEO · browser playback".into(),
                    FontId::proportional((w / 18.).min(h / 9.).clamp(1., 16.)),
                    foreground,
                );
                let position = Pos2::new((w - galley.size().x) * 0.5, h * 0.7);
                let primitives = p.ctx().tessellate(
                    vec![egui::epaint::ClippedShape {
                        clip_rect: Rect::EVERYTHING,
                        shape: Shape::Text(egui::epaint::TextShape::new(
                            position, galley, foreground,
                        )),
                    }],
                    p.ctx().pixels_per_point(),
                );
                for primitive in primitives {
                    if let egui::epaint::Primitive::Mesh(mut mesh) = primitive.primitive {
                        for vertex in &mut mesh.vertices {
                            vertex.pos = screen(vertex.pos);
                        }
                        p.add(Shape::mesh(mesh));
                    }
                }
            }
        }
        ElementKind::Bitmap { width, height, .. } => {
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(*width as f32, *height as f32));
            let mut points = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ]
            .map(screen);
            // AUTO snapping only applies to a unit mapping into physical preview pixels.
            let ppp = p.ctx().pixels_per_point();
            let matrix = item.transform;
            let scale = zoom as f64 * ppp as f64;
            if (matrix[0] * scale - 1.).abs() <= 0.001
                && (matrix[3] * scale - 1.).abs() <= 0.001
                && (matrix[1] * scale).abs() <= 0.001
                && (matrix[2] * scale).abs() <= 0.001
            {
                let top_left = (points[0].to_vec2() * ppp).round() / ppp;
                points = [
                    top_left.to_pos2(),
                    (top_left + Vec2::new(*width as f32 / ppp, 0.)).to_pos2(),
                    (top_left + Vec2::new(*width as f32 / ppp, *height as f32 / ppp)).to_pos2(),
                    (top_left + Vec2::new(0., *height as f32 / ppp)).to_pos2(),
                ];
            }
            if outlines {
                p.add(Shape::closed_line(
                    points.to_vec(),
                    Stroke::new(1.0_f32, color([90, 110, 130, 255], alpha, tint)),
                ));
            } else if let Some(texture) = bitmap_texture {
                let mut mesh = egui::Mesh::with_texture(texture);
                let uvs = [
                    Pos2::new(0., 0.),
                    Pos2::new(1., 0.),
                    Pos2::new(1., 1.),
                    Pos2::new(0., 1.),
                ];
                let c = self::color([255, 255, 255, 255], alpha, tint);
                for (pos, uv) in points.into_iter().zip(uvs) {
                    mesh.vertices
                        .push(egui::epaint::Vertex { pos, uv, color: c });
                }
                mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                p.add(Shape::mesh(mesh));
            } else {
                p.add(Shape::closed_line(
                    points.to_vec(),
                    Stroke::new(
                        1.0_f32,
                        crate::theme::UiPalette::current(p.ctx()).error_text,
                    ),
                ));
                p.line_segment(
                    [points[0], points[2]],
                    Stroke::new(
                        1.0_f32,
                        crate::theme::UiPalette::current(p.ctx()).error_text,
                    ),
                );
                p.line_segment(
                    [points[1], points[3]],
                    Stroke::new(
                        1.0_f32,
                        crate::theme::UiPalette::current(p.ctx()).error_text,
                    ),
                );
            }
        }
        ElementKind::Instance { .. } => {}
        ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
            fill_gradient,
            stroke_gradient,
        } if !outlines => {
            crate::vector::paint_gradient_path(
                p,
                commands,
                item.path_fingerprint,
                *fill_rule,
                *stroke_width,
                (fill, fill_gradient.as_ref()),
                (stroke, stroke_gradient.as_ref()),
                screen,
                |c| color(c, alpha, tint),
            );
        }
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        }
        | ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            let scale = (item.transform[0].hypot(item.transform[1]))
                .max(item.transform[2].hypot(item.transform[3]))
                .max(0.001);
            let paint = crate::vector::PathPaint {
                rule: *fill_rule,
                fill: if outlines {
                    Color32::TRANSPARENT
                } else {
                    color(*fill, alpha, tint)
                },
                stroke: if outlines {
                    color([90, 110, 130, 255], alpha, tint)
                } else {
                    color(*stroke, alpha, tint)
                },
                width: if outlines {
                    1. / (scale * zoom as f64)
                } else {
                    *stroke_width
                },
                tolerance: (0.25 / (scale * zoom as f64)).clamp(0.001, 1.) as f32,
                fingerprint: item.path_fingerprint,
            };
            if retain_solid_paths {
                crate::vector::paint_retained_stage_path(
                    p,
                    item,
                    commands,
                    paint,
                    origin,
                    zoom,
                    if outlines {
                        crate::vector::RetainedPathStyle::Outline
                    } else {
                        crate::vector::RetainedPathStyle::Artwork
                    },
                );
            } else {
                crate::vector::paint_path(p, commands, paint, screen);
            }
        }
    }
}

impl Editor {
    pub fn stage(&mut self, ctx: &egui::Context) {
        let palette = UiPalette::current(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(palette.pasteboard))
            .show(ctx, |ui| {
                // Stage toolbar docked as an edit bar (R-053): a strip above the canvas, so the
                // toolbar and breadcrumb never cover artwork.
                ui.spacing_mut().item_spacing.y = 0.;
                let edit_bar = (self.workspace.stage_toolbar
                    == crate::workspace::StageToolbarMode::EditBar)
                    .then(|| {
                        let height =
                            neoflash_ui::metrics::control() + neoflash_ui::tokens::space::S2;
                        ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), height),
                            Sense::hover(),
                        )
                        .0
                    });
                let avail = ui.available_size();
                let (rect, response) =
                    ui.allocate_exact_size(avail.max(Vec2::splat(1.)), Sense::click_and_drag());
                let stage_press = ui.input(|input| {
                    input.events.iter().find_map(|event| match event {
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            ..
                        } if rect.contains(*pos) => Some(*pos),
                        _ => None,
                    })
                });
                if stage_press.is_some_and(|pos| ui.ctx().layer_id_at(pos) == Some(ui.layer_id())) {
                    self.timeline_edit.focused = false;
                }
                let size = Vec2::new(self.project.width as f32, self.project.height as f32);
                if self.zoom == 0. {
                    self.zoom = ((rect.width() - 60.) / size.x)
                        .min((rect.height() - 60.) / size.y)
                        .clamp(0.1, 2.);
                }
                // Tools and hover targets refine this during the frame; `show_stage_cursor` paints it.
                self.stage_cursor = Some(crate::stage_cursor::tool_cursor_precise(
                    self.tool,
                    self.workspace.drawing.precise_cursors,
                ));
                if response.hovered() {
                    let zoom_delta = ctx.input(|input| input.zoom_delta());
                    if (zoom_delta - 1.0).abs() > 1e-4 {
                        self.end_vector_pointer_phase();
                        self.cancel_selection_gesture();
                        self.shape_draft = None;
                        if self.vector.point_drag.is_some() {
                            self.cancel_vector();
                        }
                        let old_zoom = self.zoom;
                        let new_zoom = (old_zoom * zoom_delta).clamp(0.1, 8.0);
                        if let Some(pointer) = ctx.input(|i| i.pointer.latest_pos()) {
                            let rel = pointer - rect.center() - self.pan;
                            self.pan -= rel * (new_zoom / old_zoom - 1.0);
                        }
                        self.zoom = new_zoom;
                    }
                    let delta = ctx.input(|input| input.smooth_scroll_delta);
                    if delta != Vec2::ZERO {
                        self.end_vector_pointer_phase();
                        self.cancel_selection_gesture();
                        self.shape_draft = None;
                        if self.vector.point_drag.is_some() {
                            self.cancel_vector();
                        }
                        if ctx.input(|input| input.modifiers.command) {
                            self.zoom = (self.zoom * (1. + delta.y * 0.002)).clamp(0.1, 4.);
                        } else {
                            self.pan += delta;
                        }
                    }
                }
                let isolated = self.edit_context_is_isolated();
                let page = Rect::from_center_size(rect.center() + self.pan, size * self.zoom);
                let origin = if isolated { page.center() } else { page.min };
                let mapping = match self.edit_local_to_context_matrix() {
                    Ok(matrix) => match StageMapping::try_new(matrix) {
                        Ok(mapping) => mapping,
                        Err(error) => {
                            self.message(format!(
                                "In-place symbol context became invalid: {error}. Returned to the nearest valid level."
                            ));
                            self.normalize_symbol_edit_session();
                            StageMapping::identity()
                        }
                    },
                    Err(error) => {
                        self.message(error.message());
                        self.normalize_symbol_edit_session();
                        StageMapping::identity()
                    }
                };
                self.stage_view_input(ui, rect, mapping, origin);
                self.scale9_guide_input(ui, rect, origin, mapping);
                self.shape_hint_input(ui, rect, page, origin, mapping);
                self.motion_curve_input(ui, rect, origin, mapping);
                self.motion_path_input(ui, rect, origin, mapping);
                self.stage_file_drop(ui, rect, origin, mapping);
                self.stage_library_drop(ui, rect, origin, mapping);
                let visible_page = page.intersect(rect);
                let center = if visible_page.is_positive() {
                    visible_page.center()
                } else {
                    page.center()
                };
                let paste_center = mapping.screen_to_edit(origin, self.zoom, center);
                self.remember_paste_view([
                    f64::from(paste_center.x),
                    f64::from(paste_center.y),
                ]);
                let p = ui.painter_at(rect);
                p.rect_filled(page.translate(Vec2::new(3., 4.)), 0, palette.stage_shadow);
                p.rect_filled(page, 0, color(self.project.background, 1., None));
                if !self.stage_view.prefs.grid_over_objects {
                    self.paint_stage_grid(&p, page, mapping, origin);
                }
                let content = p.with_clip_rect(rect);
                if self.edit_depth() > 1 {
                    let registration = mapping.edit_to_screen(origin, self.zoom, Pos2::ZERO);
                    content.line_segment(
                        [
                            registration - Vec2::new(8., 0.),
                            registration + Vec2::new(8., 0.),
                        ],
                        Stroke::new(1.0_f32, palette.origin_cross),
                    );
                    content.line_segment(
                        [
                            registration - Vec2::new(0., 8.),
                            registration + Vec2::new(0., 8.),
                        ],
                        Stroke::new(1.0_f32, palette.origin_cross),
                    );
                }
                let id = self.symbol_id().to_owned();
                let simple_button_consumed =
                    self.update_simple_buttons_stage(ui, &p, rect, mapping, origin);
                let mut authored_scene = self.current_authored_scene(self.show_layer_masking(ctx));
                let (live_preview_consumed, live_preview_changed) = self
                    .update_live_preview_stage(ui, &p, rect, mapping, origin, &authored_scene);
                if live_preview_changed {
                    authored_scene = self.current_authored_scene(self.show_layer_masking(ctx));
                }
                let preview_consumed = simple_button_consumed || live_preview_consumed;
                let symbol_context_clips = self.paint_symbol_edit_context(
                    &content,
                    origin,
                    self.show_layer_masking(ctx),
                );
                let mut displayed_scene = mapping.map_scene(&authored_scene);
                symbol_context_clips.attach(&mut displayed_scene, "active");
                // Edit multiple frames shows every keyframe in the onion range opaque.
                let multiple = self.timeline_edit.edit_multiple;
                if self.onion || multiple {
                    let (start, end) = self.onion_range();
                    let last = self.symbol().timeline.frame_count.saturating_sub(1);
                    let frames: Vec<u32> = if multiple {
                        self.edit_multiple_key_frames()
                    } else {
                        (start..=end.min(last)).collect()
                    };
                    let eval_project = self.project_for_stage_eval();
                    let masking = self.show_layer_masking(ctx);
                    let raw_ghosts: Vec<(u32, AuthoredScene)> = frames
                        .into_iter()
                        .filter(|&frame| frame != self.frame)
                        .map(|frame| {
                            let mut scene =
                                evaluate_scene(&eval_project, &id, frame, true, masking);
                            crate::preview::prepare_stage_path_fingerprints(&mut scene);
                            (frame, scene)
                        })
                        .collect();
                    let outlines = self.onion_outlines && !multiple;
                    let backdrop_len = if outlines || multiple || raw_ghosts.is_empty() {
                        0
                    } else {
                        let count = authored_scene
                            .draws
                            .iter()
                            .take_while(|cur| {
                                raw_ghosts.iter().any(|(_, g)| {
                                    g.draws
                                        .iter()
                                        .any(|d| d.item == cur.item && d.clip_ids == cur.clip_ids)
                                })
                            })
                            .count();
                        if count < authored_scene.draws.len() {
                            count
                        } else {
                            0
                        }
                    };
                    if outlines {
                        self.paint_authored_scene(
                            &content,
                            &displayed_scene,
                            DrawStyle {
                                origin,
                                zoom: self.zoom,
                                opacity: 1.,
                                tint: None,
                                outlines: false,
                                bitmap_texture: None,
                            },
                        );
                    } else if backdrop_len > 0 {
                        let mut backdrop_scene = mapping.map_scene(&AuthoredScene {
                            draws: authored_scene.draws[..backdrop_len].to_vec(),
                            clips: authored_scene.clips.clone(),
                            mask_groups: authored_scene.mask_groups.clone(),
                        });
                        symbol_context_clips.attach(&mut backdrop_scene, "onion-backdrop");
                        self.paint_authored_scene(
                            &content,
                            &backdrop_scene,
                            DrawStyle {
                                origin,
                                zoom: self.zoom,
                                opacity: 1.,
                                tint: None,
                                outlines: false,
                                bitmap_texture: None,
                            },
                        );
                    }
                    for (frame, mut ghost) in raw_ghosts {
                        if !multiple {
                            let filtered: Vec<_> = ghost
                                .draws
                                .iter()
                                .filter(|d| {
                                    !authored_scene
                                        .draws
                                        .iter()
                                        .any(|cur| cur.item == d.item && cur.clip_ids == d.clip_ids)
                                })
                                .cloned()
                                .collect();
                            if !filtered.is_empty() {
                                ghost.draws = filtered;
                            }
                        }
                        let opacity = self.onion_frame_opacity(frame);
                        let tint = if multiple {
                            None
                        } else if frame < self.frame {
                            Some(palette.onion_before)
                        } else {
                            Some(palette.onion_after)
                        };
                        let mut ghost = mapping.map_scene(&ghost);
                        symbol_context_clips.attach(&mut ghost, &format!("onion-{frame}"));
                        self.paint_authored_scene(
                            &content,
                            &ghost,
                            DrawStyle {
                                origin,
                                zoom: self.zoom,
                                opacity,
                                tint,
                                outlines,
                                bitmap_texture: None,
                            },
                        );
                    }
                    if !outlines {
                        let fg_scene = if backdrop_len > 0 {
                            AuthoredScene {
                                draws: authored_scene.draws[backdrop_len..].to_vec(),
                                clips: authored_scene.clips.clone(),
                                mask_groups: authored_scene.mask_groups.clone(),
                            }
                        } else {
                            authored_scene.clone()
                        };
                        let mut fg_scene = mapping.map_scene(&fg_scene);
                        symbol_context_clips.attach(&mut fg_scene, "onion-foreground");
                        self.paint_authored_scene(
                            &content,
                            &fg_scene,
                            DrawStyle {
                                origin,
                                zoom: self.zoom,
                                opacity: 1.,
                                tint: None,
                                outlines: false,
                                bitmap_texture: None,
                            },
                        );
                    }
                } else {
                    self.paint_authored_scene(
                        &content,
                        &displayed_scene,
                        DrawStyle {
                            origin,
                            zoom: self.zoom,
                            opacity: 1.,
                            tint: None,
                            outlines: false,
                            bitmap_texture: None,
                        },
                    );
                }
                let interactive_scene =
                    self.interactive_authored_scene(authored_scene, self.show_layer_masking(ctx));
                let scene: Vec<_> = interactive_scene
                    .draws
                    .into_iter()
                    .map(|draw| draw.item)
                    .collect();
                if let Some(warning) = self.bitmap_textures.take_warning() {
                    self.message(warning);
                }
                if let Some(warning) = self.video_textures.take_warning() {
                    self.message(warning);
                }
                if self.stage_view.prefs.grid_over_objects {
                    self.paint_stage_grid(&p, page, mapping, origin);
                }
                p.rect_stroke(
                    page,
                    0,
                    Stroke::new(1.0_f32, palette.stage_border),
                    StrokeKind::Outside,
                );
                self.paint_motion_path(&p, origin, mapping);
                self.paint_scale9_guides(&p, rect, origin, mapping);
                self.paint_stage_guides(&p, rect, mapping, origin);
                let quick_stage_exit = !preview_consumed
                    && (self.take_empty_stage_double_click(
                        ui, &p, origin, mapping, &scene, &response,
                    ) || self.take_empty_stage_exit_release(ui, &response));
                if !preview_consumed && !quick_stage_exit {
                    let sticky_consumed =
                        self.stage_sticky_workflow_step(ui, &p, origin, mapping, rect, &scene);
                    let extended_tool = sticky_consumed
                        || self.authoring_tool_stage(ui, &p, origin, mapping, rect, &scene);
                    if self.tool == Tool::Lasso {
                        // Lasso owns its input, but its completed selection still needs the
                        // shared selection chrome. This path paints only for non-arrow tools.
                        self.selection_stage(ui, &p, mapping, origin, &scene, &response);
                    }
                    if !extended_tool {
                        let zoom = self.zoom;
                        let to_local = |pos: Pos2| mapping.screen_to_edit(origin, zoom, pos);
                        let on_path_handle = self.tool == Tool::Subselect
                            && (self.selected_sampled().is_some_and(|e| {
                                if let Some(commands) = crate::vector::editable_element_commands(&e)
                                {
                                    let matrix = transform_matrix(e.transform);
                                    response.interact_pointer_pos().is_some_and(|pos| {
                                        crate::vector::handles(&commands).iter().any(|h| {
                                            mapping
                                                .edit_to_screen(
                                                    origin,
                                                    zoom,
                                                    affine(matrix, h.point),
                                                )
                                                .distance(pos)
                                                < 8.
                                        })
                                    })
                                } else {
                                    false
                                }
                            }) || ((ui.input(|i| {
                                i.modifiers.alt
                                    || i.events.iter().any(|e| {
                                        matches!(
                                            e,
                                            egui::Event::PointerButton {
                                                pressed: true,
                                                modifiers,
                                                ..
                                            } if modifiers.alt
                                        )
                                    })
                            }) || response.double_clicked())
                                && response.interact_pointer_pos().is_some_and(|pos| {
                                    self.on_selected_path_segment(origin, mapping, pos)
                                })));
                        let clicked = response.clicked() && !self.stage_view.guide_pointer;
                        if clicked
                            && self.tool == Tool::Subselect
                            && !on_path_handle
                            && self.vector.point_drag.is_none()
                            && let Some(pos) = response.interact_pointer_pos()
                        {
                            if let Some(target) =
                                self.pick_placement(&scene, &p, to_local(pos), mapping)
                            {
                                self.set_selection(vec![target]);
                            } else {
                                self.clear_selection();
                            }
                        }
                        self.selection_stage(ui, &p, mapping, origin, &scene, &response);
                        let text_edit_consumed =
                            self.stage_text_edit_stage(ui, &p, mapping, origin, rect, &scene);
                        self.shape_stage(ui, &p, mapping, origin, page, rect);
                        if !text_edit_consumed
                            && clicked
                            && self.tool == Tool::Text
                            && let Some(pos) = response.interact_pointer_pos()
                            && rect.contains(pos)
                        {
                            let local = to_local(pos);
                            self.create_stage_text(ctx, local, None);
                        }
                        self.paint_stage_text_edit_overlay(&p, mapping, origin);
                        self.vector_stage(ui, &content, origin, mapping, page, &response);
                    }
                }
                if !preview_consumed && !quick_stage_exit {
                    self.remember_empty_stage_click(ui, &p, origin, mapping, &scene, &response);
                }
                self.paint_shape_hints(&p, origin, mapping);
                self.paint_snap_ring(&p);
                self.stage_context_menu(&response, &scene, &p, mapping, origin);
                self.stage_rulers(ctx, rect, mapping, origin);
                if let Some(bar) = edit_bar {
                    self.stage_edit_bar(ui, bar);
                }
                self.stage_chrome(ui, rect);
                self.show_stage_cursor(ctx, rect, ui.layer_id());
            });
    }

    /// Treat two clean clicks on genuinely empty canvas as the classic one-level exit from symbol
    /// editing. Tracking the first click ourselves lets the second press be intercepted before a
    /// sticky drawing tool starts another gesture; active Pen/shape/paint gestures keep ownership.
    fn take_empty_stage_double_click(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        scene: &[DrawItem],
        response: &egui::Response,
    ) -> bool {
        let click_id = egui::Id::new("nf2-empty-stage-click");
        let (pressed, pos, now) = ui.input(|input| {
            (
                input.pointer.primary_pressed(),
                input.pointer.interact_pos(),
                input.time,
            )
        });
        if !pressed {
            return false;
        }
        let previous = ui
            .ctx()
            .data_mut(|data| data.remove_temp::<(String, f64, Pos2)>(click_id));
        let Some(pos) = pos else {
            return false;
        };
        if self.edit_depth() <= 1
            || self.stage_view.guide_pointer
            || self.object_gesture.is_some()
            || self.authoring_tools.is_drawing()
            || self.shape_draft.is_some()
            || self.vector.point_drag.is_some()
            || self.vector.draft.is_some()
            || !response.rect.contains(pos)
            || ui
                .ctx()
                .layer_id_at(pos)
                .is_some_and(|layer| layer != response.layer_id)
            || self
                .pick_placement(
                    scene,
                    p,
                    mapping.screen_to_edit(origin, self.zoom, pos),
                    mapping,
                )
                .is_some()
        {
            return false;
        }
        let symbol = self.symbol_id();
        let double = previous.is_some_and(|(previous_symbol, time, previous_pos)| {
            previous_symbol == symbol
                && now >= time
                && now - time <= 0.45
                && previous_pos.distance(pos) <= 8.0
        });
        if double {
            self.finish_edits();
            self.playing = false;
            let parent = self.edit_depth().saturating_sub(2);
            self.leave_symbol_to_level(parent);
            // The transition happens on the second press so a sticky authoring tool cannot claim
            // it. egui reports `double_clicked()` on the following release frame; keep that
            // matching release owned by the exit gesture or Select could immediately reopen the
            // root MovieClip underneath the same screen position.
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("nf2-empty-stage-exit-release"),
                    (now + 0.6, pos),
                );
            });
        }
        double
    }

    fn take_empty_stage_exit_release(&self, ui: &egui::Ui, response: &egui::Response) -> bool {
        let id = egui::Id::new("nf2-empty-stage-exit-release");
        let Some((deadline, press_pos)) = ui.ctx().data(|data| data.get_temp::<(f64, Pos2)>(id))
        else {
            return false;
        };
        let (now, pointer_down, released_pos) = ui.input(|input| {
            let released = input.events.iter().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => Some(*pos),
                _ => None,
            });
            (input.time, input.pointer.primary_down(), released)
        });
        if now > deadline {
            ui.ctx().data_mut(|data| {
                data.remove_temp::<(f64, Pos2)>(id);
            });
            return false;
        }
        let same_pointer = released_pos
            .or_else(|| response.interact_pointer_pos())
            .is_some_and(|pos| pos.distance(press_pos) <= 8.0);
        let owns_frame = same_pointer
            && (pointer_down
                || released_pos.is_some()
                || response.clicked()
                || response.double_clicked());
        if released_pos.is_some() || response.clicked() || response.double_clicked() {
            ui.ctx().data_mut(|data| {
                data.remove_temp::<(f64, Pos2)>(id);
            });
        }
        owns_frame
    }

    fn remember_empty_stage_click(
        &self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
        scene: &[DrawItem],
        response: &egui::Response,
    ) {
        let raw_clean_click = ui.input(|input| {
            input.pointer.primary_clicked()
                && !input.pointer.is_decidedly_dragging()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| response.rect.contains(pos))
        });
        if self.edit_depth() <= 1
            || self.stage_view.guide_pointer
            || (!response.clicked() && !raw_clean_click)
            || self.object_gesture.is_some()
            || self.authoring_tools.is_drawing()
            || self.shape_draft.is_some()
            || self.vector.point_drag.is_some()
            || self.vector.draft.is_some()
        {
            return;
        }
        let Some(pos) = response
            .interact_pointer_pos()
            .or_else(|| ui.input(|input| input.pointer.interact_pos()))
        else {
            return;
        };
        if !response.rect.contains(pos)
            || ui
                .ctx()
                .layer_id_at(pos)
                .is_some_and(|layer| layer != response.layer_id)
        {
            return;
        }
        if self
            .pick_placement(
                scene,
                p,
                mapping.screen_to_edit(origin, self.zoom, pos),
                mapping,
            )
            .is_some()
        {
            return;
        }
        let now = ui.input(|input| input.time);
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("nf2-empty-stage-click"),
                (self.symbol_id().to_owned(), now, pos),
            );
        });
    }

    /// Stage object (right-click) menu, design §8. Right-clicking an unselected object selects it
    /// under any sticky tool so context actions (`Actions…`, `Trace bitmap…`, `Bitmap properties…`)
    /// target the clicked Stage placement directly.
    fn stage_context_menu(
        &mut self,
        response: &egui::Response,
        scene: &[DrawItem],
        p: &egui::Painter,
        mapping: StageMapping,
        origin: Pos2,
    ) {
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.select_stage_context_target(
                scene,
                p,
                mapping.screen_to_edit(origin, self.zoom, pos),
                mapping,
            );
        }
        response.context_menu(|ui| {
            crate::ui_icons::menu_contents(ui, 260., |ui| self.stage_object_commands(ui));
        });
    }

    /// Hit-test `stage_point` and update the Stage selection for right-click context menus under
    /// any sticky tool (`T-190`, `T-177`).
    ///
    /// - Clicking an unselected placement selects that single placement.
    /// - Clicking inside an existing selection preserves the selection deterministically.
    pub(crate) fn select_stage_context_target(
        &mut self,
        scene: &[DrawItem],
        p: &egui::Painter,
        stage_point: Pos2,
        mapping: StageMapping,
    ) -> Option<Id> {
        let target = self.pick_placement(scene, p, stage_point, mapping)?;
        self.finish_edits();
        if !self.selection.targets.contains(&target) {
            self.set_selection(vec![target.clone()]);
        }
        Some(target.element)
    }

    /// Return the resource ID of the selected Stage bitmap when the selection is a single editable
    /// placed `ElementKind::Bitmap` on an unlocked, visible `Normal` layer (`T-177`).
    pub(crate) fn selected_stage_bitmap_resource_id(&self) -> Option<Id> {
        if self.selection.targets.len() != 1 || self.can_ungroup_selection() {
            return None;
        }
        let target = self.selection.targets.first()?;
        if !self.target_editable(target) {
            return None;
        }
        let timeline = &self.symbol().timeline;
        for layer in &timeline.layers {
            if layer.id != target.layer
                || layer.properties.role != LayerRole::Normal
                || layer_effectively_locked(timeline, layer)
                || !layer_effectively_visible(timeline, layer)
                || !self.is_layer_id_effectively_soloed(&layer.id)
            {
                continue;
            }
            for el in
                sample_layer_in_at_fps(timeline, layer, self.frame, f64::from(self.project.fps))
            {
                if el.id == target.element {
                    if let ElementKind::Bitmap { resource_id, .. } = &el.kind {
                        return Some(resource_id.clone());
                    }
                    return None;
                }
            }
        }
        None
    }

    /// Open the Trace Bitmap dialog targeting the currently selected placed Stage bitmap (`T-177`).
    pub(crate) fn open_trace_bitmap_dialog(&mut self, ctx: &egui::Context) {
        if self.selected_stage_bitmap_resource_id().is_some() && self.can_trace_bitmap() {
            self.open_trace_bitmap(ctx);
        }
    }

    /// Open the Bitmap Properties dialog for a placed Stage bitmap's resource (`T-177`).
    pub(crate) fn open_bitmap_properties_dialog(
        &mut self,
        ctx: &egui::Context,
        resource_id: Option<Id>,
    ) {
        let res_id = resource_id.or_else(|| self.selected_stage_bitmap_resource_id());
        if res_id.is_some() {
            self.open_bitmap_properties(ctx, res_id);
        }
    }

    fn stage_object_commands(&mut self, ui: &mut egui::Ui) {
        use crate::clipboard::ClipboardCommand;
        use crate::commands::CommandId as C;
        use crate::ui_icons::menu_item_enabled;
        use neoflash_ui::components as nf;
        let ctx = ui.ctx().clone();
        let selected = !self.selection.is_empty();
        for (label, cmd_id, command, enabled) in [
            ("Cut", C::EditCut, ClipboardCommand::Cut, selected),
            ("Copy", C::EditCopy, ClipboardCommand::Copy, selected),
            (
                "Paste in place",
                C::EditPasteInPlace,
                ClipboardCommand::PasteInPlace,
                self.layer_editable(),
            ),
        ] {
            if menu_item_enabled(ui, enabled, label, &self.sc(cmd_id)).clicked() {
                self.request_clipboard(&ctx, command);
                ui.close();
            }
        }
        nf::menu_separator(ui);
        if menu_item_enabled(
            ui,
            self.can_convert_selection(),
            "Convert to symbol…",
            &self.sc(C::ModifyConvertToSymbol),
        )
        .clicked()
        {
            self.convert_to_symbol();
            ui.close();
        }
        if menu_item_enabled(
            ui,
            self.can_break_apart_selection(),
            "Break apart",
            &self.sc(C::ModifyBreakApart),
        )
        .clicked()
        {
            self.break_apart_selection();
            ui.close();
        }
        let instance = self.selected_instance_symbol();
        if menu_item_enabled(ui, instance.is_some(), "Edit in place", "").clicked() {
            self.enter_selected_symbol_in_place();
            ui.close();
        }
        if self.can_enter_graphic_at_current_frame()
            && menu_item_enabled(ui, true, "Edit in place at current frame", "").clicked()
        {
            self.enter_graphic_at_current_frame();
            ui.close();
        }
        if self.has_selected_graphic_instance() {
            if menu_item_enabled(ui, true, "Graphic: Loop", "").clicked() {
                self.set_selected_graphic_playback(InstancePlayback::Loop, false);
                ui.close();
            }
            if menu_item_enabled(ui, true, "Graphic: Play once", "").clicked() {
                self.set_selected_graphic_playback(InstancePlayback::PlayOnce, false);
                ui.close();
            }
            if menu_item_enabled(ui, true, "Graphic: Single frame (current)", "").clicked() {
                self.set_selected_graphic_playback(InstancePlayback::SingleFrame(0), true);
                ui.close();
            }
        }
        Self::unavailable_item(ui, "Edit in new window", "");
        if menu_item_enabled(ui, instance.is_some(), "Swap symbol…", "").clicked() {
            let _ = self.swap_selected_instance_with_library_or_next();
            ui.close();
        }
        if instance.is_some()
            && menu_item_enabled(ui, true, "Swap symbol across span", "").clicked()
        {
            if let Some(target_sym) = self.library_selected.clone().or_else(|| {
                let current_sym = instance.as_deref()?;
                self.project
                    .symbols
                    .iter()
                    .find(|s| !self.project.is_scene(&s.id) && s.id != current_sym)
                    .map(|s| s.id.clone())
            }) {
                let _ = self.swap_symbol_across_selection_or_range(&target_sym, true);
            }
            ui.close();
        }
        if menu_item_enabled(ui, instance.is_some(), "Duplicate symbol…", "").clicked() {
            let _ = self.duplicate_selected_instance_symbol();
            ui.close();
        }
        let actions_sc = {
            let sc = self.sc(C::WindowActions);
            if sc.is_empty() { "F9".to_owned() } else { sc }
        };
        if menu_item_enabled(
            ui,
            self.can_open_selected_object_actions(),
            "Actions…",
            &actions_sc,
        )
        .clicked()
        {
            let _ = self.open_selected_object_actions();
            ui.close();
        }
        let selected_bitmap_res = self.selected_stage_bitmap_resource_id();
        nf::menu_separator(ui);
        if menu_item_enabled(
            ui,
            selected_bitmap_res.is_some() && self.can_trace_bitmap(),
            "Trace bitmap…",
            &self.sc(C::ModifyTraceBitmap),
        )
        .clicked()
        {
            self.open_trace_bitmap_dialog(&ctx);
            ui.close();
        }
        if menu_item_enabled(ui, selected_bitmap_res.is_some(), "Bitmap properties…", "").clicked()
        {
            self.open_bitmap_properties_dialog(&ctx, selected_bitmap_res);
            ui.close();
        }
        nf::menu_separator(ui);
        if crate::ui_icons::menu_item_enabled(
            ui,
            self.can_create_tween(),
            "Create motion tween",
            &self.sc(C::InsertMotionTween),
        )
        .clicked()
        {
            self.create_tween(TweenKind::Motion);
            ui.close();
        }
        if menu_item_enabled(
            ui,
            self.can_distribute_to_layers(),
            "Distribute to layers",
            "⇧⌘D",
        )
        .clicked()
        {
            self.distribute_to_layers();
            ui.close();
        }
        if menu_item_enabled(
            ui,
            self.can_distribute_to_keyframes(),
            "Distribute to keyframes",
            "",
        )
        .clicked()
        {
            self.distribute_selection_to_keyframes();
            ui.close();
        }
        nf::menu_separator(ui);
        self.arrange_menu(ui);
        if self.can_reverse_selected_elements_order()
            && menu_item_enabled(ui, true, "Reverse stacking order", "").clicked()
        {
            self.reverse_selected_elements_order();
            ui.close();
        }
        self.align_menu(ui);
        self.transform_menu(ui);
        if menu_item_enabled(
            ui,
            !self.editable_targets().is_empty(),
            "Pixel snap (round to whole pixels)",
            "",
        )
        .clicked()
        {
            self.pixel_snap_selection();
            ui.close();
        }
        if menu_item_enabled(
            ui,
            instance.is_some() || !self.editable_targets().is_empty(),
            "Auto Instance Namer…",
            "",
        )
        .clicked()
        {
            self.open_auto_instance_namer_modal(&ctx);
            ui.close();
        }
        if menu_item_enabled(
            ui,
            !self.editable_targets().is_empty(),
            "Randomize Selection…",
            "",
        )
        .clicked()
        {
            self.open_randomize_selection_modal(&ctx);
            ui.close();
        }
        nf::menu_separator(ui);
        if menu_item_enabled(
            ui,
            self.can_group_selection(),
            "Group",
            &self.sc(C::ModifyGroup),
        )
        .clicked()
        {
            self.group_selection();
            ui.close();
        }
        if menu_item_enabled(
            ui,
            self.can_ungroup_selection(),
            "Ungroup",
            &self.sc(C::ModifyUngroup),
        )
        .clicked()
        {
            self.ungroup_selection();
            ui.close();
        }
        nf::menu_separator(ui);
        if menu_item_enabled(ui, instance.is_some(), "Show in library", "").clicked() {
            self.library_selected = instance;
            self.show_panel(crate::workspace::PanelId::Library);
            ui.close();
        }
    }

    /// Hit-test top-level visible `SymbolKind::Button` instances on `self.frame` front-to-back,
    /// using frame `3` (`Hit`) when present and non-empty, otherwise falling back to frame `0` (`Up`).
    pub(crate) fn hit_test_simple_button(
        &self,
        painter: &egui::Painter,
        stage_point: Pos2,
    ) -> Option<Id> {
        let timeline = &self.symbol().timeline;
        let zoom = self.zoom.max(0.1);
        for layer in timeline.layers.iter().rev() {
            if layer.properties.role != LayerRole::Normal
                || !layer_effectively_visible(timeline, layer)
                || !self.is_layer_id_effectively_soloed(&layer.id)
            {
                continue;
            }
            for el in
                sample_layer_in_at_fps(timeline, layer, self.frame, f64::from(self.project.fps))
                    .into_iter()
                    .rev()
            {
                let ElementKind::Instance { symbol_id, .. } = &el.kind else {
                    continue;
                };
                let Some(btn_sym) = self.project.symbol(symbol_id) else {
                    continue;
                };
                if btn_sym.kind != SymbolKind::Button {
                    continue;
                }
                let Some(local_pt) = inverse(transform_matrix(el.transform), stage_point) else {
                    continue;
                };
                let hit_items = if btn_sym.timeline.frame_count > 3 {
                    let hit = evaluate(&self.project, symbol_id, 3);
                    if hit.is_empty() {
                        evaluate(&self.project, symbol_id, 0)
                    } else {
                        hit
                    }
                } else {
                    evaluate(&self.project, symbol_id, 0)
                };
                if hit_items
                    .iter()
                    .rev()
                    .any(|item| hit_item(item, painter, local_pt, zoom))
                {
                    return Some(el.id);
                }
            }
        }
        None
    }

    fn update_simple_buttons_stage(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        mapping: StageMapping,
        origin: Pos2,
    ) -> bool {
        if !self.enable_simple_buttons
            || self.playing
            || self.stage_view.guide_pointer
            || self.scale9_guide_drag.is_some()
            || self.timeline_motion.drag.is_some()
            || self.timeline_motion.curve_drag.is_some()
            || self.timeline_shape_hints.drag.is_some()
        {
            self.clear_simple_button_state();
            return false;
        }
        let pointer_pos = ui.input(|i| i.pointer.latest_pos());
        let over_canvas = pointer_pos.is_some_and(|pos| {
            rect.contains(pos) && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
        });
        let hit = if over_canvas {
            let pos = pointer_pos.unwrap();
            let stage_pt = mapping.screen_to_edit(origin, self.zoom.max(0.01), pos);
            self.hit_test_simple_button(painter, stage_pt)
        } else {
            None
        };
        let (primary_pressed, primary_down, primary_released) = ui.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.primary_down(),
                i.pointer.button_released(egui::PointerButton::Primary),
            )
        });
        self.hovered_simple_button = hit.clone();
        if primary_pressed {
            if let Some(id) = hit {
                self.finish_edits();
                self.pressed_simple_button = Some(id);
                self.simple_button_capturing = true;
            } else {
                self.pressed_simple_button = None;
                self.simple_button_capturing = false;
            }
        }
        let consumed = self.simple_button_capturing;
        if !primary_down || primary_released {
            self.pressed_simple_button = None;
            self.simple_button_capturing = false;
        }
        if self.pressed_simple_button.is_some() || self.hovered_simple_button.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        consumed
    }

    pub(crate) fn hit_test_live_preview_target(
        &self,
        painter: &egui::Painter,
        stage_point: Pos2,
        mapping: StageMapping,
        authored_scene: &AuthoredScene,
    ) -> Option<Vec<Id>> {
        let preview = self.authored_preview.as_ref()?;
        let zoom = self.zoom.max(0.1);
        let context_point = mapping.edit_to_stage_pos(stage_point);

        // 1. Front-to-back hit-test against presented draws (covers MovieClips, Buttons with visible
        //    Up/Over/Down geometry, and nested interactive clips respecting active layer masks).
        for draw in authored_scene.draws.iter().rev() {
            let Some(target_path) = preview
                .playback
                .interactive_ancestor_path(&self.project, &draw.item.instance_path)
            else {
                continue;
            };
            if !authored_scene.contains_point(
                &draw.item.instance_path,
                [stage_point.x as f64, stage_point.y as f64],
            ) {
                continue;
            }
            // If `target_path` is a Button with a dedicated Frame 3 (`Hit`) shape, prefer Frame 3
            // hit geometry over visual Up/Over/Down geometry.
            if let Some((_, btn_sym_id, world)) = preview
                .playback
                .button_hit_candidates(&self.project)
                .into_iter()
                .find(|(p, _, _)| *p == target_path)
                && let Some(btn_sym) = self.project.symbol(&btn_sym_id)
                && btn_sym.timeline.frame_count > 3
            {
                let hit_items = evaluate(&self.project, &btn_sym_id, 3);
                if !hit_items.is_empty() {
                    if let Some(local_pt) = inverse(world, stage_point)
                        && hit_items
                            .iter()
                            .rev()
                            .any(|item| hit_item(item, painter, local_pt, zoom))
                    {
                        return Some(target_path);
                    }
                    continue;
                }
            }
            let mut mapped = draw.item.clone();
            mapped.transform = mapping.map_matrix(mapped.transform);
            if hit_item_with_slop(&mapped, painter, context_point, zoom, 0.0) {
                return Some(target_path);
            }
        }

        // 2. Invisible hotspot Buttons (empty Up frame 0, shapes only on Frame 3 `Hit`).
        for (btn_path, btn_sym_id, world) in preview
            .playback
            .button_hit_candidates(&self.project)
            .into_iter()
            .rev()
        {
            let Some(btn_sym) = self.project.symbol(&btn_sym_id) else {
                continue;
            };
            if btn_sym.timeline.frame_count <= 3 {
                continue;
            }
            let hit_items = evaluate(&self.project, &btn_sym_id, 3);
            if hit_items.is_empty() {
                continue;
            }
            let Some(local_pt) = inverse(world, stage_point) else {
                continue;
            };
            if hit_items
                .iter()
                .rev()
                .any(|item| hit_item(item, painter, local_pt, zoom))
            {
                return Some(btn_path);
            }
        }

        // 3. Stage / root click listener (`stage.addEventListener("click", ...)`), active only while playing.
        if self.playing && preview.playback.has_root_click_handler() {
            return Some(Vec::new());
        }
        None
    }

    fn update_live_preview_stage(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        mapping: StageMapping,
        origin: Pos2,
        authored_scene: &AuthoredScene,
    ) -> (bool, bool) {
        let preview_active = self.live_script_preview
            && !self.stage_view.guide_pointer
            && self.scale9_guide_drag.is_none()
            && self.timeline_motion.drag.is_none()
            && self.timeline_motion.curve_drag.is_none()
            && self.timeline_shape_hints.drag.is_none()
            && self.authored_preview.as_ref().is_some_and(|p| {
                self.playing || (self.tool == Tool::Select && p.playback.is_root_halted_by_script())
            });
        if !preview_active {
            let had_button_state = self.hovered_live_preview_target.take();
            self.pressed_live_preview_target = None;
            self.live_preview_capturing = false;
            let mut changed = false;
            if let (Some(prev), Some(preview)) = (had_button_state, &mut self.authored_preview) {
                changed = preview.playback.set_button_state(&self.project, &prev, 0);
            }
            return (false, changed);
        }

        let pointer_pos = ui.input(|i| i.pointer.latest_pos());
        let over_canvas = pointer_pos.is_some_and(|pos| {
            rect.contains(pos) && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
        });
        let hit = if over_canvas {
            let pos = pointer_pos.unwrap();
            let stage_pt = mapping.screen_to_edit(origin, self.zoom.max(0.01), pos);
            self.hit_test_live_preview_target(painter, stage_pt, mapping, authored_scene)
        } else {
            None
        };

        let (primary_pressed, primary_down, primary_released) = ui.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.primary_down(),
                i.pointer.button_released(egui::PointerButton::Primary),
            )
        });

        let prev_hover = self.hovered_live_preview_target.clone();
        let prev_press = self.pressed_live_preview_target.clone();
        self.hovered_live_preview_target = hit.clone();

        if primary_pressed {
            if let Some(target) = hit.clone() {
                self.finish_edits();
                self.pressed_live_preview_target = Some(target);
                self.live_preview_capturing = true;
            } else {
                self.pressed_live_preview_target = None;
                self.live_preview_capturing = false;
            }
        }

        let mut scene_changed = false;
        // Update Button visual Up (0) / Over (1) / Down (2) states when pointer state changes.
        if prev_hover != self.hovered_live_preview_target
            || prev_press != self.pressed_live_preview_target
        {
            let mut reset_paths = Vec::new();
            if let Some(p) = prev_hover {
                reset_paths.push(p);
            }
            if let Some(p) = prev_press
                && !reset_paths.contains(&p)
            {
                reset_paths.push(p);
            }
            if let Some(preview) = &mut self.authored_preview {
                for path in &reset_paths {
                    if !path.is_empty() {
                        scene_changed |= preview.playback.set_button_state(&self.project, path, 0);
                    }
                }
                if let Some(pressed) = &self.pressed_live_preview_target {
                    if !pressed.is_empty() {
                        let btn_state =
                            if self.hovered_live_preview_target.as_ref() == Some(pressed) {
                                2
                            } else {
                                1
                            };
                        scene_changed |=
                            preview
                                .playback
                                .set_button_state(&self.project, pressed, btn_state);
                    }
                } else if let Some(hovered) = &self.hovered_live_preview_target
                    && !hovered.is_empty()
                {
                    scene_changed |= preview.playback.set_button_state(&self.project, hovered, 1);
                }
            }
        }

        let consumed = self.live_preview_capturing;
        if primary_released {
            if let Some(pressed) = self.pressed_live_preview_target.take()
                && hit.as_ref() == Some(&pressed)
            {
                if let Some(preview) = &mut self.authored_preview
                    && !pressed.is_empty()
                {
                    let _ = preview
                        .playback
                        .set_button_state(&self.project, &pressed, 1);
                }
                self.click_authored_preview(&pressed);
                scene_changed = true;
            }
            self.live_preview_capturing = false;
        } else if !primary_down {
            self.pressed_live_preview_target = None;
            self.live_preview_capturing = false;
        }

        if self
            .pressed_live_preview_target
            .as_ref()
            .is_some_and(|p| !p.is_empty())
            || self
                .hovered_live_preview_target
                .as_ref()
                .is_some_and(|p| !p.is_empty())
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        (consumed, scene_changed)
    }
}

impl Editor {
    pub(crate) fn show_layer_masking(&self, ctx: &egui::Context) -> bool {
        ctx.data(|data| {
            data.get_temp::<bool>(egui::Id::new(("layer-masking", &self.clipboard.session)))
        })
        .unwrap_or(true)
    }
    pub(crate) fn set_layer_masking(&self, ctx: &egui::Context, enabled: bool) {
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new(("layer-masking", &self.clipboard.session)),
                enabled,
            )
        });
    }
    /// Paint the frozen parent timelines behind an active in-place edit. Each parent omits only
    /// the exact placement being edited, so duplicate instances of the same symbol remain visible.
    /// These scenes are presentation-only and never enter picking or tool input.
    fn paint_symbol_edit_context(
        &mut self,
        painter: &egui::Painter,
        origin: Pos2,
        show_masking: bool,
    ) -> SymbolContextClips {
        if !self.editing_in_place() {
            return SymbolContextClips::default();
        }
        let Ok(matrices) = self.resolved_edit_matrices() else {
            return SymbolContextClips::default();
        };
        let start = self.edit_context_start();
        let parents: Vec<_> = self
            .edit_session
            .levels
            .iter()
            .enumerate()
            .skip(start + 1)
            .filter_map(|(child_index, level)| {
                let SymbolEditEntry::InPlace {
                    parent_symbol,
                    parent_frame,
                    parent_clip_frames,
                    target,
                } = &level.entry
                else {
                    return None;
                };
                Some((
                    child_index - 1,
                    parent_symbol.clone(),
                    *parent_frame,
                    parent_clip_frames.clone(),
                    target.clone(),
                ))
            })
            .collect();
        let mut inherited = SymbolContextClips::default();
        for (parent_index, parent_symbol, parent_frame, clip_frames, target) in parents {
            let Ok(mapping) = StageMapping::try_new(matrices[parent_index]) else {
                continue;
            };
            let placement = &target.element;
            let (mut scene, mut branch_clip_ids) = {
                let project = self.project_for_symbol_stage_eval(&parent_symbol);
                evaluate_scene_with_frozen_clips(
                    &project,
                    &parent_symbol,
                    parent_frame,
                    &clip_frames,
                    std::slice::from_ref(placement),
                    true,
                    show_masking,
                )
            };
            // Preserve relationships present on rendered descendants too; the
            // structural list above is what keeps an empty current branch clipped.
            branch_clip_ids.extend(
                scene
                    .draws
                    .iter()
                    .filter(|draw| draw.item.instance_path.first() == Some(placement))
                    .flat_map(|draw| draw.clip_ids.iter().cloned()),
            );
            let mapped_full = mapping.map_scene(&scene);
            scene.draws.retain(|draw| {
                draw.item
                    .instance_path
                    .first()
                    .is_none_or(|id| id != placement)
            });
            crate::preview::prepare_stage_path_fingerprints(&mut scene);
            let mut scene = mapping.map_scene(&scene);
            inherited.attach(&mut scene, &format!("parent-{parent_index}"));
            inherited.extend_from_branch(&mapped_full, placement, branch_clip_ids.into_iter());
            self.paint_authored_scene(
                painter,
                &scene,
                DrawStyle {
                    origin,
                    zoom: self.zoom,
                    // Locked `editor-editing-symbol-in-place` reference wraps the parent
                    // artwork in `opacity="0.75"`; keep the implementation on that exact state.
                    opacity: 0.75,
                    tint: None,
                    outlines: false,
                    bitmap_texture: None,
                },
            );
        }
        inherited
    }

    fn paint_authored_scene(
        &mut self,
        painter: &egui::Painter,
        scene: &AuthoredScene,
        style: DrawStyle,
    ) {
        if !style.outlines
            && (scene
                .clips
                .iter()
                .any(|clip| clip.alpha_components.is_some() || clip.invert)
                || (style.tint.is_none() && scene.has_blend_modes()))
        {
            if let Err(error) = alpha_masks::paint(self, painter, scene, style) {
                self.message(error);
            }
            return;
        }
        let DrawStyle {
            origin,
            zoom,
            opacity,
            tint,
            outlines,
            ..
        } = style;
        let clips = match crate::stage_clipping::ClipMeshes::new(&scene.clips, origin, zoom) {
            Ok(clips) => clips,
            Err(error) => {
                self.message(error);
                return;
            }
        };
        let clip_rect = painter.clip_rect();
        let cull_offscreen = scene.draws.len() > 16 && clip_rect.is_finite();
        for draw in &scene.draws {
            if cull_offscreen && draw.clip_ids.is_empty() {
                let wb = world_bounds(&draw.item, painter);
                if wb.is_finite() && wb.is_positive() {
                    let screen_bounds = Rect::from_min_max(
                        origin + wb.min.to_vec2() * zoom,
                        origin + wb.max.to_vec2() * zoom,
                    )
                    .expand(4.0);
                    if !screen_bounds.intersects(clip_rect) {
                        continue;
                    }
                }
            }
            let video_placement = if matches!(draw.item.kind, ElementKind::Video { .. }) {
                self.resolve_stage_video_placement(&draw.item)
            } else {
                None
            };
            let bitmap_texture = if let Some(placement) = &video_placement {
                let base_dir = self
                    .tracked_disk_path()
                    .and_then(std::path::Path::parent)
                    .map(std::path::Path::to_path_buf);
                self.video_textures.texture_for_placement(
                    painter.ctx(),
                    &self.project,
                    base_dir.as_deref(),
                    placement,
                    draw.item.color_transform,
                )
            } else {
                self.bitmap_textures.texture_transformed(
                    painter.ctx(),
                    &self.project,
                    &draw.item.kind,
                    draw.item.color_transform,
                )
            };
            let video_status_badge = if matches!(draw.item.kind, ElementKind::Video { .. })
                && bitmap_texture.is_none()
            {
                video_placement
                    .as_ref()
                    .and_then(|p| {
                        self.video_textures
                            .last_error(&p.resource)
                            .map(str::to_owned)
                    })
                    .or_else(|| {
                        self.timeline_video
                            .status_for_path(&draw.item.instance_path)
                            .map(|status| {
                                format!(
                                    "{:?} · {:.2}s{}",
                                    status.sync,
                                    f64::from(status.time_ms) / 1000.0,
                                    if status.playing {
                                        " · LIVE"
                                    } else {
                                        " · PAUSED"
                                    }
                                )
                            })
                    })
            } else {
                None
            };
            let result = clips.paint(painter, &draw.clip_ids, |target| {
                draw_item(
                    target,
                    &draw.item,
                    DrawStyle {
                        origin,
                        zoom,
                        opacity,
                        tint: tint.or_else(|| {
                            draw.outline_color
                                .map(|color| Color32::from_rgb(color[0], color[1], color[2]))
                        }),
                        outlines: outlines || draw.outline_color.is_some(),
                        bitmap_texture,
                    },
                    draw.clip_ids.is_empty(),
                );
                if let (Some(badge), ElementKind::Video { width, height }) =
                    (&video_status_badge, &draw.item.kind)
                    && !outlines
                    && draw.outline_color.is_none()
                    && *width > 0.0
                    && *height > 0.0
                {
                    let palette = UiPalette::current(target.ctx());
                    let fg = color(
                        draw.item
                            .color_transform
                            .apply(palette.muted_text.to_array()),
                        opacity,
                        tint,
                    );
                    let w = *width as f32;
                    let h = *height as f32;
                    let screen =
                        |pt: Pos2| origin + affine(draw.item.transform, pt).to_vec2() * zoom;
                    let galley = target.layout_no_wrap(
                        badge.clone(),
                        FontId::proportional((w / 22.).min(h / 11.).clamp(1., 13.)),
                        fg,
                    );
                    let position = Pos2::new((w - galley.size().x) * 0.5, h * 0.84);
                    let primitives = target.ctx().tessellate(
                        vec![egui::epaint::ClippedShape {
                            clip_rect: Rect::EVERYTHING,
                            shape: Shape::Text(egui::epaint::TextShape::new(position, galley, fg)),
                        }],
                        target.ctx().pixels_per_point(),
                    );
                    for primitive in primitives {
                        if let egui::epaint::Primitive::Mesh(mut mesh) = primitive.primitive {
                            for vertex in &mut mesh.vertices {
                                vertex.pos = screen(vertex.pos);
                            }
                            target.add(Shape::mesh(mesh));
                        }
                    }
                }
            });
            if let Err(error) = result {
                self.message(error);
                break;
            }
        }
    }
    /// Paint an evaluated scene opaque at `origin` / `zoom` (timeline thumbnails).
    pub(crate) fn paint_scene_at(
        &mut self,
        painter: &egui::Painter,
        scene: &AuthoredScene,
        origin: Pos2,
        zoom: f32,
    ) {
        self.paint_authored_scene(
            painter,
            scene,
            DrawStyle {
                origin,
                zoom,
                opacity: 1.,
                tint: None,
                outlines: false,
                bitmap_texture: None,
            },
        );
    }
    pub(crate) fn paint_library_preview(&mut self, ui: &mut egui::Ui, scene: &AuthoredScene) {
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(
                ui.available_width()
                    .min(ui.clip_rect().right() - ui.cursor().left())
                    .max(1.),
                ui.available_height().clamp(64., 160.),
            ),
            Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2., UiPalette::current(ui.ctx()).window_fill);
        let bounds = scene.draws.iter().fold(Rect::NOTHING, |bounds, draw| {
            bounds.union(world_bounds(&draw.item, &painter))
        });
        if !bounds.is_finite() || !bounds.is_positive() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Empty first frame",
                FontId::proportional(crate::theme::SECONDARY_FONT),
                UiPalette::current(ui.ctx()).muted_text,
            );
            return;
        }
        let zoom = ((rect.width() - 16.) / bounds.width().max(1.))
            .min((rect.height() - 16.) / bounds.height().max(1.))
            .clamp(0.001, 8.);
        let origin = rect.center() - bounds.center().to_vec2() * zoom;
        self.paint_authored_scene(
            &painter,
            scene,
            DrawStyle {
                origin,
                zoom,
                opacity: 1.,
                tint: None,
                outlines: false,
                bitmap_texture: None,
            },
        );
    }

    pub(crate) fn onion_frame_opacity(&self, frame: u32) -> f64 {
        if self.timeline_edit.edit_multiple || frame == self.frame {
            return 1.0;
        }
        let (start, end) = self.onion_range();
        let clamped_end = end.min(self.symbol().timeline.frame_count.saturating_sub(1));
        let distance = frame.abs_diff(self.frame).max(1);
        let range_len = if self.onion_anchor {
            if frame < self.frame {
                self.frame.saturating_sub(start)
            } else {
                clamped_end.saturating_sub(self.frame)
            }
        } else if frame < self.frame {
            self.onion_before
        } else {
            self.onion_after
        };
        onion_ghost_opacity(
            self.workspace.timeline_prefs.onion_falloff,
            distance,
            range_len,
        )
    }
}

#[allow(dead_code, clippy::too_many_arguments)]
impl Editor {
    // =========================================================================
    // Stage Camera, IK Bone Constraints & Stage A/B Comparison (`T-233`)
    // =========================================================================

    /// Return the sampled `CameraPose` on the active timeline at `self.frame` (`T-233`).
    pub fn active_timeline_camera_pose(&self) -> Option<CameraPose> {
        sample_timeline_camera(&self.symbol().timeline, self.frame)
    }

    /// Set or clear the `CameraPose` on the active keyframe in 1 Undo step (`T-233`).
    pub fn set_stage_camera_pose(&mut self, pose: Option<CameraPose>) -> bool {
        let clean = pose.map(|mut cam| {
            cam.x = if cam.x.is_finite() {
                cam.x.clamp(-1e6, 1e6)
            } else {
                0.0
            };
            cam.y = if cam.y.is_finite() {
                cam.y.clamp(-1e6, 1e6)
            } else {
                0.0
            };
            cam.rotation = if cam.rotation.is_finite() {
                cam.rotation.clamp(-1e6, 1e6)
            } else {
                0.0
            };
            cam.zoom = if cam.zoom.is_finite() {
                cam.zoom.clamp(0.01, 100.0)
            } else {
                1.0
            };
            cam.focal_length = if cam.focal_length.is_finite() {
                cam.focal_length.clamp(10.0, 1e6)
            } else {
                500.0
            };
            cam
        });
        let layer_idx = self.layer;
        let frame = self.frame;
        let Some(existing_cam) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .map(|k| k.camera)
        else {
            return false;
        };
        if existing_cam == clean {
            return false;
        }
        self.transact_named("Set Stage Camera", |editor| {
            let symbol = editor.symbol_mut();
            let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
                return;
            };
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                key.camera = clean;
            }
        });
        true
    }

    /// Pan the Stage camera on the active keyframe by `(dx, dy)` in 1 Undo step (`T-233`).
    pub fn pan_stage_camera(&mut self, dx: f64, dy: f64) -> bool {
        if !dx.is_finite() || !dy.is_finite() || (dx.abs() <= 1e-6 && dy.abs() <= 1e-6) {
            return false;
        }
        let mut cam = self.active_timeline_camera_pose().unwrap_or_default();
        cam.x = (cam.x + dx).clamp(-1e6, 1e6);
        cam.y = (cam.y + dy).clamp(-1e6, 1e6);
        self.set_stage_camera_pose(Some(cam))
    }

    /// Zoom the Stage camera on the active keyframe by `factor` in 1 Undo step (`T-233`).
    pub fn zoom_stage_camera(&mut self, factor: f64) -> bool {
        if !factor.is_finite() || factor <= 0.0 || (factor - 1.0).abs() <= 1e-6 {
            return false;
        }
        let mut cam = self.active_timeline_camera_pose().unwrap_or_default();
        cam.zoom = (cam.zoom * factor).clamp(0.01, 100.0);
        self.set_stage_camera_pose(Some(cam))
    }

    /// Rotate the Stage camera on the active keyframe by `delta_deg` in 1 Undo step (`T-233`).
    pub fn rotate_stage_camera(&mut self, delta_deg: f64) -> bool {
        if !delta_deg.is_finite() || delta_deg.abs() <= 1e-6 {
            return false;
        }
        let mut cam = self.active_timeline_camera_pose().unwrap_or_default();
        cam.rotation = (cam.rotation + delta_deg).clamp(-1e6, 1e6);
        self.set_stage_camera_pose(Some(cam))
    }

    /// Set the Stage camera `focal_length` (`10.0..=1e6`) on the active keyframe in 1 Undo step (`T-233`).
    pub fn set_stage_camera_focal_length(&mut self, focal_length: f64) -> bool {
        if !focal_length.is_finite() {
            return false;
        }
        let mut cam = self.active_timeline_camera_pose().unwrap_or_default();
        cam.focal_length = focal_length.clamp(10.0, 1e6);
        self.set_stage_camera_pose(Some(cam))
    }

    /// Reset (clear) the Stage camera on the active keyframe in 1 Undo step (`T-233`).
    pub fn reset_stage_camera(&mut self) -> bool {
        self.set_stage_camera_pose(None)
    }

    /// Return the session's `IkJointSpec` for `bone_id`, if configured (`T-233`).
    pub fn stage_bone_joint_spec(&self, bone_id: &str) -> Option<IkJointSpec> {
        let sessions = STAGE_AB_SESSIONS.lock().unwrap();
        sessions.get(&self.clipboard.session).and_then(|s| {
            s.joint_specs
                .iter()
                .find(|sp| sp.bone_id == bone_id)
                .cloned()
        })
    }

    /// Store or update the session's `IkJointSpec` for its `bone_id` (`T-233`).
    pub fn set_stage_bone_joint_spec(&mut self, spec: IkJointSpec) {
        let mut sessions = STAGE_AB_SESSIONS.lock().unwrap();
        let state = sessions.entry(self.clipboard.session.clone()).or_default();
        if let Some(slot) = state
            .joint_specs
            .iter_mut()
            .find(|s| s.bone_id == spec.bone_id)
        {
            *slot = spec;
        } else {
            state.joint_specs.push(spec);
        }
    }

    /// Update an IK bone's joint angle constraints (`constrain_angle`, `min_angle_deg`,
    /// `max_angle_deg`), `stiffness` (`0.0..=1.0`), and `pinned` state on the active keyframe in 1
    /// Undo step (`T-233`).
    pub fn stage_set_ik_bone_constraints(
        &mut self,
        armature_id: &str,
        bone_id: &str,
        constrain_angle: bool,
        min_angle_deg: f64,
        max_angle_deg: f64,
        stiffness: f64,
        pinned: bool,
    ) -> bool {
        if !min_angle_deg.is_finite() || !max_angle_deg.is_finite() || !stiffness.is_finite() {
            return false;
        }
        let clean_min = min_angle_deg.min(max_angle_deg).clamp(-360.0, 360.0);
        let clean_max = max_angle_deg.max(min_angle_deg).clamp(-360.0, 360.0);
        let clean_stiffness = if pinned {
            1.0
        } else {
            stiffness.clamp(0.0, 1.0)
        };
        let (min_opt, max_opt) = if constrain_angle {
            (Some(clean_min), Some(clean_max))
        } else {
            (None, None)
        };
        let layer_idx = self.layer;
        let frame = self.frame;
        let has_bone = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .and_then(|k| k.ik_armatures.iter().find(|a| a.id == armature_id))
            .is_some_and(|a| a.bones.iter().any(|b| b.id == bone_id));
        if !has_bone {
            return false;
        }
        let arm_owned = armature_id.to_owned();
        let bone_owned = bone_id.to_owned();
        self.transact_named("Set IK Bone Constraints", |editor| {
            let symbol = editor.symbol_mut();
            let Some(layer) = symbol.timeline.layers.get_mut(layer_idx) else {
                return;
            };
            let key_frame = layer.key_at(frame).map(|k| k.frame).unwrap_or(frame);
            let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) else {
                return;
            };
            let Some(arm) = key.ik_armatures.iter_mut().find(|a| a.id == arm_owned) else {
                return;
            };
            if let Some(bone) = arm.bones.iter_mut().find(|b| b.id == bone_owned) {
                bone.min_angle = min_opt;
                bone.max_angle = max_opt;
                bone.stiffness = clean_stiffness;
            }
        });
        let mut spec = self
            .stage_bone_joint_spec(bone_id)
            .unwrap_or_else(|| IkJointSpec {
                bone_id: bone_id.to_owned(),
                ..IkJointSpec::default()
            });
        spec.constrain_rotation = constrain_angle;
        spec.min_angle_deg = clean_min;
        spec.max_angle_deg = clean_max;
        spec.pinned = pinned;
        self.set_stage_bone_joint_spec(spec);
        true
    }

    /// Update an IK bone's X/Y translation constraints on the session's `IkJointSpec` (`T-233`).
    pub fn stage_set_ik_bone_translation_constraints(
        &mut self,
        bone_id: &str,
        enable_x: bool,
        constrain_x: bool,
        min_x: f64,
        max_x: f64,
        enable_y: bool,
        constrain_y: bool,
        min_y: f64,
        max_y: f64,
    ) {
        let mut spec = self
            .stage_bone_joint_spec(bone_id)
            .unwrap_or_else(|| IkJointSpec {
                bone_id: bone_id.to_owned(),
                ..IkJointSpec::default()
            });
        spec.enable_x_translation = enable_x;
        spec.constrain_x_translation = constrain_x;
        spec.min_x_offset = min_x.min(max_x);
        spec.max_x_offset = max_x.max(min_x);
        spec.enable_y_translation = enable_y;
        spec.constrain_y_translation = constrain_y;
        spec.min_y_offset = min_y.min(max_y);
        spec.max_y_offset = max_y.max(min_y);
        self.set_stage_bone_joint_spec(spec);
    }

    /// Capture a `StageSnapshotAB` in slot A from the current frame (`T-233`).
    pub fn capture_stage_snapshot_a(
        &mut self,
        label: impl Into<String>,
    ) -> Option<StageSnapshotAB> {
        let label_str = label.into();
        let snap = self.capture_stage_snapshot_ab(&label_str);
        let mut sessions = STAGE_AB_SESSIONS.lock().unwrap();
        let state = sessions.entry(self.clipboard.session.clone()).or_default();
        state.snapshot_a = Some(snap.clone());
        Some(snap)
    }

    /// Capture a `StageSnapshotAB` in slot B from the current frame (`T-233`).
    pub fn capture_stage_snapshot_b(
        &mut self,
        label: impl Into<String>,
    ) -> Option<StageSnapshotAB> {
        let label_str = label.into();
        let snap = self.capture_stage_snapshot_ab(&label_str);
        let mut sessions = STAGE_AB_SESSIONS.lock().unwrap();
        let state = sessions.entry(self.clipboard.session.clone()).or_default();
        state.snapshot_b = Some(snap.clone());
        Some(snap)
    }

    /// Capture `StageSnapshotAB` reference snapshots from `frame_a` and `frame_b` on the active
    /// timeline (`T-233`).
    pub fn set_stage_ab_reference_frames(&mut self, frame_a: u32, frame_b: u32) -> bool {
        let fps = f64::from(self.project.fps);
        let tl = &self.symbol().timeline;
        let last = tl.frame_count.saturating_sub(1);
        let fa = frame_a.min(last);
        let fb = frame_b.min(last);
        let snap_a = capture_snapshot_at_frame(tl, fa, fps, format!("A (F{})", fa + 1));
        let snap_b = capture_snapshot_at_frame(tl, fb, fps, format!("B (F{})", fb + 1));
        let mut sessions = STAGE_AB_SESSIONS.lock().unwrap();
        let state = sessions.entry(self.clipboard.session.clone()).or_default();
        state.snapshot_a = Some(snap_a);
        state.snapshot_b = Some(snap_b);
        true
    }

    /// Set the active `StageAbComparisonMode` (`Off`, `ShowA`, `ShowB`, `SplitVertical`) (`T-233`).
    pub fn set_stage_ab_mode(&mut self, mode: StageAbComparisonMode) {
        let mut sessions = STAGE_AB_SESSIONS.lock().unwrap();
        let state = sessions.entry(self.clipboard.session.clone()).or_default();
        state.mode = mode;
    }

    /// Return the active `StageAbComparisonMode` (`T-233`).
    pub fn stage_ab_mode(&self) -> StageAbComparisonMode {
        let sessions = STAGE_AB_SESSIONS.lock().unwrap();
        sessions
            .get(&self.clipboard.session)
            .map(|s| s.mode)
            .unwrap_or_default()
    }

    /// Set the vertical split ratio (`0.05..=0.95`) for `StageAbComparisonMode::SplitVertical` (`T-233`).
    pub fn set_stage_ab_split_ratio(&mut self, ratio: f32) {
        if !ratio.is_finite() {
            return;
        }
        let mut sessions = STAGE_AB_SESSIONS.lock().unwrap();
        let state = sessions.entry(self.clipboard.session.clone()).or_default();
        state.split_ratio = ratio.clamp(0.05, 0.95);
    }

    /// Return the vertical split ratio for `Stage A/B` comparison (`T-233`).
    pub fn stage_ab_split_ratio(&self) -> f32 {
        let sessions = STAGE_AB_SESSIONS.lock().unwrap();
        sessions
            .get(&self.clipboard.session)
            .map(|s| s.split_ratio)
            .unwrap_or(0.5)
    }

    /// Return the captured `(snapshot_a, snapshot_b)` pair for the current session (`T-233`).
    pub fn stage_ab_snapshots(&self) -> (Option<StageSnapshotAB>, Option<StageSnapshotAB>) {
        let sessions = STAGE_AB_SESSIONS.lock().unwrap();
        sessions
            .get(&self.clipboard.session)
            .map(|s| (s.snapshot_a.clone(), s.snapshot_b.clone()))
            .unwrap_or((None, None))
    }

    /// Return `(only_in_a, only_in_b, modified_count)` between captured `Stage A` and `Stage B`
    /// snapshots (`T-233`).
    pub fn stage_ab_diff_counts(&self) -> Option<(usize, usize, usize)> {
        let (a, b) = self.stage_ab_snapshots();
        let a = a?;
        let b = b?;
        let (only_a, only_b, modified) = diff_stage_snapshots(&a, &b);
        Some((only_a.len(), only_b.len(), modified.len()))
    }

    /// Paint the `Stage A/B` split/toggle reference frame comparison overlay on the Stage (`T-233`).
    pub fn paint_stage_ab_comparison_overlay(&self, painter: &egui::Painter, stage_rect: Rect) {
        let mode = self.stage_ab_mode();
        if mode == StageAbComparisonMode::Off {
            return;
        }
        let nf = neoflash_ui::palette(painter.ctx());
        if mode == StageAbComparisonMode::SplitVertical {
            let ratio = self.stage_ab_split_ratio();
            let split_x = stage_rect.left() + stage_rect.width() * ratio;
            painter.vline(
                split_x,
                stage_rect.y_range(),
                Stroke::new(1.5_f32, nf.guide),
            );
        }
    }
}

#[allow(dead_code)]
fn capture_snapshot_at_frame(
    tl: &Timeline,
    frame: u32,
    fps: f64,
    label: String,
) -> StageSnapshotAB {
    let mut elements = Vec::new();
    for layer in &tl.layers {
        if layer.visible {
            elements.extend(sample_layer_in_at_fps(tl, layer, frame, fps));
        }
    }
    StageSnapshotAB {
        label,
        frame,
        elements,
    }
}

#[allow(dead_code)]
fn diff_stage_snapshots(a: &StageSnapshotAB, b: &StageSnapshotAB) -> (Vec<Id>, Vec<Id>, Vec<Id>) {
    let mut only_a = Vec::new();
    let mut only_b = Vec::new();
    let mut modified = Vec::new();
    for el_a in &a.elements {
        if let Some(el_b) = b.elements.iter().find(|e| e.id == el_a.id) {
            if el_a != el_b {
                modified.push(el_a.id.clone());
            }
        } else {
            only_a.push(el_a.id.clone());
        }
    }
    for el_b in &b.elements {
        if !a.elements.iter().any(|e| e.id == el_b.id) {
            only_b.push(el_b.id.clone());
        }
    }
    (only_a, only_b, modified)
}

/// Stage A/B reference comparison mode (`T-233`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StageAbComparisonMode {
    #[default]
    Off,
    ShowA,
    ShowB,
    SplitVertical,
}

#[allow(dead_code)]
#[derive(Clone)]
struct StageAbSessionState {
    mode: StageAbComparisonMode,
    split_ratio: f32,
    snapshot_a: Option<StageSnapshotAB>,
    snapshot_b: Option<StageSnapshotAB>,
    joint_specs: Vec<IkJointSpec>,
}

impl Default for StageAbSessionState {
    fn default() -> Self {
        Self {
            mode: StageAbComparisonMode::Off,
            split_ratio: 0.5,
            snapshot_a: None,
            snapshot_b: None,
            joint_specs: Vec::new(),
        }
    }
}

#[allow(dead_code)]
static STAGE_AB_SESSIONS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::BTreeMap<String, StageAbSessionState>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));

/// Linear onion-skin ghost opacity from `falloff_percent`% at `distance == 1` to `15`% at
/// `range_len` (when `range_len <= 1`, the single ghost uses `falloff_percent`%).
pub(crate) fn onion_ghost_opacity(falloff_percent: f32, distance: u32, range_len: u32) -> f64 {
    let near = (f64::from(falloff_percent) / 100.0).clamp(0.15, 1.0);
    let far = 0.15;
    let d = distance.max(1);
    if range_len <= 1 {
        return near;
    }
    let t = (f64::from(d - 1) / f64::from(range_len - 1)).clamp(0.0, 1.0);
    near + (far - near) * t
}

#[cfg(test)]
mod symbol_context_tests {
    use super::*;

    fn draw(id: &str, group: Option<&str>) -> AuthoredDraw {
        AuthoredDraw {
            item: DrawItem {
                element_id: id.into(),
                name: String::new(),
                instance_path: vec![id.into()],
                layer_id: "layer".into(),
                transform: [1., 0., 0., 1., 0., 0.],
                alpha: 1.,
                color_transform: ColorTransform::default(),
                path_fingerprint: None,
                kind: ElementKind::Rectangle {
                    width: 10.,
                    height: 10.,
                    fill: [0, 0, 0, 255],
                    stroke: [0; 4],
                    stroke_width: 0.,
                },
            },
            clip_ids: vec![],
            outline_color: None,
            mask_group: group.map(str::to_owned),
            blend_mode: BlendMode::Normal,
        }
    }

    #[test]
    fn inherited_inverted_clip_wraps_the_composited_child_once() {
        let inherited = SymbolContextClips {
            ids: vec!["inverted-parent".into()],
            clips: vec![RuntimeClip {
                id: "inverted-parent".into(),
                components: vec![],
                alpha_components: None,
                invert: true,
            }],
        };
        let mut scene = AuthoredScene {
            draws: vec![draw("plain", None), draw("grouped", Some("inner"))],
            clips: vec![],
            mask_groups: vec![AuthoredMaskGroup {
                id: "inner".into(),
                parent_id: None,
                clip_ids: vec![],
                blend_mode: BlendMode::Normal,
            }],
        };

        inherited.attach(&mut scene, "test");

        let wrapper = scene
            .mask_groups
            .iter()
            .find(|group| group.id == "@symbol-edit-context:test")
            .expect("inverted clips need the same composite wrapper as alpha masks");
        assert_eq!(wrapper.clip_ids, ["inverted-parent"]);
        assert_eq!(
            scene.draws[0].mask_group.as_deref(),
            Some(wrapper.id.as_str())
        );
        assert_eq!(
            scene.mask_groups[0].parent_id.as_deref(),
            Some(wrapper.id.as_str())
        );
        assert!(
            scene
                .draws
                .iter()
                .all(|draw| { draw.clip_ids.iter().any(|id| id == "inverted-parent") })
        );
    }
}

#[cfg(test)]
mod stage_tools_and_context_menu_tests {
    use super::deco_spray::{StageDecoEffect, StageDecoOptions, StageSprayOptions};
    use super::ik::{Gizmo3dHandle, Stage3dAuthoringState};
    use super::properties_tool::StageStickyToolMode;
    use super::*;
    use crate::app::ScriptTarget;
    use crate::selection::SelectionTarget;

    fn make_test_editor() -> Editor {
        let ctx = egui::Context::default();
        Editor::from_project(&ctx, Project::blank())
    }

    fn make_test_element(id: &str, name: &str, x: f64, y: f64, kind: ElementKind) -> Element {
        let mut el = generate_deco_elements(
            DecoEffectKind::FlowerBrush,
            [0.0, 0.0],
            100.0,
            20.0,
            [0, 0, 0, 255],
            [0, 0, 0, 255],
        )
        .into_iter()
        .next()
        .expect("template element");
        el.id = id.into();
        el.name = name.into();
        el.transform = Transform {
            x,
            y,
            ..Transform::default()
        };
        el.merge = false;
        el.kind = kind;
        el
    }

    fn sel_target(editor: &Editor, element_id: &str) -> SelectionTarget {
        let layer_id = editor.symbol().timeline.layers[0].id.clone();
        SelectionTarget {
            layer: layer_id,
            element: element_id.into(),
        }
    }

    fn add_symbol_with_rect(editor: &mut Editor, sym_id: &str, name: &str, kind: SymbolKind) -> Id {
        let mut sym = editor.project.symbols[0].clone();
        sym.id = sym_id.into();
        sym.name = name.into();
        sym.kind = kind;
        sym.folder.clear();
        sym.linkage_id = None;
        sym.class_name = None;
        sym.set_scale9_grid(None);
        sym.timeline.frame_count = if kind == SymbolKind::Button { 4 } else { 1 };
        sym.timeline.guides.clear();
        sym.timeline.layers.truncate(1);
        let layer = &mut sym.timeline.layers[0];
        layer.id = format!("{sym_id}-layer");
        layer.properties = LayerProperties::default();
        layer.keyframes.truncate(1);
        let kf = &mut layer.keyframes[0];
        kf.frame = 0;
        kf.elements = vec![make_test_element(
            &format!("{sym_id}-rect"),
            "",
            0.0,
            0.0,
            ElementKind::Rectangle {
                width: 40.0,
                height: 30.0,
                fill: [200, 80, 60, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
        )];
        editor.project.symbols.push(sym);
        sym_id.into()
    }

    fn place_instance_on_root(
        editor: &mut Editor,
        el_id: &str,
        symbol_id: &str,
        x: f64,
        y: f64,
        playback: InstancePlayback,
    ) {
        let root_id = editor.project.root.clone();
        let sym = editor.project.symbol_mut(&root_id).unwrap();
        sym.timeline.layers[0].keyframes[0]
            .elements
            .push(make_test_element(
                el_id,
                el_id,
                x,
                y,
                ElementKind::Instance {
                    symbol_id: symbol_id.into(),
                    playback,
                },
            ));
    }

    #[test]
    fn t190_stage_context_actions_eligibility_default_events_and_sticky_right_click() {
        let mut editor = make_test_editor();
        let mc_sym = add_symbol_with_rect(&mut editor, "mc-sym", "HeroMC", SymbolKind::MovieClip);
        let btn_sym = add_symbol_with_rect(&mut editor, "btn-sym", "PlayBtn", SymbolKind::Button);
        let gfx_sym = add_symbol_with_rect(&mut editor, "gfx-sym", "BadgeGfx", SymbolKind::Graphic);

        let root_id = editor.project.root.clone();
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .clear();
        place_instance_on_root(
            &mut editor,
            "inst-mc",
            &mc_sym,
            20.0,
            20.0,
            InstancePlayback::Independent,
        );
        place_instance_on_root(
            &mut editor,
            "inst-btn",
            &btn_sym,
            100.0,
            20.0,
            InstancePlayback::SingleFrame(0),
        );
        place_instance_on_root(
            &mut editor,
            "inst-gfx",
            &gfx_sym,
            180.0,
            20.0,
            InstancePlayback::Loop,
        );
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .push(make_test_element(
                "raw-shape",
                "",
                260.0,
                20.0,
                ElementKind::Rectangle {
                    width: 30.0,
                    height: 30.0,
                    fill: [50, 120, 220, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
            ));

        // Right-clicking under a sticky tool (Brush / Deco) selects the clicked placement.
        editor.tool = Tool::Brush;
        editor.set_stage_sticky_tool_mode(StageStickyToolMode::Deco);
        let ctx = egui::Context::default();
        let mut picked_under_brush = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let p = ui.painter();
                let scene = evaluate(&editor.project, &root_id, 0);
                picked_under_brush = editor.select_stage_context_target(
                    &scene,
                    p,
                    Pos2::new(110.0, 30.0),
                    StageMapping::identity(),
                );
            });
        });
        assert_eq!(picked_under_brush.as_deref(), Some("inst-btn"));
        assert_eq!(
            editor.selection.targets,
            vec![sel_target(&editor, "inst-btn")]
        );
        assert!(editor.can_open_selected_object_actions());

        // Snapshot project & undo state before opening Actions on empty Button instance.
        let project_before = editor.project.clone();
        let undo_len_before = editor.history.undo_len();
        editor.playing = true;
        editor.script_event = ObjectEvent::EnterFrame;
        assert!(editor.open_selected_object_actions());
        assert!(!editor.playing, "opening Actions must stop playback");
        assert!(matches!(editor.script_target, ScriptTarget::Object));
        assert_eq!(
            editor.script_event,
            ObjectEvent::Click,
            "empty Button instance must default to Click"
        );
        assert_eq!(
            editor.history.undo_len(),
            undo_len_before,
            "opening Actions on empty Button must not push Undo"
        );
        assert_eq!(
            editor.project, project_before,
            "opening Actions on empty Button must not create ObjectScript or keyframe"
        );

        // Selecting an empty MovieClip instance defaults to Load without mutating the project.
        let t_mc = sel_target(&editor, "inst-mc");
        editor.set_selection(vec![t_mc.clone()]);
        assert!(editor.can_open_selected_object_actions());
        assert!(editor.open_selected_object_actions());
        assert!(matches!(editor.script_target, ScriptTarget::Object));
        assert_eq!(
            editor.script_event,
            ObjectEvent::Load,
            "empty MovieClip instance must default to Load"
        );
        assert_eq!(editor.history.undo_len(), undo_len_before);
        assert_eq!(editor.project, project_before);

        // When handlers exist on the instance, preserve matching event or pick first existing handler.
        let obj_script: ObjectScript = serde_json::from_value(serde_json::json!({
            "event": "enter_frame",
            "script": {
                "id": "script-ef",
                "name": "onEnterFrame",
                "language": "action_script2",
                "source": "this._x += 1;"
            }
        }))
        .expect("deserialize ObjectScript");
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .iter_mut()
            .find(|e| e.id == "inst-mc")
            .unwrap()
            .scripts
            .push(obj_script);
        editor.script_event = ObjectEvent::Click;
        assert!(editor.open_selected_object_actions());
        assert_eq!(editor.script_event, ObjectEvent::EnterFrame);

        // Disabled for Graphic instances, raw shapes, multi-selection, and locked layers.
        let t_gfx = sel_target(&editor, "inst-gfx");
        let t_raw = sel_target(&editor, "raw-shape");
        let t_btn = sel_target(&editor, "inst-btn");
        editor.set_selection(vec![t_gfx]);
        assert!(!editor.can_open_selected_object_actions());
        editor.set_selection(vec![t_raw.clone()]);
        assert!(!editor.can_open_selected_object_actions());
        editor.set_selection(vec![t_mc.clone(), t_btn.clone()]);
        assert!(!editor.can_open_selected_object_actions());

        editor.set_selection(vec![t_mc]);
        editor.toggle_selected_layers_lock(0);
        assert!(!editor.can_open_selected_object_actions());
        editor.toggle_selected_layers_lock(0);

        // F9 (`open_actions_for_current_context`) opens Object target when eligible instance is selected,
        // and Frame target when a non-actionable element or nothing is selected.
        editor.set_selection(vec![t_btn]);
        editor.open_actions_for_current_context();
        assert!(matches!(editor.script_target, ScriptTarget::Object));
        assert_eq!(editor.script_event, ObjectEvent::Click);

        editor.set_selection(vec![t_raw]);
        editor.open_actions_for_current_context();
        assert!(matches!(editor.script_target, ScriptTarget::Frame));
    }

    #[test]
    fn t177_stage_and_library_trace_bitmap_context_workflow() {
        let mut editor = make_test_editor();
        // Minimal valid 4x4 PNG image (red/blue pixels).
        let mut png_bytes = Vec::new();
        {
            let mut img = image::RgbaImage::new(4, 4);
            for y in 0..4 {
                for x in 0..4 {
                    let px = if x < 2 {
                        image::Rgba([220, 40, 40, 255])
                    } else {
                        image::Rgba([40, 90, 220, 255])
                    };
                    img.put_pixel(x, y, px);
                }
            }
            let mut cursor = std::io::Cursor::new(&mut png_bytes);
            image::DynamicImage::ImageRgba8(img)
                .write_to(&mut cursor, image::ImageFormat::Png)
                .expect("encode test png");
        }
        let res_unit =
            ResourceUnit::from_bytes("res-bmp", "sprite.png", "image/png", &png_bytes).unwrap();
        editor.project.resources.push(res_unit);
        editor.project.format_version = editor.project.format_version.max(3);
        editor.project.upgrade_format_for_features();

        let root_id = editor.project.root.clone();
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .clear();
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .push(make_test_element(
                "placed-bmp",
                "",
                10.0,
                15.0,
                ElementKind::Bitmap {
                    resource_id: "res-bmp".into(),
                    width: 4,
                    height: 4,
                    smoothing: false,
                },
            ));

        // Right-clicking placed bitmap under Rectangle tool selects it and enables Stage bitmap commands.
        editor.tool = Tool::Rectangle;
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let p = ui.painter();
                let scene = evaluate(&editor.project, &root_id, 0);
                let picked = editor.select_stage_context_target(
                    &scene,
                    p,
                    Pos2::new(12.0, 17.0),
                    StageMapping::identity(),
                );
                assert_eq!(picked.as_deref(), Some("placed-bmp"));
            });
        });
        assert_eq!(
            editor.selected_stage_bitmap_resource_id().as_deref(),
            Some("res-bmp")
        );
        assert!(editor.can_trace_bitmap());
        editor.open_trace_bitmap_dialog(&ctx);
        assert!(editor.bitmap_trace.dialog.is_some());
        editor.bitmap_trace.dialog = None;

        editor.open_bitmap_properties_dialog(&ctx, None);

        // Library-only trace creates a new editable vector symbol while preserving the source bitmap resource
        // in one atomic Undo step.
        editor.clear_selection();
        let symbols_before = editor.project.symbols.len();
        let undo_before = editor.history.undo_len();
        let created_sym_id = editor
            .trace_library_bitmap_resource("res-bmp")
            .expect("library bitmap trace should succeed");
        assert_eq!(editor.project.symbols.len(), symbols_before + 1);
        assert_eq!(editor.history.undo_len(), undo_before + 1);
        assert!(
            editor.project.resources.iter().any(|u| u.id == "res-bmp"),
            "source bitmap resource must remain untouched"
        );
        let traced_sym = editor
            .project
            .symbol(&created_sym_id)
            .expect("new traced symbol in library");
        assert_eq!(traced_sym.kind, SymbolKind::Graphic);
        assert!(
            traced_sym.timeline.layers[0].keyframes[0]
                .elements
                .iter()
                .all(|el| matches!(el.kind, ElementKind::Path { .. }))
        );
        assert!(
            !traced_sym.timeline.layers[0].keyframes[0]
                .elements
                .is_empty()
        );
        assert_eq!(
            editor.library_selected.as_deref(),
            Some(created_sym_id.as_str())
        );

        // Single Undo removes the traced symbol and restores the previous state cleanly.
        editor.undo();
        assert_eq!(editor.project.symbols.len(), symbols_before);
        assert!(editor.project.resources.iter().any(|u| u.id == "res-bmp"));
    }

    #[test]
    fn t180_stage_deco_and_spray_gestures_respect_transform_lock_and_single_undo() {
        let mut editor = make_test_editor();
        let leaf_sym = add_symbol_with_rect(&mut editor, "leaf-sym", "Leaf", SymbolKind::Graphic);
        let root_id = editor.project.root.clone();
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .clear();
        editor
            .project
            .symbol_mut(&root_id)
            .unwrap()
            .timeline
            .frame_count = 3;
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0]
            .keyframes
            .push(Keyframe {
                frame: 2,
                elements: vec![make_test_element(
                    "existing-frame-two",
                    "Existing",
                    200.0,
                    80.0,
                    ElementKind::Rectangle {
                        width: 24.0,
                        height: 24.0,
                        fill: [180, 80, 40, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                )],
                label: "preserve destination".into(),
                ..Keyframe::default()
            });

        // Configure animated Deco Vine Fill with step frames and custom branch angle.
        editor.set_stage_deco_options(StageDecoOptions {
            effect_index: StageDecoEffect::VineFill.index(),
            branch_angle_deg: 40.0,
            scale_pct: 115.0,
            segment_px: 24.0,
            step_frames: 2,
            animate_pattern: true,
            primary_color: [36, 160, 88, 255],
            secondary_color: [220, 190, 60, 255],
        });

        let before_deco = editor.project.clone();
        let undo_before = editor.history.undo_len();
        let created_deco = editor.apply_stage_deco_local_gesture(&[[40.0, 40.0], [120.0, 80.0]]);
        assert!(!created_deco.is_empty());
        assert_eq!(
            editor
                .selection
                .targets
                .iter()
                .map(|t| t.element.clone())
                .collect::<Vec<_>>(),
            created_deco
        );
        assert_eq!(
            editor.history.undo_len(),
            undo_before + 1,
            "one Deco gesture must commit as one Undo step"
        );
        assert!(
            editor.symbol().timeline.layers[0].keyframes.len() > 1,
            "animated Deco gesture must step keyframes across the timeline"
        );
        let occupied = editor.symbol().timeline.layers[0].key_at(2).unwrap();
        assert_eq!(occupied.label, "preserve destination");
        assert!(
            occupied
                .elements
                .iter()
                .any(|element| element.id == "existing-frame-two"),
            "animated Deco must preserve unrelated artwork on an occupied destination key"
        );

        editor.undo();
        assert_eq!(editor.project, before_deco);

        // Spray Brush gesture with Library symbol instances and deterministic seed.
        editor.set_stage_spray_options(StageSprayOptions {
            symbol_id: Some(leaf_sym),
            use_dots: false,
            fill: [50, 140, 240, 255],
            scale_w_pct: 90.0,
            scale_h_pct: 90.0,
            random_scaling: true,
            rotate_symbol: true,
            random_rotation: true,
            brush_width: 36.0,
            brush_height: 20.0,
            brush_angle_deg: 25.0,
            seed: 42,
        });
        let undo_before_spray = editor.history.undo_len();
        let sprayed =
            editor.apply_stage_spray_local_gesture(&[[30.0, 30.0], [90.0, 50.0], [150.0, 70.0]]);
        assert!(!sprayed.is_empty());
        assert_eq!(
            editor
                .selection
                .targets
                .iter()
                .map(|t| t.element.clone())
                .collect::<Vec<_>>(),
            sprayed
        );
        assert_eq!(
            editor.history.undo_len(),
            undo_before_spray + 1,
            "one Spray Brush drag must commit as one Undo step"
        );

        // Locked layer refuses both Deco and Spray gestures.
        editor.symbol_mut().timeline.layers[0].locked = true;
        assert!(
            editor
                .apply_stage_deco_local_gesture(&[[10.0, 10.0]])
                .is_empty()
        );
        assert!(
            editor
                .apply_stage_spray_local_gesture(&[[10.0, 10.0]])
                .is_empty()
        );
        editor.symbol_mut().timeline.layers[0].locked = false;

        editor.undo();
        assert!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .elements
                .is_empty()
        );

        // Auto-key creation belongs to the gesture's Undo record, not to an earlier edit.
        editor.frame = 1;
        editor.workspace.timeline_prefs.auto_keyframe = true;
        let before_auto_key = editor.project.clone();
        let sprayed = editor.apply_stage_spray_local_gesture(&[[20.0, 20.0], [60.0, 30.0]]);
        assert!(!sprayed.is_empty());
        assert!(
            editor.symbol().timeline.layers[0]
                .keyframes
                .iter()
                .any(|key| key.frame == 1)
        );
        editor.undo();
        assert_eq!(editor.project, before_auto_key);
        assert!(editor.project.validate().is_ok());
    }

    #[test]
    fn t186_stage_bone_bind_puppet_warp_and_3d_gizmo_authoring_workflows() {
        let mut editor = make_test_editor();
        let part_sym = add_symbol_with_rect(&mut editor, "limb-sym", "Limb", SymbolKind::MovieClip);
        let root_id = editor.project.root.clone();
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .clear();
        place_instance_on_root(
            &mut editor,
            "upper-arm",
            &part_sym,
            20.0,
            40.0,
            InstancePlayback::Independent,
        );
        place_instance_on_root(
            &mut editor,
            "forearm",
            &part_sym,
            90.0,
            40.0,
            InstancePlayback::Independent,
        );
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .push(make_test_element(
                "cape-path",
                "",
                0.0,
                0.0,
                ElementKind::Path {
                    commands: vec![
                        PathCommand::MoveTo { x: 20.0, y: 40.0 },
                        PathCommand::LineTo { x: 90.0, y: 40.0 },
                        PathCommand::LineTo { x: 150.0, y: 40.0 },
                    ],
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke: [240, 120, 40, 255],
                    stroke_width: 3.0,
                },
            ));

        // 1. Bone & Bind authoring: connect bones, drag CCD effector, bind vertices, and bake to keyframes.
        let leaf_bone = editor
            .stage_connect_ik_bone("upper-arm", "forearm")
            .expect("connect IK bone between upper-arm and forearm");
        editor.symbol_mut().timeline.frame_count = 2;
        editor.frame = 1;
        editor.workspace.timeline_prefs.auto_keyframe = true;
        let before_auto_key_pose = editor.project.clone();
        assert!(editor.stage_drag_ik_effector([105.0, 90.0], None));
        assert!(
            editor.symbol().timeline.layers[0]
                .keyframes
                .iter()
                .any(|key| key.frame == 1)
        );
        editor.undo();
        assert_eq!(editor.project, before_auto_key_pose);
        editor.frame = 0;
        assert!(editor.stage_drag_ik_effector([120.0, 100.0], None));

        assert!(editor.stage_bind_select_vertices("cape-path", vec![0, 1]));
        assert!(
            editor
                .stage_bind_assign_vertices_to_bone("cape-path", &leaf_bone, &[0, 1], 1.0, false)
                .is_some()
        );

        let baked = editor.stage_bake_ik_to_keyframes(0, 3);
        assert_eq!(baked, 4);

        // 2. Puppet / Asset Warp authoring: place pin, drag pin to deform mesh, reset, and delete pin.
        let pin_id = editor
            .stage_place_puppet_pin("cape-path", [90.0, 40.0], WarpPinKind::Open)
            .expect("place puppet pin");
        assert!(editor.stage_drag_puppet_pin("cape-path", &pin_id, [90.0, 85.0], 15.0));
        let deformed_cmd = &editor.symbol().timeline.layers[0].keyframes[0]
            .elements
            .iter()
            .find(|e| e.id == "cape-path")
            .unwrap()
            .kind;
        if let ElementKind::Path { commands, .. } = deformed_cmd {
            assert!(
                matches!(commands[1], PathCommand::LineTo { y, .. } if y > 60.0),
                "dragging puppet pin must deform the path vertex live"
            );
        } else {
            panic!("expected deformed path");
        }
        assert!(editor.stage_reset_puppet_pins("cape-path"));
        assert!(editor.stage_delete_puppet_pin("cape-path", &pin_id));

        // 3. 3D Translation & 3D Rotation gizmo handles on selected MovieClip instance.
        let t_upper = sel_target(&editor, "upper-arm");
        editor.set_selection(vec![t_upper]);
        let undo_before_3d = editor.history.undo_len();
        assert!(editor.stage_drag_3d_gizmo("upper-arm", Gizmo3dHandle::TranslateZ, 18.0, -24.0,));
        assert!(editor.stage_drag_3d_gizmo("upper-arm", Gizmo3dHandle::RotateY, 30.0, 0.0,));
        assert!(
            editor.stage_drag_3d_gizmo("upper-arm", Gizmo3dHandle::VanishingPoint, 12.0, -8.0,)
        );
        let state_3d: Stage3dAuthoringState = editor.element_3d_state("upper-arm");
        assert!(state_3d.translate_3d[2].abs() > 1.0);
        assert!(state_3d.rotate_3d[1].abs() > 1.0);
        assert_eq!(editor.history.undo_len(), undo_before_3d + 3);
        let (_, _, geom) = editor
            .selected_3d_gizmo_screen_geometry(Pos2::ZERO, StageMapping::identity())
            .expect("3D gizmo screen geometry");
        assert!(geom.center.distance(geom.x_tip) > 10.0);
        assert!(editor.project.validate().is_ok());
    }

    #[test]
    fn t199_global_secondary_press_outside_stage_response_does_not_retarget_selection() {
        let mut editor = make_test_editor();
        let root_id = editor.project.root.clone();
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .push(make_test_element(
                "outside-stage-response",
                "",
                140.0,
                140.0,
                ElementKind::Rectangle {
                    width: 30.0,
                    height: 30.0,
                    fill: [80, 120, 200, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
            ));
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(300.0))),
            events: vec![
                egui::Event::PointerMoved(Pos2::new(150.0, 150.0)),
                egui::Event::PointerButton {
                    pos: Pos2::new(150.0, 150.0),
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..egui::RawInput::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.interact(
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0)),
                    ui.id().with("bounded-stage-response"),
                    egui::Sense::click(),
                );
                let scene = evaluate(&editor.project, &root_id, 0);
                editor.stage_context_menu(
                    &response,
                    &scene,
                    ui.painter(),
                    StageMapping::identity(),
                    Pos2::ZERO,
                );
            });
        });
        assert!(editor.selection.is_empty());
    }
}
