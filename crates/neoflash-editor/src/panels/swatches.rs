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
                ..Default::default()
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

    /// Add the active fill/stroke colour to `WorkspacePrefs::custom_swatches` and document swatches (`Add swatch`).
    pub(crate) fn add_active_swatch(&mut self, ctx: &egui::Context) {
        let mut color = self.active_swatch_color(ctx);
        if color[3] == 0 {
            color[3] = 255;
        }
        if !self.workspace.custom_swatches.contains(&color) {
            self.workspace.custom_swatches.push(color);
            self.workspace.custom_swatches.truncate(64);
        }
        let mut doc_swatches = self.project.custom_swatches();
        if !doc_swatches.contains(&color) {
            doc_swatches.push(color);
            doc_swatches.truncate(64);
            self.transact_named("Add swatch", |editor| {
                editor.project.set_custom_swatches(&doc_swatches);
            });
        }
    }

    pub(crate) fn parse_palette_file_bytes(
        bytes: &[u8],
        file_name: &str,
    ) -> Result<Vec<neoflash_core::GlobalSwatch>, String> {
        let ext = std::path::Path::new(file_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        let prefix = std::path::Path::new(file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Swatch");
        let swatches = match ext.as_str() {
            "act" => neoflash_import::import_act_palette(bytes, prefix)
                .map_err(|e| format!("{file_name} [palette-act]: {e}"))?,
            "clr" => neoflash_import::import_clr_palette(bytes, prefix)
                .map_err(|e| format!("{file_name} [palette-clr]: {e}"))?,
            "ase" => neoflash_import::import_ase_palette(bytes)
                .map_err(|e| format!("{file_name} [palette-ase]: {e}"))?,
            _ => {
                return Err(format!(
                    "{file_name}: unsupported palette extension (expected .act, .clr, or .ase)"
                ));
            }
        };
        if swatches.is_empty() {
            return Err(format!("{file_name}: colour palette contains no swatches"));
        }
        Ok(swatches)
    }

    pub(crate) fn merge_swatches_into_project(
        project: &mut Project,
        swatches: &[neoflash_core::GlobalSwatch],
    ) -> usize {
        if swatches.is_empty() {
            return 0;
        }
        let mut globals = project.global_swatches();
        let mut custom = project.custom_swatches();
        for sw in swatches {
            if let Some(existing) = globals
                .iter_mut()
                .find(|g| g.id == sw.id && g.name == sw.name)
            {
                existing.color = sw.color;
            } else {
                let mut entry = sw.clone();
                let base_id = entry.id.clone();
                let mut suffix = 2;
                while globals.iter().any(|g| g.id == entry.id) {
                    entry.id = format!("{base_id}-{suffix}");
                    suffix += 1;
                }
                globals.push(entry);
            }
            push_unique_color(&mut custom, sw.color);
        }
        custom.truncate(256);
        project.set_global_swatches(&globals);
        project.set_custom_swatches(&custom);
        swatches.len()
    }

    pub(crate) fn import_palette_paths(
        &mut self,
        paths: &[std::path::PathBuf],
    ) -> Result<usize, String> {
        if paths.is_empty() {
            return Err("No palette files selected".into());
        }
        let mut all_swatches = Vec::new();
        for path in paths {
            let display = path.display().to_string();
            let bytes = std::fs::read(path).map_err(|e| format!("{display}: {e}"))?;
            let mut swatches = Self::parse_palette_file_bytes(&bytes, &display)?;
            all_swatches.append(&mut swatches);
        }
        let count = all_swatches.len();
        self.transact_named("Import colour palette", |editor| {
            Self::merge_swatches_into_project(&mut editor.project, &all_swatches);
        });
        self.message(format!(
            "Imported {count} swatch(es) from {} palette file(s)",
            paths.len()
        ));
        Ok(count)
    }

    pub(crate) fn import_palette_dialog(&mut self) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter("Colour palettes", &["act", "clr", "ase"])
            .pick_files()
        else {
            return;
        };
        if let Err(err) = self.import_palette_paths(&paths) {
            self.message(err);
        }
    }

    /// Delete a custom swatch from both document swatches and `WorkspacePrefs::custom_swatches`.
    #[allow(dead_code)]
    pub(crate) fn delete_custom_swatch(&mut self, color: Color) -> bool {
        let opaque = [
            color[0],
            color[1],
            color[2],
            if color[3] == 0 { 255 } else { color[3] },
        ];
        let before_ws = self.workspace.custom_swatches.len();
        self.workspace
            .custom_swatches
            .retain(|c| *c != opaque && *c != color);
        let removed_ws = self.workspace.custom_swatches.len() != before_ws;
        let mut doc_swatches = self.project.custom_swatches();
        let before_doc = doc_swatches.len();
        doc_swatches.retain(|c| *c != opaque && *c != color);
        let removed_doc = doc_swatches.len() != before_doc;
        if removed_doc {
            self.transact_named("Delete swatch", |editor| {
                editor.project.set_custom_swatches(&doc_swatches);
            });
        }
        removed_ws || removed_doc
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
        for custom in self.project.custom_swatches() {
            push_unique_color(&mut colors, custom);
        }
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
        let multi = targets.len() > 1;
        let label = if next == [0, 0, 0, 0] {
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
                let eligible = if stroke_active {
                    crate::authoring_modify::element_has_stroke(&element)
                        || (!multi && !is_merge_fill(&element))
                } else {
                    let has_bitmap = s.target_has_bitmap_fill(&target);
                    crate::authoring_modify::element_has_fill(&element, has_bitmap)
                        || (!multi
                            && !crate::authoring_modify::element_has_stroke(&element)
                            && !is_merge_stroke(&element))
                };
                if !eligible {
                    continue;
                }
                if set_element_solid_paint(&mut element.kind, stroke_active, next) {
                    s.ensure_key_for(&target.layer, false);
                    let frame = s.frame;
                    if let Some(layer) = s
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == target.layer)
                        && let Some(key) = layer.key_at_mut(frame)
                    {
                        if !stroke_active {
                            key.bitmap_fills.retain(|b| b.element_id != target.element);
                        }
                        if let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element) {
                            el.kind = element.kind;
                        }
                    }
                }
            }
        });
    }

    pub(crate) fn apply_swatch_gradient(&mut self, ctx: &egui::Context, paint: &GradientPaint) {
        let stroke_active = self.active_stroke_well(ctx);
        if let Some(first) = paint.stops.first() {
            if stroke_active {
                self.stroke = crate::authoring_modify::ensure_solid_non_none(first.color);
            } else {
                self.fill = crate::authoring_modify::ensure_solid_non_none(first.color);
            }
        }
        if stroke_active {
            self.authoring_modify.drawing_stroke_gradient = Some(paint.clone());
        } else {
            self.authoring_modify.drawing_fill_gradient = Some(paint.clone());
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
        let multi = targets.len() > 1;
        let label = if stroke_active {
            "Stroke gradient"
        } else {
            "Fill gradient"
        };
        let template = paint.clone();
        self.transact_named(label, |s| {
            let mut upgraded = false;
            for (target, mut element) in targets {
                let eligible = if stroke_active {
                    crate::authoring_modify::element_has_stroke(&element)
                        || (!multi && !is_merge_fill(&element))
                } else {
                    let has_bitmap = s.target_has_bitmap_fill(&target);
                    crate::authoring_modify::element_has_fill(&element, has_bitmap)
                        || (!multi
                            && !crate::authoring_modify::element_has_stroke(&element)
                            && !is_merge_stroke(&element))
                };
                if !eligible {
                    continue;
                }
                if set_element_gradient_paint(&mut element, stroke_active, &template) {
                    s.ensure_key_for(&target.layer, false);
                    let frame = s.frame;
                    if let Some(layer) = s
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == target.layer)
                        && let Some(key) = layer.key_at_mut(frame)
                    {
                        if !stroke_active {
                            key.bitmap_fills.retain(|b| b.element_id != target.element);
                        }
                        if let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element) {
                            *el = element;
                            upgraded = true;
                        }
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
                let pop_out = c::SwatchesPopover {
                    color: &mut color,
                    recent: &recent,
                    document: &document,
                    palette: &palette,
                    gradients: &gradients,
                }
                .show(ui);
                ui.add_space(neoflash_ui::tokens::space::S2);
                crate::panels::properties::inspector_section(ui, "Project colours", |ui| {
                    self.project_colors_section_ui(ui, false);
                });
                pop_out
            })
            .inner;

        if out.no_colour {
            self.apply_swatch_solid(&ctx, [0, 0, 0, 0]);
        } else if out.picked || color != before {
            let mut rgba = color.to_srgba_unmultiplied();
            let before_rgba = before.to_srgba_unmultiplied();
            if rgba[3] == 0 && (out.picked || rgba[..3] != before_rgba[..3]) {
                rgba[3] = 255;
            }
            let rgba = crate::authoring_modify::ensure_solid_non_none(rgba);
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
            self.workspace.color_picker_expanded = true;
            crate::authoring_modify::request_color_popover(
                &ctx,
                "swatches-open-mixer",
                ui.min_rect(),
                self.active_stroke_well(&ctx),
                !self.selection.targets.is_empty(),
            );
            self.show_panel(PanelId::Color);
        }
        self.show_active_color_popover(ui);
    }

    /// Collect `(symbol_id, layer_id, keyframe_frame, element_id)` placements for the current
    /// Stage selection, recursively expanding selected `SymbolInstance` elements so Figma-style
    /// `Selection colours` covers both direct shapes/texts and nested symbol artwork (`T-197`).
    pub(crate) fn selected_element_placements(&self) -> Vec<(Id, Id, u32, Id)> {
        let mut placements = Vec::new();
        let mut visited_syms = std::collections::HashSet::new();
        let mut pending_syms = Vec::new();
        let active_sym = self.symbol_id().to_owned();
        if let Some(sym) = self.project.symbol(&active_sym) {
            for target in &self.selection.targets {
                if let Some(layer) = sym.timeline.layers.iter().find(|l| l.id == target.layer)
                    && let Some(key) = layer.key_at(self.frame)
                    && let Some(el) = key.elements.iter().find(|e| e.id == target.element)
                {
                    let loc = (
                        active_sym.clone(),
                        layer.id.clone(),
                        key.frame,
                        el.id.clone(),
                    );
                    if !placements.contains(&loc) {
                        placements.push(loc);
                    }
                    if let ElementKind::Instance { symbol_id, .. } = &el.kind
                        && visited_syms.insert(symbol_id.clone())
                    {
                        pending_syms.push(symbol_id.clone());
                    }
                }
            }
        }
        while let Some(sym_id) = pending_syms.pop() {
            let Some(sym) = self.project.symbol(&sym_id) else {
                continue;
            };
            for layer in &sym.timeline.layers {
                for key in &layer.keyframes {
                    for el in &key.elements {
                        let loc = (sym_id.clone(), layer.id.clone(), key.frame, el.id.clone());
                        if !placements.contains(&loc) {
                            placements.push(loc);
                        }
                        if let ElementKind::Instance { symbol_id, .. } = &el.kind
                            && visited_syms.insert(symbol_id.clone())
                        {
                            pending_syms.push(symbol_id.clone());
                        }
                    }
                }
            }
        }
        placements
    }

    /// All unique visible colours used across the project (`T-197`).
    pub(crate) fn project_colors(&self) -> Vec<ProjectColorEntry> {
        collect_project_colors(&self.project)
    }

    /// All unique visible colours used in the current selection (`T-197`).
    pub(crate) fn selection_colors(&self) -> Vec<ProjectColorEntry> {
        let placements = self.selected_element_placements();
        if placements.is_empty() {
            return Vec::new();
        }
        let set: std::collections::HashSet<(Id, Id, u32, Id)> = placements.into_iter().collect();
        collect_project_colors_scoped(&self.project, None, Some(&set))
    }

    /// Replace `from` with `to` across the entire project (or only the current selection when
    /// `selection_only` is `true`) in a single Undo step (`T-197`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn replace_project_color(
        &mut self,
        from: Color,
        to: Color,
        selection_only: bool,
    ) -> usize {
        if from == to {
            return 0;
        }
        let mut opts = ChangeColorOptions::new(from, to);
        if selection_only {
            let placements = self.selected_element_placements();
            if placements.is_empty() {
                return 0;
            }
            opts.target_elements = Some(placements);
            opts.include_custom_swatches = false;
        }
        let mut replaced_total = 0usize;
        let label = if selection_only {
            "Change Selection Colour"
        } else {
            "Change Project Colour"
        };
        self.transact_named(label, |editor| {
            if let Ok(report) = change_project_color(&mut editor.project, &opts) {
                replaced_total = report.total_replaced();
            }
        });
        if !selection_only && replaced_total > 0 {
            for sw in &mut self.workspace.custom_swatches {
                if *sw == from {
                    *sw = to;
                }
            }
            if self.fill == from {
                self.fill = to;
            }
            if self.stroke == from {
                self.stroke = to;
            }
        }
        replaced_total
    }

    /// Execute the canonical `CHANGE_COLOR_JSFL_SCRIPT` (`change_color.jsfl`) in the sandboxed
    /// JSFL VM and commit the updated project in one Undo step (`T-197`).
    pub(crate) fn run_jsfl_change_color_command(
        &mut self,
        from: Color,
        to: Color,
    ) -> Result<usize, String> {
        let state = JsflAuthoringState::for_project(&self.project);
        let res =
            run_change_color_jsfl(&self.project, &state, from, to).map_err(|d| d.to_string())?;
        let replaced = res.return_value.as_number().unwrap_or(0.0).max(0.0) as usize;
        if replaced > 0 {
            let next_project = res.project;
            self.transact_named("JSFL Change Colour", |editor| {
                editor.project = next_project;
            });
            for sw in &mut self.workspace.custom_swatches {
                if *sw == from {
                    *sw = to;
                }
            }
        }
        Ok(replaced)
    }

    /// Select all editable elements on the active symbol and frame that use `color` (`T-197`).
    pub(crate) fn select_elements_with_color_on_frame(&mut self, color: Color) -> usize {
        let active_sym = self.symbol_id().to_owned();
        let frame = self.frame;
        let editable = self.editable_targets();
        let mut matched = Vec::new();
        for target in editable {
            let mut single_set = std::collections::HashSet::new();
            if let Some(sym) = self.project.symbol(&active_sym)
                && let Some(layer) = sym.timeline.layers.iter().find(|l| l.id == target.layer)
                && let Some(key) = layer.key_at(frame)
                && let Some(el) = key.elements.iter().find(|e| e.id == target.element)
            {
                single_set.insert((
                    active_sym.clone(),
                    layer.id.clone(),
                    key.frame,
                    el.id.clone(),
                ));
                if let ElementKind::Instance { symbol_id, .. } = &el.kind
                    && let Some(child_sym) = self.project.symbol(symbol_id)
                {
                    for cl in &child_sym.timeline.layers {
                        for ck in &cl.keyframes {
                            for ce in &ck.elements {
                                single_set.insert((
                                    symbol_id.clone(),
                                    cl.id.clone(),
                                    ck.frame,
                                    ce.id.clone(),
                                ));
                            }
                        }
                    }
                }
            }
            if !single_set.is_empty() {
                let colors = collect_project_colors_scoped(&self.project, None, Some(&single_set));
                if colors.iter().any(|entry| entry.color == color) {
                    matched.push(target);
                }
            }
        }
        let count = matched.len();
        if count > 0 {
            self.selection = crate::selection::Selection {
                symbol: active_sym,
                targets: matched,
            };
        }
        count
    }

    /// Figma-style interactive list of all colours in the project (or current selection) with live
    /// RGBA popover replacement, usage counts/roles, stage selection, and JSFL Change Colour (`T-197`).
    pub(crate) fn project_colors_section_ui(&mut self, ui: &mut Ui, selection_only: bool) {
        let ctx = ui.ctx().clone();
        let scope_tag = if selection_only {
            "selection-colors"
        } else {
            "project-colors"
        };
        let scope = egui::Id::new((
            scope_tag,
            &self.clipboard.session,
            self.symbol_id(),
            self.frame,
        ));
        self.property_edit_scope(scope);

        let entries = if selection_only {
            self.selection_colors()
        } else {
            self.project_colors()
        };

        if entries.is_empty() {
            let p = neoflash_ui::palette(&ctx);
            ui.add(
                egui::Label::new(
                    neoflash_ui::typography::rich(
                        if selection_only {
                            "No editable colours in the current selection."
                        } else {
                            "No colours in the document yet."
                        },
                        neoflash_ui::TextStyle::Small,
                    )
                    .color(p.text_secondary),
                )
                .wrap(),
            );
            return;
        }

        let live_state_id = egui::Id::new(("project-color-live-drag", scope));
        let placements_snapshot = if selection_only {
            Some(self.selected_element_placements())
        } else {
            None
        };

        let mut select_color_requested: Option<Color> = None;

        for (row_idx, entry) in entries.iter().enumerate() {
            let row_id = format!("{scope_tag}-row-{row_idx}");
            let label = format!("Colour {}", row_idx + 1);
            let mut edited = entry.color;

            // Keep the active drag's current target colour displayed even if the user drags across
            // another existing colour in the project.
            if let Some((active_row, _origin, current_target, _base_proj)) =
                ctx.data(|d| d.get_temp::<(usize, Color, Color, Project)>(live_state_id))
                && active_row == row_idx
            {
                edited = current_target;
            }

            let resp = crate::paint_controls::color_row(ui, &row_id, "#", &label, &mut edited);
            if resp.changed() && edited != entry.color {
                let existing_drag =
                    ctx.data(|d| d.get_temp::<(usize, Color, Color, Project)>(live_state_id));
                let (origin_color, base_proj) = match existing_drag {
                    Some((active_row, orig, _, base)) if active_row == row_idx => (orig, base),
                    _ => {
                        let base = self.project.clone();
                        self.begin_property_edit(scope, resp.id);
                        (entry.color, base)
                    }
                };
                let mut candidate = base_proj.clone();
                let mut opts = ChangeColorOptions::new(origin_color, edited);
                if selection_only {
                    opts.target_elements = placements_snapshot.clone();
                    opts.include_custom_swatches = false;
                }
                if change_project_color(&mut candidate, &opts).is_ok() {
                    self.project = candidate;
                    ctx.data_mut(|d| {
                        d.insert_temp(live_state_id, (row_idx, origin_color, edited, base_proj));
                    });
                    if !selection_only {
                        for sw in &mut self.workspace.custom_swatches {
                            if *sw == origin_color {
                                *sw = edited;
                            }
                        }
                    }
                }
            }

            let roles = entry.roles_summary();
            let summary = if roles.is_empty() {
                format!("{} ×", entry.total_uses())
            } else {
                format!("{} × · {}", entry.total_uses(), roles)
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = neoflash_ui::tokens::space::S2;
                ui.add(c::Badge::new(&summary, c::BadgeType::Neutral));
                if !selection_only
                    && ui
                        .add(c::IconButton::new(
                            neoflash_ui::icons::i16::SELECT_ALL,
                            "Select elements with this colour on the current frame",
                        ))
                        .clicked()
                {
                    select_color_requested = Some(entry.color);
                }
            });
        }

        if let Some(target_color) = select_color_requested {
            self.select_elements_with_color_on_frame(target_color);
        }

        // Batch JSFL Change Colour bar (minimal & scriptable `change_color.jsfl` runner)
        if !selection_only {
            let jsfl_pair_id = egui::Id::new(("jsfl-change-color-pair", &self.clipboard.session));
            let default_from = entries.first().map(|e| e.color).unwrap_or(self.fill);
            let default_to = self.fill;
            let (mut jsfl_from, mut jsfl_to) = ctx
                .data(|d| d.get_temp::<(Color, Color)>(jsfl_pair_id))
                .unwrap_or((default_from, default_to));
            if !entries.iter().any(|e| e.color == jsfl_from)
                && let Some(first) = entries.first()
            {
                jsfl_from = first.color;
            }
            ui.add_space(neoflash_ui::tokens::space::S1);
            let from_resp = crate::paint_controls::color_row(
                ui,
                "jsfl-change-color-from",
                "From",
                "From colour",
                &mut jsfl_from,
            );
            let to_resp = crate::paint_controls::color_row(
                ui,
                "jsfl-change-color-to",
                "To",
                "To colour",
                &mut jsfl_to,
            );
            if from_resp.changed() || to_resp.changed() {
                ctx.data_mut(|d| d.insert_temp(jsfl_pair_id, (jsfl_from, jsfl_to)));
            }
            if ui
                .add(c::Button::new("Replace colour across project (JSFL)"))
                .on_hover_text(
                    "Run built-in change_color.jsfl across all scenes, Library symbols, gradients, strokes and text",
                )
                .clicked()
                && let Ok(count) = self.run_jsfl_change_color_command(jsfl_from, jsfl_to)
                    && count > 0
                {
                    ctx.data_mut(|d| d.insert_temp(jsfl_pair_id, (jsfl_to, jsfl_to)));
                }
            if ui
                .add(c::Button::new("Import palette (.act, .clr, .ase)…"))
                .on_hover_text(
                    "Import colour swatches from an Adobe Color Table (.act), Flash Color Set (.clr), or Adobe Swatch Exchange (.ase) palette",
                )
                .clicked()
            {
                self.import_palette_dialog();
            }
        }

        self.end_property_interaction(&ctx);
        if self.property_edit.is_none() {
            ctx.data_mut(|d| d.remove::<(usize, Color, Color, Project)>(live_state_id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::{Selection, SelectionTarget};

    #[test]
    fn project_and_selection_colors_replace_and_jsfl_change_color_undo() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        editor
            .project
            .symbols
            .retain(|s| s.id == editor.project.root);
        editor.project.set_custom_swatches(&[]);
        let c_coral = [230, 57, 70, 255];
        let c_teal = [42, 157, 143, 255];
        let c_navy = [29, 53, 87, 255];
        let c_gold = [233, 196, 106, 255];

        let root_id = editor.project.root.clone();
        editor
            .project
            .symbol_mut(&root_id)
            .unwrap()
            .timeline
            .layers
            .truncate(1);
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0]
            .keyframes
            .truncate(1);
        let layer_id = editor.project.symbol(&root_id).unwrap().timeline.layers[0]
            .id
            .clone();

        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: "el-a".to_owned(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: 100.0,
                    height: 80.0,
                    fill: c_coral,
                    stroke: c_teal,
                    stroke_width: 2.0,
                },
                ..Default::default()
            },
            Element {
                id: "el-b".to_owned(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Ellipse {
                    width: 60.0,
                    height: 60.0,
                    fill: c_coral,
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Default::default()
            },
        ];

        // 1. Project colours inventory
        let all_colors = editor.project_colors();
        assert_eq!(all_colors.len(), 2);
        assert_eq!(all_colors[0].color, c_coral);
        assert_eq!(all_colors[0].fill_count, 2);
        assert_eq!(all_colors[1].color, c_teal);
        assert_eq!(all_colors[1].stroke_count, 1);

        // 2. Scoped selection colour replacement (only el-a selected)
        editor.selection = Selection {
            symbol: root_id.clone(),
            targets: vec![SelectionTarget {
                layer: layer_id.clone(),
                element: "el-a".to_owned(),
            }],
        };
        let sel_colors = editor.selection_colors();
        assert_eq!(sel_colors.len(), 2);

        let replaced_sel = editor.replace_project_color(c_coral, c_navy, true);
        assert_eq!(replaced_sel, 1);
        assert!(validate(&editor.project).is_empty());

        let els =
            &editor.project.symbol(&root_id).unwrap().timeline.layers[0].keyframes[0].elements;
        assert!(matches!(els[0].kind, ElementKind::Rectangle { fill, .. } if fill == c_navy));
        assert!(matches!(els[1].kind, ElementKind::Ellipse { fill, .. } if fill == c_coral));

        // Single Undo restores el-a back to c_coral
        editor.undo();
        let els =
            &editor.project.symbol(&root_id).unwrap().timeline.layers[0].keyframes[0].elements;
        assert!(matches!(els[0].kind, ElementKind::Rectangle { fill, .. } if fill == c_coral));

        // 3. Project-wide JSFL Change Colour replaces both el-a and el-b in one Undo step
        let jsfl_count = editor
            .run_jsfl_change_color_command(c_coral, c_gold)
            .unwrap();
        assert_eq!(jsfl_count, 2);
        assert!(validate(&editor.project).is_empty());

        // Select elements on frame by colour
        let selected_count = editor.select_elements_with_color_on_frame(c_gold);
        assert_eq!(selected_count, 2);

        // Single Undo restores project before JSFL Change Colour
        editor.undo();
        let els =
            &editor.project.symbol(&root_id).unwrap().timeline.layers[0].keyframes[0].elements;
        assert!(matches!(els[0].kind, ElementKind::Rectangle { fill, .. } if fill == c_coral));
        assert!(matches!(els[1].kind, ElementKind::Ellipse { fill, .. } if fill == c_coral));
    }
}
