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
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UsedColorEntry {
    pub entry: ProjectColorEntry,
    pub stage_uses: usize,
    pub library_uses: usize,
}

/// Owned by the in-progress property transaction. Freeze row identities during a drag so
/// merging into an existing colour, or making a colour transparent, cannot retarget the control.
pub(crate) struct UsedColorEdit {
    row: usize,
    current: Color,
    base: Project,
    entries: Arc<[UsedColorEntry]>,
    options: ChangeColorOptions,
}

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
                crate::panels::properties::inspector_section(ui, "Used colours", |ui| {
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
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn project_colors(&self) -> Vec<ProjectColorEntry> {
        collect_project_colors(&self.project)
    }

    /// Partition symbol IDs into `(stage_symbols, library_symbols)` relative to `self.symbol_id()` (`T-386`):
    /// - `stage_symbols`: `self.symbol_id()` plus every non-scene symbol transitively instantiated inside it (cycle-safe).
    /// - `library_symbols`: all non-scene Library symbols (`!project.is_scene(&sym.id)`) NOT in `stage_symbols`.
    pub(crate) fn stage_and_library_symbol_scopes(
        &self,
    ) -> (std::collections::HashSet<Id>, std::collections::HashSet<Id>) {
        let active_sym = self.symbol_id().to_owned();
        let mut stage_symbols = std::collections::HashSet::new();
        let mut pending = Vec::new();
        if self.project.symbol(&active_sym).is_some() {
            stage_symbols.insert(active_sym.clone());
            pending.push(active_sym);
        }
        while let Some(sym_id) = pending.pop() {
            let Some(sym) = self.project.symbol(&sym_id) else {
                continue;
            };
            for layer in &sym.timeline.layers {
                for key in &layer.keyframes {
                    for el in &key.elements {
                        if let ElementKind::Instance { symbol_id, .. } = &el.kind
                            && !self.project.is_scene(symbol_id)
                            && self.project.symbol(symbol_id).is_some()
                            && stage_symbols.insert(symbol_id.clone())
                        {
                            pending.push(symbol_id.clone());
                        }
                    }
                }
            }
        }
        let library_symbols: std::collections::HashSet<Id> = self
            .project
            .symbols
            .iter()
            .filter(|sym| !self.project.is_scene(&sym.id) && !stage_symbols.contains(&sym.id))
            .map(|sym| sym.id.clone())
            .collect();
        (stage_symbols, library_symbols)
    }

    fn used_color_options(
        &self,
        from: Color,
        to: Color,
        only_on_stage: bool,
    ) -> ChangeColorOptions {
        let (mut stage, library) = self.stage_and_library_symbol_scopes();
        if !only_on_stage {
            stage.extend(library);
        }
        let mut options = ChangeColorOptions::new(from, to);
        options.target_symbol_ids = Some(stage.into_iter().collect());
        options.include_custom_swatches = false;
        options
    }

    /// Context-aware `Used colours` entries with exact `(stage_uses, library_uses)` partition (`T-386`).
    pub(crate) fn used_colors(&self, only_on_stage: bool) -> Vec<UsedColorEntry> {
        let (stage_symbols, library_symbols) = self.stage_and_library_symbol_scopes();
        let slot_uses =
            |e: &ProjectColorEntry| e.fill_count + e.stroke_count + e.gradient_count + e.text_count;
        let stage_entries =
            collect_project_colors_scoped(&self.project, Some(&stage_symbols), None);
        if only_on_stage {
            return stage_entries
                .into_iter()
                .filter_map(|entry| {
                    let stage_uses = slot_uses(&entry);
                    (stage_uses > 0).then_some(UsedColorEntry {
                        entry,
                        stage_uses,
                        library_uses: 0,
                    })
                })
                .collect();
        }
        let library_entries =
            collect_project_colors_scoped(&self.project, Some(&library_symbols), None);
        let stage_map: std::collections::HashMap<Color, usize> = stage_entries
            .iter()
            .map(|e| (e.color, slot_uses(e)))
            .collect();
        let lib_map: std::collections::HashMap<Color, usize> = library_entries
            .iter()
            .map(|e| (e.color, slot_uses(e)))
            .collect();
        let union_symbols: std::collections::HashSet<Id> =
            stage_symbols.into_iter().chain(library_symbols).collect();
        collect_project_colors_scoped(&self.project, Some(&union_symbols), None)
            .into_iter()
            .filter_map(|entry| {
                let stage_uses = stage_map.get(&entry.color).copied().unwrap_or(0);
                let library_uses = lib_map.get(&entry.color).copied().unwrap_or(0);
                (stage_uses > 0 || library_uses > 0).then_some(UsedColorEntry {
                    entry,
                    stage_uses,
                    library_uses,
                })
            })
            .collect()
    }

    /// Replace `from` with `to` in the active `Used colours` scope (`only_on_stage == true` -> Stage placements;
    /// `only_on_stage == false` -> Stage ∪ Library placements) in one Undo step (`T-386`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn replace_used_color(
        &mut self,
        from: Color,
        to: Color,
        only_on_stage: bool,
    ) -> usize {
        if from == to {
            return 0;
        }
        let opts = self.used_color_options(from, to, only_on_stage);
        let mut replaced_total = 0usize;
        let label = if only_on_stage {
            "Change Stage Colour"
        } else {
            "Change Used Colour"
        };
        self.transact_named(label, |editor| {
            if let Ok(report) = change_project_color(&mut editor.project, &opts) {
                replaced_total = report.total_replaced();
            }
        });
        replaced_total
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
        self.finish_edits();
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
            self.finish_edits();
            self.clear_timeline_frame_selection();
            self.set_selection(matched);
        }
        count
    }

    /// Format a colour's usage roles as `"5 × text"` or `"2 × fill · 1 × stroke"` (`R-094`).
    fn color_usage_text(entry: &ProjectColorEntry) -> String {
        let mut parts = Vec::new();
        if entry.fill_count > 0 {
            parts.push(format!("{} × fill", entry.fill_count));
        }
        if entry.stroke_count > 0 {
            parts.push(format!("{} × stroke", entry.stroke_count));
        }
        if entry.gradient_count > 0 {
            parts.push(format!("{} × gradient", entry.gradient_count));
        }
        if entry.text_count > 0 {
            parts.push(format!("{} × text", entry.text_count));
        }
        if parts.is_empty() {
            if entry.in_custom_swatches {
                parts.push("1 × swatch".to_owned());
            } else {
                parts.push(format!("{} ×", entry.total_uses()));
            }
        }
        parts.join(" · ")
    }

    /// Format a colour's usage roles as `"5 × text"` or `"2 × fill · 1 × stroke"` (`R-094`),
    /// optionally prefixed with `"{stage_uses} on stage · {library_uses} in library"` when
    /// `Only on stage` is unchecked (`T-386`).
    pub(crate) fn used_color_usage_text(
        used: &UsedColorEntry,
        include_stage_library_counts: bool,
    ) -> String {
        let roles = Self::color_usage_text(&used.entry);
        if include_stage_library_counts {
            format!(
                "{} on stage · {} in library · {}",
                used.stage_uses, used.library_uses, roles
            )
        } else {
            roles
        }
    }

    fn active_used_color_edit(&self, scope: egui::Id) -> Option<&UsedColorEdit> {
        self.property_edit
            .as_ref()
            .filter(|edit| edit.has_scope(scope))?
            .used_color
            .as_ref()
    }

    fn preview_used_color_edit(
        &mut self,
        scope: egui::Id,
        control: egui::Id,
        rows: (usize, &Arc<[UsedColorEntry]>),
        color: Color,
        selection_only: bool,
        only_on_stage: bool,
    ) {
        let (row, entries) = rows;
        self.begin_property_edit(scope, control);
        if self.active_used_color_edit(scope).is_none() {
            let from = entries[row].entry.color;
            let options = if selection_only {
                let mut options = ChangeColorOptions::new(from, color);
                options.target_elements = Some(self.selected_element_placements());
                options.include_custom_swatches = false;
                options
            } else {
                self.used_color_options(from, color, only_on_stage)
            };
            self.property_edit.as_mut().unwrap().used_color = Some(UsedColorEdit {
                row,
                current: from,
                base: self.project.clone(),
                entries: Arc::clone(entries),
                options,
            });
        }
        let edit = self
            .property_edit
            .as_mut()
            .unwrap()
            .used_color
            .as_mut()
            .unwrap();
        let mut candidate = edit.base.clone();
        edit.options.to = color;
        if change_project_color(&mut candidate, &edit.options).is_ok() {
            self.project = candidate;
            edit.current = color;
        }
    }

    /// Figma-style interactive list of used colours on Stage / in Library (or current selection) with live
    /// RGBA popover replacement, usage counts/roles, stage selection, and JSFL Change Colour (`T-197`, `T-386`).
    pub(crate) fn project_colors_section_ui(&mut self, ui: &mut Ui, selection_only: bool) {
        let ctx = ui.ctx().clone();
        if !selection_only
            && ui
                .add(c::Checkbox::new(
                    &mut self.workspace.used_colors_only_on_stage,
                    "Only on stage",
                ))
                .changed()
        {
            self.persist_workspace_requested = true;
        }
        let only_on_stage = self.workspace.used_colors_only_on_stage;
        let scope_tag = if selection_only {
            "selection-colors"
        } else {
            "project-colors"
        };
        let scope = egui::Id::new((
            scope_tag,
            ctx.viewport_id(),
            ui.id(),
            &self.clipboard.session,
            self.symbol_id(),
            self.frame,
            !selection_only && only_on_stage,
            selection_only.then_some(&self.selection.targets),
        ));
        self.property_edit_scope(scope);

        let used_entries = if let Some(edit) = self.active_used_color_edit(scope) {
            Arc::clone(&edit.entries)
        } else if selection_only {
            self.selection_colors()
                .into_iter()
                .map(|entry| {
                    let stage_uses = entry.fill_count
                        + entry.stroke_count
                        + entry.gradient_count
                        + entry.text_count;
                    UsedColorEntry {
                        entry,
                        stage_uses,
                        library_uses: 0,
                    }
                })
                .collect::<Arc<[_]>>()
        } else {
            self.used_colors(only_on_stage).into()
        };

        if used_entries.is_empty() {
            let p = neoflash_ui::palette(&ctx);
            ui.add(
                egui::Label::new(
                    neoflash_ui::typography::rich(
                        if selection_only {
                            "No editable colours in the current selection."
                        } else if only_on_stage {
                            "No colours on the stage yet."
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

        let p = neoflash_ui::palette(&ctx);
        let mut select_color_requested: Option<Color> = None;

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = neoflash_ui::tokens::space::S3;
            for (row_idx, used) in used_entries.iter().enumerate() {
                let entry = &used.entry;
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = neoflash_ui::tokens::space::S1;
                    let row_id = format!("{scope_tag}-row-{row_idx}");
                    let label = format!("Colour {}", row_idx + 1);
                    let current = self
                        .active_used_color_edit(scope)
                        .filter(|edit| edit.row == row_idx)
                        .map_or(entry.color, |edit| edit.current);
                    let mut edited = current;

                    let resp = ui
                        .push_id(scope, |ui| {
                            crate::paint_controls::color_row(ui, &row_id, "#", &label, &mut edited)
                        })
                        .inner;
                    if !resp.changed() {
                        super::dialog_helpers::track_rgba_popup(ui.ctx(), resp.id);
                    }
                    if resp.changed() && edited != current {
                        self.preview_used_color_edit(
                            scope,
                            resp.id,
                            (row_idx, &used_entries),
                            edited,
                            selection_only,
                            only_on_stage,
                        );
                    }

                    let summary =
                        Self::used_color_usage_text(used, !selection_only && !only_on_stage);
                    if selection_only {
                        ui.add(
                            egui::Label::new(
                                neoflash_ui::typography::rich(
                                    &summary,
                                    neoflash_ui::TextStyle::Small,
                                )
                                .color(p.text_tertiary),
                            )
                            .wrap(),
                        );
                    } else {
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 20.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = neoflash_ui::tokens::space::S2;
                                ui.add(
                                    egui::Label::new(
                                        neoflash_ui::typography::rich(
                                            &summary,
                                            neoflash_ui::TextStyle::Small,
                                        )
                                        .color(p.text_tertiary),
                                    )
                                    .wrap(),
                                );
                                if ui
                                    .add(
                                        c::IconButton::new(
                                            neoflash_ui::icons::i16::SELECT_ALL,
                                            "Select elements with this colour on the current frame",
                                        )
                                        .side(20.0),
                                    )
                                    .clicked()
                                {
                                    select_color_requested = Some(entry.color);
                                }
                            },
                        );
                    }
                });
            }
        });

        if let Some(target_color) = select_color_requested {
            self.select_elements_with_color_on_frame(target_color);
        }

        // Batch JSFL Change Colour bar (minimal & scriptable `change_color.jsfl` runner)
        if !selection_only {
            let jsfl_pair_id = egui::Id::new(("jsfl-change-color-pair", &self.clipboard.session));
            let default_from = used_entries
                .first()
                .map(|u| u.entry.color)
                .unwrap_or(self.fill);
            let default_to = self.fill;
            let (mut jsfl_from, mut jsfl_to) = ctx
                .data(|d| d.get_temp::<(Color, Color)>(jsfl_pair_id))
                .unwrap_or((default_from, default_to));
            if !used_entries.iter().any(|u| u.entry.color == jsfl_from)
                && let Some(first) = used_entries.first()
            {
                jsfl_from = first.entry.color;
            }
            ui.add_space(neoflash_ui::tokens::space::S1);
            let from_resp = crate::paint_controls::color_row(
                ui,
                "jsfl-change-color-from",
                "From",
                "From colour",
                &mut jsfl_from,
            );
            if !from_resp.changed() {
                super::dialog_helpers::track_rgba_popup(ui.ctx(), from_resp.id);
            }
            let to_resp = crate::paint_controls::color_row(
                ui,
                "jsfl-change-color-to",
                "To",
                "To colour",
                &mut jsfl_to,
            );
            if !to_resp.changed() {
                super::dialog_helpers::track_rgba_popup(ui.ctx(), to_resp.id);
            }
            if from_resp.changed() || to_resp.changed() {
                ctx.data_mut(|d| d.insert_temp(jsfl_pair_id, (jsfl_from, jsfl_to)));
            }
            if c::tooltip(
                ui.add(c::Button::new("Replace colour across project (JSFL)")),
                "Run built-in change_color.jsfl across all scenes, Library symbols, gradients, strokes and text",
                None,
            )
            .clicked()
                && let Ok(count) = self.run_jsfl_change_color_command(jsfl_from, jsfl_to)
                    && count > 0
                {
                    ctx.data_mut(|d| d.insert_temp(jsfl_pair_id, (jsfl_to, jsfl_to)));
                }
            if c::tooltip(
                ui.add(c::Button::new("Import palette (.act, .clr, .ase)…")),
                "Import colour swatches from an Adobe Color Table (.act), Flash Color Set (.clr), or Adobe Swatch Exchange (.ase) palette",
                None,
            )
            .clicked()
            {
                self.import_palette_dialog();
            }
        }

        self.end_property_interaction(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::{Selection, SelectionTarget};

    #[test]
    fn used_color_drag_keeps_its_slots_and_cannot_restore_an_old_document() {
        let ctx = egui::Context::default();
        let original = [230, 57, 70, 255];
        let other = [42, 157, 143, 255];
        let transparent = [230, 57, 70, 0];
        let final_color = [233, 196, 106, 255];
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.symbol_mut().timeline.layers[0].keyframes[0].elements = [original, other]
            .into_iter()
            .enumerate()
            .map(|(i, fill)| Element {
                id: format!("colour-{i}"),
                kind: ElementKind::Rectangle {
                    width: 80.0,
                    height: 60.0,
                    fill,
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                ..Default::default()
            })
            .collect();

        let scope = egui::Id::new("swatches");
        let control = scope.with("colour-row");
        let fills = |ed: &Editor| {
            ed.symbol().timeline.layers[0].keyframes[0]
                .elements
                .iter()
                .map(|el| match el.kind {
                    ElementKind::Rectangle { fill, .. } => fill,
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
        };
        let rows: Arc<[_]> = editor.used_colors(true).into();
        let row = rows
            .iter()
            .position(|entry| entry.entry.color == original)
            .unwrap();
        editor.preview_used_color_edit(scope, control, (row, &rows), other, false, true);
        assert_eq!(fills(&editor), [other, other]);
        assert_eq!(editor.used_colors(true).len(), 1);
        editor.preview_used_color_edit(scope, control, (row, &rows), transparent, false, true);
        assert_eq!(fills(&editor), [transparent, other]);
        editor.preview_used_color_edit(scope, control, (row, &rows), original, false, true);
        assert_eq!(fills(&editor), [original, other]);
        editor.finish_property_edit();
        assert!(editor.history.undo_label().is_none());

        editor.preview_used_color_edit(scope, control, (row, &rows), other, false, true);
        editor.transact_named("Rename", |ed| ed.project.name = "New title".into());
        assert!(editor.active_used_color_edit(scope).is_none());
        let before = editor.project.clone();
        let new_rows = editor.used_colors(true).into();
        editor.preview_used_color_edit(scope, control, (0, &new_rows), final_color, false, true);
        editor.finish_property_edit();
        assert_eq!(editor.project.name, "New title");
        assert_eq!(fills(&editor), [final_color, final_color]);
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before);
        editor.undo();
        assert_eq!(fills(&editor), [other, other]);
        editor.undo();
        assert_eq!(fills(&editor), [original, other]);
    }

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

    #[test]
    fn r094_selection_and_project_colors_render_small_tertiary_usage_text_at_60pt_pitch() {
        let coral_only = ProjectColorEntry {
            color: [230, 57, 70, 255],
            hex: "#E63946".to_owned(),
            fill_count: 2,
            stroke_count: 1,
            gradient_count: 0,
            text_count: 0,
            in_custom_swatches: false,
        };
        assert_eq!(
            Editor::color_usage_text(&coral_only),
            "2 × fill · 1 × stroke"
        );
        let text_only = ProjectColorEntry {
            color: [42, 157, 143, 255],
            hex: "#2A9D8F".to_owned(),
            fill_count: 0,
            stroke_count: 0,
            gradient_count: 0,
            text_count: 5,
            in_custom_swatches: false,
        };
        assert_eq!(Editor::color_usage_text(&text_only), "5 × text");

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        editor
            .project
            .symbols
            .retain(|s| s.id == editor.project.root);
        editor.project.set_custom_swatches(&[]);
        let c_coral = [230, 57, 70, 255];
        let c_teal = [42, 157, 143, 255];
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
        editor.selection = Selection {
            symbol: root_id,
            targets: vec![
                SelectionTarget {
                    layer: layer_id.clone(),
                    element: "el-a".to_owned(),
                },
                SelectionTarget {
                    layer: layer_id,
                    element: "el-b".to_owned(),
                },
            ],
        };

        let run_section = |ed: &mut Editor, selection_only: bool| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(300.0, 500.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::SidePanel::right("props")
                        .exact_width(268.0)
                        .show(ctx, |ui| {
                            ed.project_colors_section_ui(ui, selection_only);
                        });
                },
            )
        };

        let p = neoflash_ui::palette(&ctx);
        for selection_only in [true, false] {
            let out = run_section(&mut editor, selection_only);
            let mut usage_coral_y = None;
            let mut usage_teal_y = None;
            let mut row1_y = None;
            let mut row2_y = None;
            for s in &out.shapes {
                if let egui::Shape::Text(t) = &s.shape {
                    let txt = t.galley.text();
                    if txt == "2 × fill" {
                        usage_coral_y = Some(t.pos.y);
                        let format_color =
                            t.galley.job.sections.first().map(|sec| sec.format.color);
                        assert_eq!(
                            format_color,
                            Some(p.text_tertiary),
                            "usage text must use text/tertiary (selection_only={selection_only})"
                        );
                    } else if txt == "1 × stroke" {
                        usage_teal_y = Some(t.pos.y);
                    } else if txt.eq_ignore_ascii_case("#E63946") {
                        row1_y.get_or_insert(t.pos.y);
                    } else if txt.eq_ignore_ascii_case("#2A9D8F") {
                        row2_y.get_or_insert(t.pos.y);
                    }
                }
            }
            assert!(
                usage_coral_y.is_some() && usage_teal_y.is_some(),
                "expected '2 × fill' and '1 × stroke' usage labels (selection_only={selection_only})"
            );
            let y1 = row1_y.expect("first color row rendered");
            let y2 = row2_y.expect("second color row rendered");
            let pitch = y2 - y1;
            if selection_only {
                assert!(
                    (46.0..=64.0).contains(&pitch),
                    "R-094 Selection colours pitch should be ~60 pt (got {pitch})"
                );
            } else {
                assert!(
                    (50.0..=70.0).contains(&pitch),
                    "R-094 Project colours compact pitch should stay well under 88 pt (got {pitch})"
                );
            }
            // No Badge pill background should sit behind the usage text.
            let uy = usage_coral_y.unwrap();
            let has_badge_pill = out.shapes.iter().any(|s| {
                matches!(
                    &s.shape,
                    egui::Shape::Rect(r)
                        if r.fill == p.bg_hover
                            && r.rect.height() >= 18.0
                            && r.rect.height() <= 24.0
                            && r.rect.top() <= uy + 2.0
                            && r.rect.bottom() >= uy + 10.0
                )
            });
            assert!(
                !has_badge_pill,
                "usage text must not be wrapped in a Badge pill (selection_only={selection_only})"
            );
        }
    }

    #[test]
    fn t386_used_colors_only_on_stage_context_scoping_and_stage_library_counts() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        assert!(
            editor.workspace.used_colors_only_on_stage,
            "Only on stage must default to true"
        );

        let c_root_only = [220, 40, 50, 255];
        let c_hero_only = [40, 140, 220, 255];
        let c_nested = [120, 80, 210, 255];
        let c_shared = [50, 190, 110, 255];
        let c_lib_only = [245, 170, 30, 255];
        let c_swatch_only = [190, 30, 180, 255];
        let c_replaced = [20, 20, 20, 255];

        editor.project.set_custom_swatches(&[c_swatch_only]);

        let rect_el = |id: &str, fill: Color| Element {
            id: id.to_owned(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 40.0,
                height: 40.0,
                fill,
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Default::default()
        };

        // Nested symbol placed inside Hero.
        let badge_sym = Symbol {
            id: "sym-badge".into(),
            name: "Badge".into(),
            kind: SymbolKind::Graphic,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "badge-layer".into(),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![rect_el("badge-rect", c_nested)],
                        ..Default::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            linkage_id: None,
            class_name: None,
            folder: String::new(),
        };

        // Hero MovieClip placed on root Stage (uses c_hero_only + c_shared + nested Badge).
        let hero_sym = Symbol {
            id: "sym-hero".into(),
            name: "Hero".into(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "hero-layer".into(),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![
                            rect_el("hero-rect", c_hero_only),
                            rect_el("hero-shared", c_shared),
                            Element {
                                id: "hero-badge-inst".into(),
                                name: String::new(),
                                transform: Transform::default(),
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-badge".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            linkage_id: None,
            class_name: None,
            folder: String::new(),
        };

        // Unplaced Enemy MovieClip in Library (uses c_lib_only once + c_shared twice).
        let enemy_sym = Symbol {
            id: "sym-enemy".into(),
            name: "Enemy".into(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "enemy-layer".into(),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![
                            rect_el("enemy-only", c_lib_only),
                            rect_el("enemy-shared-1", c_shared),
                            rect_el("enemy-shared-2", c_shared),
                        ],
                        ..Default::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            linkage_id: None,
            class_name: None,
            folder: String::new(),
        };

        let root_id = editor.project.root.clone();
        editor.project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].elements = vec![
            rect_el("root-rect", c_root_only),
            Element {
                id: "root-hero-inst".into(),
                name: "hero".into(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: "sym-hero".into(),
                    playback: InstancePlayback::Independent,
                },
                ..Default::default()
            },
        ];
        editor.project.symbols.push(badge_sym);
        editor.project.symbols.push(hero_sym);
        editor.project.symbols.push(enemy_sym);
        editor.project.validate().unwrap();

        // 1. At root with Only on stage = true: lists root + Hero + Badge colours; excludes Enemy & swatch-only.
        let root_stage = editor.used_colors(true);
        let root_stage_colors: Vec<Color> = root_stage.iter().map(|u| u.entry.color).collect();
        assert_eq!(
            root_stage_colors,
            vec![c_root_only, c_nested, c_hero_only, c_shared]
        );

        // 2. At root with Only on stage = false: adds Enemy's c_lib_only and partitions stage_uses vs library_uses.
        let root_all = editor.used_colors(false);
        let shared_entry = root_all
            .iter()
            .find(|u| u.entry.color == c_shared)
            .expect("shared colour");
        assert_eq!(shared_entry.stage_uses, 1);
        assert_eq!(shared_entry.library_uses, 2);
        assert_eq!(
            Editor::used_color_usage_text(shared_entry, true),
            "1 on stage · 2 in library · 3 × fill"
        );
        let lib_only_entry = root_all
            .iter()
            .find(|u| u.entry.color == c_lib_only)
            .expect("library-only colour");
        assert_eq!(lib_only_entry.stage_uses, 0);
        assert_eq!(lib_only_entry.library_uses, 1);
        assert_eq!(
            Editor::used_color_usage_text(lib_only_entry, true),
            "0 on stage · 1 in library · 1 × fill"
        );
        assert!(
            !root_all.iter().any(|u| u.entry.color == c_swatch_only),
            "unused custom swatch is excluded from Used colours"
        );

        // 3. Scoped replacement at root: Only on stage = true replaces Hero's c_shared, leaving Enemy untouched.
        let before_replace = editor.project.clone();
        assert_eq!(editor.replace_used_color(c_shared, c_replaced, true), 1);
        editor.project.validate().unwrap();
        assert!(matches!(
            editor.project.symbol("sym-hero").unwrap().timeline.layers[0].keyframes[0].elements[1].kind,
            ElementKind::Rectangle { fill, .. } if fill == c_replaced
        ));
        assert!(matches!(
            editor.project.symbol("sym-enemy").unwrap().timeline.layers[0].keyframes[0].elements[1].kind,
            ElementKind::Rectangle { fill, .. } if fill == c_shared
        ));
        editor.undo();
        assert_eq!(editor.project, before_replace);

        // Only on stage = false replaces both Hero (1) and Enemy (2) in one Undo step.
        assert_eq!(editor.replace_used_color(c_shared, c_replaced, false), 3);
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, before_replace);

        // 4. Enter Hero MovieClip: Stage context excludes parent root's c_root_only!
        editor.select_single_id("root-hero-inst".into());
        editor.enter_selected_symbol_in_place();
        assert_eq!(editor.symbol_id(), "sym-hero");

        let hero_stage = editor.used_colors(true);
        let hero_stage_colors: Vec<Color> = hero_stage.iter().map(|u| u.entry.color).collect();
        assert_eq!(hero_stage_colors, vec![c_nested, c_hero_only, c_shared]);
        assert!(!hero_stage_colors.contains(&c_root_only));
        assert!(!hero_stage_colors.contains(&c_lib_only));

        let hero_plus_lib = editor.used_colors(false);
        let hero_plus_lib_colors: Vec<Color> =
            hero_plus_lib.iter().map(|u| u.entry.color).collect();
        assert!(hero_plus_lib_colors.contains(&c_lib_only));
        assert!(
            !hero_plus_lib_colors.contains(&c_root_only),
            "parent Scene colours must never leak into an edited MovieClip"
        );
        editor.leave_symbol_to_level(0);

        // 5. WorkspacePrefs persistence + Swatches UI header ("Used colours") + checkbox toggle.
        editor.workspace.used_colors_only_on_stage = false;
        let saved_json = serde_json::to_string(&editor.workspace).unwrap();
        let loaded = crate::workspace::WorkspacePrefs::from_json(&saved_json);
        assert!(!loaded.used_colors_only_on_stage);
        let mut templated = loaded.clone();
        templated.apply_template(crate::workspace::WorkspaceTemplate::Animator);
        assert!(
            !templated.used_colors_only_on_stage,
            "apply_template must preserve used_colors_only_on_stage"
        );
        let legacy = crate::workspace::WorkspacePrefs::from_json(r#"{"version":7,"theme":"dark"}"#);
        assert!(
            legacy.used_colors_only_on_stage,
            "missing key in saved prefs defaults to true"
        );

        editor.workspace.used_colors_only_on_stage = true;
        editor.persist_workspace_requested = false;
        let run_swatches = |ed: &mut Editor, events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(340.0, 1400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ed.swatches_panel_ui(ui);
                    });
                },
            )
        };
        let _ = run_swatches(&mut editor, vec![]);
        let out = run_swatches(&mut editor, vec![]);
        let ak = out.platform_output.accesskit_update.as_ref().unwrap();
        assert!(
            ak.nodes
                .iter()
                .any(|(_, n)| n.label() == Some("Used colours")),
            "Swatches renders 'Used colours' section header"
        );
        assert!(
            !ak.nodes
                .iter()
                .any(|(_, n)| n.label() == Some("Project colours")),
            "Swatches must not render legacy 'Project colours' header"
        );
        let chk_bounds = ak
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("Only on stage"))
            .expect("Only on stage checkbox")
            .1
            .bounds()
            .unwrap();
        let center = egui::pos2(
            ((chk_bounds.x0 + chk_bounds.x1) * 0.5) as f32,
            ((chk_bounds.y0 + chk_bounds.y1) * 0.5) as f32,
        );
        let _ = run_swatches(
            &mut editor,
            vec![
                egui::Event::PointerMoved(center),
                crate::tests::primary_button(center, true),
            ],
        );
        let out_unchecked = run_swatches(
            &mut editor,
            vec![crate::tests::primary_button(center, false)],
        );
        assert!(!editor.workspace.used_colors_only_on_stage);
        assert!(editor.persist_workspace_requested);
        let rendered_texts: Vec<String> = out_unchecked
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        assert!(
            rendered_texts
                .iter()
                .any(|t| t.contains("1 on stage · 2 in library")),
            "unchecking Only on stage displays stage/library breakdown, got {rendered_texts:?}"
        );
    }
}
