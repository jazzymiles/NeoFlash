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
            data.insert_temp(egui::Id::new("nf2-focus-text-edit"), true);
        });
    }

    /// Flash CS5 `Properties — Text` `Character` section (`PR-01`, NF2 `Panel / Properties — Text`).
    pub(crate) fn text_box_properties(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        kind: &mut ElementKind,
        alpha: &mut f64,
    ) -> Option<egui::Id> {
        use crate::paint_controls::{PropertyChanges, color_row, two_column_row};
        use crate::ui_icons::{
            TextPropertyGlyph, text_optional_color_swatch, text_property_icon_button,
        };
        use neoflash_ui::{components as nf, tokens::space};
        let ElementKind::TextBox { text, format, .. } = kind else {
            return None;
        };
        let mut changes = PropertyChanges::default();
        let mut field_options = format.field_options().unwrap_or_default();
        let before_options = field_options.clone();
        let mut synthetic_id: Option<egui::Id> = None;

        // Compact 2-row text editor (preserving keyboard focus for newly created stage text).
        let focus_text = ctx.data_mut(|data| {
            data.remove_temp::<bool>(egui::Id::new("nf2-focus-text-edit"))
                .unwrap_or(false)
        });
        egui::ScrollArea::vertical()
            .id_salt("text-box-content")
            .max_height(46.)
            .themed()
            .show(ui, |ui| {
                let response = ui.add_sized(
                    [ui.available_width(), 38.],
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

        // Row 1: Text field type (Static / Dynamic / Input) + Orientation icon button (Flash CS5 header).
        let before_font = format.font.clone();
        let current_type_idx = match (field_options.field_type, field_options.selectable) {
            (neoflash_core::TextFieldType::Input, _) => 2usize,
            (neoflash_core::TextFieldType::Dynamic, false) => 0usize,
            _ => 1usize,
        };
        let type_labels = ["Static text", "Dynamic text", "Input text"];
        let mut picked_type = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let orient_btn_w = crate::theme::control_h().min(22.0);
            let type_w = (ui.available_width() - orient_btn_w - space::S2).max(100.0);
            let (mut type_resp, _) =
                nf::Select::new("text_box_type", type_labels[current_type_idx])
                    .width(type_w)
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

            let orient_glyph = match field_options.orientation.as_str() {
                "vertical_ltr" => TextPropertyGlyph::OrientationVerticalLtr,
                "vertical_rtl" => TextPropertyGlyph::OrientationVerticalRtl,
                _ => TextPropertyGlyph::OrientationHorizontal,
            };
            let orient_btn = text_property_icon_button(
                ui,
                orient_glyph,
                field_options.orientation != "horizontal" && !field_options.orientation.is_empty(),
                true,
                "Change orientation of text",
            );
            let orient_id = orient_btn.id;
            nf::popup_below(&orient_btn, 210.0, |ui| {
                for (code, label) in [
                    ("horizontal", "Horizontal"),
                    ("vertical_ltr", "Vertical, Left to Right"),
                    ("vertical_rtl", "Vertical, Right to Left"),
                ] {
                    let cur = if field_options.orientation.is_empty() {
                        "horizontal"
                    } else {
                        field_options.orientation.as_str()
                    };
                    if ui
                        .add(nf::MenuItem::new(label).checked(cur == code))
                        .clicked()
                    {
                        field_options.orientation = code.to_owned();
                        synthetic_id = Some(orient_id);
                        ui.close();
                    }
                }
            });
        });

        // Row 2: Family selector (full width, matching Flash CS5).
        let current_font = format.font.clone();
        let (mut f_resp, _) = nf::Select::new("text_box_font", &current_font)
            .width(ui.available_width())
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
        changes.note(f_resp);

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
        let mut style_control = |ui: &mut egui::Ui, width: f32| {
            let mut style_changed = false;
            let (mut style_resp, _) =
                nf::Select::new("text_box_style", style_options[current_style_idx].1)
                    .width(width)
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
            style_resp
        };
        let mut embed_controls = |ui: &mut egui::Ui,
                                  enable_after_confirm: &mut bool,
                                  open_embedding: &mut bool,
                                  changes: &mut PropertyChanges| {
            let embed_response = ui.add(nf::Checkbox::new(
                &mut field_options.embed_fonts,
                "Embed font",
            ));
            *enable_after_confirm = embed_response.changed() && field_options.embed_fonts;
            if *enable_after_confirm {
                // Enabling is transactional with the dialog: Cancel must not leave the field in
                // embedded-only mode without a matching embedded font resource.
                field_options.embed_fonts = false;
            }
            changes.note(embed_response);

            let embedding_response = ui
                .add(nf::Button::new("Embedding…"))
                .on_hover_text("Set font embedding options for this text field");
            *open_embedding = *enable_after_confirm || embedding_response.clicked();
        };
        if ui.available_width() < 312.0 {
            let width = ui.available_width();
            changes.note(style_control(ui, width));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                embed_controls(
                    ui,
                    &mut enable_after_confirm,
                    &mut open_embedding,
                    &mut changes,
                );
            });
        } else {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                let style_w = (ui.available_width() - 184.0).max(84.0);
                changes.note(style_control(ui, style_w));
                embed_controls(
                    ui,
                    &mut enable_after_confirm,
                    &mut open_embedding,
                    &mut changes,
                );
            });
        }
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
        let (size_response, spacing_response) = two_column_row(
            ui,
            80.0,
            |ui, width| {
                ui.add(
                    nf::Field::new(&mut format.size, "Size")
                        .prefix("Size")
                        .unit("pt")
                        .decimals(1)
                        .range(0.1..=10000.)
                        .width(width),
                )
            },
            |ui, width| {
                ui.add(
                    nf::Field::new(&mut format.letter_spacing, "Letter spacing")
                        .prefix("Tracking")
                        .unit("px")
                        .decimals(1)
                        .range(-1000.0..=1000.0)
                        .width(width),
                )
            },
        );
        let next_min_leading = (-format.size * 1.2 + 0.1).max(-10000.);
        if size_response.changed() && format.leading < next_min_leading {
            format.leading = next_min_leading;
        }
        changes.note(size_response);
        changes.note(spacing_response);
        let _ = min_leading;

        // Row 4: Color swatch/hex/alpha + Highlight optional color swatch + Auto kern on 1 compact row.
        let mut rgba = [
            ((format.color >> 16) & 0xff) as u8,
            ((format.color >> 8) & 0xff) as u8,
            (format.color & 0xff) as u8,
            (*alpha * 255.0).round().clamp(0.0, 255.0) as u8,
        ];
        let mut auto_kern = format.kerning.unwrap_or(false);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let color_w = (ui.available_width() - 134.0 - space::S2 * 2.0).max(108.0);
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
            if text_optional_color_swatch(
                ui,
                "text-box-highlight",
                &mut field_options.highlight,
                &mut field_options.highlight_color,
                "Highlight color",
            ) {
                synthetic_id = Some(ui.make_persistent_id("text-box-highlight-changed"));
            }
            let kern_resp = ui.add(nf::Checkbox::new(&mut auto_kern, "Auto kern"));
            if kern_resp.changed() {
                format.kerning = auto_kern.then_some(true);
            }
            changes.note(kern_resp);
        });

        // Row 5: Anti-alias mode + Rotation + [Underline] [Strikethrough] [Superscript] [Subscript] buttons.
        let current_aa_idx = match (
            field_options.anti_alias_type.as_str(),
            field_options.grid_fit_type.as_str(),
            field_options.sharpness != 0.0 || field_options.thickness != 0.0,
        ) {
            ("advanced", _, true) => 4usize,
            ("advanced", "subpixel", false) => 3usize,
            ("advanced", "none", false) => 2usize,
            ("normal", "none", _) => 1usize,
            _ => 0usize,
        };
        let aa_options = [
            (0usize, "Anti-alias: Readability"),
            (1usize, "Anti-alias: Animation"),
            (2usize, "Anti-alias: Bitmap text"),
            (3usize, "Anti-alias: Subpixel"),
            (4usize, "Anti-alias: Custom"),
        ];
        let rot_options = [
            ("auto", "Rot: Auto"),
            ("0", "Rot: 0°"),
            ("270", "Rot: 270°"),
        ];
        let cur_rot_label = rot_options
            .iter()
            .find(|(k, _)| *k == field_options.glyph_rotation)
            .map(|(_, l)| *l)
            .unwrap_or("Rot: Auto");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::S1;
            let icon_group_w = 4.0 * crate::theme::control_h().min(22.0) + 3.0 * space::S1;
            let rot_w = 78.0;
            let aa_w = (ui.available_width() - rot_w - icon_group_w - space::S1 * 2.0).max(76.0);
            let mut picked_aa = None;
            let (mut aa_resp, _) = nf::Select::new("text_box_aa", aa_options[current_aa_idx].1)
                .width(aa_w)
                .show(ui, |ui| {
                    for (idx, label) in aa_options {
                        if ui
                            .add(nf::MenuItem::new(label).checked(idx == current_aa_idx))
                            .clicked()
                        {
                            picked_aa = Some(idx);
                            ui.close();
                        }
                    }
                });
            if let Some(idx) = picked_aa {
                match idx {
                    0 => {
                        field_options.anti_alias_type = "normal".into();
                        field_options.grid_fit_type = "pixel".into();
                        field_options.sharpness = 0.0;
                        field_options.thickness = 0.0;
                    }
                    1 => {
                        field_options.anti_alias_type = "normal".into();
                        field_options.grid_fit_type = "none".into();
                        field_options.sharpness = 0.0;
                        field_options.thickness = 0.0;
                    }
                    2 => {
                        field_options.anti_alias_type = "advanced".into();
                        field_options.grid_fit_type = "none".into();
                        field_options.sharpness = 0.0;
                        field_options.thickness = 0.0;
                    }
                    3 => {
                        field_options.anti_alias_type = "advanced".into();
                        field_options.grid_fit_type = "subpixel".into();
                        field_options.sharpness = 0.0;
                        field_options.thickness = 0.0;
                    }
                    _ => {
                        field_options.anti_alias_type = "advanced".into();
                        field_options.grid_fit_type = "pixel".into();
                        if field_options.sharpness == 0.0 && field_options.thickness == 0.0 {
                            field_options.sharpness = 100.0;
                        }
                    }
                }
                aa_resp.mark_changed();
            }
            changes.note(aa_resp);

            let (mut rot_resp, _) = nf::Select::new("text_box_glyph_rot", cur_rot_label)
                .width(rot_w)
                .show(ui, |ui| {
                    for (code, label) in rot_options {
                        if ui
                            .add(
                                nf::MenuItem::new(label)
                                    .checked(field_options.glyph_rotation == code),
                            )
                            .clicked()
                        {
                            field_options.glyph_rotation = code.to_owned();
                            ui.close();
                        }
                    }
                });
            if field_options.glyph_rotation != before_options.glyph_rotation {
                rot_resp.mark_changed();
            }
            changes.note(rot_resp);

            let mut u_resp = text_property_icon_button(
                ui,
                TextPropertyGlyph::Underline,
                format.underline,
                true,
                "Underline",
            );
            if u_resp.clicked() {
                format.underline = !format.underline;
                u_resp.mark_changed();
            }
            changes.note(u_resp);

            let mut s_resp = text_property_icon_button(
                ui,
                TextPropertyGlyph::Strikethrough,
                field_options.strikethrough,
                true,
                "Strikethrough",
            );
            if s_resp.clicked() {
                field_options.strikethrough = !field_options.strikethrough;
                s_resp.mark_changed();
            }
            changes.note(s_resp);

            let mut sup_resp = text_property_icon_button(
                ui,
                TextPropertyGlyph::Superscript,
                field_options.superscript,
                true,
                "Superscript",
            );
            if sup_resp.clicked() {
                field_options.superscript = !field_options.superscript;
                if field_options.superscript {
                    field_options.subscript = false;
                }
                sup_resp.mark_changed();
            }
            changes.note(sup_resp);

            let mut sub_resp = text_property_icon_button(
                ui,
                TextPropertyGlyph::Subscript,
                field_options.subscript,
                true,
                "Subscript",
            );
            if sub_resp.clicked() {
                field_options.subscript = !field_options.subscript;
                if field_options.subscript {
                    field_options.superscript = false;
                }
                sub_resp.mark_changed();
            }
            changes.note(sub_resp);
        });

        if current_aa_idx == 4 {
            let (sharpness, thickness) = two_column_row(
                ui,
                80.0,
                |ui, width| {
                    ui.add(
                        nf::Field::new(&mut field_options.sharpness, "Sharpness")
                            .prefix("Sharpness")
                            .decimals(0)
                            .range(-400.0..=400.0)
                            .width(width),
                    )
                },
                |ui, width| {
                    ui.add(
                        nf::Field::new(&mut field_options.thickness, "Thickness")
                            .prefix("Thickness")
                            .decimals(0)
                            .range(-200.0..=200.0)
                            .width(width),
                    )
                },
            );
            changes.note(sharpness);
            changes.note(thickness);
        }

        // Row 6: Compact character & field toggles on 1 row (Selectable, HTML, Border).
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            changes.note(ui.add(nf::Checkbox::new(
                &mut field_options.selectable,
                "Selectable",
            )));
            changes.note(ui.add(nf::Checkbox::new(&mut field_options.html, "HTML")));
            changes.note(ui.add(nf::Checkbox::new(&mut field_options.border, "Border")));
        });

        if field_options != before_options {
            format.set_field_options((!field_options.is_default()).then_some(field_options));
            if changes.control.is_none() {
                changes.control =
                    synthetic_id.or_else(|| Some(ui.make_persistent_id("text-box-options-edit")));
            }
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

    /// Flash CS5 `Advanced character`, `Paragraph`, `Container and flow`, and `Options` collapsible
    /// sections for `Properties — Text` (`PR-01`).
    pub(crate) fn text_box_paragraph_and_options_ui(
        &mut self,
        ui: &mut egui::Ui,
        kind: &mut ElementKind,
    ) -> Option<egui::Id> {
        use crate::paint_controls::{PropertyChanges, text_row, two_column_row};
        use crate::ui_icons::{
            TextPropertyGlyph, text_optional_color_swatch, text_property_badge,
            text_property_icon_button,
        };
        use neoflash_ui::{
            components::{self as nf, SectionHeader},
            icons::i16,
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
        let min_leading = (-format.size * 1.2 + 0.1).max(-10000.);

        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = crate::theme::control_h();

            // 1. ADVANCED CHARACTER section (Flash CS5)
            SectionHeader::new("Advanced character")
                .id_salt("text-box-advanced-character")
                .default_open(false)
                .show(ui, |ui| {
                    let mut field_options = format.field_options().unwrap_or_default();
                    let before_opts = field_options.clone();

                    // Row 1: Case + Ligatures
                    let case_opts = [
                        ("default", "Case: Default"),
                        ("uppercase", "Case: Uppercase"),
                        ("lowercase", "Case: Lowercase"),
                        ("caps_to_small_caps", "Case: Small Caps"),
                        ("lowercase_to_small_caps", "Case: Lower to Small"),
                    ];
                    let lig_opts = [
                        ("minimum", "Ligatures: Minimum"),
                        ("common", "Ligatures: Common"),
                        ("uncommon", "Ligatures: Uncommon"),
                        ("exotic", "Ligatures: Exotic"),
                    ];
                    let cur_case = case_opts
                        .iter()
                        .find(|(k, _)| *k == field_options.typographic_case)
                        .map(|(_, l)| *l)
                        .unwrap_or("Case: Default");
                    let cur_lig = lig_opts
                        .iter()
                        .find(|(k, _)| *k == field_options.ligatures)
                        .map(|(_, l)| *l)
                        .unwrap_or("Ligatures: Common");
                    let (mut case_resp, mut lig_resp) =
                        two_column_row(
                            ui,
                            80.0,
                            |ui, width| {
                                nf::Select::new("text_box_case", cur_case)
                                    .width(width)
                                    .show(ui, |ui| {
                                        for (code, label) in case_opts {
                                            if ui
                                                .add(nf::MenuItem::new(label).checked(
                                                    field_options.typographic_case == code,
                                                ))
                                                .clicked()
                                            {
                                                field_options.typographic_case = code.to_owned();
                                                ui.close();
                                            }
                                        }
                                    })
                                    .0
                            },
                            |ui, width| {
                                nf::Select::new("text_box_ligatures", cur_lig)
                                    .width(width)
                                    .show(ui, |ui| {
                                        for (code, label) in lig_opts {
                                            if ui
                                                .add(
                                                    nf::MenuItem::new(label)
                                                        .checked(field_options.ligatures == code),
                                                )
                                                .clicked()
                                            {
                                                field_options.ligatures = code.to_owned();
                                                ui.close();
                                            }
                                        }
                                    })
                                    .0
                            },
                        );
                    if field_options.typographic_case != before_opts.typographic_case {
                        case_resp.mark_changed();
                    }
                    if field_options.ligatures != before_opts.ligatures {
                        lig_resp.mark_changed();
                    }
                    changes.note(case_resp);
                    changes.note(lig_resp);

                    // Row 2: Digit Case + Digit Width
                    let dcase_opts = [
                        ("default", "Digits: Default"),
                        ("lining", "Digits: Lining"),
                        ("old_style", "Digits: Old Style"),
                    ];
                    let dwidth_opts = [
                        ("default", "Width: Default"),
                        ("proportional", "Width: Proportional"),
                        ("tabular", "Width: Tabular"),
                    ];
                    let cur_dc = dcase_opts
                        .iter()
                        .find(|(k, _)| *k == field_options.digit_case)
                        .map(|(_, l)| *l)
                        .unwrap_or("Digits: Default");
                    let cur_dw = dwidth_opts
                        .iter()
                        .find(|(k, _)| *k == field_options.digit_width)
                        .map(|(_, l)| *l)
                        .unwrap_or("Width: Default");
                    let (mut dc_resp, mut dw_resp) = two_column_row(
                        ui,
                        80.0,
                        |ui, width| {
                            nf::Select::new("text_box_digit_case", cur_dc)
                                .width(width)
                                .show(ui, |ui| {
                                    for (code, label) in dcase_opts {
                                        if ui
                                            .add(
                                                nf::MenuItem::new(label)
                                                    .checked(field_options.digit_case == code),
                                            )
                                            .clicked()
                                        {
                                            field_options.digit_case = code.to_owned();
                                            ui.close();
                                        }
                                    }
                                })
                                .0
                        },
                        |ui, width| {
                            nf::Select::new("text_box_digit_width", cur_dw)
                                .width(width)
                                .show(ui, |ui| {
                                    for (code, label) in dwidth_opts {
                                        if ui
                                            .add(
                                                nf::MenuItem::new(label)
                                                    .checked(field_options.digit_width == code),
                                            )
                                            .clicked()
                                        {
                                            field_options.digit_width = code.to_owned();
                                            ui.close();
                                        }
                                    }
                                })
                                .0
                        },
                    );
                    if field_options.digit_case != before_opts.digit_case {
                        dc_resp.mark_changed();
                    }
                    if field_options.digit_width != before_opts.digit_width {
                        dw_resp.mark_changed();
                    }
                    changes.note(dc_resp);
                    changes.note(dw_resp);

                    // Row 3: Line Break + Baseline Shift
                    let break_opts = [
                        ("auto", "Break: Auto"),
                        ("all", "Break: All"),
                        ("any", "Break: Any"),
                        ("no_break", "Break: No Break"),
                    ];
                    let cur_brk = break_opts
                        .iter()
                        .find(|(k, _)| *k == field_options.line_break)
                        .map(|(_, l)| *l)
                        .unwrap_or("Break: Auto");
                    let (mut break_resp, baseline_resp) = two_column_row(
                        ui,
                        80.0,
                        |ui, width| {
                            nf::Select::new("text_box_line_break", cur_brk)
                                .width(width)
                                .show(ui, |ui| {
                                    for (code, label) in break_opts {
                                        if ui
                                            .add(
                                                nf::MenuItem::new(label)
                                                    .checked(field_options.line_break == code),
                                            )
                                            .clicked()
                                        {
                                            field_options.line_break = code.to_owned();
                                            ui.close();
                                        }
                                    }
                                })
                                .0
                        },
                        |ui, width| {
                            ui.add(
                                nf::Field::new(&mut field_options.baseline_shift, "Baseline shift")
                                    .prefix("Baseline")
                                    .unit("pt")
                                    .decimals(1)
                                    .range(-720.0..=720.0)
                                    .width(width),
                            )
                        },
                    );
                    if field_options.line_break != before_opts.line_break {
                        break_resp.mark_changed();
                    }
                    changes.note(break_resp);
                    changes.note(baseline_resp);

                    // Row 4: Locale
                    let locale_opts = [
                        ("en_US", "Locale: English (en_US)"),
                        ("ru_RU", "Locale: Russian (ru_RU)"),
                        ("de_DE", "Locale: German (de_DE)"),
                        ("fr_FR", "Locale: French (fr_FR)"),
                        ("es_ES", "Locale: Spanish (es_ES)"),
                        ("ja_JP", "Locale: Japanese (ja_JP)"),
                        ("zh_CN", "Locale: Chinese (zh_CN)"),
                        ("ar_SA", "Locale: Arabic (ar_SA)"),
                    ];
                    let cur_loc = locale_opts
                        .iter()
                        .find(|(k, _)| *k == field_options.locale)
                        .map(|(_, l)| *l)
                        .unwrap_or("Locale: English (en_US)");
                    let (mut loc_resp, _) = nf::Select::new("text_box_locale", cur_loc)
                        .width(ui.available_width())
                        .show(ui, |ui| {
                            for (code, label) in locale_opts {
                                if ui
                                    .add(
                                        nf::MenuItem::new(label)
                                            .checked(field_options.locale == code),
                                    )
                                    .clicked()
                                {
                                    field_options.locale = code.to_owned();
                                    ui.close();
                                }
                            }
                        });
                    if field_options.locale != before_opts.locale {
                        loc_resp.mark_changed();
                    }
                    changes.note(loc_resp);

                    if field_options != before_opts {
                        format.set_field_options(
                            (!field_options.is_default()).then_some(field_options),
                        );
                    }
                });

            // 2. PARAGRAPH section (Flash CS5)
            SectionHeader::new("Paragraph")
                .id_salt("text-box-paragraph")
                .default_open(true)
                .show(ui, |ui| {
                    let mut field_options = format.field_options().unwrap_or_default();
                    let before_opts = field_options.clone();

                    // Row 1: Flash CS5 Format toolbar — Align Left / Center / Right / Justify Last Left / Justify All + Bullet + Direction LTR / RTL.
                    let cur_align = format.effective_align_mode().to_owned();
                    let cur_dir = if field_options.direction.is_empty() {
                        "ltr".to_owned()
                    } else {
                        field_options.direction.clone()
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S1;
                        for (mode, glyph, label) in [
                            ("left", TextPropertyGlyph::AlignLeft, "Align left"),
                            ("center", TextPropertyGlyph::AlignCenter, "Align center"),
                            ("right", TextPropertyGlyph::AlignRight, "Align right"),
                            (
                                "justify",
                                TextPropertyGlyph::JustifyLastLeft,
                                "Justify with last line aligned left",
                            ),
                            (
                                "justify_all",
                                TextPropertyGlyph::JustifyAll,
                                "Justify all lines",
                            ),
                        ] {
                            let is_sel = cur_align == mode
                                || (mode == "left" && cur_align == "start")
                                || (mode == "right" && cur_align == "end");
                            let mut btn = text_property_icon_button(ui, glyph, is_sel, true, label);
                            if btn.clicked() && !is_sel {
                                match mode {
                                    "left" => {
                                        format.align = neoflash_core::TextAlign::Left;
                                        format.set_align_mode(None);
                                    }
                                    "center" => {
                                        format.align = neoflash_core::TextAlign::Center;
                                        format.set_align_mode(None);
                                    }
                                    "right" => {
                                        format.align = neoflash_core::TextAlign::Right;
                                        format.set_align_mode(None);
                                    }
                                    "justify" => {
                                        format.align = neoflash_core::TextAlign::Left;
                                        format.set_align_mode(Some("justify".into()));
                                    }
                                    _ => {
                                        format.align = neoflash_core::TextAlign::Left;
                                        format.set_align_mode(Some("justify_all".into()));
                                    }
                                }
                                btn.mark_changed();
                            }
                            changes.note(btn);
                        }

                        let mut bullet_btn = text_property_icon_button(
                            ui,
                            TextPropertyGlyph::BulletList,
                            format.bullet,
                            true,
                            "Bullet list",
                        );
                        if bullet_btn.clicked() {
                            format.bullet = !format.bullet;
                            bullet_btn.mark_changed();
                        }
                        changes.note(bullet_btn);

                        let mut ltr_btn = text_property_icon_button(
                            ui,
                            TextPropertyGlyph::DirectionLtr,
                            cur_dir != "rtl",
                            true,
                            "Left-to-right direction",
                        );
                        if ltr_btn.clicked() && cur_dir == "rtl" {
                            field_options.direction = "ltr".into();
                            ltr_btn.mark_changed();
                        }
                        changes.note(ltr_btn);

                        let mut rtl_btn = text_property_icon_button(
                            ui,
                            TextPropertyGlyph::DirectionRtl,
                            cur_dir == "rtl",
                            true,
                            "Right-to-left direction",
                        );
                        if rtl_btn.clicked() && cur_dir != "rtl" {
                            field_options.direction = "rtl".into();
                            rtl_btn.mark_changed();
                        }
                        changes.note(rtl_btn);
                    });

                    // Row 2: Spacing — Indent (px) + Line / Leading (pt or %) in 2 columns.
                    let is_pct_leading = field_options.leading_mode == "percent";
                    let lead_unit = if is_pct_leading { "%" } else { "pt" };
                    let (indent_resp, (leading_resp, mut mode_resp)) = two_column_row(
                        ui,
                        108.0,
                        |ui, width| {
                            ui.add(
                                nf::Field::new(&mut format.indent, "Indent")
                                    .prefix("Indent")
                                    .unit("px")
                                    .decimals(1)
                                    .range(-10000.0..=10000.0)
                                    .width(width),
                            )
                        },
                        |ui, width| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S1;
                                let mode_w = 48.0;
                                let lead_w = (width - mode_w - space::S1).max(56.0);
                                let leading_resp = ui.add(
                                    nf::Field::new(&mut format.leading, "Leading")
                                        .prefix("Line")
                                        .unit(lead_unit)
                                        .decimals(1)
                                        .range(min_leading..=10000.0)
                                        .width(lead_w),
                                );
                                let (mode_resp, _) =
                                    nf::Select::new("text_box_leading_mode", lead_unit)
                                        .width(mode_w)
                                        .show(ui, |ui| {
                                            for (code, label) in [("pt", "pt"), ("percent", "%")] {
                                                if ui
                                                    .add(
                                                        nf::MenuItem::new(label).checked(
                                                            field_options.leading_mode == code
                                                                || (code == "pt"
                                                                    && field_options
                                                                        .leading_mode
                                                                        .is_empty()),
                                                        ),
                                                    )
                                                    .clicked()
                                                {
                                                    if field_options.leading_mode != code {
                                                        if code == "percent"
                                                            && format.leading == 0.0
                                                        {
                                                            format.leading = 120.0;
                                                        } else if code == "pt"
                                                            && (format.leading - 120.0).abs() < 1e-6
                                                        {
                                                            format.leading = 0.0;
                                                        }
                                                        field_options.leading_mode =
                                                            code.to_owned();
                                                    }
                                                    ui.close();
                                                }
                                            }
                                        });
                                (leading_resp, mode_resp)
                            })
                            .inner
                        },
                    );
                    if field_options.leading_mode != before_opts.leading_mode {
                        mode_resp.mark_changed();
                    }
                    changes.note(indent_resp);
                    changes.note(leading_resp);
                    changes.note(mode_resp);

                    // Row 3: Margins — Left (px) + Right (px) in 2 columns.
                    let (left_resp, right_resp) = two_column_row(
                        ui,
                        80.0,
                        |ui, width| {
                            ui.add(
                                nf::Field::new(&mut format.left_margin, "Left margin")
                                    .prefix("Left")
                                    .unit("px")
                                    .decimals(1)
                                    .range(0.0..=10000.0)
                                    .width(width),
                            )
                        },
                        |ui, width| {
                            ui.add(
                                nf::Field::new(&mut format.right_margin, "Right margin")
                                    .prefix("Right")
                                    .unit("px")
                                    .decimals(1)
                                    .range(0.0..=10000.0)
                                    .width(width),
                            )
                        },
                    );
                    changes.note(left_resp);
                    changes.note(right_resp);

                    // Row 4: Paragraph Space Before (pt) + Space After (pt) in 2 columns.
                    let (before_resp, after_resp) = two_column_row(
                        ui,
                        80.0,
                        |ui, width| {
                            ui.add(
                                nf::Field::new(&mut field_options.space_before, "Space before")
                                    .prefix("Before")
                                    .unit("pt")
                                    .decimals(1)
                                    .range(0.0..=720.0)
                                    .width(width),
                            )
                        },
                        |ui, width| {
                            ui.add(
                                nf::Field::new(&mut field_options.space_after, "Space after")
                                    .prefix("After")
                                    .unit("pt")
                                    .decimals(1)
                                    .range(0.0..=720.0)
                                    .width(width),
                            )
                        },
                    );
                    changes.note(before_resp);
                    changes.note(after_resp);

                    // Row 5: Text justify (Word spacing / Letter spacing) + Block indent (px).
                    let tj_opts = [
                        ("word_spacing", "Justify: Word spacing"),
                        ("letter_spacing", "Justify: Letter spacing"),
                    ];
                    let cur_tj = tj_opts
                        .iter()
                        .find(|(k, _)| *k == field_options.text_justify)
                        .map(|(_, l)| *l)
                        .unwrap_or("Justify: Word spacing");
                    let (mut justify_resp, block_resp) = two_column_row(
                        ui,
                        80.0,
                        |ui, width| {
                            nf::Select::new("text_box_justify_rule", cur_tj)
                                .width(width)
                                .show(ui, |ui| {
                                    for (code, label) in tj_opts {
                                        if ui
                                            .add(
                                                nf::MenuItem::new(label)
                                                    .checked(field_options.text_justify == code),
                                            )
                                            .clicked()
                                        {
                                            field_options.text_justify = code.to_owned();
                                            ui.close();
                                        }
                                    }
                                })
                                .0
                        },
                        |ui, width| {
                            ui.add(
                                nf::Field::new(&mut format.block_indent, "Block indent")
                                    .prefix("Block")
                                    .unit("px")
                                    .decimals(1)
                                    .range(0.0..=10000.0)
                                    .width(width),
                            )
                        },
                    );
                    if field_options.text_justify != before_opts.text_justify {
                        justify_resp.mark_changed();
                    }
                    changes.note(justify_resp);
                    changes.note(block_resp);

                    if field_options != before_opts {
                        format.set_field_options(
                            (!field_options.is_default()).then_some(field_options),
                        );
                    }
                });

            // 3. CONTAINER AND FLOW section (Flash CS5 — Columns, Gutter, Vertical Alignment, 4-sided Padding, Border/Background, 1st Line Offset)
            SectionHeader::new("Container and flow")
                .id_salt("text-box-container-and-flow")
                .default_open(true)
                .show(ui, |ui| {
                    let mut field_options = format.field_options().unwrap_or_default();
                    let before_opts = field_options.clone();
                    let mut synthetic_flow_id: Option<egui::Id> = None;

                    // Row 1: Behavior (Single line / Multiline / Multiline no wrap / Password).
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
                        beh_resp.mark_changed();
                    }
                    changes.note(beh_resp);

                    // Row 2: Max chars + Vertical Alignment icon buttons (Top / Center / Bottom / Justify).
                    let mut max_chars = field_options.max_chars as f64;
                    let cur_valign = if field_options.vertical_align.is_empty() {
                        "top"
                    } else {
                        field_options.vertical_align.as_str()
                    }
                    .to_owned();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S1;
                        let v_icons_w = 4.0 * crate::theme::control_h().min(22.0) + 3.0 * space::S1;
                        let max_w = (ui.available_width() - v_icons_w - space::S2).max(80.0);
                        let m_resp = ui.add(
                            nf::Field::new(&mut max_chars, "Max chars")
                                .prefix("Max chars")
                                .decimals(0)
                                .range(0.0..=65535.0)
                                .width(max_w),
                        );
                        if m_resp.changed() {
                            field_options.max_chars = max_chars.round().max(0.0) as u32;
                        }
                        changes.note(m_resp);

                        for (mode, glyph, label) in [
                            ("top", TextPropertyGlyph::VerticalAlignTop, "Align top"),
                            (
                                "center",
                                TextPropertyGlyph::VerticalAlignCenter,
                                "Align middle",
                            ),
                            (
                                "bottom",
                                TextPropertyGlyph::VerticalAlignBottom,
                                "Align bottom",
                            ),
                            (
                                "justify",
                                TextPropertyGlyph::VerticalAlignJustify,
                                "Justify vertically",
                            ),
                        ] {
                            let is_sel = cur_valign == mode;
                            let mut btn = text_property_icon_button(ui, glyph, is_sel, true, label);
                            if btn.clicked() && !is_sel {
                                field_options.vertical_align = mode.to_owned();
                                btn.mark_changed();
                            }
                            changes.note(btn);
                        }
                    });

                    // Row 3: Columns (1..=50) + Column gutter (0..=1000 px) with Flash CS5 column icons.
                    let mut cols_f64 = field_options.column_count.max(1) as f64;
                    let (col_resp, gut_resp) = two_column_row(
                        ui,
                        96.0,
                        |ui, width| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S1;
                                text_property_badge(
                                    ui,
                                    TextPropertyGlyph::ColumnsCount,
                                    "Number of columns",
                                );
                                ui.add(
                                    nf::Field::new(&mut cols_f64, "Columns")
                                        .prefix("Columns")
                                        .decimals(0)
                                        .range(1.0..=50.0)
                                        .width((width - 20.0).max(56.0)),
                                )
                            })
                            .inner
                        },
                        |ui, width| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S1;
                                text_property_badge(
                                    ui,
                                    TextPropertyGlyph::ColumnGutter,
                                    "Column gutter width (px)",
                                );
                                ui.add(
                                    nf::Field::new(
                                        &mut field_options.column_gutter,
                                        "Column gutter",
                                    )
                                    .prefix("Gutter")
                                    .unit("px")
                                    .decimals(1)
                                    .range(0.0..=1000.0)
                                    .width((width - 20.0).max(56.0)),
                                )
                            })
                            .inner
                        },
                    );
                    if col_resp.changed() {
                        field_options.column_count = cols_f64.round().clamp(1.0, 50.0) as u32;
                        if field_options.column_count > 1 {
                            *multiline = true;
                        }
                    }
                    changes.note(col_resp);
                    changes.note(gut_resp);

                    // Row 4 & Row 5: 4-sided Padding (L, R, T, B) with Link button.
                    let pad_link_id = egui::Id::new("properties-textbox-padding-linked");
                    let default_linked = (field_options.padding_left - field_options.padding_right)
                        .abs()
                        < 1e-6
                        && (field_options.padding_left - field_options.padding_top).abs() < 1e-6
                        && (field_options.padding_left - field_options.padding_bottom).abs() < 1e-6;
                    let mut pad_linked =
                        ui.data(|d| d.get_temp::<bool>(pad_link_id).unwrap_or(default_linked));
                    let link_w = crate::theme::control_h();
                    let pad_col_w =
                        ((ui.available_width() - link_w - space::S2 * 2.0) / 2.0).max(56.0);
                    let (old_pl, old_pr, old_pt, old_pb) = (
                        field_options.padding_left,
                        field_options.padding_right,
                        field_options.padding_top,
                        field_options.padding_bottom,
                    );
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        if ui
                            .add(
                                nf::IconButton::new(i16::LINK, "Link padding values")
                                    .selected(pad_linked),
                            )
                            .clicked()
                        {
                            pad_linked = !pad_linked;
                        }
                        changes.note(
                            ui.add(
                                nf::Field::new(&mut field_options.padding_left, "Padding left")
                                    .prefix("Pad L")
                                    .unit("px")
                                    .decimals(1)
                                    .range(0.0..=1000.0)
                                    .width(pad_col_w),
                            ),
                        );
                        changes.note(
                            ui.add(
                                nf::Field::new(&mut field_options.padding_right, "Padding right")
                                    .prefix("Pad R")
                                    .unit("px")
                                    .decimals(1)
                                    .range(0.0..=1000.0)
                                    .width(pad_col_w),
                            ),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S2;
                        ui.add_space(link_w);
                        changes.note(
                            ui.add(
                                nf::Field::new(&mut field_options.padding_top, "Padding top")
                                    .prefix("Pad T")
                                    .unit("px")
                                    .decimals(1)
                                    .range(0.0..=1000.0)
                                    .width(pad_col_w),
                            ),
                        );
                        changes.note(
                            ui.add(
                                nf::Field::new(&mut field_options.padding_bottom, "Padding bottom")
                                    .prefix("Pad B")
                                    .unit("px")
                                    .decimals(1)
                                    .range(0.0..=1000.0)
                                    .width(pad_col_w),
                            ),
                        );
                    });
                    ui.data_mut(|d| d.insert_temp(pad_link_id, pad_linked));
                    if pad_linked {
                        let new_pad = if (field_options.padding_left - old_pl).abs() > 1e-6 {
                            Some(field_options.padding_left)
                        } else if (field_options.padding_right - old_pr).abs() > 1e-6 {
                            Some(field_options.padding_right)
                        } else if (field_options.padding_top - old_pt).abs() > 1e-6 {
                            Some(field_options.padding_top)
                        } else if (field_options.padding_bottom - old_pb).abs() > 1e-6 {
                            Some(field_options.padding_bottom)
                        } else {
                            None
                        };
                        if let Some(v) = new_pad {
                            field_options.padding_left = v;
                            field_options.padding_right = v;
                            field_options.padding_top = v;
                            field_options.padding_bottom = v;
                        }
                    }

                    // Row 6: Border color swatch + Border width (pt) + Background color swatch (Flash CS5 style).
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S1;
                        text_property_badge(ui, TextPropertyGlyph::BorderStroke, "Border color");
                        if text_optional_color_swatch(
                            ui,
                            "text-box-border-color",
                            &mut field_options.border,
                            &mut field_options.border_color,
                            "Border color",
                        ) {
                            synthetic_flow_id =
                                Some(ui.make_persistent_id("text-box-border-color-changed"));
                        }
                        let bw_resp = ui.add(
                            nf::Field::new(&mut field_options.border_width, "Border width")
                                .prefix("W")
                                .unit("pt")
                                .decimals(1)
                                .range(0.5..=200.0)
                                .width(78.0),
                        );
                        changes.note(bw_resp);

                        ui.add_space(space::S2);
                        text_property_badge(
                            ui,
                            TextPropertyGlyph::BackgroundFill,
                            "Background color",
                        );
                        if text_optional_color_swatch(
                            ui,
                            "text-box-bg-color",
                            &mut field_options.background,
                            &mut field_options.background_color,
                            "Background color",
                        ) {
                            synthetic_flow_id =
                                Some(ui.make_persistent_id("text-box-bg-color-changed"));
                        }
                    });

                    // Row 7: 1st Line Offset (Auto / Ascent / Line Height / Fixed pt).
                    let flo_opts = [
                        ("auto", "1st Line Offset: Auto"),
                        ("ascent", "1st Line Offset: Ascent"),
                        ("line_height", "1st Line Offset: Line Height"),
                        ("pt", "1st Line Offset: Fixed (pt)"),
                    ];
                    let cur_flo = flo_opts
                        .iter()
                        .find(|(k, _)| *k == field_options.first_line_offset_mode)
                        .map(|(_, l)| *l)
                        .unwrap_or("1st Line Offset: Auto");
                    let is_fixed_pt = field_options.first_line_offset_mode == "pt";
                    if is_fixed_pt {
                        let (mut mode_resp, offset_resp) = two_column_row(
                            ui,
                            80.0,
                            |ui, width| {
                                nf::Select::new("text_box_first_line_offset", cur_flo)
                                    .width(width)
                                    .show(ui, |ui| {
                                        for (code, label) in flo_opts {
                                            if ui
                                                .add(nf::MenuItem::new(label).checked(
                                                    field_options.first_line_offset_mode == code,
                                                ))
                                                .clicked()
                                            {
                                                field_options.first_line_offset_mode =
                                                    code.to_owned();
                                                ui.close();
                                            }
                                        }
                                    })
                                    .0
                            },
                            |ui, width| {
                                ui.add(
                                    nf::Field::new(
                                        &mut field_options.first_line_offset,
                                        "First line offset",
                                    )
                                    .prefix("Offset")
                                    .unit("pt")
                                    .decimals(1)
                                    .range(0.0..=720.0)
                                    .width(width),
                                )
                            },
                        );
                        if field_options.first_line_offset_mode
                            != before_opts.first_line_offset_mode
                        {
                            mode_resp.mark_changed();
                        }
                        changes.note(mode_resp);
                        changes.note(offset_resp);
                    } else {
                        let (mut mode_resp, _) =
                            nf::Select::new("text_box_first_line_offset", cur_flo)
                                .width(ui.available_width())
                                .show(ui, |ui| {
                                    for (code, label) in flo_opts {
                                        if ui
                                            .add(nf::MenuItem::new(label).checked(
                                                field_options.first_line_offset_mode == code,
                                            ))
                                            .clicked()
                                        {
                                            field_options.first_line_offset_mode = code.to_owned();
                                            ui.close();
                                        }
                                    }
                                });
                        if field_options.first_line_offset_mode
                            != before_opts.first_line_offset_mode
                        {
                            mode_resp.mark_changed();
                        }
                        changes.note(mode_resp);
                    }

                    if field_options != before_opts {
                        format.set_field_options(
                            (!field_options.is_default()).then_some(field_options),
                        );
                        if changes.control.is_none() {
                            changes.control = synthetic_flow_id.or_else(|| {
                                Some(ui.make_persistent_id("text-box-container-flow-edit"))
                            });
                        }
                    }
                });

            // 4. OPTIONS section (Link / Variable / Restrict / Auto size)
            SectionHeader::new("Options")
                .id_salt("text-box-options")
                .default_open(false)
                .show(ui, |ui| {
                    let mut field_options = format.field_options().unwrap_or_default();
                    let before_opts = field_options.clone();
                    changes.note(text_row(ui, "URL", &mut format.url));
                    let mut target_str = format.target_str().to_owned();
                    let auto_size_opts = [
                        ("none", "Auto size: None"),
                        ("left", "Auto size: Left"),
                        ("center", "Auto size: Center"),
                        ("right", "Auto size: Right"),
                    ];
                    let cur_as = auto_size_opts
                        .iter()
                        .find(|(k, _)| *k == field_options.auto_size)
                        .map(|(_, l)| *l)
                        .unwrap_or("Auto size: None");
                    let (target_resp, mut auto_size_resp) = two_column_row(
                        ui,
                        80.0,
                        |ui, width| {
                            let mut response = ui.add(
                                nf::TextField::new(&mut target_str, "Target")
                                    .prefix("Target")
                                    .width(width),
                            );
                            response.id = nf::TextField::edit_id(response.id);
                            response
                        },
                        |ui, width| {
                            nf::Select::new("text_box_auto_size", cur_as)
                                .width(width)
                                .show(ui, |ui| {
                                    for (code, label) in auto_size_opts {
                                        if ui
                                            .add(
                                                nf::MenuItem::new(label)
                                                    .checked(field_options.auto_size == code),
                                            )
                                            .clicked()
                                        {
                                            field_options.auto_size = code.to_owned();
                                            ui.close();
                                        }
                                    }
                                })
                                .0
                        },
                    );
                    if target_resp.changed() {
                        format.set_target_str(target_str);
                    }
                    if field_options.auto_size != before_opts.auto_size {
                        auto_size_resp.mark_changed();
                    }
                    changes.note(target_resp);
                    changes.note(auto_size_resp);

                    let mut var_str = field_options.variable.clone().unwrap_or_default();
                    let mut restrict_str = field_options.restrict.clone().unwrap_or_default();
                    let (variable_resp, restrict_resp) = two_column_row(
                        ui,
                        80.0,
                        |ui, width| {
                            let mut response = ui.add(
                                nf::TextField::new(&mut var_str, "Variable")
                                    .prefix("Variable")
                                    .width(width),
                            );
                            response.id = nf::TextField::edit_id(response.id);
                            response
                        },
                        |ui, width| {
                            let mut response = ui.add(
                                nf::TextField::new(&mut restrict_str, "Restrict")
                                    .prefix("Restrict")
                                    .width(width),
                            );
                            response.id = nf::TextField::edit_id(response.id);
                            response
                        },
                    );
                    if variable_resp.changed() {
                        field_options.variable =
                            (!var_str.trim().is_empty()).then(|| var_str.trim().to_owned());
                    }
                    if restrict_resp.changed() {
                        field_options.restrict = (!restrict_str.is_empty()).then_some(restrict_str);
                    }
                    changes.note(variable_resp);
                    changes.note(restrict_resp);

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
        let bw = (result.surface.border_width as f32).clamp(0.5, 200.0);
        let corners = [
            screen(Pos2::new(0.0, 0.0)),
            screen(Pos2::new(*width as f32, 0.0)),
            screen(Pos2::new(*width as f32, *height as f32)),
            screen(Pos2::new(0.0, *height as f32)),
            screen(Pos2::new(0.0, 0.0)),
        ];
        p.add(Shape::line(corners.to_vec(), egui::Stroke::new(bw, bd_col)));
    }
    for hl in &result.highlights {
        let hl_rect = Rect::from_min_size(
            Pos2::new(hl.x as f32, hl.y as f32),
            Vec2::new(hl.width as f32, hl.height.max(1.0) as f32),
        )
        .intersect(clip);
        if hl_rect.is_positive() {
            let hl_col = hl.color.map_or(rgb_to_color32(0xffff00), rgb_to_color32);
            let corners = [
                screen(hl_rect.left_top()),
                screen(hl_rect.right_top()),
                screen(hl_rect.right_bottom()),
                screen(hl_rect.left_bottom()),
            ];
            p.add(Shape::convex_polygon(
                corners.to_vec(),
                hl_col,
                egui::Stroke::NONE,
            ));
        }
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

/// Paints Flash CS5 multi-column divider guides and TLF In/Out flow ports when a multi-column
/// `TextBox` is selected on the Stage.
pub(crate) fn paint_container_guides(
    p: &egui::Painter,
    kind: &ElementKind,
    screen_rect: Rect,
    zoom: f32,
) {
    let ElementKind::TextBox {
        format,
        width,
        height,
        ..
    } = kind
    else {
        return;
    };
    let Some(opts) = format.field_options() else {
        return;
    };
    let cols = opts.column_count.clamp(1, 50);
    if cols <= 1 || *width <= 0.0 || *height <= 0.0 {
        return;
    }
    let palette = neoflash_ui::palette(p.ctx());
    let guide_stroke = egui::Stroke::new(1.0_f32, palette.accent_default.gamma_multiply(0.55));
    let pad_l = opts.padding_left.max(0.0) as f32 * zoom;
    let pad_r = opts.padding_right.max(0.0) as f32 * zoom;
    let pad_t = opts.padding_top.max(0.0) as f32 * zoom;
    let pad_b = opts.padding_bottom.max(0.0) as f32 * zoom;
    let gutter = opts.column_gutter.max(0.0) as f32 * zoom;
    let inner_w = (screen_rect.width() - pad_l - pad_r).max(0.0);
    let col_w = ((inner_w - gutter * (cols - 1) as f32) / cols as f32).max(0.0);
    let top_y = (screen_rect.top() + pad_t).min(screen_rect.bottom());
    let bot_y = (screen_rect.bottom() - pad_b).max(top_y);

    for c in 0..(cols - 1) {
        let right_edge = screen_rect.left() + pad_l + (c + 1) as f32 * col_w + c as f32 * gutter;
        let next_left_edge = right_edge + gutter;
        if right_edge > screen_rect.left() && right_edge < screen_rect.right() {
            p.line_segment(
                [Pos2::new(right_edge, top_y), Pos2::new(right_edge, bot_y)],
                guide_stroke,
            );
        }
        if gutter > 0.5
            && next_left_edge > screen_rect.left()
            && next_left_edge < screen_rect.right()
        {
            p.line_segment(
                [
                    Pos2::new(next_left_edge, top_y),
                    Pos2::new(next_left_edge, bot_y),
                ],
                guide_stroke,
            );
        }
    }

    // Flash CS5 TLF In-port (top-left) and Out-port (bottom-right) squares.
    let port_size = Vec2::splat(7.0);
    let in_port = Rect::from_center_size(
        Pos2::new(screen_rect.left(), screen_rect.top() + 10.0),
        port_size,
    );
    let out_port = Rect::from_center_size(
        Pos2::new(screen_rect.right(), screen_rect.bottom() - 10.0),
        port_size,
    );
    for port in [in_port, out_port] {
        p.rect_filled(port, 0.0, palette.bg_panel);
        p.rect_stroke(
            port,
            0.0,
            egui::Stroke::new(1.0_f32, palette.accent_default),
            egui::StrokeKind::Inside,
        );
    }
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
