use crate::app::{Editor, Tool};
use crate::theme::UiPalette;
use eframe::egui::{
    self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2,
};
use neoflash_core::*;

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
        } => path_bounds(commands)
            .ok()
            .flatten()
            .map(|b| {
                Rect::from_min_max(
                    Pos2::new(b.x_min as f32, b.y_min as f32),
                    Pos2::new(b.x_max as f32, b.y_max as f32),
                )
                .expand(*stroke_width as f32 / 2.)
            })
            .unwrap_or(Rect::NOTHING),
    }
}
pub(crate) fn world_bounds(item: &DrawItem, p: &egui::Painter) -> Rect {
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
    let Some(local) = inverse(item.transform, point) else {
        return false;
    };
    let bounds = local_bounds(item, painter);
    if !bounds.contains(local) {
        return false;
    }
    match &item.kind {
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
        } => hit_test_path(
            commands,
            *fill_rule,
            [local.x as f64, local.y as f64],
            fill[3] > 0 || matches!(&item.kind, ElementKind::PaintedPath { fill_gradient: Some(g), .. } if g.stops.iter().any(|s|s.color[3] > 0)),
            if stroke[3] > 0 || matches!(&item.kind, ElementKind::PaintedPath { stroke_gradient: Some(g), .. } if g.stops.iter().any(|s|s.color[3] > 0)) { *stroke_width } else { 0. },
            FlattenOptions {
                tolerance: 0.25
                    / (item.transform[0]
                        .hypot(item.transform[1])
                        .max(item.transform[2].hypot(item.transform[3]))
                        .max(0.001)
                        * zoom as f64),
                ..Default::default()
            },
        )
        .unwrap_or(false),
        ElementKind::Ellipse { .. } => {
            let nx = (local.x - bounds.center().x) / (bounds.width() / 2.);
            let ny = (local.y - bounds.center().y) / (bounds.height() / 2.);
            nx * nx + ny * ny <= 1.
        }
        _ => true,
    }
}
struct DrawStyle {
    origin: Pos2,
    zoom: f32,
    opacity: f64,
    tint: Option<Color32>,
    outlines: bool,
    bitmap_texture: Option<egui::TextureId>,
}
fn draw_item(p: &egui::Painter, item: &DrawItem, style: DrawStyle) {
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
            let points = [
                Pos2::ZERO,
                Pos2::new(w, 0.),
                Pos2::new(w, h),
                Pos2::new(0., h),
            ]
            .map(screen);
            if outlines {
                p.add(Shape::closed_line(
                    points.to_vec(),
                    Stroke::new(1.0_f32, foreground),
                ));
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
            crate::vector::paint_path(
                p,
                commands,
                crate::vector::PathPaint {
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
                },
                screen,
            );
        }
    }
}

impl Editor {
    pub fn stage(&mut self, ctx: &egui::Context) {
        let palette = UiPalette::current(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(palette.pasteboard))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(10.);
                    ui.label(
                        egui::RichText::new(&self.project.name)
                            .strong()
                            .color(palette.stage_title),
                    );
                    if self.dirty() {
                        ui.label("•");
                    }
                    if self.onion {
                        ui.label(
                            egui::RichText::new("Onion skin")
                                .small()
                                .color(palette.onion_before),
                        )
                        .on_hover_text(
                            "Tinted outlines or shapes show neighbouring frames. They are not content of the current frame; turn off Onion Skin to see this frame alone.",
                        );
                    }
                    if self.vector.draft.is_some() {
                        if ui.small_button("Finish · Enter").clicked() {
                            self.finish_path(false);
                        }
                        if ui.small_button("Close path").clicked() {
                            self.finish_path(true);
                        }
                        if ui.small_button("Cancel · Esc").clicked() {
                            self.vector.draft = None;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(10.);
                        ui.label(format!("{}%", (self.zoom * 100.).round()));
                        if ui.small_button("Fit stage").clicked() {
                            self.end_vector_pointer_phase();
                            self.cancel_selection_gesture();
                            self.shape_draft = None;
                            if self.vector.point_drag.is_some() {
                                self.cancel_vector();
                            }
                            self.zoom = 0.;
                            self.pan = Vec2::ZERO;
                        }
                    });
                });
                let avail = ui.available_size() - Vec2::new(0., 25.);
                let (rect, response) =
                    ui.allocate_exact_size(avail.max(Vec2::splat(1.)), Sense::click_and_drag());
                let stage_press = ui.input(|input| input.events.iter().find_map(|event| match event {
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } if rect.contains(*pos) => Some(*pos),
                    _ => None,
                }));
                if stage_press.is_some_and(|pos| ui.ctx().layer_id_at(pos) == Some(ui.layer_id())) {
                    self.timeline_edit.focused = false;
                }
                let size = Vec2::new(self.project.width as f32, self.project.height as f32);
                if self.zoom == 0. {
                    self.zoom = ((rect.width() - 60.) / size.x)
                        .min((rect.height() - 60.) / size.y)
                        .clamp(0.1, 2.);
                }
                if response.hovered() {
                    ctx.set_cursor_icon(match self.tool {
                        Tool::Select | Tool::Transform | Tool::Subselect => {
                            egui::CursorIcon::Default
                        }
                        Tool::Text => egui::CursorIcon::Text,
                        _ => egui::CursorIcon::Crosshair,
                    });
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
                let nested = self.editing.len() > 1;
                let page = Rect::from_center_size(rect.center() + self.pan, size * self.zoom);
                let origin = if nested { page.center() } else { page.min };
                self.stage_file_drop(ui, rect, origin);
                self.stage_library_drop(ui, rect, origin);
                let visible_page = page.intersect(rect);
                let center = if visible_page.is_positive() {
                    visible_page.center()
                } else {
                    page.center()
                };
                self.remember_paste_view([
                    (center.x - origin.x) as f64 / self.zoom as f64,
                    (center.y - origin.y) as f64 / self.zoom as f64,
                ]);
                let p = ui.painter_at(rect);
                p.rect_filled(page.translate(Vec2::new(3., 4.)), 0, palette.stage_shadow);
                p.rect_filled(page, 0, color(self.project.background, 1., None));
                let content = p.with_clip_rect(page.intersect(rect));
                if nested {
                    content.line_segment(
                        [origin - Vec2::new(8., 0.), origin + Vec2::new(8., 0.)],
                        Stroke::new(1.0_f32, palette.origin_cross),
                    );
                    content.line_segment(
                        [origin - Vec2::new(0., 8.), origin + Vec2::new(0., 8.)],
                        Stroke::new(1.0_f32, palette.origin_cross),
                    );
                }
                let id = self.symbol_id().to_owned();
                if self.onion {
                    let (start, end) = self.onion_range();
                    for frame in start..=end.min(self.symbol().timeline.frame_count - 1) {
                        if frame == self.frame {
                            continue;
                        }
                        let distance = frame.abs_diff(self.frame).max(1);
                        let opacity = self.onion_opacity as f64 / (distance as f64).sqrt();
                        let tint = if frame < self.frame {
                            palette.onion_before
                        } else {
                            palette.onion_after
                        };
                        let ghost = evaluate_scene(&self.project, &id, frame, true, self.show_layer_masking(ctx));
                        self.paint_authored_scene(&content, &ghost, DrawStyle { origin, zoom: self.zoom, opacity, tint: Some(tint), outlines: self.onion_outlines, bitmap_texture: None });
                    }
                }
                let authored_scene = self.current_authored_scene(self.show_layer_masking(ctx));
                self.paint_authored_scene(&content, &authored_scene, DrawStyle { origin, zoom: self.zoom, opacity: 1., tint: None, outlines: false, bitmap_texture: None });
                let scene: Vec<_> = authored_scene.draws.into_iter().map(|draw| draw.item).collect();
                if let Some(warning) = self.bitmap_textures.take_warning() {
                    self.message(warning);
                }
                p.rect_stroke(
                    page,
                    0,
                    Stroke::new(1.0_f32, palette.stage_border),
                    StrokeKind::Outside,
                );
                let extended_tool = self.authoring_tool_stage(ui, &p, origin, page, rect, &scene);
                if !extended_tool {
                let zoom = self.zoom;
                let to_local = |pos: Pos2| (pos - origin) / zoom;
                let on_path_handle = self.tool == Tool::Subselect
                    && self.selected_sampled().is_some_and(|e| {
                        if let ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } = &e.kind {
                            let matrix = transform_matrix(e.transform);
                            response.interact_pointer_pos().is_some_and(|pos| {
                                crate::vector::handles(commands).iter().any(|h| {
                                    (origin + affine(matrix, h.point).to_vec2() * zoom)
                                        .distance(pos)
                                        < 8.
                                })
                            })
                        } else {
                            false
                        }
                    });
                if response.clicked()
                    && self.tool == Tool::Subselect
                    && !on_path_handle
                    && self.vector.point_drag.is_none()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    if let Some(target) =
                        self.pick_placement(&scene, &p, Pos2::ZERO + to_local(pos))
                    {
                        self.set_selection(vec![target]);
                    } else {
                        self.clear_selection();
                    }
                }
                self.selection_stage(ui, &p, origin, rect, &scene, &response);
                self.shape_stage(ui, &p, origin, page, rect);
                if response.clicked()
                    && self.tool == Tool::Text
                    && let Some(pos) = response.interact_pointer_pos()
                    && page.contains(pos)
                {
                    let local = to_local(pos);
                    let font = if self.fonts.families.iter().any(|s| s == "Arial") {
                        "Arial"
                    } else {
                        "Helvetica"
                    };
                    let _ = self.fonts.load(ctx, font);
                    self.add_element_transformed(
                        ElementKind::TextBox {
                            text: "Your text".into(),
                            width: 240.,
                            height: 100.,
                            format: TextFormat {
                                font: font.into(),
                                size: 32.,
                                color: u32::from_be_bytes([
                                    0,
                                    self.fill[0],
                                    self.fill[1],
                                    self.fill[2],
                                ]),
                                ..Default::default()
                            },
                            multiline: true,
                            word_wrap: true,
                        },
                        Transform {
                            x: local.x as f64,
                            y: local.y as f64,
                            alpha: self.fill[3] as f64 / 255.,
                            ..Default::default()
                        },
                    );
                }
                self.vector_stage(ui, &content, origin, page, &response);
                }
                ui.horizontal(|ui| {
                    ui.add_space(8.);
                    ui.label(egui::RichText::new(&self.status).size(11.));
                });
            });
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
    fn paint_authored_scene(
        &mut self,
        painter: &egui::Painter,
        scene: &AuthoredScene,
        style: DrawStyle,
    ) {
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
        for draw in &scene.draws {
            let bitmap_texture = self.bitmap_textures.texture_transformed(
                painter.ctx(),
                &self.project,
                &draw.item.kind,
                draw.item.color_transform,
            );
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
                )
            });
            if let Err(error) = result {
                self.message(error);
                break;
            }
        }
    }
    pub(crate) fn paint_library_preview(&mut self, ui: &mut egui::Ui, symbol_id: &str) {
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
        let scene = evaluate_scene(&self.project, symbol_id, 0, false, true);
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
            &scene,
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
}
