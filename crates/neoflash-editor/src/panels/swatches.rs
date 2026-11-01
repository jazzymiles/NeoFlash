//! Swatches panel (NF2 `Panel / Swatches`, PP-10 / W0.9; reference `panels-more.png` &
//! `swatches-popover.png`): hosts `NF2/Swatches Popover`, scans document colours and gradients in
//! stable order, persists recent colours and custom swatches in `WorkspacePrefs`, and applies
//! picked solid colours, no-colour or gradient swatches to the active fill/stroke and editable
//! selection in one Undo step.
use crate::app::{Editor, Tool};
use crate::theme::ScrollTheme as _;
use crate::workspace::PanelId;
use eframe::egui::{self, Color32, Ui};
use neoflash_core::*;
use neoflash_ui::components as c;

fn to_color32(color: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3])
}

fn push_unique_color(out: &mut Vec<Color>, color: Color) {
    if color[3] == 0 {
        return;
    }
    let opaque = [color[0], color[1], color[2], color[3]];
    if !out.contains(&opaque) {
        out.push(opaque);
    }
}

fn gradient_swatch(paint: &GradientPaint) -> Option<c::GradientSwatch> {
    let first = paint.stops.first()?;
    let last = paint.stops.last()?;
    Some(c::GradientSwatch {
        from: to_color32(first.color),
        to: to_color32(last.color),
        radial: paint.kind == GradientKind::Radial,
    })
}

fn preset_gradients(palette: &[Color32]) -> Vec<(c::GradientSwatch, GradientPaint)> {
    let pick = |i: usize| palette[i % palette.len()];
    let to_rgba = |c: Color32| c.to_srgba_unmultiplied();
    let specs = [
        (0, 11, false),
        (11, 26, false),
        (67, 91, false),
        (48, 84, true),
        (53, 89, true),
        (50, 48, false),
    ];
    specs
        .into_iter()
        .map(|(a, b, radial)| {
            let from = pick(a);
            let to = pick(b);
            let swatch = c::GradientSwatch { from, to, radial };
            let paint = GradientPaint {
                kind: if radial {
                    GradientKind::Radial
                } else {
                    GradientKind::Linear
                },
                matrix: [100.0 / 1638.4, 0.0, 0.0, 100.0 / 1638.4, 50.0, 50.0],
                stops: vec![
                    GradientStop {
                        ratio: 0,
                        color: to_rgba(from),
                    },
                    GradientStop {
                        ratio: 255,
                        color: to_rgba(to),
                    },
                ],
                focal_point_ratio: 0.0,
            };
            (swatch, paint)
        })
        .collect()
}

fn element_paint(kind: &ElementKind, stroke_active: bool) -> Option<Color> {
    match kind {
        ElementKind::Rectangle { fill, stroke, .. }
        | ElementKind::Ellipse { fill, stroke, .. }
        | ElementKind::Path { fill, stroke, .. }
        | ElementKind::PaintedPath { fill, stroke, .. } => {
            Some(if stroke_active { *stroke } else { *fill })
        }
        ElementKind::Text { color, .. } => (!stroke_active).then_some(*color),
        ElementKind::TextBox { format, .. } => (!stroke_active).then(|| {
            let [_, r, g, b] = format.color.to_be_bytes();
            [r, g, b, 255]
        }),
        _ => None,
    }
}

fn set_element_solid_paint(kind: &mut ElementKind, stroke_active: bool, next: Color) -> bool {
    match kind {
        ElementKind::Rectangle { fill, stroke, .. }
        | ElementKind::Ellipse { fill, stroke, .. }
        | ElementKind::Path { fill, stroke, .. } => {
            let slot = if stroke_active { stroke } else { fill };
            let changed = *slot != next;
            *slot = next;
            changed
        }
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            ..
        } => {
            if stroke_active {
                let changed = *stroke != next || stroke_gradient.is_some();
                *stroke = next;
                *stroke_gradient = None;
                changed
            } else {
                let changed = *fill != next || fill_gradient.is_some();
                *fill = next;
                *fill_gradient = None;
                changed
            }
        }
        ElementKind::Text { color, .. } if !stroke_active => {
            let changed = *color != next;
            *color = next;
            changed
        }
        ElementKind::TextBox { format, .. } if !stroke_active => {
            let packed = u32::from_be_bytes([0, next[0], next[1], next[2]]);
            let changed = format.color != packed;
            format.color = packed;
            changed
        }
        _ => false,
    }
}

fn set_element_gradient_paint(
    element: &mut Element,
    stroke_active: bool,
    template: &GradientPaint,
) -> bool {
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
        }
        | ElementKind::PaintedPath {
            fill,
            stroke,
            stroke_width,
            ..
        } => (*fill, *stroke, *stroke_width),
        _ => return false,
    };
    if matches!(element.kind, ElementKind::Ellipse { .. }) && !element.oval_options().close_path {
        fill = [0, 0, 0, 0];
    }
    let mut fitted = template.clone();
    fitted.matrix = crate::authoring_modify::default_gradient_matrix(&commands);
    let first = fitted.stops.first().map_or([0, 0, 0, 255], |s| s.color);
    let (existing_fill, existing_stroke) = match &element.kind {
        ElementKind::PaintedPath {
            fill_gradient,
            stroke_gradient,
            ..
        } => (fill_gradient.clone(), stroke_gradient.clone()),
        _ => (None, None),
    };
    let (fill_gradient, stroke_gradient) = if stroke_active {
        stroke = first;
        (existing_fill, Some(fitted))
    } else {
        fill = first;
        (Some(fitted), existing_stroke)
    };
    let next = ElementKind::PaintedPath {
        commands,
        fill_rule,
        fill,
        stroke,
        fill_gradient,
        stroke_gradient,
        stroke_width,
    };
    let changed = element.kind != next || element.transform.primitive.is_some();
    element.transform.primitive = None;
    element.kind = next;
    changed
}

impl Editor {
    fn active_stroke_well(&self, ctx: &egui::Context) -> bool {
        ctx.data(|d| {
            d.get_temp::<bool>(egui::Id::new("color-panel-active-stroke"))
                .unwrap_or(false)
        })
    }

    /// Active fill/stroke colour shown in Swatches (from editable selection when `color_selection`
    /// is enabled, otherwise the tool fill/stroke).
    pub(crate) fn active_swatch_color(&self, ctx: &egui::Context) -> Color {
        let stroke_active = self.active_stroke_well(ctx);
        if self.authoring_modify.color_selection {
            for (target, element) in self.sampled_targets() {
                if self.target_editable(&target)
                    && let Some(c) = element_paint(&element.kind, stroke_active)
                {
                    return c;
                }
            }
        }
        if stroke_active {
            self.stroke
        } else {
            self.fill
        }
    }

    /// Add the active fill/stroke colour to `WorkspacePrefs::custom_swatches` (`Add swatch`).
    pub(crate) fn add_active_swatch(&mut self, ctx: &egui::Context) {
        let mut color = self.active_swatch_color(ctx);
        if color[3] == 0 {
            color[3] = 255;
        }
        if !self.workspace.custom_swatches.contains(&color) {
            self.workspace.custom_swatches.push(color);
            self.workspace.custom_swatches.truncate(64);
        }
    }

    fn record_recent_color(&mut self, ctx: &egui::Context, color: Color) {
        let opaque = [color[0], color[1], color[2], 255];
        self.workspace.recent_colors.retain(|c| *c != opaque);
        self.workspace.recent_colors.insert(0, opaque);
        self.workspace
            .recent_colors
            .truncate(c::swatches::PALETTE_COLUMNS);
        // Keep the Tools-rail swatches popover recent strip in sync during this session.
        let recent_c32: Vec<Color32> = self
            .workspace
            .recent_colors
            .iter()
            .copied()
            .map(to_color32)
            .collect();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("tools-recent-colours"), recent_c32));
    }

    /// Unique solid colours used across the document (plus custom swatches), in stable order.
    pub(crate) fn document_swatch_colors(&self) -> Vec<Color> {
        let mut colors = Vec::new();
        for &custom in &self.workspace.custom_swatches {
            push_unique_color(&mut colors, custom);
        }
        for symbol in &self.project.symbols {
            for layer in &symbol.timeline.layers {
                for key in &layer.keyframes {
                    for element in &key.elements {
                        match &element.kind {
                            ElementKind::Rectangle { fill, stroke, .. }
                            | ElementKind::Ellipse { fill, stroke, .. }
                            | ElementKind::Path { fill, stroke, .. } => {
                                push_unique_color(&mut colors, *fill);
                                push_unique_color(&mut colors, *stroke);
                            }
                            ElementKind::PaintedPath {
                                fill,
                                stroke,
                                fill_gradient,
                                stroke_gradient,
                                ..
                            } => {
                                if let Some(g) = fill_gradient {
                                    for stop in &g.stops {
                                        push_unique_color(&mut colors, stop.color);
                                    }
                                } else {
                                    push_unique_color(&mut colors, *fill);
                                }
                                if let Some(g) = stroke_gradient {
                                    for stop in &g.stops {
                                        push_unique_color(&mut colors, stop.color);
                                    }
                                } else {
                                    push_unique_color(&mut colors, *stroke);
                                }
                            }
                            ElementKind::Text { color, .. } => {
                                push_unique_color(&mut colors, *color);
                            }
                            ElementKind::TextBox { format, .. } => {
                                let [_, r, g, b] = format.color.to_be_bytes();
                                push_unique_color(&mut colors, [r, g, b, 255]);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        if colors.is_empty() {
            push_unique_color(&mut colors, self.fill);
            push_unique_color(&mut colors, self.stroke);
        }
        colors
    }

    /// Authored gradients in the document followed by NF2 preset gradients.
    pub(crate) fn document_gradient_swatches(
        &self,
        palette: &[Color32],
    ) -> Vec<(c::GradientSwatch, GradientPaint)> {
        let mut list: Vec<(c::GradientSwatch, GradientPaint)> = Vec::new();
        let mut push_grad = |paint: &GradientPaint| {
            if let Some(swatch) = gradient_swatch(paint)
                && !list.iter().any(|(s, _)| *s == swatch)
            {
                list.push((swatch, paint.clone()));
            }
        };
        for symbol in &self.project.symbols {
            for layer in &symbol.timeline.layers {
                for key in &layer.keyframes {
                    for element in &key.elements {
                        if let ElementKind::PaintedPath {
                            fill_gradient,
                            stroke_gradient,
                            ..
                        } = &element.kind
                        {
                            if let Some(g) = fill_gradient {
                                push_grad(g);
                            }
                            if let Some(g) = stroke_gradient {
                                push_grad(g);
                            }
                        }
                    }
                }
            }
        }
        for (swatch, paint) in preset_gradients(palette) {
            if !list.iter().any(|(s, _)| *s == swatch) {
                list.push((swatch, paint));
            }
        }
        list
    }

    pub(crate) fn apply_swatch_solid(&mut self, ctx: &egui::Context, next: Color) {
        let stroke_active = self.active_stroke_well(ctx);
        if stroke_active {
            self.stroke = next;
            self.authoring_modify.drawing_stroke_gradient = None;
        } else {
            self.fill = next;
            self.authoring_modify.drawing_fill_gradient = None;
        }
        if !self.authoring_modify.color_selection {
            return;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        if targets.is_empty() {
            return;
        }
        let label = if next[3] == 0 {
            if stroke_active {
                "No stroke"
            } else {
                "No fill"
            }
        } else if stroke_active {
            "Stroke colour"
        } else {
            "Fill colour"
        };
        self.transact_named(label, |s| {
            for (target, mut element) in targets {
                if set_element_solid_paint(&mut element.kind, stroke_active, next) {
                    s.ensure_key_for(&target.layer, false);
                    let frame = s.frame;
                    if let Some(layer) = s
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == target.layer)
                        && let Some(el) = layer
                            .key_at_mut(frame)
                            .and_then(|k| k.elements.iter_mut().find(|e| e.id == target.element))
                    {
                        el.kind = element.kind;
                    }
                }
            }
        });
    }

    pub(crate) fn apply_swatch_gradient(&mut self, ctx: &egui::Context, paint: &GradientPaint) {
        let stroke_active = self.active_stroke_well(ctx);
        if let Some(first) = paint.stops.first() {
            if stroke_active {
                self.stroke = first.color;
            } else {
                self.fill = first.color;
            }
        }
        if !self.authoring_modify.color_selection {
            if stroke_active {
                self.authoring_modify.drawing_stroke_gradient = Some(paint.clone());
            } else {
                self.authoring_modify.drawing_fill_gradient = Some(paint.clone());
            }
            return;
        }
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        if targets.is_empty() {
            return;
        }
        let label = if stroke_active {
            "Stroke gradient"
        } else {
            "Fill gradient"
        };
        let template = paint.clone();
        self.transact_named(label, |s| {
            let mut upgraded = false;
            for (target, mut element) in targets {
                if set_element_gradient_paint(&mut element, stroke_active, &template) {
                    s.ensure_key_for(&target.layer, false);
                    let frame = s.frame;
                    if let Some(layer) = s
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == target.layer)
                        && let Some(el) = layer
                            .key_at_mut(frame)
                            .and_then(|k| k.elements.iter_mut().find(|e| e.id == target.element))
                    {
                        *el = element;
                        upgraded = true;
                    }
                }
            }
            if upgraded {
                s.project.format_version = s.project.format_version.max(5);
            }
        });
    }

    pub(crate) fn swatches_panel_ui(&mut self, ui: &mut Ui) {
        self.panel_title(ui, PanelId::Swatches, "Swatches");
        let ctx = ui.ctx().clone();
        let palette = c::default_palette();
        let recent: Vec<Color32> = self
            .workspace
            .recent_colors
            .iter()
            .copied()
            .map(to_color32)
            .collect();
        let document: Vec<Color32> = self
            .document_swatch_colors()
            .into_iter()
            .map(to_color32)
            .collect();
        let grad_pairs = self.document_gradient_swatches(&palette);
        let gradients: Vec<c::GradientSwatch> = grad_pairs.iter().map(|(s, _)| *s).collect();
        let active = self.active_swatch_color(&ctx);
        let mut color = to_color32(active);
        let before = color;

        let out = egui::ScrollArea::vertical()
            .id_salt("swatches-panel-scroll")
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                c::SwatchesPopover {
                    color: &mut color,
                    recent: &recent,
                    document: &document,
                    palette: &palette,
                    gradients: &gradients,
                }
                .show(ui)
            })
            .inner;

        if out.no_colour {
            let mut none = active;
            none[3] = 0;
            self.apply_swatch_solid(&ctx, none);
        } else if out.picked || color != before {
            let mut rgba = color.to_srgba_unmultiplied();
            if rgba[3] == 0 {
                rgba[3] = 255;
            }
            if out.picked {
                self.record_recent_color(&ctx, rgba);
            }
            self.apply_swatch_solid(&ctx, rgba);
        }
        if let Some(swatch) = out.gradient
            && let Some((_, paint)) = grad_pairs.iter().find(|(s, _)| *s == swatch)
        {
            self.apply_swatch_gradient(&ctx, paint);
        }
        if out.add_swatch {
            self.add_active_swatch(&ctx);
        }
        if out.eyedropper {
            self.set_tool(Tool::Eyedropper);
        }
        if out.open_mixer {
            self.show_panel(PanelId::Color);
        }
    }
}
