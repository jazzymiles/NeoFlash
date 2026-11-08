//! Native font measurements and paint for the shared Rust text layout.
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Shape, Vec2};
use neoflash_core::{
    AuthoredTextFieldOptions, ElementKind, FillRule, TextFieldState, TextFormat, TextFormatPatch,
    TextFormatRangePatch, TextLayout, TextPatch, layout_text,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

struct CachedLayout {
    kind: ElementKind,
    family: FontFamily,
    ppp: u32,
    layout: Arc<TextLayout>,
    bytes: usize,
}
#[derive(Default)]
struct LayoutCache {
    entries: VecDeque<CachedLayout>,
    bytes: usize,
}
const CACHE_BYTES: usize = 8 * 1024 * 1024;

impl crate::app::Editor {
    /// Create authored text from the Stage Text tool. A click creates point text whose width follows
    /// its content; a drag creates a fixed, wrapping text box. Both paths share the workspace font
    /// defaults and enter the existing Properties text editor immediately.
    pub(crate) fn create_stage_text(
        &mut self,
        ctx: &egui::Context,
        position: Pos2,
        bounds: Option<Rect>,
    ) {
        let preferred = self.workspace.text_font.clone();
        let font = if !preferred.is_empty() && self.fonts.families.contains(&preferred) {
            preferred
        } else if self.fonts.families.iter().any(|family| family == "Arial") {
            "Arial".to_owned()
        } else {
            "Helvetica".to_owned()
        };
        let size = self.workspace.text_size;
        let _ = self.fonts.load(ctx, &font);
        let (kind, transform) = if let Some(bounds) = bounds {
            (
                ElementKind::TextBox {
                    text: "Your text".into(),
                    width: f64::from(bounds.width()),
                    height: f64::from(bounds.height()),
                    format: TextFormat {
                        font,
                        size,
                        color: u32::from_be_bytes([0, self.fill[0], self.fill[1], self.fill[2]]),
                        ..Default::default()
                    },
                    multiline: true,
                    word_wrap: true,
                },
                neoflash_core::Transform {
                    x: f64::from(bounds.min.x),
                    y: f64::from(bounds.min.y),
                    alpha: f64::from(self.fill[3]) / 255.,
                    ..Default::default()
                },
            )
        } else {
            (
                ElementKind::Text {
                    text: "Your text".into(),
                    font_family: font,
                    font_size: size,
                    color: self.fill,
                },
                neoflash_core::Transform {
                    x: f64::from(position.x),
                    y: f64::from(position.y),
                    ..Default::default()
                },
            )
        };
        self.add_element_transformed(kind, transform);
        if self.selected_element().is_some_and(|element| {
            matches!(
                element.kind,
                ElementKind::Text { .. } | ElementKind::TextBox { .. }
            )
        }) {
            self.focus_selected_text_editor(ctx);
        }
    }

    pub(crate) fn focus_selected_text_editor(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("properties-tool-tab"), false);
            data.insert_temp(egui::Id::new("nf2-focus-text-edit"), true);
        });
    }

    /// Compact Two-Row `Properties — Text` `Character` section (`PR-01`, NF2 `Panel / Properties — Text`).
    pub(crate) fn text_box_properties(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        kind: &mut ElementKind,
        alpha: &mut f64,
    ) -> Option<egui::Id> {
        use crate::paint_controls::{PropertyChanges, color_row};
        use neoflash_ui::{components as nf, tokens::space};
        let ElementKind::TextBox { text, format, .. } = kind else {
            return None;
        };
        let mut changes = PropertyChanges::default();
        let mut field_options = format.field_options().unwrap_or_default();
        let before_options = field_options.clone();

        // Compact 2-row text editor (preserving keyboard focus for newly created stage text).
        let focus_text = ctx.data_mut(|data| {
            data.remove_temp::<bool>(egui::Id::new("nf2-focus-text-edit"))
                .unwrap_or(false)
        });
        egui::ScrollArea::vertical()
            .id_salt("text-box-content")
            .max_height(68.)
            .themed()
            .show(ui, |ui| {
                let response = ui.add_sized(
                    [ui.available_width(), 48.],
                    egui::TextEdit::multiline(text).desired_rows(2),
                );
                if focus_text {
                    response.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ctx, response.id) {
                        let end = egui::text::CCursor::new(text.chars().count());
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                end,
                            )));
                        state.store(ctx, response.id);
                    }
                }
                changes.note(response);
            });

        let half = ((ui.available_width() - space::S2) / 2.0).max(60.0);

        // Row 1: Text field type (Static / Dynamic / Input) + Family selector in 2 columns.
        let before_font = format.font.clone();
        let current_type_idx = match (field_options.field_type, field_options.selectable) {
            (neoflash_core::TextFieldType::Input, _) => 2usize,
            (neoflash_core::TextFieldType::Dynamic, false) => 0usize,
            _ => 1usize,
        };
        let type_labels = ["Static text", "Dynamic text", "Input text"];
        let mut picked_type = None;
        let mut font_response = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let (mut type_resp, _) =
                nf::Select::new("text_box_type", type_labels[current_type_idx])
                    .width(half)
                    .show(ui, |ui| {
                        for (idx, label) in type_labels.into_iter().enumerate() {
                            if ui
                                .add(nf::MenuItem::new(label).checked(idx == current_type_idx))
                                .clicked()
                            {
                                picked_type = Some(idx);
                                ui.close();
                            }
                        }
                    });
            if let Some(idx) = picked_type {
                match idx {
                    0 => {
                        field_options.field_type = neoflash_core::TextFieldType::Dynamic;
                        field_options.selectable = false;
                    }
                    1 => {
                        field_options.field_type = neoflash_core::TextFieldType::Dynamic;
                        field_options.selectable = true;
                    }
                    _ => {
                        field_options.field_type = neoflash_core::TextFieldType::Input;
                        field_options.selectable = true;
                    }
                }
                if field_options != before_options {
                    type_resp.mark_changed();
                }
            }
            changes.note(type_resp);

            let current_font = format.font.clone();
            let (mut f_resp, _) = nf::Select::new("text_box_font", &current_font)
                .width(half)
                .show(ui, |ui| {
                    ui.add(nf::Search::new(&mut self.font_filter, "Find font").width(220.));
                    for family in &self.fonts.families {
                        if (self.font_filter.is_empty()
                            || family
                                .to_lowercase()
                                .contains(&self.font_filter.to_lowercase()))
                            && ui
                                .add(nf::MenuItem::new(family).checked(family == &format.font))
                                .clicked()
                        {
                            format.font = family.clone();
                            ui.close();
                        }
                    }
                });
            if format.font != before_font {
                f_resp.mark_changed();
            }
            font_response = Some(f_resp);
        });
        if let Some(f_resp) = font_response {
            changes.note(f_resp);
        }

        // Row 2: Style (Regular / Bold / Italic / Bold Italic) + Embed font + Embedding… on 1 compact row.
        let current_style_idx = match (format.bold, format.italic) {
            (false, false) => 0usize,
            (true, false) => 1usize,
            (false, true) => 2usize,
            (true, true) => 3usize,
        };
        let style_options = [
            (0usize, "Regular", false, false),
            (1usize, "Bold", true, false),
            (2usize, "Italic", false, true),
            (3usize, "Bold Italic", true, true),
        ];
        let mut enable_after_confirm = false;
        let mut open_embedding = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let style_w = (ui.available_width() - 184.0).max(84.0);
            let mut style_changed = false;
            let (mut style_resp, _) =
                nf::Select::new("text_box_style", style_options[current_style_idx].1)
                    .width(style_w)
                    .show(ui, |ui| {
                        for (idx, label, b, i) in style_options {
                            if ui
                                .add(nf::MenuItem::new(label).checked(idx == current_style_idx))
                                .clicked()
                            {
                                if (format.bold, format.italic) != (b, i) {
                                    format.bold = b;
                                    format.italic = i;
                                    style_changed = true;
                                }
                                ui.close();
                            }
                        }
                    });
            if style_changed {
                style_resp.mark_changed();
            }
            changes.note(style_resp);

            let embed_response = ui.add(nf::Checkbox::new(
                &mut field_options.embed_fonts,
                "Embed font",
            ));
            enable_after_confirm = embed_response.changed() && field_options.embed_fonts;
            if enable_after_confirm {
                // Enabling is transactional with the dialog: Cancel must not leave the field in
                // embedded-only mode without a matching embedded font resource.
                field_options.embed_fonts = false;
            }
            changes.note(embed_response);

            let embedding_response = ui
                .add(nf::Button::new("Embedding…"))
                .on_hover_text("Set font embedding options for this text field");
            open_embedding = enable_after_confirm || embedding_response.clicked();
        });
        if open_embedding {
            let family = format.font.clone();
            let bold = format.bold;
            let italic = format.italic;
            let authored_text = text.clone();
            self.open_font_embedding_for_text(
                ctx,
                &family,
                bold,
                italic,
                &authored_text,
                enable_after_confirm,
            );
        }

        // Row 3: Size (pt) + Letter spacing (px) in 2 columns.
        let min_leading = (-format.size * 1.2 + 0.1).max(-10000.);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let size_response = ui.add(
                nf::Field::new(&mut format.size, "Size")
                    .prefix("Size")
                    .unit("pt")
                    .decimals(1)
                    .range(0.1..=10000.)
                    .width(half),
            );
            let next_min_leading = (-format.size * 1.2 + 0.1).max(-10000.);
            if size_response.changed() && format.leading < next_min_leading {
                format.leading = next_min_leading;
            }
            changes.note(size_response);

            let spacing_response = ui.add(
                nf::Field::new(&mut format.letter_spacing, "Letter spacing")
                    .prefix("Spacing")
                    .unit("px")
                    .decimals(1)
                    .range(-1000.0..=1000.0)
                    .width(half),
            );
            changes.note(spacing_response);
        });
        let _ = min_leading;

        // Row 4: Color swatch/hex/alpha + Auto kern on 1 compact row.
        let mut rgba = [
            ((format.color >> 16) & 0xff) as u8,
            ((format.color >> 8) & 0xff) as u8,
            (format.color & 0xff) as u8,
            (*alpha * 255.0).round().clamp(0.0, 255.0) as u8,
        ];
        let mut auto_kern = format.kerning.unwrap_or(false);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let color_w = (ui.available_width() - 86.0 - space::S2).max(120.0);
            ui.allocate_ui_with_layout(
                egui::vec2(color_w, crate::theme::control_h()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let c_resp = color_row(ui, "text-box-color", "", "Color", &mut rgba);
                    if c_resp.changed() {
                        format.color = u32::from_be_bytes([0, rgba[0], rgba[1], rgba[2]]);
                        *alpha = f64::from(rgba[3]) / 255.0;
                    }
                    changes.note(c_resp);
                },
            );
            let kern_resp = ui.add(nf::Checkbox::new(&mut auto_kern, "Auto kern"));
            if kern_resp.changed() {
                format.kerning = auto_kern.then_some(true);
            }
            changes.note(kern_resp);
        });

        // Row 5: Compact character & field toggles on 1 row (Selectable, HTML, Border, Underline).
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            changes.note(ui.add(nf::Checkbox::new(
                &mut field_options.selectable,
                "Selectable",
            )));
            changes.note(ui.add(nf::Checkbox::new(&mut field_options.html, "HTML")));
            changes.note(ui.add(nf::Checkbox::new(&mut field_options.border, "Border")));
            changes.note(ui.add(nf::Checkbox::new(&mut format.underline, "Underline")));
        });

        if field_options != before_options {
            format.set_field_options((!field_options.is_default()).then_some(field_options));
        }

        if format
            .field_options()
            .is_some_and(|options| options.embed_fonts)
            && !neoflash_export::has_embedded_font_face(
                &self.project,
                &format.font,
                format.bold,
                format.italic,
            )
        {
            ui.add(nf::InlineMessage::warning(
                "This embedded-only field has no matching embedded face. Open Embedding… before publishing.",
            ));
        }
        if let Err(error) = self
            .fonts
            .load_style(ctx, &format.font, format.bold, format.italic)
            && self.workspace.text_prefs.substitution_warning
        {
            ui.colored_label(crate::theme::UiPalette::current(ctx).error_text, error);
        }
        changes.control
    }

    /// `Paragraph` and `Options` collapsible sections for `Properties — Text` (`PR-01`).
    pub(crate) fn text_box_paragraph_and_options_ui(
        &mut self,
        ui: &mut egui::Ui,
        kind: &mut ElementKind,
    ) -> Option<egui::Id> {
        use crate::paint_controls::{PropertyChanges, text_row};
        use neoflash_ui::{
            components::{self as nf, SectionHeader, Segmented},
            tokens::space,
        };
        let ElementKind::TextBox {
            format,
            word_wrap,
            multiline,
            ..
        } = kind
        else {
            return None;
        };
        let mut changes = PropertyChanges::default();
        let half = ((ui.available_width() - space::S2) / 2.0).max(60.0);
        let min_leading = (-format.size * 1.2 + 0.1).max(-10000.);

        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = crate::theme::control_h();
            SectionHeader::new("Paragraph")
                .id_salt("text-box-paragraph")
                .default_open(true)
                .show(ui, |ui| {
                    // Row 1: Format alignment (Left / Center / Right / Justify).
                    let mut align_mode = match format.effective_align_mode() {
                        "center" => 1usize,
                        "right" | "end" => 2usize,
                        "justify" => 3usize,
                        _ => 0usize,
                    };
                    if let Some(mut align_resp) = Segmented::show(
                        ui,
                        &mut align_mode,
                        &[
                            (0usize, "Left"),
                            (1usize, "Center"),
                            (2usize, "Right"),
                            (3usize, "Justify"),
                        ],
                        true,
                    ) {
                        match align_mode {
                            0 => {
                                format.align = neoflash_core::TextAlign::Left;
                                format.set_align_mode(None);
                            }
                            1 => {
                                format.align = neoflash_core::TextAlign::Center;
                                format.set_align_mode(None);
                            }
                            2 => {
                                format.align = neoflash_core::TextAlign::Right;
                                format.set_align_mode(None);
                            }
                            _ => {
                                format.align = neoflash_core::TextAlign::Left;
                                format.set_align_mode(Some("justify".into()));
                            }
                        }
                        align_resp.mark_changed();
                        changes.note(align_resp);
                    }

                    // Row 2: Spacing — Indent (px) + Line / Leading (pt) in 2 columns.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        changes.note(
                            ui.add(
                                nf::Field::new(&mut format.indent, "Indent")
                                    .prefix("Indent")
                                    .unit("px")
                                    .decimals(1)
                                    .range(-10000.0..=10000.0)
                                    .width(half),
                            ),
                        );
                        changes.note(
                            ui.add(
                                nf::Field::new(&mut format.leading, "Leading")
                                    .prefix("Line")
                                    .unit("pt")
                                    .decimals(1)
                                    .range(min_leading..=10000.0)
                                    .width(half),
                            ),
                        );
                    });

                    // Row 3: Margins — Left (px) + Right (px) in 2 columns.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        changes.note(
                            ui.add(
                                nf::Field::new(&mut format.left_margin, "Left margin")
                                    .prefix("Left")
                                    .unit("px")
                                    .decimals(1)
                                    .range(0.0..=10000.0)
                                    .width(half),
                            ),
                        );
                        changes.note(
                            ui.add(
                                nf::Field::new(&mut format.right_margin, "Right margin")
                                    .prefix("Right")
                                    .unit("px")
                                    .decimals(1)
                                    .range(0.0..=10000.0)
                                    .width(half),
                            ),
                        );
                    });

                    // Row 4: Behavior (Single line / Multiline / Multiline no wrap / Password).
                    let mut field_options = format.field_options().unwrap_or_default();
                    let before_opts = field_options.clone();
                    let current_beh = if field_options.display_as_password {
                        3usize
                    } else if !*multiline {
                        0usize
                    } else if *word_wrap {
                        1usize
                    } else {
                        2usize
                    };
                    let beh_options = [
                        (0usize, "Behavior: Single line", "Single line"),
                        (1usize, "Behavior: Multiline", "Multiline"),
                        (2usize, "Behavior: Multiline no wrap", "Multiline no wrap"),
                        (3usize, "Behavior: Password", "Password"),
                    ];
                    let mut picked_beh = None;
                    let (mut beh_resp, _) =
                        nf::Select::new("text_box_behavior", beh_options[current_beh].1)
                            .width(ui.available_width())
                            .show(ui, |ui| {
                                for (idx, _, label) in beh_options {
                                    if ui
                                        .add(nf::MenuItem::new(label).checked(idx == current_beh))
                                        .clicked()
                                    {
                                        picked_beh = Some(idx);
                                        ui.close();
                                    }
                                }
                            });
                    if let Some(idx) = picked_beh
                        && idx != current_beh
                    {
                        match idx {
                            0 => {
                                *multiline = false;
                                *word_wrap = false;
                                field_options.display_as_password = false;
                            }
                            1 => {
                                *multiline = true;
                                *word_wrap = true;
                                field_options.display_as_password = false;
                            }
                            2 => {
                                *multiline = true;
                                *word_wrap = false;
                                field_options.display_as_password = false;
                            }
                            _ => {
                                *multiline = false;
                                *word_wrap = false;
                                field_options.display_as_password = true;
                            }
                        }
                        if field_options != before_opts {
                            format.set_field_options(
                                (!field_options.is_default()).then_some(field_options),
                            );
                        }
                        beh_resp.mark_changed();
                    }
                    changes.note(beh_resp);
                });

            SectionHeader::new("Options")
                .id_salt("text-box-options")
                .default_open(false)
                .show(ui, |ui| {
                    let mut field_options = format.field_options().unwrap_or_default();
                    let before_opts = field_options.clone();
                    changes.note(text_row(ui, "URL", &mut format.url));
                    let mut target_str = format.target_str().to_owned();
                    let mut max_chars = field_options.max_chars as f64;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        let mut t_resp = ui.add(
                            nf::TextField::new(&mut target_str, "Target")
                                .prefix("Target")
                                .width(half),
                        );
                        t_resp.id = nf::TextField::edit_id(t_resp.id);
                        if t_resp.changed() {
                            format.set_target_str(target_str);
                        }
                        changes.note(t_resp);

                        let m_resp = ui.add(
                            nf::Field::new(&mut max_chars, "Max chars")
                                .prefix("Max")
                                .decimals(0)
                                .range(0.0..=65535.0)
                                .width(half),
                        );
                        if m_resp.changed() {
                            field_options.max_chars = max_chars.round().max(0.0) as u32;
                        }
                        changes.note(m_resp);
                    });

                    let mut var_str = field_options.variable.clone().unwrap_or_default();
                    let mut restrict_str = field_options.restrict.clone().unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        let mut v_resp = ui.add(
                            nf::TextField::new(&mut var_str, "Variable")
                                .prefix("Variable")
                                .width(half),
                        );
                        v_resp.id = nf::TextField::edit_id(v_resp.id);
                        if v_resp.changed() {
                            field_options.variable =
                                (!var_str.trim().is_empty()).then(|| var_str.trim().to_owned());
                        }
                        changes.note(v_resp);

                        let mut r_resp = ui.add(
                            nf::TextField::new(&mut restrict_str, "Restrict")
                                .prefix("Restrict")
                                .width(half),
                        );
                        r_resp.id = nf::TextField::edit_id(r_resp.id);
                        if r_resp.changed() {
                            field_options.restrict =
                                (!restrict_str.is_empty()).then_some(restrict_str);
                        }
                        changes.note(r_resp);
                    });

                    if field_options != before_opts {
                        format.set_field_options(
                            (!field_options.is_default()).then_some(field_options),
                        );
                    }
                });
        });

        changes.control
    }

    /// Apply a `TextPatch` (including per-range `format_range`, `format_runs`, `field_type`,
    /// `selectable`, `display_as_password`, `max_chars`, `restrict`, `as2_viewport`, etc.)
    /// to the currently selected `TextBox` in a single `"Text format"` Undo transaction.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn apply_selected_text_patch(&mut self, patch: &TextPatch) -> Result<(), String> {
        self.finish_edits();
        let Some(el_id) = self.selection.single().map(|t| t.element.clone()) else {
            return Err("Select a single TextBox element first.".into());
        };
        let mut candidate = self.project.clone();
        let sym_id = self.symbol_id().to_owned();
        let frame = self.frame;
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return Err("Active symbol not found.".into());
        };
        let mut updated = false;
        for layer in &mut sym.timeline.layers {
            let Some(key_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame) else {
                continue;
            };
            if let Some(el) = layer.keyframes[key_idx]
                .elements
                .iter_mut()
                .find(|e| e.id == el_id)
            {
                let Some(mut state) = TextFieldState::from_kind(&el.kind) else {
                    return Err("Selected element is not a text field.".into());
                };
                state.apply(patch).map_err(|e| e.to_string())?;
                state.write_to_kind(&mut el.kind);
                updated = true;
                break;
            }
        }
        if !updated {
            return Err("Selected TextBox was not found on the current frame.".into());
        }
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("Text format", |s| {
            s.project = candidate;
        });
        Ok(())
    }

    /// Apply a rich-text `TextFormatPatch` over UTF-16 range `[begin, end)` on the selected `TextBox`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn apply_selected_text_format_range(
        &mut self,
        begin: u32,
        end: u32,
        format: TextFormatPatch,
    ) -> Result<(), String> {
        self.apply_selected_text_patch(&TextPatch {
            format_range: Some(TextFormatRangePatch { begin, end, format }),
            ..TextPatch::default()
        })
    }

    /// Parse HTML markup (`<b>`, `<i>`, `<u>`, `<font>`, `<a href>`, `<p align>`, `<br>`, CSS classes/inline styles)
    /// into the selected `TextBox` in one `"HTML text"` Undo transaction.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_selected_text_html(&mut self, html_source: &str) -> Result<(), String> {
        self.finish_edits();
        let Some(el_id) = self.selection.single().map(|t| t.element.clone()) else {
            return Err("Select a single TextBox element first.".into());
        };
        let mut candidate = self.project.clone();
        let sym_id = self.symbol_id().to_owned();
        let frame = self.frame;
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return Err("Active symbol not found.".into());
        };
        let mut updated = false;
        for layer in &mut sym.timeline.layers {
            let Some(key_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame) else {
                continue;
            };
            if let Some(el) = layer.keyframes[key_idx]
                .elements
                .iter_mut()
                .find(|e| e.id == el_id)
            {
                let Some(mut state) = TextFieldState::from_kind(&el.kind) else {
                    return Err("Selected element is not a text field.".into());
                };
                state
                    .set_html_text(html_source, None, state.condense_white)
                    .map_err(|e| e.to_string())?;
                state.write_to_kind(&mut el.kind);
                if let ElementKind::TextBox { format, .. } = &mut el.kind {
                    let mut opts = format.field_options().unwrap_or_default();
                    opts.html = true;
                    format.set_field_options(Some(opts));
                }
                updated = true;
                break;
            }
        }
        if !updated {
            return Err("Selected TextBox was not found on the current frame.".into());
        }
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("HTML text", |s| {
            s.project = candidate;
        });
        Ok(())
    }

    /// Serialize the selected `TextBox` (including its rich-text `format_runs` and links) to HTML markup.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn selected_text_to_html(&self) -> Option<String> {
        let el = self.selected_element()?;
        let state = TextFieldState::from_kind(&el.kind)?;
        Some(state.to_html_text())
    }

    /// Update the authored `AuthoredTextFieldOptions` on the selected `TextBox` in one Undo step.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_selected_text_field_options(
        &mut self,
        options: AuthoredTextFieldOptions,
    ) -> Result<(), String> {
        self.finish_edits();
        let Some(el_id) = self.selection.single().map(|t| t.element.clone()) else {
            return Err("Select a single TextBox element first.".into());
        };
        let mut candidate = self.project.clone();
        let sym_id = self.symbol_id().to_owned();
        let frame = self.frame;
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return Err("Active symbol not found.".into());
        };
        let mut updated = false;
        for layer in &mut sym.timeline.layers {
            let Some(key_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame) else {
                continue;
            };
            if let Some(el) = layer.keyframes[key_idx]
                .elements
                .iter_mut()
                .find(|e| e.id == el_id)
                && let ElementKind::TextBox { format, .. } = &mut el.kind
            {
                format.set_field_options((!options.is_default()).then_some(options));
                updated = true;
                break;
            }
        }
        if !updated {
            return Err("Selected TextBox was not found on the current frame.".into());
        }
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("Text options", |s| {
            s.project = candidate;
        });
        Ok(())
    }

    /// Convert the selected `Text` or `TextBox` element into editable vector `Path` outlines in one Undo step.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn convert_selected_text_to_outlines(&mut self) -> Result<usize, String> {
        self.finish_edits();
        let Some(el_id) = self.selection.single().map(|t| t.element.clone()) else {
            return Err("Select a text element first.".into());
        };
        let source = self.selected_element().cloned().ok_or_else(|| {
            "Selected text element was not found on the current frame.".to_owned()
        })?;
        let state = TextFieldState::from_kind(&source.kind)
            .ok_or_else(|| "Selected element is not text.".to_owned())?;
        let fragments = self.fonts.field_outline_fragments(&state)?;
        if fragments.is_empty() {
            return Err("Text has no visible glyph outlines.".into());
        }
        let mut outlines: Vec<([u8; 4], Vec<neoflash_core::PathCommand>)> = Vec::new();
        for fragment in fragments {
            if let Some((_, commands)) = outlines
                .last_mut()
                .filter(|(fill, _)| *fill == fragment.fill)
            {
                commands.extend(fragment.commands);
            } else {
                outlines.push((fragment.fill, fragment.commands));
            }
        }
        if outlines.len() > 1
            && (!source.scripts.is_empty()
                || source.accessibility.is_some()
                || source.cache_as_bitmap
                || source.blend_mode != neoflash_core::BlendMode::Normal
                || !source.filters.is_empty())
        {
            return Err("Mixed-colour text with scripts, accessibility, filters, blending or bitmap caching cannot be split into separate outline paths without changing its behavior.".into());
        }
        let cmd_count = outlines.iter().map(|(_, commands)| commands.len()).sum();
        let mut candidate = self.project.clone();
        let sym_id = self.symbol_id().to_owned();
        let frame = self.frame;
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return Err("Active symbol not found.".into());
        };
        let mut replacement_targets = Vec::new();
        for layer in &mut sym.timeline.layers {
            let Some(key_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame) else {
                continue;
            };
            let layer_id = layer.id.clone();
            let elements = &mut layer.keyframes[key_idx].elements;
            if let Some(at) = elements.iter().position(|element| element.id == el_id) {
                let mut replacements = Vec::with_capacity(outlines.len());
                for (index, (fill, commands)) in outlines.iter().enumerate() {
                    let mut replacement = source.clone();
                    if index > 0 {
                        replacement.id = neoflash_core::new_id("object");
                        replacement.name.clear();
                        replacement.scripts.clear();
                        replacement.accessibility = None;
                    }
                    replacement.transform.primitive = None;
                    replacement.kind = ElementKind::Path {
                        commands: commands.clone(),
                        fill_rule: FillRule::NonZero,
                        fill: *fill,
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    };
                    replacement_targets.push(crate::selection::SelectionTarget {
                        layer: layer_id.clone(),
                        element: replacement.id.clone(),
                    });
                    replacements.push(replacement);
                }
                elements.splice(at..=at, replacements);
                break;
            }
        }
        if replacement_targets.is_empty() {
            return Err("Selected text element was not found on the current frame.".into());
        }
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("Convert text to outlines", |s| {
            s.project = candidate;
            s.set_selection(replacement_targets);
        });
        Ok(cmd_count)
    }

    /// Sets the selected authored text field's `auto_size` mode (`"none"`, `"left"`, `"center"`, `"right"`)
    /// and resizes/anchors its bounding box to fit its natural text layout in a single Undo step.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_selected_text_auto_size(&mut self, mode: &str) -> Result<(f64, f64), String> {
        self.finish_edits();
        let normalized = mode.trim().to_ascii_lowercase();
        if !matches!(normalized.as_str(), "none" | "left" | "center" | "right") {
            return Err(format!("Unsupported text auto-size mode: {mode}"));
        }
        let sel = self
            .selection
            .single()
            .cloned()
            .ok_or_else(|| "Select a text element first.".to_string())?;
        let mut candidate = self.project.clone();
        let sym_id = self.symbol_id().to_owned();
        let frame = self.frame;
        let sym = candidate
            .symbol_mut(&sym_id)
            .ok_or_else(|| "Active symbol not found.".to_string())?;
        let mut new_size = None;
        let measure_segments = |segments: &[String],
                                seg_format: &TextFormat|
         -> Result<Vec<f64>, neoflash_core::TextError> {
            Ok(segments
                .iter()
                .map(|s| s.chars().count() as f64 * seg_format.size * 0.56)
                .collect())
        };
        for layer in &mut sym.timeline.layers {
            if layer.id != sel.layer {
                continue;
            }
            let Some(key_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame) else {
                continue;
            };
            if let Some(el) = layer.keyframes[key_idx]
                .elements
                .iter_mut()
                .find(|e| e.id == sel.element)
            {
                let mut state = TextFieldState::from_kind(&el.kind)
                    .ok_or_else(|| "Selected element is not a text object.".to_string())?;
                let old_width = state.box_size.map(|b| b.width).unwrap_or_else(|| {
                    layout_text(&state, measure_segments)
                        .map(|l| l.surface.width)
                        .unwrap_or(100.0)
                        .max(4.0)
                });
                state.as2_viewport.auto_size = normalized.clone();
                if normalized != "none" {
                    let layout =
                        layout_text(&state, measure_segments).map_err(|e| e.to_string())?;
                    let natural_w = layout.lines.iter().map(|l| l.width).fold(0.0_f64, f64::max);
                    let natural_h = layout
                        .lines
                        .last()
                        .map(|l| l.y + l.height)
                        .unwrap_or(state.format.size);
                    let target_w = if state.word_wrap && state.multiline {
                        old_width.max(4.0)
                    } else {
                        (natural_w + 4.0).max(8.0)
                    };
                    let target_h = (natural_h + 4.0).max(state.format.size + 4.0);
                    match normalized.as_str() {
                        "center" => {
                            el.transform.x += (old_width - target_w) * 0.5;
                        }
                        "right" => {
                            el.transform.x += old_width - target_w;
                        }
                        _ => {}
                    }
                    state.box_size = Some(neoflash_core::TextBoxSize {
                        width: target_w,
                        height: target_h,
                    });
                    new_size = Some((target_w, target_h));
                } else {
                    new_size = Some(
                        state
                            .box_size
                            .map(|b| (b.width, b.height))
                            .unwrap_or((old_width.max(8.0), (state.format.size + 4.0).max(8.0))),
                    );
                }
                state.write_to_kind(&mut el.kind);
                break;
            }
        }
        let size = new_size.ok_or_else(|| {
            "Selected text element was not found on the current frame.".to_string()
        })?;
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("Text auto-size", |s| {
            s.project = candidate;
        });
        Ok(size)
    }
}

fn family(p: &egui::Painter, format: &TextFormat) -> FontFamily {
    let named = crate::fonts::styled_family(&format.font, format.bold, format.italic);
    if p.ctx().fonts(|fonts| fonts.families().contains(&named)) {
        named
    } else {
        FontFamily::Proportional
    }
}

pub(crate) fn layout(p: &egui::Painter, kind: &ElementKind) -> Result<Arc<TextLayout>, String> {
    let ElementKind::TextBox { format, .. } = kind else {
        return Err("Expected text box".into());
    };
    let top_family = family(p, format);
    let ppp = p.ctx().pixels_per_point().to_bits();
    let cache = p.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<LayoutCache>>>(egui::Id::new(
            "native-text-layouts",
        ))
        .clone()
    });
    {
        let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(index) = cache
            .entries
            .iter()
            .position(|entry| entry.kind == *kind && entry.family == top_family && entry.ppp == ppp)
        {
            let entry = cache.entries.remove(index).expect("cached layout index");
            let result = entry.layout.clone();
            cache.entries.push_back(entry);
            return Ok(result);
        }
    }
    let field = TextFieldState::from_kind(kind).ok_or("Expected text box")?;
    let result = Arc::new(
        layout_text(&field, |segments, seg_format| {
            let seg_family = family(p, seg_format);
            Ok(segments
                .iter()
                .map(|text| {
                    p.layout_no_wrap(
                        text.clone(),
                        FontId::new(seg_format.size as f32, seg_family.clone()),
                        Color32::WHITE,
                    )
                    .size()
                    .x as f64
                })
                .collect())
        })
        .map_err(|error| error.to_string())?,
    );
    let bytes = field.text.len()
        + result
            .lines
            .iter()
            .map(|line| line.text.len() + 80)
            .sum::<usize>();
    if bytes <= CACHE_BYTES {
        let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
        while cache.entries.len() >= 32 || cache.bytes + bytes > CACHE_BYTES {
            let Some(entry) = cache.entries.pop_front() else {
                break;
            };
            cache.bytes -= entry.bytes;
        }
        cache.bytes += bytes;
        cache.entries.push_back(CachedLayout {
            kind: kind.clone(),
            family: top_family,
            ppp,
            layout: result.clone(),
            bytes,
        });
    }
    Ok(result)
}

pub(crate) fn paint(
    p: &egui::Painter,
    kind: &ElementKind,
    color: Color32,
    screen: impl Fn(Pos2) -> Pos2,
) -> Result<(), String> {
    let ElementKind::TextBox {
        format,
        width,
        height,
        ..
    } = kind
    else {
        return Err("Expected text box".into());
    };
    let result = layout(p, kind)?;
    let default_family = family(p, format);
    let clip = Rect::from_min_size(Pos2::ZERO, Vec2::new(*width as f32, *height as f32));
    let alpha = color.a();
    let rgb_to_color32 = |rgb: u32| {
        Color32::from_rgba_unmultiplied(
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
            alpha,
        )
    };
    if let Some(bg) = result.surface.background {
        let bg_col = rgb_to_color32(bg);
        let corners = [
            screen(Pos2::new(0.0, 0.0)),
            screen(Pos2::new(*width as f32, 0.0)),
            screen(Pos2::new(*width as f32, *height as f32)),
            screen(Pos2::new(0.0, *height as f32)),
        ];
        p.add(Shape::convex_polygon(
            corners.to_vec(),
            bg_col,
            egui::Stroke::NONE,
        ));
    }
    if let Some(bd) = result.surface.border {
        let bd_col = rgb_to_color32(bd);
        let corners = [
            screen(Pos2::new(0.0, 0.0)),
            screen(Pos2::new(*width as f32, 0.0)),
            screen(Pos2::new(*width as f32, *height as f32)),
            screen(Pos2::new(0.0, *height as f32)),
            screen(Pos2::new(0.0, 0.0)),
        ];
        p.add(Shape::line(
            corners.to_vec(),
            egui::Stroke::new(1.0_f32, bd_col),
        ));
    }
    if !result.runs.is_empty() {
        for run in &result.runs {
            let run_fmt = run.format.as_ref().unwrap_or(format);
            let run_h = run_fmt.size * 1.2;
            if run.y >= clip.max.y as f64 || run.y + run_h <= 0. {
                continue;
            }
            let run_family = family(p, run_fmt);
            let run_color = if run.format.is_some() {
                rgb_to_color32(run_fmt.color)
            } else {
                color
            };
            let galley = p.layout_no_wrap(
                run.text.clone(),
                FontId::new(run_fmt.size as f32, run_family),
                run_color,
            );
            let shape = Shape::Text(egui::epaint::TextShape::new(
                Pos2::new(run.x as f32, run.y as f32),
                galley,
                run_color,
            ));
            for primitive in p.ctx().tessellate(
                vec![egui::epaint::ClippedShape {
                    clip_rect: Rect::EVERYTHING,
                    shape,
                }],
                p.ctx().pixels_per_point(),
            ) {
                if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                    let mut mesh = clip_mesh(mesh, clip);
                    for vertex in &mut mesh.vertices {
                        vertex.pos = screen(vertex.pos);
                    }
                    p.add(Shape::mesh(mesh));
                }
            }
        }
    } else {
        for line in &result.lines {
            if line.y >= clip.max.y as f64 || line.y + line.height <= 0. {
                continue;
            }
            let galley = p.layout_no_wrap(
                line.text.clone(),
                FontId::new(format.size as f32, default_family.clone()),
                color,
            );
            let shape = Shape::Text(egui::epaint::TextShape::new(
                Pos2::new(line.x as f32, line.y as f32),
                galley,
                color,
            ));
            for primitive in p.ctx().tessellate(
                vec![egui::epaint::ClippedShape {
                    clip_rect: Rect::EVERYTHING,
                    shape,
                }],
                p.ctx().pixels_per_point(),
            ) {
                if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                    let mut mesh = clip_mesh(mesh, clip);
                    for vertex in &mut mesh.vertices {
                        vertex.pos = screen(vertex.pos);
                    }
                    p.add(Shape::mesh(mesh));
                }
            }
        }
    }
    for dec in &result.decorations {
        let dec_rect = Rect::from_min_size(
            Pos2::new(dec.x as f32, dec.y as f32),
            Vec2::new(dec.width as f32, dec.height.max(1.0) as f32),
        )
        .intersect(clip);
        if dec_rect.is_positive() {
            let dec_col = dec.color.map_or(color, rgb_to_color32);
            let corners = [
                screen(dec_rect.left_top()),
                screen(dec_rect.right_top()),
                screen(dec_rect.right_bottom()),
                screen(dec_rect.left_bottom()),
            ];
            p.add(Shape::convex_polygon(
                corners.to_vec(),
                dec_col,
                egui::Stroke::NONE,
            ));
        }
    }
    Ok(())
}

// Clip in field coordinates before the affine transform, including rotated boxes.
fn clip_mesh(mesh: egui::Mesh, rect: Rect) -> egui::Mesh {
    let mut output = egui::Mesh::with_texture(mesh.texture_id);
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let mut polygon: Vec<_> = triangle
            .iter()
            .map(|index| mesh.vertices[*index as usize])
            .collect();
        for (axis, edge, lower) in [
            (0, rect.min.x, true),
            (0, rect.max.x, false),
            (1, rect.min.y, true),
            (1, rect.max.y, false),
        ] {
            if polygon.is_empty() {
                break;
            }
            let old = std::mem::take(&mut polygon);
            let inside = |vertex: &egui::epaint::Vertex| {
                if lower {
                    vertex.pos[axis] >= edge
                } else {
                    vertex.pos[axis] <= edge
                }
            };
            let mut previous = *old.last().expect("nonempty polygon");
            for current in old {
                if inside(&previous) != inside(&current) {
                    let t = (edge - previous.pos[axis]) / (current.pos[axis] - previous.pos[axis]);
                    let a = previous.color.to_array();
                    let b = current.color.to_array();
                    let c = std::array::from_fn::<_, 4, _>(|i| {
                        (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8
                    });
                    polygon.push(egui::epaint::Vertex {
                        pos: previous.pos.lerp(current.pos, t),
                        uv: previous.uv.lerp(current.uv, t),
                        color: Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3]),
                    });
                }
                if inside(&current) {
                    polygon.push(current);
                }
                previous = current;
            }
        }
        if polygon.len() >= 3 {
            let start = output.vertices.len() as u32;
            for index in 1..polygon.len() as u32 - 1 {
                output
                    .indices
                    .extend_from_slice(&[start, start + index, start + index + 1]);
            }
            output.vertices.extend(polygon);
        }
    }
    output
}
