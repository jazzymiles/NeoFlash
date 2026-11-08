//! Contextual authoring inspector (NF2 `Properties`): the selected object,
//! Timeline range, Stage or active authoring tool chooses the inspector body.
//! Shared field controls live in paint_controls.
use crate::theme::DesignUi as _;
use crate::{
    app::{Editor, Tool},
    theme::UiPalette,
};
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::{
    TextStyle,
    components::{self as nf, Badge, BadgeType, MenuItem, SectionHeader, TypeHeader},
    icons::i16,
    tokens::space,
    typography::rich,
};

#[derive(Clone, Debug, PartialEq)]
struct PropertiesToolContextStamp {
    selection: crate::selection::Selection,
    frame_range: Option<FrameRange>,
    symbol: Id,
    layer: usize,
    frame: u32,
    tool: Tool,
}

const PROPERTIES_TOOL_CONTEXT_ID: &str = "properties-context-tool-request";

impl Editor {
    fn properties_tool_context_id(&self) -> egui::Id {
        egui::Id::new((PROPERTIES_TOOL_CONTEXT_ID, self.clipboard.session.clone()))
    }

    fn properties_tool_context_stamp(&self) -> PropertiesToolContextStamp {
        PropertiesToolContextStamp {
            selection: self.selection.clone(),
            frame_range: self.selected_frame_range().and_then(|_| self.frame_range()),
            symbol: self.symbol_id().to_owned(),
            layer: self.layer,
            frame: self.frame,
            tool: self.tool,
        }
    }

    /// Explicitly show active-tool settings (for example Brushes ▸ Brush
    /// options) without reviving a permanent Selection/Tool mode. The request
    /// remains valid only while the authored selection and Timeline target are
    /// unchanged; selecting another object/frame or Stage context restores the
    /// automatic inspector source.
    pub(crate) fn request_properties_tool_context(&self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.insert_temp(
                self.properties_tool_context_id(),
                self.properties_tool_context_stamp(),
            );
        });
    }

    pub(crate) fn clear_properties_tool_context(&self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.remove::<PropertiesToolContextStamp>(self.properties_tool_context_id());
        });
    }

    fn properties_tool_context_requested(&self, ctx: &egui::Context) -> bool {
        let id = self.properties_tool_context_id();
        let requested = ctx.data(|data| data.get_temp::<PropertiesToolContextStamp>(id));
        let valid = requested
            .as_ref()
            .is_some_and(|stamp| stamp == &self.properties_tool_context_stamp());
        if requested.is_some() && !valid {
            self.clear_properties_tool_context(ctx);
        }
        valid
    }

    pub(crate) fn properties(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.properties_contents(ui, ctx);
        self.show_active_color_popover(ui);
    }

    fn properties_contents(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::paint_controls::{
            PropertyChanges, color_row, numeric_pair, numeric_pair_with_suffix, numeric_row,
            paint_row, percent_row, property_row, set_paint_row_gradient, text_row,
        };
        // Figma inspector body: V gap 12 between all rows.
        ui.spacing_mut().item_spacing.y = space::S3;
        // Context is automatic: authored selection and Timeline ranges always
        // win, an empty Select-tool click means the Stage, and any other active
        // tool exposes its options/defaults. This preserves the full former
        // Tool branch without making the user keep a manual mode in sync.
        let frame_context = self.selected_frame_range().is_some();
        let tool_context = self.properties_tool_context_requested(ctx)
            || (self.selection.is_empty() && !frame_context && !matches!(self.tool, Tool::Select));
        if tool_context {
            self.finish_property_edit();
            match self.tool {
                Tool::Brush => {
                    self.brush_tool_contents(ui);
                    return;
                }
                Tool::Pencil => {
                    self.pencil_tool_contents(ui);
                    return;
                }
                _ => {}
            }
            inspector_section(ui, "Tool options", |ui| self.authoring_tool_options(ui));
            inspector_section(ui, "Drawing defaults", |ui| {
                let (old_fill, old_stroke) = (self.fill, self.stroke);
                set_paint_row_gradient(
                    ctx,
                    "inspector-tool-fill",
                    self.authoring_modify.drawing_fill_gradient.clone(),
                );
                set_paint_row_gradient(
                    ctx,
                    "inspector-tool-stroke",
                    self.authoring_modify.drawing_stroke_gradient.clone(),
                );
                if paint_row(ui, "inspector-tool-fill", "Fill", &mut self.fill).changed() {
                    crate::authoring_modify::update_gradient_for_color_edit(
                        &mut self.authoring_modify.drawing_fill_gradient,
                        old_fill,
                        self.fill,
                    );
                }
                if paint_row(ui, "inspector-tool-stroke", "Stroke", &mut self.stroke).changed() {
                    crate::authoring_modify::update_gradient_for_color_edit(
                        &mut self.authoring_modify.drawing_stroke_gradient,
                        old_stroke,
                        self.stroke,
                    );
                }
                numeric_row(ui, "Stroke width", &mut self.stroke_width, 0. ..=100., 0.25);
            });
            return;
        }
        if self.selection.len() > 1 {
            self.finish_property_edit();
            ui.add(TypeHeader {
                icon: i16::SELECT_ALL,
                title: &format!("{} objects selected", self.selection.len()),
                subtitle: "Mixed selection",
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                if ui.add(nf::Button::new("Transform…")).clicked() {
                    self.show_panel(crate::workspace::PanelId::Transform);
                }
                let color_btn = ui.add(nf::Button::new("Color…"));
                crate::authoring_modify::update_color_popover_anchor(
                    ctx,
                    "multi-select-color",
                    color_btn.rect,
                );
                if color_btn.clicked() {
                    self.workspace.color_picker_expanded = true;
                    self.authoring_modify.color_selection = true;
                    crate::authoring_modify::request_color_popover(
                        ctx,
                        "multi-select-color",
                        color_btn.rect,
                        false,
                        true,
                    );
                    self.show_panel(crate::workspace::PanelId::Color);
                }
            });
            return;
        }
        let object = self.selected_sampled();
        let scope = egui::Id::new((
            "object-properties",
            &self.clipboard.session,
            self.symbol_id(),
            self.selected_frame_range().is_some(),
            object.as_ref().map(|e| std::mem::discriminant(&e.kind)),
        ));
        self.property_edit_scope(scope);
        let mut changes = PropertyChanges::default();
        ui.push_id(scope, |ui| {
            if let Some(mut element) = object {
                let original = element.clone();
                let mut edit_symbol = false;
                let mut convert_text = false;
                let mut swap_to_symbol: Option<Id> = None;
                let mut set_symbol_kind: Option<(Id, SymbolKind)> = None;
                let mut duplicate_symbol_clicked = false;
                let is_instance = matches!(element.kind, ElementKind::Instance { .. });
                let is_text_box = matches!(element.kind, ElementKind::TextBox { .. });
                let (icon, title, subtitle) = self.object_header(&element);
                ui.add(TypeHeader { icon, title: &title, subtitle: &subtitle });
                ui.add_enabled_ui_semantic(self.selected_editable(), |ui| {
                    if let ElementKind::Instance { symbol_id, .. } = &element.kind {
                        let cur_sym_id = symbol_id.clone();
                        let cur_sym_kind = self
                            .project
                            .symbol(&cur_sym_id)
                            .map(|s| s.kind)
                            .unwrap_or(SymbolKind::MovieClip);
                        // Row 1: Instance Name + Swap… button on one compact row (NF2 Properties — Movie clip).
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S2;
                            let swap_w = 72.0;
                            let name_w = (ui.available_width() - swap_w - space::S2).max(80.0);
                            let mut n_resp = ui.add(
                                nf::TextField::new(&mut element.name, "Name")
                                    .prefix("Name")
                                    .width(name_w),
                            );
                            n_resp.id = nf::TextField::edit_id(n_resp.id);
                            n_resp.widget_info(|| {
                                let mut info = egui::WidgetInfo::text_edit(
                                    ui.is_enabled(),
                                    &element.name,
                                    &element.name,
                                    "Name",
                                );
                                info.label = Some("Name".into());
                                info
                            });
                            changes.note(n_resp);

                            let swap_btn = ui
                                .add(nf::Button::new("Swap…"))
                                .on_hover_text("Swap symbol instance with another Library symbol");
                            nf::popup_below(&swap_btn, 220.0, |ui| {
                                if let Some(lib_id) = self.library_selected.clone()
                                    && self.can_swap_instance_symbol(&lib_id)
                                    && let Some(lib_sym) = self.project.symbol(&lib_id)
                                {
                                    let label = format!("Use selected “{}”", lib_sym.name);
                                    if ui.add(MenuItem::new(&label)).clicked() {
                                        swap_to_symbol = Some(lib_id);
                                        ui.close();
                                    }
                                    nf::menu_separator(ui);
                                }
                                let symbols: Vec<(Id, String, bool)> = self
                                    .project
                                    .symbols
                                    .iter()
                                    .filter(|s| !self.project.is_scene(&s.id))
                                    .map(|s| {
                                        let allowed = s.id == cur_sym_id
                                            || self.can_swap_instance_symbol(&s.id);
                                        (s.id.clone(), s.name.clone(), allowed)
                                    })
                                    .collect();
                                for (sid, sname, allowed) in symbols {
                                    if ui
                                        .add_enabled(
                                            allowed,
                                            MenuItem::new(&sname).checked(sid == cur_sym_id),
                                        )
                                        .clicked()
                                    {
                                        if sid != cur_sym_id {
                                            swap_to_symbol = Some(sid);
                                        }
                                        ui.close();
                                    }
                                }
                            });
                        });

                        // Row 2: Symbol behavior selector (Movie Clip / Button / Graphic) + Edit / Duplicate.
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S2;
                            let half = ((ui.available_width() - space::S2) / 2.0).max(60.0);
                            let kind_label = match cur_sym_kind {
                                SymbolKind::MovieClip => "Movie Clip",
                                SymbolKind::Button => "Button",
                                SymbolKind::Graphic => "Graphic",
                            };
                            nf::Select::new("instance-symbol-behavior", kind_label)
                                .width(half)
                                .show(ui, |ui| {
                                    for (k, label) in [
                                        (SymbolKind::MovieClip, "Movie Clip"),
                                        (SymbolKind::Button, "Button"),
                                        (SymbolKind::Graphic, "Graphic"),
                                    ] {
                                        if ui
                                            .add(MenuItem::new(label).checked(k == cur_sym_kind))
                                            .clicked()
                                        {
                                            if k != cur_sym_kind {
                                                set_symbol_kind = Some((cur_sym_id.clone(), k));
                                            }
                                            ui.close();
                                        }
                                    }
                                });
                            if ui.add(nf::Button::new("Edit symbol")).clicked() {
                                edit_symbol = true;
                            }
                            if ui
                                .add(nf::Button::new("Duplicate…"))
                                .on_hover_text("Duplicate symbol and swap this instance")
                                .clicked()
                            {
                                duplicate_symbol_clicked = true;
                            }
                        });
                    } else {
                        changes.note(text_row(ui, "Name", &mut element.name));
                    }

                    let is_text_any =
                        is_text_box || matches!(element.kind, ElementKind::Text { .. });
                    let pos_title = if is_instance || is_text_any {
                        "Position and size"
                    } else {
                        "Position and transform"
                    };
                    inspector_section_with_open(ui, pos_title, !is_text_any, |ui| {
                        // Row 1: W and H + Constrain proportions lock icon on one line.
                        let link_id = egui::Id::new("properties-element-size-linked");
                        let mut linked =
                            ui.data(|d| d.get_temp::<bool>(link_id).unwrap_or(false));
                        match &mut element.kind {
                            ElementKind::TextBox { width, height, .. }
                            | ElementKind::Rectangle { width, height, .. }
                            | ElementKind::Ellipse { width, height, .. }
                            | ElementKind::Video { width, height } => {
                                let (old_w, old_h) = (*width, *height);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = space::S2;
                                    let pair_width = (ui.available_width()
                                        - crate::theme::control_h()
                                        - space::S2)
                                        .max(80.0);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(pair_width, crate::theme::control_h()),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            for r in numeric_pair(
                                                ui,
                                                ["W", "H"],
                                                [width, height],
                                                1. ..=10000.,
                                                1.,
                                            ) {
                                                changes.note(r);
                                            }
                                        },
                                    );
                                    if ui
                                        .add(
                                            nf::IconButton::new(
                                                i16::LINK,
                                                "Constrain proportions",
                                            )
                                            .selected(linked),
                                        )
                                        .clicked()
                                    {
                                        linked = !linked;
                                    }
                                });
                                if linked && old_w > 0.0 && old_h > 0.0 {
                                    if (*width - old_w).abs() > 1e-6 {
                                        *height = (*width * old_h / old_w).clamp(1.0, 10000.0);
                                    } else if (*height - old_h).abs() > 1e-6 {
                                        *width = (*height * old_w / old_h).clamp(1.0, 10000.0);
                                    }
                                }
                            }
                            other_kind => {
                                let (base_w, base_h) = match other_kind {
                                    ElementKind::Instance { symbol_id, .. } => {
                                        self.unscaled_symbol_size(ctx, symbol_id)
                                    }
                                    ElementKind::Bitmap { width, height, .. } => (
                                        f64::from(*width).max(1.0),
                                        f64::from(*height).max(1.0),
                                    ),
                                    ElementKind::Path { commands, .. }
                                    | ElementKind::PaintedPath { commands, .. } => {
                                        if let Ok(Some(b)) = neoflash_core::path_bounds(commands) {
                                            (
                                                (b.x_max - b.x_min).max(1.0),
                                                (b.y_max - b.y_min).max(1.0),
                                            )
                                        } else {
                                            (100.0, 100.0)
                                        }
                                    }
                                    ElementKind::Text {
                                        text, font_size, ..
                                    } => (
                                        (text.chars().count().max(1) as f64 * *font_size * 0.6)
                                            .max(1.0),
                                        (*font_size * 1.2).max(1.0),
                                    ),
                                    _ => (100.0, 100.0),
                                };
                                let mut w = (base_w * element.transform.scale_x.abs()).max(0.01);
                                let mut h = (base_h * element.transform.scale_y.abs()).max(0.01);
                                let (old_w, old_h) = (w, h);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = space::S2;
                                    let pair_width = (ui.available_width()
                                        - crate::theme::control_h()
                                        - space::S2)
                                        .max(80.0);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(pair_width, crate::theme::control_h()),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            for r in numeric_pair(
                                                ui,
                                                ["W", "H"],
                                                [&mut w, &mut h],
                                                0.01..=100_000.,
                                                1.,
                                            ) {
                                                changes.note(r);
                                            }
                                        },
                                    );
                                    if ui
                                        .add(
                                            nf::IconButton::new(
                                                i16::LINK,
                                                "Constrain proportions",
                                            )
                                            .selected(linked),
                                        )
                                        .clicked()
                                    {
                                        linked = !linked;
                                    }
                                });
                                let w_changed = (w - old_w).abs() > 1e-6;
                                let h_changed = (h - old_h).abs() > 1e-6;
                                if linked && old_w > 0.0 && old_h > 0.0 {
                                    if w_changed {
                                        h = (w * old_h / old_w).clamp(0.01, 100_000.0);
                                    } else if h_changed {
                                        w = (h * old_w / old_h).clamp(0.01, 100_000.0);
                                    }
                                }
                                if w_changed || (linked && h_changed) {
                                    let sign_x = if element.transform.scale_x < 0.0 {
                                        -1.0
                                    } else {
                                        1.0
                                    };
                                    element.transform.scale_x =
                                        sign_x * (w / base_w.max(1e-6)).clamp(0.0001, 1000.0);
                                }
                                if h_changed || (linked && w_changed) {
                                    let sign_y = if element.transform.scale_y < 0.0 {
                                        -1.0
                                    } else {
                                        1.0
                                    };
                                    element.transform.scale_y =
                                        sign_y * (h / base_h.max(1e-6)).clamp(0.0001, 1000.0);
                                }
                            }
                        }
                        ui.data_mut(|d| d.insert_temp(link_id, linked));

                        // Row 2: X and Y.
                        for r in numeric_pair(
                            ui,
                            ["X", "Y"],
                            [&mut element.transform.x, &mut element.transform.y],
                            -1_000_000. ..=1_000_000.,
                            1.,
                        ) {
                            changes.note(r);
                        }

                        // Row 3: Scale X and Scale Y in 100 % (instead of 1.0).
                        let mut sx_pct = element.transform.scale_x * 100.0;
                        let mut sy_pct = element.transform.scale_y * 100.0;
                        let (old_sx, old_sy) = (sx_pct, sy_pct);
                        let [rx, ry] = numeric_pair_with_suffix(
                            ui,
                            ["Scale X", "Scale Y"],
                            [&mut sx_pct, &mut sy_pct],
                            -100_000. ..=100_000.,
                            1.,
                            " %",
                        );
                        if rx.changed() || ry.changed() {
                            if linked && old_sx.abs() > 1e-6 && old_sy.abs() > 1e-6 {
                                if (sx_pct - old_sx).abs() > 1e-6 {
                                    sy_pct =
                                        (sx_pct * old_sy / old_sx).clamp(-100_000.0, 100_000.0);
                                } else if (sy_pct - old_sy).abs() > 1e-6 {
                                    sx_pct =
                                        (sy_pct * old_sx / old_sy).clamp(-100_000.0, 100_000.0);
                                }
                            }
                            element.transform.scale_x = sx_pct / 100.0;
                            element.transform.scale_y = sy_pct / 100.0;
                        }
                        changes.note(rx);
                        changes.note(ry);

                        // Row 4: Rotation (and Skew for instances).
                        if is_instance {
                            for r in numeric_pair(
                                ui,
                                ["Rotation", "Skew"],
                                [
                                    &mut element.transform.rotation,
                                    &mut element.transform.skew,
                                ],
                                -36000. ..=36000.,
                                1.,
                            ) {
                                changes.note(r);
                            }
                        } else {
                            changes.note(numeric_row(
                                ui,
                                "Rotation",
                                &mut element.transform.rotation,
                                -36000. ..=36000.,
                                1.,
                            ));
                        }
                    });
                    if let ElementKind::Instance { symbol_id, playback } = &mut element.kind
                        && let Some(count) = self
                            .project
                            .symbol(symbol_id)
                            .filter(|s| s.kind == SymbolKind::MovieClip)
                            .map(|s| s.timeline.frame_count.max(1))
                    {
                        inspector_section(ui, "Timeline", |ui| {
                            if let Some(control) =
                                Self::movie_clip_timeline_section_ui(ui, count, playback)
                            {
                                changes.control = Some(control);
                            }
                        });
                    }
                    inspector_section(ui, appearance_title(&element.kind), |ui| {
                        if !is_instance && !is_text_box {
                            changes.note(percent_row(ui, "Opacity", &mut element.transform.alpha));
                        }
                        let (orig_fill_grad, orig_stroke_grad) = match &original.kind {
                            ElementKind::PaintedPath {
                                fill_gradient,
                                stroke_gradient,
                                ..
                            } => (fill_gradient.clone(), stroke_gradient.clone()),
                            _ => (None, None),
                        };
                        match &mut element.kind {
                            ElementKind::Rectangle { fill, stroke, stroke_width, .. }
                            | ElementKind::Ellipse { fill, stroke, stroke_width, .. } => {
                                set_paint_row_gradient(ctx, "shape-fill", None);
                                set_paint_row_gradient(ctx, "shape-stroke", None);
                                changes.note(paint_row(ui, "shape-fill", "Fill", fill));
                                changes.note(paint_row(ui, "shape-stroke", "Stroke", stroke));
                                changes.note(numeric_row(ui, "Stroke width", stroke_width, 0. ..=100., 0.25));
                            }
                            ElementKind::Text { text, font_family, font_size, color } => {
                                let focus_text = ctx.data_mut(|data| {
                                    data.remove_temp::<bool>(egui::Id::new("nf2-focus-text-edit"))
                                        .unwrap_or(false)
                                });
                                let text_resp = text_row(ui, "Text", text);
                                if focus_text {
                                    text_resp.request_focus();
                                    if let Some(mut state) = egui::TextEdit::load_state(ctx, text_resp.id) {
                                        let end = egui::text::CCursor::new(text.chars().count());
                                        state.cursor.set_char_range(Some(
                                            egui::text::CCursorRange::two(egui::text::CCursor::new(0), end),
                                        ));
                                        state.store(ctx, text_resp.id);
                                    }
                                }
                                changes.note(text_resp);
                                let before = font_family.clone();
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = space::S2;
                                    let btn_w = 104.0;
                                    let font_w = (ui.available_width() - btn_w - space::S2).max(80.0);
                                    let current = font_family.clone();
                                    let (mut response, _) = nf::Select::new("font_family", &current)
                                        .width(font_w)
                                        .show(ui, |ui| {
                                            ui.add(nf::Search::new(&mut self.font_filter, "Find font").width(220.));
                                            for family in &self.fonts.families {
                                                if (self.font_filter.is_empty()
                                                    || family.to_lowercase().contains(&self.font_filter.to_lowercase()))
                                                    && ui.add(MenuItem::new(family).checked(family == font_family)).clicked()
                                                {
                                                    *font_family = family.clone();
                                                    ui.close();
                                                }
                                            }
                                        });
                                    if *font_family != before {
                                        response.mark_changed();
                                    }
                                    changes.note(response);
                                    let convert_resp = ui.add(nf::Button::new("Use text box"));
                                    if convert_resp.clicked() {
                                        convert_text = true;
                                        changes.control = Some(convert_resp.id);
                                    }
                                });
                                changes.note(numeric_row(ui, "Size", font_size, 4. ..=512., 1.));
                                changes.note(paint_row(ui, "text-color", "Color", color));
                                if let Err(error) = self.fonts.load(ctx, font_family) {
                                    ui.label(rich(error, TextStyle::Small).color(UiPalette::current(ctx).error_text));
                                }
                            }
                            kind @ ElementKind::TextBox { .. } => {
                                if let Some(control) =
                                    self.text_box_properties(ui, ctx, kind, &mut element.transform.alpha)
                                {
                                    changes.control = Some(control);
                                }
                            }
                            ElementKind::Video { .. } => {
                                hint(ui, "Attach a NetStream from ActionScript. Playback runs in the HTML5 preview.");
                            }
                            ElementKind::Bitmap { resource_id, width, height, smoothing } => {
                                let path = self.project.resources.iter().find(|r| r.id == *resource_id).map(|r| r.path.as_str()).unwrap_or("Missing image");
                                ui.add(egui::Label::new(rich(path, TextStyle::Body).color(neoflash_ui::palette(ctx).text_primary)).truncate()).on_hover_text(path);
                                hint(ui, &format!("{width} × {height} px"));
                                changes.note(ui.add(nf::Checkbox::new(smoothing, "Smoothing")));
                            }
                            ElementKind::Path { commands, fill_rule, fill, stroke, stroke_width } | ElementKind::PaintedPath { commands, fill_rule, fill, stroke, stroke_width, .. } => {
                                set_paint_row_gradient(ctx, "path-fill", orig_fill_grad);
                                set_paint_row_gradient(ctx, "path-stroke", orig_stroke_grad);
                                hint(ui, &format!("{} commands", commands.len()));
                                changes.note(paint_row(ui, "path-fill", "Fill", fill));
                                changes.note(paint_row(ui, "path-stroke", "Stroke", stroke));
                                changes.note(numeric_row(ui, "Stroke width", stroke_width, 0. ..=100., 0.25));
                                changes.note(select_row(ui, "Fill rule", "fill_rule", fill_rule, &[(FillRule::NonZero, "Nonzero"), (FillRule::EvenOdd, "Even–odd")]));
                                let edit_pts_sc = self.tooltip_sc(crate::commands::CommandId::ToolSubselect);
                                let mut edit_pts_btn = nf::Button::new("Edit points");
                                if let Some(sc) = edit_pts_sc.as_deref() {
                                    edit_pts_btn = edit_pts_btn.shortcut(sc);
                                }
                                if ui.add(edit_pts_btn).clicked() { self.set_tool(Tool::Subselect); self.select_single_id(original.id.clone()); }
                            }
                            ElementKind::Instance { .. } => {
                                if let Some(control) =
                                    Self::instance_color_effect_ui(ui, &original.id, &mut element.transform)
                                {
                                    changes.control = Some(control);
                                }
                            }
                        }
                    });
                    if is_text_box {
                        if let Some(control) =
                            self.text_box_paragraph_and_options_ui(ui, &mut element.kind)
                        {
                            changes.control = Some(control);
                        }
                    } else if let ElementKind::Text {
                        text,
                        font_family,
                        font_size,
                        color,
                    } = &element.kind
                    {
                        let named = crate::fonts::styled_family(font_family, false, false);
                        let family = if ctx.fonts(|fonts| fonts.families().contains(&named)) {
                            named
                        } else {
                            egui::FontFamily::Proportional
                        };
                        let galley = ui.painter().layout_no_wrap(
                            text.clone(),
                            egui::FontId::new(*font_size as f32, family),
                            egui::Color32::WHITE,
                        );
                        let mut temp_box = ElementKind::TextBox {
                            text: text.clone(),
                            width: (galley.size().x as f64 + 8.0).clamp(180.0, 1_000_000.0),
                            height: (galley.size().y as f64 + 8.0).clamp(80.0, 1_000_000.0),
                            format: TextFormat {
                                font: font_family.clone(),
                                size: *font_size,
                                color: u32::from_be_bytes([0, color[0], color[1], color[2]]),
                                ..Default::default()
                            },
                            word_wrap: true,
                            multiline: true,
                        };
                        let before_temp = temp_box.clone();
                        if let Some(control) =
                            self.text_box_paragraph_and_options_ui(ui, &mut temp_box)
                        {
                            changes.control = Some(control);
                        }
                        if temp_box != before_temp {
                            if let ElementKind::TextBox { width, format, .. } = &mut temp_box
                                && format
                                    .field_options()
                                    .is_some_and(|o| o.column_count > 1)
                            {
                                *width = (*width).max(260.0);
                            }
                            element.transform.alpha *= color[3] as f64 / 255.0;
                            element.kind = temp_box;
                        }
                    }
                    if let Some(control) = self.primitive_options_properties_ui(ui, &mut element) {
                        changes.control = Some(control);
                    }
                    inspector_section(ui, "Behavior / linkage", |ui| {
                        if let ElementKind::Instance { symbol_id, playback } = &mut element.kind {
                            if let Some(control) = self.instance_playback_ui(ui, symbol_id, playback) { changes.control = Some(control); }
                            if let Some(symbol) = self.project.symbol(symbol_id) {
                                for (label, value) in [("Linkage", symbol.linkage_id.as_deref()), ("Class", symbol.class_name.as_deref())] {
                                    if let Some(value) = value { property_row(ui, label, |ui| { ui.add(egui::Label::new(rich(value, TextStyle::Mono).color(neoflash_ui::palette(ctx).text_primary)).truncate()).on_hover_text(value); }); }
                                }
                            }
                            if ui.add(nf::Button::new("Edit symbol")).clicked() { edit_symbol = true; }
                        }
                        let obj_act_sc = self.tooltip_sc(crate::commands::CommandId::WindowActions);
                        let mut obj_act_btn = nf::Button::new("Object actions…");
                        if let Some(sc) = obj_act_sc.as_deref() {
                            obj_act_btn = obj_act_btn.shortcut(sc);
                        }
                        if ui.add(obj_act_btn).clicked() {
                            self.script_target = crate::app::ScriptTarget::Object;
                            self.show_panel(crate::workspace::PanelId::Actions);
                        }
                    });
                    inspector_section(ui, "Accessibility", |ui| {
                        hint(ui, element.accessibility.as_ref().map(|a| a.name.as_str()).unwrap_or("Not exposed to assistive technology"));
                        if ui.add(nf::Button::new("Edit accessibility…")).clicked() { self.show_panel(crate::workspace::PanelId::Accessibility); }
                    });
                });
                if let (ElementKind::PaintedPath { fill: old_fill, stroke: old_stroke, .. },
                    ElementKind::PaintedPath { fill, stroke, fill_gradient, stroke_gradient, .. })
                    = (&original.kind, &mut element.kind) {
                    crate::authoring_modify::update_gradient_for_color_edit(fill_gradient, *old_fill, *fill);
                    crate::authoring_modify::update_gradient_for_color_edit(stroke_gradient, *old_stroke, *stroke);
                }
                if convert_text && let ElementKind::Text { text, font_family, font_size, color } = &element.kind {
                    let named = crate::fonts::styled_family(font_family, false, false);
                    let family = if ctx.fonts(|fonts| fonts.families().contains(&named)) { named } else { egui::FontFamily::Proportional };
                    let galley = ui.painter().layout_no_wrap(text.clone(), egui::FontId::new(*font_size as f32, family), egui::Color32::WHITE);
                    let kind = ElementKind::TextBox {
                        text: text.clone(), width: (galley.size().x as f64 + 4.).clamp(100., 1000000.), height: (galley.size().y as f64 + 4.).clamp(100., 1000000.),
                        format: TextFormat { font: font_family.clone(), size: *font_size, color: u32::from_be_bytes([0,color[0],color[1],color[2]]), ..Default::default() }, word_wrap: true, multiline: true,
                    };
                    element.transform.alpha *= color[3] as f64 / 255.;
                    element.kind = kind;
                }
                if original != element && let Some(control) = changes.control {
                    if let Some(field) = TextFieldState::from_kind(&element.kind) && let Err(error) = field.validate() { self.message(format!("Text box edit rejected: {error}")); return; }
                    match (&original.kind, &element.kind) {
                        (
                            ElementKind::Rectangle { fill: of, stroke: os, stroke_width: ow, .. }
                            | ElementKind::Ellipse { fill: of, stroke: os, stroke_width: ow, .. }
                            | ElementKind::Path { fill: of, stroke: os, stroke_width: ow, .. }
                            | ElementKind::PaintedPath { fill: of, stroke: os, stroke_width: ow, .. },
                            ElementKind::Rectangle { fill: nf, stroke: ns, stroke_width: nw, .. }
                            | ElementKind::Ellipse { fill: nf, stroke: ns, stroke_width: nw, .. }
                            | ElementKind::Path { fill: nf, stroke: ns, stroke_width: nw, .. }
                            | ElementKind::PaintedPath { fill: nf, stroke: ns, stroke_width: nw, .. },
                        ) => {
                            if nf != of {
                                self.fill = *nf;
                            }
                            if ns != os {
                                self.stroke = *ns;
                            }
                            if nw != ow {
                                self.stroke_width = *nw;
                            }
                        }
                        (ElementKind::Text { color: oc, .. }, ElementKind::Text { color: nc, .. })
                            if nc != oc =>
                        {
                            self.fill = *nc;
                        }
                        _ => {}
                    }
                    self.begin_property_edit(scope, control);
                    self.select_single_id(original.id.clone());
                    self.ensure_selected_key();
                    if let Some(current) = self.selected_element_mut() {
                        current.name = element.name;
                        current.transform = element.transform;
                        current.kind = element.kind;
                    }
                    self.project.upgrade_format_for_features();
                }
                if let Some(target_sym_id) = swap_to_symbol {
                    let _ = self.swap_instance_symbol(&target_sym_id);
                }
                if let Some((sym_id, next_kind)) = set_symbol_kind {
                    self.finish_edits();
                    self.transact_named("Change symbol behavior", move |editor| {
                        if let Some(sym) = editor.project.symbol_mut(&sym_id) {
                            sym.kind = next_kind;
                        }
                    });
                }
                if duplicate_symbol_clicked {
                    let _ = self.duplicate_selected_instance_symbol();
                }
                if is_instance {
                    self.display_and_filters_sections_ui(ui, "props-mc", true, false);
                } else if is_text_box {
                    self.display_and_filters_sections_ui(ui, "props-text", false, false);
                }
                if edit_symbol { self.enter_selected_symbol_in_place(); }
            } else {
                let mut document = self.project.clone();
                let symbol_id = self.symbol_id().to_owned();
                let layer_id = self.symbol().timeline.layers[self.layer].id.clone();
                let layer = self.layer;
                let frame = self.frame;
                let frame_context = self.selected_frame_range().is_some();
                if !frame_context {
                inspector_section(ui, "Stage", |ui| {
                changes.note(text_row(ui, "Name", &mut document.name));
                let (mut width, mut height) = (document.width as f64, document.height as f64);
                let (old_width, old_height) = (width, height);
                let link_id = egui::Id::new("properties-stage-size-linked");
                let mut linked = ui.data(|d| d.get_temp::<bool>(link_id).unwrap_or(false));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let pair_width = ui.available_width() - crate::theme::control_h() - space::S2;
                    ui.allocate_ui_with_layout(egui::vec2(pair_width, crate::theme::control_h()), egui::Layout::top_down(egui::Align::Min), |ui| {
                        for r in numeric_pair(ui, ["W", "H"], [&mut width, &mut height], 1. ..=8192., 1.) { changes.note(r); }
                    });
                    let link = ui.add(nf::IconButton::new(i16::LINK, "Constrain stage proportions").selected(linked));
                    if link.clicked() { linked = !linked; }
                });
                ui.data_mut(|d| d.insert_temp(link_id, linked));
                if linked && old_width > 0. && old_height > 0. {
                    if width != old_width { height = (width * old_height / old_width).round().clamp(1., 8192.); }
                    else if height != old_height { width = (height * old_width / old_height).round().clamp(1., 8192.); }
                }
                document.width = width.round() as u32; document.height = height.round() as u32;
                changes.note(color_row(ui, "background", "", "Background", &mut document.background));
                });
                }
                let mut fps = document.fps;
                let timeline = &mut document.symbol_mut(&symbol_id).unwrap().timeline;
                if frame_context {
                    let range = self.selected_frame_range();
                    let subtitle = range.map(|(start, end, layers)| format!("Frames {}–{} · {} layer{}", start + 1, end + 1, layers, if layers == 1 { "" } else { "s" })).unwrap_or_default();
                    // Inside a motion tween the frame inspector is Properties — Motion tween (PR-10).
                    let motion = self.motion_tween_span();
                    match &motion {
                        Some(span) => { ui.add(TypeHeader { icon: i16::TWEEN_MOTION, title: "Motion tween", subtitle: &format!("Layer \"{}\" · {}", span.layer_name, span.frame_label()) }); }
                        None => { ui.add(TypeHeader { icon: i16::FRAME, title: "Frame", subtitle: &subtitle }); }
                    }
                    let mut layer_end = None;
                    inspector_section(ui, "Range / duration", |ui| {
                        changes.note(text_row(ui, "Layer", &mut timeline.layers[layer].name));
                        let mut end = timeline.layers[layer].properties.end_frame.unwrap_or(timeline.frame_count);
                        let response = ui.add(nf::DragField::new(&mut end, "Layer end").prefix("Layer end").range(1..=10000).speed(0.25));
                        if response.changed() { layer_end = Some(end); }
                    });
                    if let Some(end) = layer_end { self.set_layer_duration(end); return; }
                    if let Some(span) = &motion { self.motion_tween_properties(ui, span); } else {
                    inspector_section(ui, "Tween", |ui| {
                        ui.add_enabled_ui_semantic(self.layer_editable(), |ui| {
                            let guided = timeline.layers[layer].properties.motion_guide.is_some();
                            if let Some(key) = timeline.layers[layer].key_at_mut(frame) {
                                let mut options = vec![
                                    (None, "No tween"),
                                    (Some(Easing::Linear), "Linear"),
                                    (Some(Easing::EaseIn), "Ease in"),
                                    (Some(Easing::EaseOut), "Ease out"),
                                    (Some(Easing::EaseInOut), "Ease in/out"),
                                    (Some(Easing::Bounce), "Bounce"),
                                    (Some(Easing::Elastic), "Elastic"),
                                ];
                                options.push((if let Some(curve @ Easing::CubicBezier { .. }) = key.tween { Some(curve) } else { Some(Easing::CubicBezier { x1: 0.25, y1: 0.1, x2: 0.25, y2: 1. }) }, "Custom curve"));
                                options.push((if let Some(flash @ Easing::Flash { .. }) = key.tween { Some(flash) } else { Some(Easing::Flash { acceleration: 0. }) }, "Flash ease"));
                                changes.note(select_row(ui, "Easing", "tween", &mut key.tween, &options));
                                if key.tween.is_some() && key.tween_kind == TweenKind::Classic {
                                    ui.horizontal(|ui| {
                                        let half = (ui.available_width() - space::S2) / 2.;
                                        let mut turns = key.classic_rotate_turns as f64;
                                        let r_turns = ui.add(
                                            nf::Field::new(&mut turns, "Rotate")
                                                .prefix("Rotate")
                                                .unit("×")
                                                .range(0.0..=(Keyframe::MAX_ROTATE_TURNS as f64))
                                                .stepper()
                                                .width(half),
                                        );
                                        if r_turns.changed() {
                                            key.classic_rotate_turns = (turns.round().max(0.0) as u32)
                                                .min(Keyframe::MAX_ROTATE_TURNS);
                                        }
                                        changes.note(r_turns);
                                        let dir_label = format!(
                                            "Direction: {}",
                                            key.classic_rotate_dir.label()
                                        );
                                        let mut dir_changed = false;
                                        let (mut r_dir, _) =
                                            nf::Select::new("classic-direction", &dir_label)
                                                .width(half)
                                                .show(ui, |ui| {
                                                    for dir in ClassicRotateDirection::ALL {
                                                        if ui
                                                            .add(
                                                                MenuItem::new(dir.label()).checked(
                                                                    key.classic_rotate_dir == dir,
                                                                ),
                                                            )
                                                            .clicked()
                                                        {
                                                            if key.classic_rotate_dir != dir {
                                                                key.classic_rotate_dir = dir;
                                                                dir_changed = true;
                                                            }
                                                            ui.close();
                                                        }
                                                    }
                                                });
                                        if dir_changed {
                                            r_dir.mark_changed();
                                        }
                                        changes.note(r_dir);
                                    });
                                    changes.note(ui.add_enabled(guided, nf::Checkbox::new(&mut key.orient_to_path, "Orient to path")).on_disabled_hover_text("Needs a motion guide layer"));
                                    changes.note(ui.add_enabled(guided, nf::Checkbox::new(&mut key.snap_to_guide, "Snap to motion guide")).on_disabled_hover_text("Needs a motion guide layer"));
                                    changes.note(ui.add(nf::Checkbox::new(&mut key.sync_symbols, "Sync graphic symbols")));
                                    changes.note(ui.add(nf::Checkbox::new(&mut key.tween_scale, "Scale")));
                                }

                                // Y may overshoot (Back) or undershoot; the document allows |y| ≤ 100, x stays in 0–1.
                                if let Some(Easing::CubicBezier{x1,y1,x2,y2})=&mut key.tween {
                                    for (labels, values) in [(["X1","Y1"],[&mut *x1,&mut *y1]),(["X2","Y2"],[&mut *x2,&mut *y2])] {
                                        for r in numeric_pair(ui, labels, values,-100. ..=100.,0.01) { changes.note(r); }
                                    }
                                    *x1 = x1.clamp(0., 1.); *x2 = x2.clamp(0., 1.);
                                }
                                if let Some(Easing::Flash{acceleration})=&mut key.tween {
                                    changes.note(numeric_row(ui, "Acceleration", acceleration, -100. ..=100., 1.));
                                }
                            } else { hint(ui, "Select a frame with a keyframe to edit its tween."); }
                        });
                    });
                    }
                    inspector_section(ui, "Label / script", |ui| {
                        ui.add_enabled_ui_semantic(self.layer_editable(), |ui| {
                            if let Some(key) = timeline.layers[layer].key_at_mut(frame) { changes.note(text_row(ui, "Frame label", &mut key.label)); }
                        });
                        let frame_act_sc = self.tooltip_sc(crate::commands::CommandId::WindowActions);
                        let mut frame_act_btn = nf::Button::new("Frame script…");
                        if let Some(sc) = frame_act_sc.as_deref() {
                            frame_act_btn = frame_act_btn.shortcut(sc);
                        }
                        if ui.add(frame_act_btn).clicked() { self.open_frame_actions(); }
                    });
                    self.frame_sound_section(ui);
                } else {
                    let min = timeline.layers.iter().flat_map(|l| &l.keyframes).map(|k| k.frame+1).chain(timeline.scripts.iter().map(|s|s.frame+1)).chain(timeline.layers.iter().filter_map(|l|l.properties.end_frame)).max().unwrap_or(1);
                    inspector_section(ui, "Playback", |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S2;
                            let width = (ui.available_width() - space::S2) / 2.;
                            changes.note(ui.add(nf::DragField::new(&mut fps, "FPS").prefix("FPS").range(1..=1000).speed(0.25).width(width)));
                            changes.note(ui.add(nf::DragField::new(&mut timeline.frame_count, "Frames").prefix("Frames").range(min..=10000).speed(0.25).width(width)));
                        });
                    });
                    inspector_section(ui, "Publish", |ui| {
                        ui.add(Badge::new("HTML5 · Rust / WebAssembly", BadgeType::Accent));
                        // §4: "Publish settings…"; publishing itself is File ▸ Publish and the top-bar Publish button.
                        if ui.add(nf::Button::new("Publish settings…")).on_hover_text("Publish settings (preview). Publish HTML5 with the Publish button or File ▸ Publish.").clicked() { Self::open_planned(ui.ctx(), "Properties ▸ Publish settings…"); }
                    });
                }
                document.fps = fps;
                if document != self.project && let Some(control) = changes.control {
                    self.begin_property_edit(scope, control);
                    // Copy only changed authoring fields, never the cloned document.
                    self.project.name=document.name.clone(); self.project.width=document.width; self.project.height=document.height;
                    self.project.fps=document.fps; self.project.background=document.background;
                    let draft=&document.symbol(&symbol_id).unwrap().timeline;
                    let project_snapshot = self.project.clone();
                    if let Some(symbol)=self.project.symbol_mut(&symbol_id) {
                        let timeline=&mut symbol.timeline;
                        let min=timeline.layers.iter().flat_map(|l|&l.keyframes).map(|k|k.frame+1).chain(timeline.scripts.iter().map(|s|s.frame+1)).chain(timeline.layers.iter().filter_map(|l|l.properties.end_frame)).max().unwrap_or(1);
                        timeline.frame_count=draft.frame_count.max(min);
                        if let Some(target)=timeline.layers.iter_mut().find(|l|l.id==layer_id) {
                            target.name=draft.layers[layer].name.clone();
                            if target.visible && !target.locked && let Some(changed)=draft.layers[layer].key_at(frame) && let Some(key)=target.keyframes.iter_mut().find(|k|k.frame==changed.frame) {
                                let sync_turned_on = !key.sync_symbols && changed.sync_symbols;
                                let key_frame = key.frame;
                                key.label=changed.label.clone();
                                key.tween=changed.tween;
                                key.orient_to_path=changed.orient_to_path;
                                key.snap_to_guide=changed.snap_to_guide;
                                key.classic_rotate_turns=changed.classic_rotate_turns;
                                key.classic_rotate_dir=changed.classic_rotate_dir;
                                key.sync_symbols=changed.sync_symbols;
                                key.tween_scale=changed.tween_scale;
                                target.normalize_motion_keys();
                                if sync_turned_on {
                                    let end_frame = target.keyframes.iter().filter(|k| k.frame > key_frame).map(|k| k.frame).min().unwrap_or_else(|| timeline.frame_count.saturating_sub(1));
                                    neoflash_core::sync_layer_symbols(&project_snapshot, target, key_frame, end_frame);
                                }
                            }
                            if matches!(draft.layers[layer].key_at(frame).and_then(|k| k.tween), Some(Easing::Flash { .. })) { self.project.format_version = self.project.format_version.max(5); }
                        }
                    }
                    self.project.upgrade_format_for_features();
                    self.normalize_selection();
                }
            }
        });
        self.end_property_interaction(ctx);
    }
}

impl Editor {
    /// Icon, title and subtitle of the Properties type header for one object.
    fn object_header(&self, element: &Element) -> (neoflash_ui::Icon, String, String) {
        let layer = self
            .selection
            .targets
            .first()
            .and_then(|target| {
                self.symbol()
                    .timeline
                    .layers
                    .iter()
                    .find(|l| l.id == target.layer)
            })
            .map(|l| format!("layer “{}”", l.name))
            .unwrap_or_default();
        let (icon, title) = match &element.kind {
            ElementKind::Rectangle { .. } => (i16::RECT, "Rectangle"),
            ElementKind::Ellipse { .. } => (i16::OVAL, "Oval"),
            ElementKind::Text { .. } => (i16::TEXT, "Text"),
            ElementKind::TextBox { .. } => (i16::TEXT, "Text box"),
            ElementKind::Video { .. } => (i16::VIDEO, "Video"),
            ElementKind::Bitmap { .. } => (i16::BITMAP, "Bitmap"),
            ElementKind::Path { .. } => (i16::PEN, "Vector path"),
            ElementKind::PaintedPath { .. } => (i16::BRUSH, "Painted path"),
            ElementKind::Instance { symbol_id, .. } => {
                let symbol = self.project.symbol(symbol_id);
                let (icon, title) = match symbol.map(|s| s.kind) {
                    Some(SymbolKind::Graphic) => (i16::GRAPHIC, "Graphic"),
                    Some(SymbolKind::Button) => (i16::BUTTON_SYMBOL, "Button"),
                    _ => (i16::MOVIECLIP, "Movie clip"),
                };
                let name = symbol.map(|s| s.name.as_str()).unwrap_or("Missing");
                return (icon, title.into(), format!("Instance of “{name}”"));
            }
        };
        let subtitle = if layer.is_empty() {
            "Drawing object".to_owned()
        } else {
            format!("Drawing object · {layer}")
        };
        (icon, title.into(), subtitle)
    }

    /// Unscaled local width and height of `symbol_id` at frame 0 (fallback `100.0 × 100.0`).
    fn unscaled_symbol_size(&self, ctx: &egui::Context, symbol_id: &str) -> (f64, f64) {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let mut union = egui::Rect::NOTHING;
        for item in evaluate(&self.project, symbol_id, 0) {
            let r = crate::stage::world_bounds(&item, &painter);
            if r.is_finite() {
                union = union.union(r);
            }
        }
        if union.is_finite() && union.width() > 1e-3 && union.height() > 1e-3 {
            (f64::from(union.width()), f64::from(union.height()))
        } else {
            (100.0, 100.0)
        }
    }

    /// `Colour effect` section for Symbol Instances (`PR-05`, NF2 `Panel / Properties — Movie clip`
    /// and classic Flash Color Effect: `None`, `Brightness`, `Tint`, `Advanced`, `Alpha`).
    fn instance_color_effect_ui(
        ui: &mut egui::Ui,
        element_id: &str,
        transform: &mut Transform,
    ) -> Option<egui::Id> {
        use crate::paint_controls::{PropertyChanges, color_row};
        let mut changes = PropertyChanges::default();
        let half = ((ui.available_width() - space::S2) / 2.0).max(60.0);

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum ColorEffectStyle {
            None,
            Brightness,
            Tint,
            Advanced,
            Alpha,
        }

        let style_key = egui::Id::new(("mc-color-effect-style-state", element_id));
        let inferred = if !transform.color_transform.is_identity() {
            let ct = transform.color_transform;
            let same_mul = (ct.multipliers[0] - ct.multipliers[1]).abs() < 1e-4
                && (ct.multipliers[1] - ct.multipliers[2]).abs() < 1e-4;
            let same_off = (ct.offsets[0] - ct.offsets[1]).abs() < 1e-2
                && (ct.offsets[1] - ct.offsets[2]).abs() < 1e-2;
            if same_mul
                && same_off
                && (ct.multipliers[3] - 1.0).abs() < 1e-4
                && ct.offsets[3].abs() < 1e-2
            {
                ColorEffectStyle::Brightness
            } else if same_mul
                && (ct.multipliers[3] - 1.0).abs() < 1e-4
                && ct.offsets[3].abs() < 1e-2
            {
                ColorEffectStyle::Tint
            } else {
                ColorEffectStyle::Advanced
            }
        } else {
            ColorEffectStyle::Alpha
        };
        let mut style = ui
            .data(|d| d.get_temp::<ColorEffectStyle>(style_key))
            .unwrap_or(inferred);

        let style_label = match style {
            ColorEffectStyle::None => "Style: None",
            ColorEffectStyle::Brightness => "Style: Brightness",
            ColorEffectStyle::Tint => "Style: Tint",
            ColorEffectStyle::Advanced => "Style: Advanced",
            ColorEffectStyle::Alpha => "Style: Alpha",
        };
        let mut picked_style = None;

        match style {
            ColorEffectStyle::None => {
                let (mut s_resp, _) = nf::Select::new("mc-color-effect-style", style_label)
                    .width(ui.available_width())
                    .show(ui, |ui| {
                        for (st, name) in [
                            (ColorEffectStyle::None, "None"),
                            (ColorEffectStyle::Brightness, "Brightness"),
                            (ColorEffectStyle::Tint, "Tint"),
                            (ColorEffectStyle::Advanced, "Advanced"),
                            (ColorEffectStyle::Alpha, "Alpha"),
                        ] {
                            if ui.add(MenuItem::new(name).checked(st == style)).clicked() {
                                picked_style = Some(st);
                                ui.close();
                            }
                        }
                    });
                if let Some(next) = picked_style {
                    style = next;
                    if next == ColorEffectStyle::None {
                        transform.alpha = 1.0;
                        transform.color_transform = ColorTransform::default();
                        s_resp.mark_changed();
                    }
                }
                changes.note(s_resp);
            }
            ColorEffectStyle::Alpha => {
                let mut alpha_pct = (transform.alpha * 100.0).clamp(0.0, 100.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let (mut s_resp, _) = nf::Select::new("mc-color-effect-style", style_label)
                        .width(half)
                        .show(ui, |ui| {
                            for (st, name) in [
                                (ColorEffectStyle::None, "None"),
                                (ColorEffectStyle::Brightness, "Brightness"),
                                (ColorEffectStyle::Tint, "Tint"),
                                (ColorEffectStyle::Advanced, "Advanced"),
                                (ColorEffectStyle::Alpha, "Alpha"),
                            ] {
                                if ui.add(MenuItem::new(name).checked(st == style)).clicked() {
                                    picked_style = Some(st);
                                    ui.close();
                                }
                            }
                        });
                    if let Some(next) = picked_style {
                        style = next;
                        if next == ColorEffectStyle::None {
                            transform.alpha = 1.0;
                            transform.color_transform = ColorTransform::default();
                            s_resp.mark_changed();
                        }
                    }
                    changes.note(s_resp);

                    let f_resp = ui.add(
                        nf::Field::new(&mut alpha_pct, "Opacity")
                            .prefix("α")
                            .unit("%")
                            .decimals(0)
                            .range(0.0..=100.0)
                            .width(half),
                    );
                    if f_resp.changed() {
                        transform.alpha = (alpha_pct / 100.0).clamp(0.0, 1.0);
                    }
                    changes.note(f_resp);
                });
                let sl_resp = ui.add(
                    nf::Slider::new(&mut alpha_pct, 0.0..=100.0, "Opacity")
                        .width(ui.available_width()),
                );
                if sl_resp.changed() {
                    transform.alpha = (alpha_pct / 100.0).clamp(0.0, 1.0);
                }
                changes.note(sl_resp);
            }
            ColorEffectStyle::Brightness => {
                let ct = transform.color_transform;
                let avg_off = (ct.offsets[0] + ct.offsets[1] + ct.offsets[2]) / (3.0 * 255.0);
                let cur_b = if avg_off.abs() > 1e-4 {
                    avg_off.clamp(-1.0, 1.0)
                } else {
                    let avg_mul = (ct.multipliers[0] + ct.multipliers[1] + ct.multipliers[2]) / 3.0;
                    (avg_mul - 1.0).clamp(-1.0, 1.0)
                };
                let mut b_pct = (cur_b * 100.0).clamp(-100.0, 100.0);
                let apply_b = |ct: &mut ColorTransform, pct: f64| {
                    let b = (pct / 100.0).clamp(-1.0, 1.0);
                    if b >= 0.0 {
                        let m = 1.0 - b;
                        let o = 255.0 * b;
                        ct.multipliers[0] = m;
                        ct.multipliers[1] = m;
                        ct.multipliers[2] = m;
                        ct.offsets[0] = o;
                        ct.offsets[1] = o;
                        ct.offsets[2] = o;
                    } else {
                        let m = 1.0 + b;
                        ct.multipliers[0] = m;
                        ct.multipliers[1] = m;
                        ct.multipliers[2] = m;
                        ct.offsets[0] = 0.0;
                        ct.offsets[1] = 0.0;
                        ct.offsets[2] = 0.0;
                    }
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let (mut s_resp, _) = nf::Select::new("mc-color-effect-style", style_label)
                        .width(half)
                        .show(ui, |ui| {
                            for (st, name) in [
                                (ColorEffectStyle::None, "None"),
                                (ColorEffectStyle::Brightness, "Brightness"),
                                (ColorEffectStyle::Tint, "Tint"),
                                (ColorEffectStyle::Advanced, "Advanced"),
                                (ColorEffectStyle::Alpha, "Alpha"),
                            ] {
                                if ui.add(MenuItem::new(name).checked(st == style)).clicked() {
                                    picked_style = Some(st);
                                    ui.close();
                                }
                            }
                        });
                    if let Some(next) = picked_style {
                        style = next;
                        if next == ColorEffectStyle::None {
                            transform.alpha = 1.0;
                            transform.color_transform = ColorTransform::default();
                            s_resp.mark_changed();
                        }
                    }
                    changes.note(s_resp);

                    let f_resp = ui.add(
                        nf::Field::new(&mut b_pct, "Brightness")
                            .prefix("Bright")
                            .unit("%")
                            .decimals(0)
                            .range(-100.0..=100.0)
                            .width(half),
                    );
                    if f_resp.changed() {
                        apply_b(&mut transform.color_transform, b_pct);
                    }
                    changes.note(f_resp);
                });
                let sl_resp = ui.add(
                    nf::Slider::new(&mut b_pct, -100.0..=100.0, "Brightness")
                        .width(ui.available_width()),
                );
                if sl_resp.changed() {
                    apply_b(&mut transform.color_transform, b_pct);
                }
                changes.note(sl_resp);
            }
            ColorEffectStyle::Tint => {
                let ct = transform.color_transform;
                let avg_mul = ((ct.multipliers[0] + ct.multipliers[1] + ct.multipliers[2]) / 3.0)
                    .clamp(0.0, 1.0);
                let mut tint_pct = ((1.0 - avg_mul) * 100.0).clamp(0.0, 100.0);
                let t_denom = (1.0 - avg_mul).max(1e-3);
                let mut tint_rgba = [
                    (ct.offsets[0] / t_denom).round().clamp(0.0, 255.0) as u8,
                    (ct.offsets[1] / t_denom).round().clamp(0.0, 255.0) as u8,
                    (ct.offsets[2] / t_denom).round().clamp(0.0, 255.0) as u8,
                    (tint_pct / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8,
                ];
                let apply_tint = |ct: &mut ColorTransform, pct: f64, rgb: [u8; 3]| {
                    let t = (pct / 100.0).clamp(0.0, 1.0);
                    let m = 1.0 - t;
                    ct.multipliers[0] = m;
                    ct.multipliers[1] = m;
                    ct.multipliers[2] = m;
                    ct.offsets[0] = f64::from(rgb[0]) * t;
                    ct.offsets[1] = f64::from(rgb[1]) * t;
                    ct.offsets[2] = f64::from(rgb[2]) * t;
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let (mut s_resp, _) = nf::Select::new("mc-color-effect-style", style_label)
                        .width(half)
                        .show(ui, |ui| {
                            for (st, name) in [
                                (ColorEffectStyle::None, "None"),
                                (ColorEffectStyle::Brightness, "Brightness"),
                                (ColorEffectStyle::Tint, "Tint"),
                                (ColorEffectStyle::Advanced, "Advanced"),
                                (ColorEffectStyle::Alpha, "Alpha"),
                            ] {
                                if ui.add(MenuItem::new(name).checked(st == style)).clicked() {
                                    picked_style = Some(st);
                                    ui.close();
                                }
                            }
                        });
                    if let Some(next) = picked_style {
                        style = next;
                        if next == ColorEffectStyle::None {
                            transform.alpha = 1.0;
                            transform.color_transform = ColorTransform::default();
                            s_resp.mark_changed();
                        }
                    }
                    changes.note(s_resp);

                    let f_resp = ui.add(
                        nf::Field::new(&mut tint_pct, "Tint")
                            .prefix("Tint")
                            .unit("%")
                            .decimals(0)
                            .range(0.0..=100.0)
                            .width(half),
                    );
                    if f_resp.changed() {
                        apply_tint(
                            &mut transform.color_transform,
                            tint_pct,
                            [tint_rgba[0], tint_rgba[1], tint_rgba[2]],
                        );
                    }
                    changes.note(f_resp);
                });
                let c_resp = color_row(ui, "mc-tint-color", "", "Tint color", &mut tint_rgba);
                if c_resp.changed() {
                    tint_pct = (f64::from(tint_rgba[3]) / 255.0 * 100.0).clamp(0.0, 100.0);
                    apply_tint(
                        &mut transform.color_transform,
                        tint_pct,
                        [tint_rgba[0], tint_rgba[1], tint_rgba[2]],
                    );
                }
                changes.note(c_resp);
            }
            ColorEffectStyle::Advanced => {
                let mut alpha_pct = (transform.alpha * 100.0).clamp(0.0, 100.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S2;
                    let (mut s_resp, _) = nf::Select::new("mc-color-effect-style", style_label)
                        .width(half)
                        .show(ui, |ui| {
                            for (st, name) in [
                                (ColorEffectStyle::None, "None"),
                                (ColorEffectStyle::Brightness, "Brightness"),
                                (ColorEffectStyle::Tint, "Tint"),
                                (ColorEffectStyle::Advanced, "Advanced"),
                                (ColorEffectStyle::Alpha, "Alpha"),
                            ] {
                                if ui.add(MenuItem::new(name).checked(st == style)).clicked() {
                                    picked_style = Some(st);
                                    ui.close();
                                }
                            }
                        });
                    if let Some(next) = picked_style {
                        style = next;
                        if next == ColorEffectStyle::None {
                            transform.alpha = 1.0;
                            transform.color_transform = ColorTransform::default();
                            s_resp.mark_changed();
                        }
                    }
                    changes.note(s_resp);

                    let f_resp = ui.add(
                        nf::Field::new(&mut alpha_pct, "Opacity")
                            .prefix("α")
                            .unit("%")
                            .decimals(0)
                            .range(0.0..=100.0)
                            .width(half),
                    );
                    if f_resp.changed() {
                        transform.alpha = (alpha_pct / 100.0).clamp(0.0, 1.0);
                    }
                    changes.note(f_resp);
                });
                for (ch_idx, mul_label, off_label) in [
                    (0usize, "R ×", "+ R"),
                    (1usize, "G ×", "+ G"),
                    (2usize, "B ×", "+ B"),
                    (3usize, "A ×", "+ A"),
                ] {
                    let mut mul_pct = transform.color_transform.multipliers[ch_idx] * 100.0;
                    let mut off_val = transform.color_transform.offsets[ch_idx];
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        let r_mul = ui.add(
                            nf::Field::new(&mut mul_pct, mul_label)
                                .prefix(mul_label)
                                .unit("%")
                                .decimals(0)
                                .range(-1000.0..=1000.0)
                                .width(half),
                        );
                        if r_mul.changed() {
                            transform.color_transform.multipliers[ch_idx] =
                                (mul_pct / 100.0).clamp(-16.0, 16.0);
                        }
                        changes.note(r_mul);

                        let r_off = ui.add(
                            nf::Field::new(&mut off_val, off_label)
                                .prefix(off_label)
                                .decimals(0)
                                .range(-255.0..=255.0)
                                .width(half),
                        );
                        if r_off.changed() {
                            transform.color_transform.offsets[ch_idx] =
                                off_val.clamp(-255.0, 255.0);
                        }
                        changes.note(r_off);
                    });
                }
            }
        }

        ui.data_mut(|d| d.insert_temp(style_key, style));
        changes.control
    }
}

fn appearance_title(kind: &ElementKind) -> &'static str {
    match kind {
        ElementKind::Rectangle { .. }
        | ElementKind::Ellipse { .. }
        | ElementKind::Path { .. }
        | ElementKind::PaintedPath { .. } => "Fill and stroke",
        ElementKind::Text { .. } | ElementKind::TextBox { .. } => "Character",
        ElementKind::Instance { .. } => "Colour effect",
        ElementKind::Video { .. } | ElementKind::Bitmap { .. } => "Display",
    }
}

/// Secondary helper text (`NF2/Small`, `text/secondary`).
pub(crate) fn hint(ui: &mut egui::Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    let color = if ui.is_enabled() {
        p.text_secondary
    } else {
        p.text_disabled
    };
    ui.add(egui::Label::new(rich(text, TextStyle::Small).color(color)).wrap());
}

/// Labelled `NF2/Select` over fixed options. The response is marked changed
/// only when the value changed, and keeps the Select's stable id.
pub(crate) fn select_row<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    label: &str,
    id_salt: &str,
    value: &mut T,
    options: &[(T, &str)],
) -> egui::Response {
    let before = *value;
    let current = options
        .iter()
        .find(|(v, _)| *v == *value)
        .map_or("—", |(_, l)| *l);
    let mut response = crate::paint_controls::property_row(ui, label, |ui| {
        let width = ui.available_width();
        nf::Select::new(id_salt, current)
            .width(width)
            .show(ui, |ui| {
                for (option, name) in options {
                    if ui
                        .add(MenuItem::new(name).checked(*option == *value))
                        .clicked()
                    {
                        *value = *option;
                    }
                }
            })
            .0
    });
    if *value != before {
        response.mark_changed();
    }
    response
}

/// `NF2/Section Header` (collapsible overline) with 8 px row gaps.
pub(crate) fn inspector_section<R>(
    ui: &mut egui::Ui,
    title: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) {
    let default_open = !matches!(title, "Behavior / linkage" | "Accessibility");
    inspector_section_with_open(ui, title, default_open, content);
}

pub(crate) fn inspector_section_with_open<R>(
    ui: &mut egui::Ui,
    title: &str,
    default_open: bool,
    content: impl FnOnce(&mut egui::Ui) -> R,
) {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = crate::theme::control_h();
        SectionHeader::new(title)
            .id_salt(("inspector-section", title, default_open))
            .default_open(default_open)
            .show(ui, content);
    });
}
