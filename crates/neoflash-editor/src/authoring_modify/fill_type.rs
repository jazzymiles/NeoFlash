use super::*;

impl Editor {
    pub(crate) fn apply_drawing_gradients_to_element(&self, element: &mut Element) -> bool {
        if self.authoring_modify.drawing_fill_gradient.is_none()
            && self.authoring_modify.drawing_stroke_gradient.is_none()
        {
            return false;
        }
        let Some((commands, fill_rule)) = element.to_path_commands() else {
            return false;
        };
        let Some((fill, stroke, stroke_width, _)) = paints(&element.kind) else {
            return false;
        };
        let matrix = default_gradient_matrix(&commands);
        let fill_gradient = self
            .authoring_modify
            .drawing_fill_gradient
            .clone()
            .map(|mut paint| {
                if self.tool != crate::app::Tool::Brush || !self.authoring_tools.brush_lock_fill {
                    paint.matrix = matrix;
                }
                paint
            });
        let stroke_gradient =
            self.authoring_modify
                .drawing_stroke_gradient
                .clone()
                .map(|mut paint| {
                    paint.matrix = matrix;
                    paint
                });
        let effective_fill = if matches!(element.kind, ElementKind::Ellipse { .. })
            && !element.oval_options().close_path
        {
            [0, 0, 0, 0]
        } else {
            fill
        };
        element.transform.primitive = None;
        element.kind = ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill: if effective_fill == [0, 0, 0, 0] && fill_gradient.is_none() {
                effective_fill
            } else {
                fill
            },
            stroke,
            fill_gradient: if effective_fill == [0, 0, 0, 0] {
                None
            } else {
                fill_gradient
            },
            stroke_gradient,
            stroke_width,
        };
        true
    }

    pub(crate) fn target_has_bitmap_fill(&self, target: &SelectionTarget) -> bool {
        self.symbol()
            .timeline
            .layers
            .iter()
            .find(|l| l.id == target.layer)
            .and_then(|l| l.key_at(self.frame))
            .is_some_and(|k| {
                k.bitmap_fills
                    .iter()
                    .any(|bf| bf.element_id == target.element)
            })
    }

    pub(crate) fn contextual_paints(
        &self,
    ) -> (
        Color,
        Option<GradientPaint>,
        Color,
        Option<GradientPaint>,
        f64,
    ) {
        if self.authoring_modify.color_selection {
            let values: Vec<_> = self
                .sampled_targets()
                .into_iter()
                .filter(|(t, _)| self.target_editable(t))
                .filter_map(|(t, e)| {
                    paints_with_gradients_for_element(&e, self.target_has_bitmap_fill(&t))
                })
                .collect();
            if !values.is_empty() {
                let (fill, fill_grad) =
                    if let Some(fill_sample) = values.iter().find(|v| v.has_fill) {
                        (fill_sample.fill, fill_sample.fill_gradient.clone())
                    } else {
                        ([0, 0, 0, 0], None)
                    };
                let (stroke, stroke_grad, width) =
                    if let Some(stroke_sample) = values.iter().find(|v| v.has_stroke) {
                        (
                            stroke_sample.stroke,
                            stroke_sample.stroke_gradient.clone(),
                            stroke_sample.stroke_width,
                        )
                    } else if let Some(stroke_sample) = values.iter().find(|v| v.supports_stroke) {
                        ([0, 0, 0, 0], None, stroke_sample.stroke_width)
                    } else {
                        (
                            self.stroke,
                            self.authoring_modify.drawing_stroke_gradient.clone(),
                            self.stroke_width,
                        )
                    };
                return (fill, fill_grad, stroke, stroke_grad, width);
            }
        }
        (
            self.fill,
            self.authoring_modify.drawing_fill_gradient.clone(),
            self.stroke,
            self.authoring_modify.drawing_stroke_gradient.clone(),
            self.stroke_width,
        )
    }

    pub(crate) fn set_selection_fill_type(&mut self, stroke_well: bool, mode: PaintFillType) {
        if !self.authoring_modify.color_selection {
            self.set_drawing_fill_type(stroke_well, mode);
            return;
        }
        if mode == PaintFillType::BitmapFill {
            if !stroke_well {
                let idx = self.authoring_modify.drawing_bitmap_fill.unwrap_or(0);
                self.authoring_modify.drawing_bitmap_fill = Some(idx);
                self.authoring_modify.drawing_fill_gradient = None;
                let _ = self.apply_bitmap_swatch_tile(idx);
            }
            return;
        }
        if !stroke_well {
            self.authoring_modify.drawing_bitmap_fill = None;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        if targets.is_empty() {
            self.set_drawing_fill_type(stroke_well, mode);
            return;
        }
        let multi = targets.len() > 1;
        let had_bitmap_targets: Vec<SelectionTarget> = if !stroke_well {
            targets
                .iter()
                .filter(|(t, _)| self.target_has_bitmap_fill(t))
                .map(|(t, _)| t.clone())
                .collect()
        } else {
            Vec::new()
        };
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(target, mut element)| {
                let had_bitmap = had_bitmap_targets.contains(&target);
                let eligible = if stroke_well {
                    element_has_stroke(&element) || (!multi && !is_merge_fill(&element))
                } else {
                    element_has_fill(&element, had_bitmap)
                        || (!multi && !element_has_stroke(&element) && !is_merge_stroke(&element))
                };
                if !eligible {
                    return None;
                }
                (set_element_fill_type(&mut element, stroke_well, mode) || had_bitmap)
                    .then_some((target, element))
            })
            .collect();
        if matches!(mode, PaintFillType::None | PaintFillType::Solid)
            && let Some((_, first_el)) = changes.first()
            && let Some(sample) = paints_with_gradients(&first_el.kind)
        {
            if stroke_well {
                self.stroke = sample.stroke;
                self.authoring_modify.drawing_stroke_gradient = None;
            } else {
                self.fill = sample.fill;
                self.authoring_modify.drawing_fill_gradient = None;
            }
        }
        if !had_bitmap_targets.is_empty() {
            let frame = self.frame;
            self.finish_edits();
            self.transact(|editor| {
                for target in &had_bitmap_targets {
                    editor.ensure_key_for(&target.layer, false);
                    if let Some(layer) = editor
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == target.layer)
                        && let Some(key) = layer.key_at_mut(frame)
                    {
                        key.bitmap_fills
                            .retain(|bf| bf.element_id != target.element);
                    }
                }
                for (target, element) in changes {
                    editor.ensure_key_for(&target.layer, false);
                    if let Some(layer) = editor
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == target.layer)
                        && let Some(key) = layer.key_at_mut(frame)
                        && let Some(slot) = key.elements.iter_mut().find(|e| e.id == target.element)
                    {
                        *slot = element;
                    }
                }
            });
            return;
        }
        self.apply_modified(changes, None);
    }

    pub(crate) fn set_drawing_fill_type(&mut self, stroke_well: bool, mode: PaintFillType) {
        if mode == PaintFillType::BitmapFill {
            if !stroke_well {
                let idx = self.authoring_modify.drawing_bitmap_fill.unwrap_or(0);
                self.authoring_modify.drawing_bitmap_fill = Some(idx);
                self.authoring_modify.drawing_fill_gradient = None;
                let _ = self.apply_bitmap_swatch_tile(idx);
            }
            return;
        }
        if !stroke_well {
            self.authoring_modify.drawing_bitmap_fill = None;
        }
        let (color, grad) = if stroke_well {
            (
                &mut self.stroke,
                &mut self.authoring_modify.drawing_stroke_gradient,
            )
        } else {
            (
                &mut self.fill,
                &mut self.authoring_modify.drawing_fill_gradient,
            )
        };
        match mode {
            PaintFillType::None => {
                *color = [0, 0, 0, 0];
                *grad = None;
            }
            PaintFillType::Solid => {
                if *color == [0, 0, 0, 0] {
                    *color = if stroke_well {
                        [0, 0, 0, 255]
                    } else {
                        [255, 255, 255, 255]
                    };
                } else if grad.is_some() && color[3] == 0 {
                    color[3] = 255;
                }
                *grad = None;
            }
            PaintFillType::LinearGradient | PaintFillType::RadialGradient => {
                let kind = if mode == PaintFillType::LinearGradient {
                    GradientKind::Linear
                } else {
                    GradientKind::Radial
                };
                if color[3] == 0 {
                    *color = if stroke_well {
                        [0, 0, 0, 255]
                    } else {
                        [255, 255, 255, 255]
                    };
                }
                if let Some(existing) = grad {
                    existing.kind = kind;
                } else {
                    let box_cmds =
                        rectangle_path_commands(100.0, 100.0, RectangleOptions::default());
                    *grad = Some(default_gradient_paint(kind, *color, &box_cmds));
                }
            }
            PaintFillType::BitmapFill => {}
        }
    }

    pub(crate) fn set_selection_gradient_stops(
        &mut self,
        stroke_well: bool,
        stops: Vec<GradientStop>,
        control: Option<egui::Id>,
    ) {
        let normalized = normalize_gradient_stops(stops);
        if normalized.is_empty() {
            return;
        }
        {
            let (color, grad) = if stroke_well {
                (
                    &mut self.stroke,
                    &mut self.authoring_modify.drawing_stroke_gradient,
                )
            } else {
                (
                    &mut self.fill,
                    &mut self.authoring_modify.drawing_fill_gradient,
                )
            };
            if let Some(paint) = grad {
                paint.stops = normalized.clone();
                *color = normalized[0].color;
            }
        }
        if !self.authoring_modify.color_selection {
            return;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        let mut first_updated_grad = None;
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(target, mut element)| {
                if let ElementKind::PaintedPath {
                    fill,
                    stroke,
                    fill_gradient,
                    stroke_gradient,
                    ..
                } = &mut element.kind
                {
                    let (base, slot) = if stroke_well {
                        (stroke, stroke_gradient)
                    } else {
                        (fill, fill_gradient)
                    };
                    if let Some(paint) = slot
                        && paint.stops != normalized
                    {
                        paint.stops = normalized.clone();
                        *base = normalized[0].color;
                        if first_updated_grad.is_none() {
                            first_updated_grad = Some(paint.clone());
                        }
                        return Some((target, element));
                    }
                }
                None
            })
            .collect();
        if let Some(updated) = first_updated_grad {
            let (color, grad) = if stroke_well {
                (
                    &mut self.stroke,
                    &mut self.authoring_modify.drawing_stroke_gradient,
                )
            } else {
                (
                    &mut self.fill,
                    &mut self.authoring_modify.drawing_fill_gradient,
                )
            };
            *color = updated.stops[0].color;
            if grad.is_none() {
                *grad = Some(updated);
            }
        }
        self.apply_modified(changes, control);
    }

    pub(crate) fn set_selection_gradient_options(
        &mut self,
        stroke_well: bool,
        spread: neoflash_core::GradientSpread,
        linear_rgb: bool,
    ) {
        {
            let grad = if stroke_well {
                &mut self.authoring_modify.drawing_stroke_gradient
            } else {
                &mut self.authoring_modify.drawing_fill_gradient
            };
            if let Some(paint) = grad {
                paint.spread = spread;
                paint.linear_rgb = linear_rgb;
            }
        }
        if !self.authoring_modify.color_selection {
            return;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        let mut first_updated_grad = None;
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(target, mut element)| {
                if let ElementKind::PaintedPath {
                    fill_gradient,
                    stroke_gradient,
                    ..
                } = &mut element.kind
                {
                    let slot = if stroke_well {
                        stroke_gradient
                    } else {
                        fill_gradient
                    };
                    if let Some(paint) = slot
                        && (paint.spread != spread || paint.linear_rgb != linear_rgb)
                    {
                        paint.spread = spread;
                        paint.linear_rgb = linear_rgb;
                        if first_updated_grad.is_none() {
                            first_updated_grad = Some(paint.clone());
                        }
                        return Some((target, element));
                    }
                }
                None
            })
            .collect();
        if let Some(updated) = first_updated_grad {
            let grad = if stroke_well {
                &mut self.authoring_modify.drawing_stroke_gradient
            } else {
                &mut self.authoring_modify.drawing_fill_gradient
            };
            if grad.is_none() {
                *grad = Some(updated);
            }
        }
        self.apply_modified(changes, None);
    }

    pub(crate) fn swap_active_paints(&mut self) {
        let (ctx_fill, ctx_fill_grad, ctx_stroke, ctx_stroke_grad, _) = self.contextual_paints();
        self.fill = ctx_stroke;
        self.stroke = ctx_fill;
        self.authoring_modify.drawing_fill_gradient = ctx_stroke_grad;
        self.authoring_modify.drawing_stroke_gradient = ctx_fill_grad;
        if !self.authoring_modify.color_selection {
            return;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        let multi = targets.len() > 1;
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(target, mut element)| {
                if multi
                    && (!element_has_fill(&element, self.target_has_bitmap_fill(&target))
                        || !element_has_stroke(&element))
                {
                    return None;
                }
                match &mut element.kind {
                    ElementKind::Rectangle { fill, stroke, .. }
                    | ElementKind::Ellipse { fill, stroke, .. }
                    | ElementKind::Path { fill, stroke, .. } => {
                        std::mem::swap(fill, stroke);
                        Some((target, element))
                    }
                    ElementKind::PaintedPath {
                        fill,
                        stroke,
                        fill_gradient,
                        stroke_gradient,
                        ..
                    } => {
                        std::mem::swap(fill, stroke);
                        std::mem::swap(fill_gradient, stroke_gradient);
                        Some((target, element))
                    }
                    _ => None,
                }
            })
            .collect();
        self.apply_modified(changes, None);
    }

    pub(crate) fn reset_default_paints(&mut self) {
        self.fill = [255, 255, 255, 255];
        self.stroke = [0, 0, 0, 255];
        self.authoring_modify.drawing_fill_gradient = None;
        self.authoring_modify.drawing_stroke_gradient = None;
        self.authoring_modify.drawing_bitmap_fill = None;
        if !self.authoring_modify.color_selection {
            return;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(target, mut element)| {
                let has_fill = element_has_fill(&element, self.target_has_bitmap_fill(&target));
                let has_stroke = element_has_stroke(&element);
                match &mut element.kind {
                    ElementKind::Rectangle { fill, stroke, .. }
                    | ElementKind::Ellipse { fill, stroke, .. }
                    | ElementKind::Path { fill, stroke, .. } => {
                        let mut changed = false;
                        if has_fill && *fill != [255, 255, 255, 255] {
                            *fill = [255, 255, 255, 255];
                            changed = true;
                        }
                        if has_stroke && *stroke != [0, 0, 0, 255] {
                            *stroke = [0, 0, 0, 255];
                            changed = true;
                        }
                        changed.then_some((target, element))
                    }
                    ElementKind::PaintedPath {
                        fill,
                        stroke,
                        fill_gradient,
                        stroke_gradient,
                        ..
                    } => {
                        let mut changed = false;
                        if has_fill && (*fill != [255, 255, 255, 255] || fill_gradient.is_some()) {
                            *fill = [255, 255, 255, 255];
                            *fill_gradient = None;
                            changed = true;
                        }
                        if has_stroke && (*stroke != [0, 0, 0, 255] || stroke_gradient.is_some()) {
                            *stroke = [0, 0, 0, 255];
                            *stroke_gradient = None;
                            changed = true;
                        }
                        changed.then_some((target, element))
                    }
                    ElementKind::Text { color, .. } => {
                        let changed = *color != [0, 0, 0, 255];
                        *color = [0, 0, 0, 255];
                        changed.then_some((target, element))
                    }
                    ElementKind::TextBox { format, .. } => {
                        let changed = format.color != 0;
                        format.color = 0;
                        changed.then_some((target, element))
                    }
                    _ => None,
                }
            })
            .collect();
        self.apply_modified(changes, None);
    }
}

pub(crate) fn ensure_solid_non_none(color: Color) -> Color {
    if color == [0, 0, 0, 0] {
        [1, 0, 0, 0]
    } else {
        color
    }
}

pub(crate) fn element_has_fill(element: &Element, has_bitmap_fill: bool) -> bool {
    if is_merge_stroke(element) {
        return false;
    }
    match &element.kind {
        ElementKind::Text { .. } | ElementKind::TextBox { .. } => true,
        ElementKind::Rectangle { fill, .. } => *fill != [0, 0, 0, 0] || has_bitmap_fill,
        ElementKind::Ellipse { fill, .. } => {
            element.oval_options().close_path && (*fill != [0, 0, 0, 0] || has_bitmap_fill)
        }
        ElementKind::Path { commands, fill, .. } => {
            !flatten_closed_rings(commands).is_empty() && (*fill != [0, 0, 0, 0] || has_bitmap_fill)
        }
        ElementKind::PaintedPath {
            commands,
            fill,
            fill_gradient,
            ..
        } => {
            !flatten_closed_rings(commands).is_empty()
                && (fill_gradient.is_some() || *fill != [0, 0, 0, 0] || has_bitmap_fill)
        }
        _ => false,
    }
}

pub(crate) fn element_has_stroke(element: &Element) -> bool {
    if is_merge_fill(element) {
        return false;
    }
    match &element.kind {
        ElementKind::Rectangle {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            stroke,
            stroke_width,
            ..
        } => *stroke_width > 0.0 && *stroke != [0, 0, 0, 0],
        ElementKind::PaintedPath {
            stroke,
            stroke_gradient,
            stroke_width,
            ..
        } => *stroke_width > 0.0 && (*stroke != [0, 0, 0, 0] || stroke_gradient.is_some()),
        _ => false,
    }
}

#[derive(Clone)]
pub(super) struct PaintSample {
    pub(super) fill: Color,
    pub(super) stroke: Color,
    pub(super) fill_gradient: Option<GradientPaint>,
    pub(super) stroke_gradient: Option<GradientPaint>,
    pub(super) stroke_width: f64,
    pub(super) supports_stroke: bool,
    pub(super) has_fill: bool,
    pub(super) has_stroke: bool,
}

pub(super) fn paints_with_gradients_for_element(
    element: &Element,
    has_bitmap_fill: bool,
) -> Option<PaintSample> {
    let mut sample = paints_with_gradients(&element.kind)?;
    sample.has_fill = element_has_fill(element, has_bitmap_fill);
    sample.has_stroke = element_has_stroke(element);
    Some(sample)
}

pub(super) fn paints_with_gradients(kind: &ElementKind) -> Option<PaintSample> {
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
        } => Some(PaintSample {
            fill: *fill,
            stroke: *stroke,
            fill_gradient: None,
            stroke_gradient: None,
            stroke_width: *stroke_width,
            supports_stroke: true,
            has_fill: *fill != [0, 0, 0, 0],
            has_stroke: *stroke_width > 0.0 && *stroke != [0, 0, 0, 0],
        }),
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => Some(PaintSample {
            fill: *fill,
            stroke: *stroke,
            fill_gradient: fill_gradient.clone(),
            stroke_gradient: stroke_gradient.clone(),
            stroke_width: *stroke_width,
            supports_stroke: true,
            has_fill: fill_gradient.is_some() || *fill != [0, 0, 0, 0],
            has_stroke: *stroke_width > 0.0
                && (*stroke != [0, 0, 0, 0] || stroke_gradient.is_some()),
        }),
        ElementKind::Text { color, .. } => Some(PaintSample {
            fill: *color,
            stroke: [0; 4],
            fill_gradient: None,
            stroke_gradient: None,
            stroke_width: 0.,
            supports_stroke: false,
            has_fill: true,
            has_stroke: false,
        }),
        ElementKind::TextBox { format, .. } => {
            let [_, r, g, b] = format.color.to_be_bytes();
            Some(PaintSample {
                fill: [r, g, b, 255],
                stroke: [0; 4],
                fill_gradient: None,
                stroke_gradient: None,
                stroke_width: 0.,
                supports_stroke: false,
                has_fill: true,
                has_stroke: false,
            })
        }
        _ => None,
    }
}

pub(super) fn classify_fill_type(
    color: Color,
    gradient: Option<&GradientPaint>,
    has_bitmap_fill: bool,
) -> PaintFillType {
    if has_bitmap_fill {
        return PaintFillType::BitmapFill;
    }
    match gradient.map(|g| g.kind) {
        Some(GradientKind::Linear) => PaintFillType::LinearGradient,
        Some(GradientKind::Radial) => PaintFillType::RadialGradient,
        None if color == [0, 0, 0, 0] => PaintFillType::None,
        None => PaintFillType::Solid,
    }
}

pub(crate) fn default_gradient_matrix(commands: &[PathCommand]) -> [f64; 6] {
    let (w, h, cx, cy) = match path_bounds(commands).ok().flatten() {
        Some(b) => (
            b.width().max(1.0),
            b.height().max(1.0),
            (b.x_min + b.x_max) * 0.5,
            (b.y_min + b.y_max) * 0.5,
        ),
        None => (100.0, 100.0, 50.0, 50.0),
    };
    [
        (w / 1638.4).clamp(1e-4, 1e4),
        0.0,
        0.0,
        (h / 1638.4).clamp(1e-4, 1e4),
        cx.clamp(-1e9, 1e9),
        cy.clamp(-1e9, 1e9),
    ]
}

pub(super) fn default_gradient_paint(
    kind: GradientKind,
    base_color: Color,
    commands: &[PathCommand],
) -> GradientPaint {
    let start = if base_color[3] == 0 {
        [255, 255, 255, 255]
    } else {
        base_color
    };
    let end = if u16::from(start[0]) + u16::from(start[1]) + u16::from(start[2]) < 128 {
        [255, 255, 255, start[3]]
    } else {
        [0, 0, 0, start[3]]
    };
    GradientPaint {
        kind,
        matrix: default_gradient_matrix(commands),
        stops: vec![
            GradientStop {
                ratio: 0,
                color: start,
            },
            GradientStop {
                ratio: 255,
                color: end,
            },
        ],
        focal_point_ratio: 0.0,
        ..Default::default()
    }
}

pub(super) fn normalize_gradient_stops(mut stops: Vec<GradientStop>) -> Vec<GradientStop> {
    stops.sort_by_key(|s| s.ratio);
    stops.truncate(15);
    stops
}

pub(super) fn set_element_fill_type(
    element: &mut Element,
    stroke_well: bool,
    mode: PaintFillType,
) -> bool {
    match mode {
        PaintFillType::None => match &mut element.kind {
            ElementKind::Rectangle { fill, stroke, .. }
            | ElementKind::Ellipse { fill, stroke, .. }
            | ElementKind::Path { fill, stroke, .. } => {
                let target = if stroke_well { stroke } else { fill };
                if *target == [0, 0, 0, 0] {
                    return false;
                }
                *target = [0, 0, 0, 0];
                true
            }
            ElementKind::PaintedPath {
                fill,
                stroke,
                fill_gradient,
                stroke_gradient,
                ..
            } => {
                let (target, slot) = if stroke_well {
                    (stroke, stroke_gradient)
                } else {
                    (fill, fill_gradient)
                };
                if *target == [0, 0, 0, 0] && slot.is_none() {
                    return false;
                }
                *target = [0, 0, 0, 0];
                *slot = None;
                true
            }
            ElementKind::Text { color, .. } if !stroke_well => {
                if *color == [0, 0, 0, 0] {
                    return false;
                }
                *color = [0, 0, 0, 0];
                true
            }
            _ => false,
        },
        PaintFillType::Solid => match &mut element.kind {
            ElementKind::Rectangle { fill, stroke, .. }
            | ElementKind::Ellipse { fill, stroke, .. }
            | ElementKind::Path { fill, stroke, .. } => {
                let target = if stroke_well { stroke } else { fill };
                if *target != [0, 0, 0, 0] {
                    return false;
                }
                *target = if stroke_well {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
                true
            }
            ElementKind::PaintedPath {
                fill,
                stroke,
                fill_gradient,
                stroke_gradient,
                ..
            } => {
                let (target, slot) = if stroke_well {
                    (stroke, stroke_gradient)
                } else {
                    (fill, fill_gradient)
                };
                if *target != [0, 0, 0, 0] && slot.is_none() {
                    return false;
                }
                if let Some(prev) = slot.take()
                    && let Some(first) = prev.stops.first()
                {
                    *target = ensure_solid_non_none(first.color);
                } else if *target == [0, 0, 0, 0] {
                    *target = if stroke_well {
                        [0, 0, 0, 255]
                    } else {
                        [255, 255, 255, 255]
                    };
                }
                true
            }
            ElementKind::Text { color, .. } if !stroke_well => {
                if *color != [0, 0, 0, 0] {
                    return false;
                }
                *color = [0, 0, 0, 255];
                true
            }
            _ => false,
        },
        PaintFillType::LinearGradient | PaintFillType::RadialGradient => {
            let kind = if mode == PaintFillType::LinearGradient {
                GradientKind::Linear
            } else {
                GradientKind::Radial
            };
            if let ElementKind::PaintedPath {
                commands,
                fill,
                stroke,
                fill_gradient,
                stroke_gradient,
                ..
            } = &mut element.kind
            {
                let (base, slot) = if stroke_well {
                    (stroke, stroke_gradient)
                } else {
                    (fill, fill_gradient)
                };
                if let Some(existing) = slot {
                    if existing.kind == kind {
                        return false;
                    }
                    existing.kind = kind;
                } else {
                    if base[3] == 0 {
                        *base = if stroke_well {
                            [0, 0, 0, 255]
                        } else {
                            [255, 255, 255, 255]
                        };
                    }
                    *slot = Some(default_gradient_paint(kind, *base, commands));
                }
                return true;
            }
            let Some((commands, fill_rule)) = element.to_path_commands() else {
                return false;
            };
            let (mut fill, mut stroke, stroke_width) = match &element.kind {
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
                } => (*fill, *stroke, *stroke_width),
                _ => return false,
            };
            if matches!(element.kind, ElementKind::Ellipse { .. })
                && !element.oval_options().close_path
            {
                fill = [0, 0, 0, 0];
            }
            let base = if stroke_well { &mut stroke } else { &mut fill };
            if base[3] == 0 {
                *base = if stroke_well {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
            }
            let paint = default_gradient_paint(kind, *base, &commands);
            element.transform.primitive = None;
            element.kind = ElementKind::PaintedPath {
                commands,
                fill_rule,
                fill,
                stroke,
                fill_gradient: (!stroke_well).then_some(paint.clone()),
                stroke_gradient: stroke_well.then_some(paint),
                stroke_width,
            };
            true
        }
        PaintFillType::BitmapFill => false,
    }
}

pub(super) fn paints(kind: &ElementKind) -> Option<(Color, Color, f64, bool)> {
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
        } => Some((*fill, *stroke, *stroke_width, true)),
        ElementKind::Text { color, .. } => Some((*color, [0; 4], 0., false)),
        ElementKind::TextBox { format, .. } => {
            let [_, r, g, b] = format.color.to_be_bytes();
            Some(([r, g, b, 255], [0; 4], 0., false))
        }
        _ => None,
    }
}
pub(super) fn edit_paints(
    kind: &mut ElementKind,
    old: (Color, Color, f64),
    new: (Color, Color, f64),
) -> bool {
    let apply_color = |value: &mut Color, before: Color, after: Color| {
        if before[..3] != after[..3] {
            value[..3].copy_from_slice(&after[..3]);
        }
        if before[3] != after[3] {
            value[3] = after[3];
        }
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
        }
        | ElementKind::PaintedPath {
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            apply_color(fill, old.0, new.0);
            apply_color(stroke, old.1, new.1);
            if old.2 != new.2 {
                *stroke_width = new.2;
            }
        }
        ElementKind::Text { color, .. } => apply_color(color, old.0, new.0),
        ElementKind::TextBox { format, .. } => {
            let [_, r, g, b] = format.color.to_be_bytes();
            let mut color = [r, g, b, 255];
            apply_color(&mut color, old.0, new.0);
            format.color = u32::from_be_bytes([0, color[0], color[1], color[2]]);
        }
        _ => return false,
    }
    if let ElementKind::PaintedPath {
        fill_gradient,
        stroke_gradient,
        ..
    } = kind
    {
        for (gradient, before, after) in [
            (fill_gradient, old.0, new.0),
            (stroke_gradient, old.1, new.1),
        ] {
            update_gradient_for_color_edit(gradient, before, after);
        }
    }
    true
}

/// Solid RGB fields replace a gradient; a standalone opacity edit keeps its
/// geometry and stop colors, applying the requested alpha to every stop.
pub(crate) fn update_gradient_for_color_edit(
    gradient: &mut Option<GradientPaint>,
    before: Color,
    after: Color,
) {
    if before[..3] != after[..3] {
        *gradient = None;
    } else if before[3] != after[3]
        && let Some(paint) = gradient
    {
        for stop in &mut paint.stops {
            stop.color[3] = after[3];
        }
    }
}
