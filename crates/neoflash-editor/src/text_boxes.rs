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
    pub(crate) fn focus_selected_text_editor(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.insert_temp(egui::Id::new("properties-tool-tab"), false);
            data.insert_temp(egui::Id::new("nf2-focus-text-edit"), true);
        });
    }

    pub(crate) fn text_box_properties(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        kind: &mut ElementKind,
    ) -> Option<egui::Id> {
        use crate::paint_controls::{PropertyChanges, numeric_row, property_row};
        let ElementKind::TextBox {
            text,
            width,
            height,
            format,
            word_wrap,
            multiline,
        } = kind
        else {
            return None;
        };
        let mut changes = PropertyChanges::default();
        ui.strong("Text box");
        changes.note(numeric_row(ui, "Width", width, 0. ..=10000., 1.));
        changes.note(numeric_row(ui, "Height", height, 0. ..=10000., 1.));
        changes.note(ui.checkbox(multiline, "Multiline"));
        changes.note(ui.checkbox(word_wrap, "Word wrap"));
        let focus_text = ctx.data_mut(|data| {
            data.remove_temp::<bool>(egui::Id::new("nf2-focus-text-edit"))
                .unwrap_or(false)
        });
        egui::ScrollArea::vertical()
            .id_salt("text-box-content")
            .max_height(120.)
            .themed()
            .show(ui, |ui| {
                let response = ui.add_sized(
                    [ui.available_width(), 74.],
                    egui::TextEdit::multiline(text).desired_rows(3),
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
        let before_font = format.font.clone();
        let mut response = property_row(ui, "Font", |ui| {
            egui::ComboBox::from_id_salt("text_box_font")
                .selected_text(&format.font)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.font_filter)
                            .hint_text("Find font")
                            .desired_width(220.),
                    );
                    for family in &self.fonts.families {
                        if self.font_filter.is_empty()
                            || family
                                .to_lowercase()
                                .contains(&self.font_filter.to_lowercase())
                        {
                            ui.selectable_value(&mut format.font, family.clone(), family);
                        }
                    }
                })
                .response
        });
        if format.font != before_font {
            response.mark_changed();
        }
        changes.note(response);
        let size_response = numeric_row(ui, "Size", &mut format.size, 0.1..=10000., 1.);
        let min_leading = (-format.size * 1.2 + 0.1).max(-10000.);
        if size_response.changed() && format.leading < min_leading {
            format.leading = min_leading;
        }
        changes.note(size_response);
        changes.note(ui.checkbox(&mut format.bold, "Bold"));
        changes.note(ui.checkbox(&mut format.italic, "Italic"));
        changes.note(property_row(ui, "Color", |ui| {
            let mut rgb = [
                (format.color >> 16) as u8,
                (format.color >> 8) as u8,
                format.color as u8,
            ];
            let response = ui.color_edit_button_srgb(&mut rgb);
            if response.changed() {
                format.color = u32::from_be_bytes([0, rgb[0], rgb[1], rgb[2]]);
            }
            ui.label(
                egui::RichText::new(format!("#{:06X}", format.color))
                    .monospace()
                    .size(crate::theme::SECONDARY_FONT),
            );
            response
        }));
        changes.note(numeric_row(
            ui,
            "Leading",
            &mut format.leading,
            min_leading..=10000.,
            0.25,
        ));
        let before_align = format.align;
        let mut response = property_row(ui, "Alignment", |ui| {
            egui::ComboBox::from_id_salt("text_box_align")
                .selected_text(format!("{:?}", format.align))
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (neoflash_core::TextAlign::Left, "Left"),
                        (neoflash_core::TextAlign::Center, "Center"),
                        (neoflash_core::TextAlign::Right, "Right"),
                    ] {
                        ui.selectable_value(&mut format.align, value, label);
                    }
                })
                .response
        });
        if format.align != before_align {
            response.mark_changed();
        }
        changes.note(response);
        if let Err(error) = self
            .fonts
            .load_style(ctx, &format.font, format.bold, format.italic)
            && self.workspace.text_prefs.substitution_warning
        {
            ui.colored_label(crate::theme::UiPalette::current(ctx).error_text, error);
        }
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
        let mut candidate = self.project.clone();
        let sym_id = self.symbol_id().to_owned();
        let frame = self.frame;
        let Some(sym) = candidate.symbol_mut(&sym_id) else {
            return Err("Active symbol not found.".into());
        };
        let mut cmd_count = 0usize;
        for layer in &mut sym.timeline.layers {
            let Some(key_idx) = layer.keyframes.iter().rposition(|k| k.frame <= frame) else {
                continue;
            };
            if let Some(el) = layer.keyframes[key_idx]
                .elements
                .iter_mut()
                .find(|e| e.id == el_id)
            {
                let Some(state) = TextFieldState::from_kind(&el.kind) else {
                    return Err("Selected element is not text.".into());
                };
                let cmds = TextFieldState::static_text_to_outline_commands(
                    &state.text,
                    &state.format.font,
                    state.format.size,
                    None,
                );
                if cmds.is_empty() {
                    return Err("Text has no visible glyph outlines.".into());
                }
                cmd_count = cmds.len();
                let rgb = state.format.color;
                let fill = [
                    ((rgb >> 16) & 0xff) as u8,
                    ((rgb >> 8) & 0xff) as u8,
                    (rgb & 0xff) as u8,
                    state.alpha,
                ];
                el.kind = ElementKind::Path {
                    commands: cmds,
                    fill_rule: FillRule::NonZero,
                    fill,
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                };
                break;
            }
        }
        if cmd_count == 0 {
            return Err("Selected text element was not found on the current frame.".into());
        }
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("Convert text to outlines", |s| {
            s.project = candidate;
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
