use crate::app::{Editor, Tool};
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
fn local_bounds(item: &DrawItem, p: &egui::Painter) -> Rect {
    match &item.kind {
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. } => {
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
fn world_bounds(item: &DrawItem, p: &egui::Painter) -> Rect {
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
fn draw_item(
    p: &egui::Painter,
    item: &DrawItem,
    origin: Pos2,
    zoom: f32,
    opacity: f64,
    tint: Option<Color32>,
    outlines: bool,
) {
    let screen = |pt: Pos2| origin + affine(item.transform, pt).to_vec2() * zoom;
    let alpha = item.alpha * opacity;
    match &item.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        }
        | ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let points = if matches!(&item.kind, ElementKind::Ellipse { .. }) {
                (0..64)
                    .map(|i| {
                        let a = i as f64 / 64. * std::f64::consts::TAU;
                        screen(Pos2::new(
                            (width / 2. * (1. + a.cos())) as f32,
                            (height / 2. * (1. + a.sin())) as f32,
                        ))
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![
                    screen(Pos2::ZERO),
                    screen(Pos2::new(*width as f32, 0.)),
                    screen(Pos2::new(*width as f32, *height as f32)),
                    screen(Pos2::new(0., *height as f32)),
                ]
            };
            let fill = if outlines {
                Color32::TRANSPARENT
            } else {
                color(*fill, alpha, tint)
            };
            let edge = if outlines {
                Stroke::new(1.0_f32, color([90, 110, 130, 255], alpha, tint))
            } else {
                Stroke::new(*stroke_width as f32 * zoom, color(*stroke, alpha, tint))
            };
            p.add(Shape::convex_polygon(points, fill, edge));
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
        ElementKind::Instance { .. } => {}
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
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
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(163, 167, 173)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(10.);
                    ui.label(
                        egui::RichText::new(&self.project.name)
                            .strong()
                            .color(Color32::from_gray(35)),
                    );
                    if self.dirty() {
                        ui.label("•");
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
                            self.zoom = 0.;
                            self.pan = Vec2::ZERO;
                        }
                    });
                });
                let avail = ui.available_size() - Vec2::new(0., 25.);
                let (rect, response) =
                    ui.allocate_exact_size(avail.max(Vec2::splat(1.)), Sense::click_and_drag());
                let size = Vec2::new(self.project.width as f32, self.project.height as f32);
                if self.zoom == 0. {
                    self.zoom = ((rect.width() - 60.) / size.x)
                        .min((rect.height() - 60.) / size.y)
                        .clamp(0.1, 2.);
                }
                let nested = self.editing.len() > 1;
                let page = Rect::from_center_size(rect.center() + self.pan, size * self.zoom);
                let origin = if nested { page.center() } else { page.min };
                let p = ui.painter_at(rect);
                p.rect_filled(
                    page.translate(Vec2::new(3., 4.)),
                    0,
                    Color32::from_black_alpha(40),
                );
                p.rect_filled(page, 0, color(self.project.background, 1., None));
                let content = p.with_clip_rect(page.intersect(rect));
                if nested {
                    content.line_segment(
                        [origin - Vec2::new(8., 0.), origin + Vec2::new(8., 0.)],
                        Stroke::new(1.0_f32, Color32::from_gray(120)),
                    );
                    content.line_segment(
                        [origin - Vec2::new(0., 8.), origin + Vec2::new(0., 8.)],
                        Stroke::new(1.0_f32, Color32::from_gray(120)),
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
                            Color32::from_rgb(37, 110, 191)
                        } else {
                            Color32::from_rgb(219, 97, 66)
                        };
                        for item in evaluate(&self.project, &id, frame) {
                            draw_item(
                                &content,
                                &item,
                                origin,
                                self.zoom,
                                opacity,
                                Some(tint),
                                self.onion_outlines,
                            );
                        }
                    }
                }
                let scene = evaluate(&self.project, &id, self.frame);
                for item in &scene {
                    draw_item(&content, item, origin, self.zoom, 1., None, false);
                }
                p.rect_stroke(
                    page,
                    0,
                    Stroke::new(1.0_f32, Color32::from_gray(114)),
                    StrokeKind::Outside,
                );
                if let Some(selected) = &self.selected {
                    let mut bounds = Rect::NOTHING;
                    for item in &scene {
                        if item.instance_path.first() == Some(selected)
                            || &item.element_id == selected
                        {
                            bounds = bounds.union(world_bounds(item, &p));
                        }
                    }
                    if bounds.is_finite() {
                        let r = Rect::from_min_max(
                            origin + bounds.min.to_vec2() * self.zoom,
                            origin + bounds.max.to_vec2() * self.zoom,
                        );
                        p.rect_stroke(
                            r,
                            0,
                            Stroke::new(1.0_f32, Color32::from_rgb(36, 104, 190)),
                            StrokeKind::Outside,
                        );
                        for corner in [
                            r.left_top(),
                            r.right_top(),
                            r.right_bottom(),
                            r.left_bottom(),
                            r.center_top(),
                            r.center_bottom(),
                            r.left_center(),
                            r.right_center(),
                        ] {
                            p.rect_filled(
                                Rect::from_center_size(corner, Vec2::splat(5.)),
                                0,
                                Color32::from_rgb(36, 104, 190),
                            );
                        }
                    }
                }
                let zoom = self.zoom;
                let to_local = |pos: Pos2| (pos - origin) / zoom;
                let pick = |pos: Pos2| -> Option<(String, String)> {
                    let point = Pos2::ZERO + to_local(pos);
                    scene
                        .iter()
                        .rev()
                        .find(|item| {
                            let Some(local) = inverse(item.transform, point) else {
                                return false;
                            };
                            let bounds = local_bounds(item, &p);
                            if !bounds.contains(local) {
                                return false;
                            }
                            if let ElementKind::Path {
                                commands,
                                fill_rule,
                                fill,
                                stroke,
                                stroke_width,
                            } = &item.kind
                            {
                                hit_test_path(
                                    commands,
                                    *fill_rule,
                                    [local.x as f64, local.y as f64],
                                    fill[3] > 0,
                                    if stroke[3] > 0 { *stroke_width } else { 0. },
                                    FlattenOptions {
                                        tolerance: 0.25
                                            / ((item.transform[0].hypot(item.transform[1]))
                                                .max(item.transform[2].hypot(item.transform[3]))
                                                .max(0.001)
                                                * zoom as f64),
                                        ..Default::default()
                                    },
                                )
                                .unwrap_or(false)
                            } else if matches!(item.kind, ElementKind::Ellipse { .. }) {
                                let nx = (local.x - bounds.center().x) / (bounds.width() / 2.);
                                let ny = (local.y - bounds.center().y) / (bounds.height() / 2.);
                                nx * nx + ny * ny <= 1.
                            } else {
                                true
                            }
                        })
                        .map(|item| {
                            (
                                item.instance_path
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| item.element_id.clone()),
                                item.layer_id.clone(),
                            )
                        })
                };
                if response.hovered() {
                    ctx.set_cursor_icon(match self.tool {
                        Tool::Select | Tool::Subselect => egui::CursorIcon::Default,
                        Tool::Text => egui::CursorIcon::Text,
                        _ => egui::CursorIcon::Crosshair,
                    });
                    let delta = ctx.input(|i| i.smooth_scroll_delta);
                    if ctx.input(|i| i.modifiers.command) {
                        if delta.y != 0. {
                            self.zoom = (self.zoom * (1. + delta.y * 0.002)).clamp(0.1, 4.);
                        }
                    } else {
                        self.pan += delta;
                    }
                }
                let on_path_handle = self.tool == Tool::Subselect
                    && self.selected_sampled().is_some_and(|e| {
                        if let ElementKind::Path { commands, .. } = &e.kind {
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
                    && matches!(self.tool, Tool::Select | Tool::Subselect)
                    && !on_path_handle
                    && self.vector.point_drag.is_none()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    if let Some((object, _layerid)) = pick(pos) {
                        if let Some(index) = self.symbol().timeline.layers.iter().position(|l| {
                            l.key_at(self.frame)
                                .is_some_and(|k| k.elements.iter().any(|e| e.id == object))
                        }) {
                            self.layer = index;
                        }
                        self.selected = Some(object);
                    } else {
                        self.selected = None;
                    }
                }
                if response.double_clicked()
                    && self.tool == Tool::Select
                    && let Some(Element {
                        kind: ElementKind::Instance { symbol_id, .. },
                        ..
                    }) = self.selected_element().cloned()
                {
                    self.enter_symbol(symbol_id);
                }
                // Preserve complete gestures even if a fast input device delivers
                // press/move/release before the next repaint (egui then has no drag flags).
                let pointer_edges: Vec<(Pos2, bool)> = ui.input(|i| {
                    i.events
                        .iter()
                        .filter_map(|e| match e {
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                ..
                            } => Some((*pos, *pressed)),
                            _ => None,
                        })
                        .collect()
                });
                let mut raw_release = None;
                for (pos, pressed) in pointer_edges {
                    if pressed
                        && matches!(self.tool, Tool::Rectangle | Tool::Ellipse)
                        && page.contains(pos)
                        && rect.contains(pos)
                        && response.contains_pointer()
                    {
                        self.draw_start = Some(pos);
                    }
                    if !pressed {
                        raw_release = Some(pos);
                    }
                }
                if response.drag_started()
                    && let Some(pos) = ui
                        .input(|i| i.pointer.press_origin())
                        .or_else(|| response.interact_pointer_pos())
                {
                    match self.tool {
                        Tool::Select => {
                            if let Some((object, _layerid)) = pick(pos) {
                                if let Some(index) =
                                    self.symbol().timeline.layers.iter().position(|l| {
                                        l.key_at(self.frame).is_some_and(|k| {
                                            k.elements.iter().any(|e| e.id == object)
                                        })
                                    })
                                {
                                    self.layer = index;
                                }
                                self.selected = Some(object);
                            }
                            if pick(pos).is_none() {
                                self.selected = None;
                            }
                            if self.layer_editable() && self.selected.is_some() {
                                self.history.begin(&self.project);
                                self.ensure_key(false);
                                if let Some(element) = self.selected_element() {
                                    self.drag_origin = Some((pos, element.transform));
                                }
                            }
                        }
                        Tool::Rectangle | Tool::Ellipse => {
                            if page.contains(pos) {
                                self.draw_start.get_or_insert(pos);
                            }
                        }
                        Tool::Text | Tool::Pen | Tool::Pencil | Tool::Subselect => {}
                    }
                }
                if response.dragged()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    if let Some((start, transform)) = self.drag_origin {
                        let delta = (pos - start) / self.zoom;
                        if let Some(e) = self.selected_element_mut() {
                            e.transform.x = transform.x + delta.x as f64;
                            e.transform.y = transform.y + delta.y as f64;
                        }
                    }
                    if let Some(start) = self.draw_start {
                        p.rect_stroke(
                            Rect::from_two_pos(start, pos),
                            0,
                            Stroke::new(1.0_f32, Color32::from_rgb(36, 104, 190)),
                            StrokeKind::Outside,
                        );
                    }
                }
                if response.drag_stopped() || raw_release.is_some() {
                    if self.drag_origin.take().is_some() {
                        self.history.commit(&self.project);
                    }
                    if let Some(start) = self.draw_start.take()
                        && let Some(pos) = raw_release
                            .or_else(|| response.interact_pointer_pos())
                            .or_else(|| ui.input(|i| i.pointer.latest_pos()))
                    {
                        let r = Rect::from_two_pos(
                            Pos2::ZERO + to_local(start),
                            Pos2::ZERO + to_local(pos),
                        );
                        if r.width() > 2. && r.height() > 2. {
                            let kind = if self.tool == Tool::Rectangle {
                                ElementKind::Rectangle {
                                    width: r.width() as f64,
                                    height: r.height() as f64,
                                    fill: self.fill,
                                    stroke: [30, 50, 70, 255],
                                    stroke_width: 0.,
                                }
                            } else {
                                ElementKind::Ellipse {
                                    width: r.width() as f64,
                                    height: r.height() as f64,
                                    fill: self.fill,
                                    stroke: [30, 50, 70, 255],
                                    stroke_width: 0.,
                                }
                            };
                            self.add_element(kind, r.left() as f64, r.top() as f64);
                        }
                    }
                }
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
                    self.add_element(
                        ElementKind::Text {
                            text: "Your text".into(),
                            font_family: font.into(),
                            font_size: 32.,
                            color: self.fill,
                        },
                        local.x as f64,
                        local.y as f64,
                    );
                }
                self.vector_stage(ui, &content, origin, page, &response);
                ui.horizontal(|ui| {
                    ui.add_space(8.);
                    ui.label(egui::RichText::new(&self.status).size(11.));
                });
            });
    }
}
