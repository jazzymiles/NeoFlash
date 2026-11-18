//! Font embedding modal dialog and live floating body (NF2 `Dialog / Font embedding`, `DL-11`,
//! `W3.8`; reference `publish-misc-dialogs.png`), opened from `Library menu ▸ Font embedding…`
//! and `Window ▸ Panels ▸ Planned panels ▸ Font embedding`.

use super::dialog_helpers::{DialogShell, fixed_dialog_size, form_row, group_caption, nav_row};
use crate::app::Editor;
use eframe::egui::{self, Ui};
use neoflash_core::{ResourceUnit, new_id};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;
use serde::{Deserialize, Serialize};

const FONT_EMBEDDING_METADATA_PATH: &str = "fonts/embedding.json";

const RANGE_SPECS: [(&str, usize); 8] = [
    ("Uppercase [A–Z] (27)", 27),
    ("Lowercase [a–z] (27)", 27),
    ("Numerals [0–9] (11)", 11),
    ("Punctuation (52)", 52),
    ("Basic Latin (95)", 95),
    ("Cyrillic (256)", 256),
    ("Latin Extended A (128)", 128),
    ("Greek (117)", 117),
];

const FONT_FAMILIES: [&str; 6] = [
    "Inter",
    "Roboto Mono",
    "Helvetica",
    "Arial",
    "Georgia",
    "Courier New",
];

const FONT_STYLES: [&str; 5] = ["Regular", "Semi Bold", "Bold", "Italic", "Bold Italic"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct EmbeddedFontEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) family: String,
    pub(crate) style: String,
    pub(crate) ranges: [bool; 8],
    pub(crate) extra_chars: String,
    pub(crate) export_for_actionscript: bool,
    pub(crate) class_name: String,
}

impl EmbeddedFontEntry {
    pub(crate) fn glyph_count(&self) -> usize {
        let range_glyphs: usize = self
            .ranges
            .iter()
            .enumerate()
            .filter_map(|(i, on)| (*on).then_some(RANGE_SPECS[i].1))
            .sum();
        let extra = self
            .extra_chars
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .count();
        (range_glyphs + extra).max(1)
    }

    pub(crate) fn approx_kb(&self) -> usize {
        ((self.glyph_count() * 124 + 512) / 1024).max(2)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FontEmbeddingDraft {
    pub(crate) entries: Vec<EmbeddedFontEntry>,
    pub(crate) selected: usize,
    original_entries: Vec<EmbeddedFontEntry>,
    pending_text_target: Option<PendingTextEmbedding>,
}

#[derive(Clone, Debug, PartialEq)]
struct PendingTextEmbedding {
    symbol_id: String,
    frame: u32,
    element_id: String,
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-font-embedding-draft")
}

/// Font embedding: `dialog_l` × `dialog_l_height`, whatever the font list and the selected face.
pub(crate) fn font_embedding_size(ctx: &egui::Context) -> egui::Vec2 {
    fixed_dialog_size(ctx, metrics::dialog_l(), metrics::dialog_l_height())
}

fn inline_draft_id() -> egui::Id {
    egui::Id::new("nf2-font-embedding-inline-draft")
}

pub(crate) fn has_font_embedding_dialog(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<FontEmbeddingDraft>(draft_id()))
        .is_some()
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn font_embedding_draft(ctx: &egui::Context) -> Option<FontEmbeddingDraft> {
    ctx.data(|d| d.get_temp::<FontEmbeddingDraft>(draft_id()))
        .or_else(|| ctx.data(|d| d.get_temp::<FontEmbeddingDraft>(inline_draft_id())))
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn update_font_embedding_draft(
    ctx: &egui::Context,
    mut f: impl FnMut(&mut FontEmbeddingDraft),
) {
    for id in [draft_id(), inline_draft_id()] {
        if let Some(mut draft) = ctx.data(|d| d.get_temp::<FontEmbeddingDraft>(id)) {
            f(&mut draft);
            ctx.data_mut(|d| d.insert_temp(id, draft));
        }
    }
}

#[cfg(test)]
pub(crate) fn set_inline_font_embedding_draft(ctx: &egui::Context, draft: FontEmbeddingDraft) {
    ctx.data_mut(|data| data.insert_temp(inline_draft_id(), draft));
}

fn stored_embedding_entries(project: &neoflash_core::Project) -> Vec<EmbeddedFontEntry> {
    project
        .resources
        .iter()
        .find(|resource| {
            resource
                .path
                .eq_ignore_ascii_case(FONT_EMBEDDING_METADATA_PATH)
        })
        .and_then(|resource| resource.decoded_bytes().ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn embedding_draft_for_project(project: &neoflash_core::Project) -> FontEmbeddingDraft {
    let mut draft = FontEmbeddingDraft::default();
    let stored_entries = stored_embedding_entries(project);
    let existing_fonts: Vec<EmbeddedFontEntry> = project
        .resources
        .iter()
        .filter(|resource| neoflash_export::is_font_resource(resource))
        .map(|resource| {
            let family = neoflash_export::font_family_for_resource(resource);
            let class_name: String = family
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .collect();
            EmbeddedFontEntry {
                id: resource.id.clone(),
                name: family.clone(),
                family,
                style: "Regular".to_owned(),
                ranges: [true, true, true, true, false, false, false, false],
                extra_chars: String::new(),
                export_for_actionscript: !class_name.is_empty(),
                class_name,
            }
        })
        .collect();
    if !stored_entries.is_empty() {
        draft.entries = stored_entries;
        draft.original_entries = draft.entries.clone();
    } else if !existing_fonts.is_empty() {
        draft.entries = existing_fonts;
        draft.original_entries = draft.entries.clone();
    }
    draft.selected = 0;
    draft
}

fn selected_text_entry(family: &str, bold: bool, italic: bool, text: &str) -> EmbeddedFontEntry {
    let style = match (bold, italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    };
    let mut ranges = [true, true, true, true, false, false, false, false];
    ranges[5] = text
        .chars()
        .any(|ch| ('\u{0400}'..='\u{04ff}').contains(&ch));
    ranges[6] = text
        .chars()
        .any(|ch| ('\u{0100}'..='\u{017f}').contains(&ch));
    ranges[7] = text
        .chars()
        .any(|ch| ('\u{0370}'..='\u{03ff}').contains(&ch));
    let mut extra_chars = String::new();
    for character in text.chars().filter(|character| {
        !character.is_whitespace()
            && !character.is_ascii()
            && !('\u{0100}'..='\u{017f}').contains(character)
            && !('\u{0370}'..='\u{03ff}').contains(character)
            && !('\u{0400}'..='\u{04ff}').contains(character)
    }) {
        if !extra_chars.contains(character) {
            extra_chars.push(character);
        }
    }
    EmbeddedFontEntry {
        id: new_id("res"),
        name: format!("{family} {style}"),
        family: family.to_owned(),
        style: style.to_owned(),
        ranges,
        extra_chars,
        export_for_actionscript: false,
        class_name: String::new(),
    }
}

impl Editor {
    /// Open the `Font embedding` modal dialog (`DL-11`).
    pub(crate) fn open_font_embedding(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        let draft = embedding_draft_for_project(&self.project);
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    /// Open font embedding from a selected text field, with that field's exact face selected.
    /// A project without prior font resources starts with only this face instead of sample entries.
    pub(crate) fn open_font_embedding_for_text(
        &mut self,
        ctx: &egui::Context,
        family: &str,
        bold: bool,
        italic: bool,
        text: &str,
        enable_after_confirm: bool,
    ) {
        let pending_text_target = enable_after_confirm.then(|| PendingTextEmbedding {
            symbol_id: self.symbol_id().to_owned(),
            frame: self.frame,
            element_id: self
                .selection
                .single()
                .map(|target| target.element.clone())
                .unwrap_or_default(),
        });
        self.finish_edits();
        let mut draft = embedding_draft_for_project(&self.project);
        let has_authored_entries = !stored_embedding_entries(&self.project).is_empty()
            || self
                .project
                .resources
                .iter()
                .any(neoflash_export::is_font_resource);
        if !has_authored_entries {
            draft.entries.clear();
            draft.original_entries.clear();
        }
        let selected = selected_text_entry(family, bold, italic, text);
        let selected_index = draft
            .entries
            .iter()
            .position(|entry| {
                entry.family.eq_ignore_ascii_case(&selected.family)
                    && entry.style.eq_ignore_ascii_case(&selected.style)
            })
            .unwrap_or_else(|| {
                draft.entries.push(selected);
                draft.entries.len() - 1
            });
        draft.selected = selected_index;
        draft.pending_text_target = pending_text_target;
        ctx.data_mut(|data| data.insert_temp(draft_id(), draft));
    }

    pub(crate) fn cancel_font_embedding(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|data| {
            data.remove::<FontEmbeddingDraft>(draft_id());
            data.remove::<FontEmbeddingDraft>(inline_draft_id());
        });
    }

    /// Commit the active `Font embedding` draft into `self.project.resources` in one
    /// `"Font embedding"` Undo step.
    pub(crate) fn confirm_font_embedding(&mut self, ctx: &egui::Context) -> Result<usize, String> {
        let draft = ctx
            .data(|d| d.get_temp::<FontEmbeddingDraft>(draft_id()))
            .or_else(|| ctx.data(|d| d.get_temp::<FontEmbeddingDraft>(inline_draft_id())))
            .unwrap_or_else(|| embedding_draft_for_project(&self.project));

        let count = draft.entries.len();
        let original_fonts: Vec<_> = self
            .project
            .resources
            .iter()
            .filter(|resource| neoflash_export::is_font_resource(resource))
            .cloned()
            .collect();
        let metadata_id = self
            .project
            .resources
            .iter()
            .find(|resource| {
                resource
                    .path
                    .eq_ignore_ascii_case(FONT_EMBEDDING_METADATA_PATH)
            })
            .map(|resource| resource.id.clone())
            .unwrap_or_else(|| new_id("res"));
        let mut candidate = self.project.clone();
        candidate.resources.retain(|resource| {
            !neoflash_export::is_font_resource(resource)
                && !resource
                    .path
                    .eq_ignore_ascii_case(FONT_EMBEDDING_METADATA_PATH)
        });
        let mut saved_entries = Vec::with_capacity(draft.entries.len());
        for entry in &draft.entries {
            let clean_stem: String = entry
                .name
                .trim()
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric()
                        || character == '-'
                        || character == '_'
                        || character == ' '
                    {
                        character
                    } else {
                        '_'
                    }
                })
                .collect();
            let stem = if clean_stem.trim().is_empty() {
                "EmbeddedFont"
            } else {
                clean_stem.trim()
            };
            let path = format!("fonts/{stem}.ttf");
            let existing = original_fonts
                .iter()
                .find(|resource| resource.id == entry.id);
            let face_unchanged = draft
                .original_entries
                .iter()
                .find(|original| original.id == entry.id)
                .map(|original| original.family == entry.family && original.style == entry.style)
                .unwrap_or(false);
            let unit = if let Some(existing) = existing.filter(|_| face_unchanged) {
                existing.clone()
            } else {
                let bytes = match crate::fonts::embeddable_font_bytes(&entry.family, &entry.style) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        let error = format!("Could not update font embedding: {error}");
                        self.message(&error);
                        return Err(error);
                    }
                };
                let resource_id = existing
                    .map(|resource| resource.id.clone())
                    .unwrap_or_else(|| new_id("res"));
                match ResourceUnit::from_bytes(resource_id, path, "font/ttf", &bytes) {
                    Ok(unit) => unit,
                    Err(error) => {
                        let error = format!("Could not update font embedding: {error}");
                        self.message(&error);
                        return Err(error);
                    }
                }
            };
            let mut stored_entry = entry.clone();
            stored_entry.id = unit.id.clone();
            candidate.resources.push(unit);
            saved_entries.push(stored_entry);
        }
        let metadata = match serde_json::to_vec(&saved_entries)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                ResourceUnit::from_bytes(
                    metadata_id,
                    FONT_EMBEDDING_METADATA_PATH,
                    "application/json",
                    &bytes,
                )
                .map_err(|error| error.to_string())
            }) {
            Ok(metadata) => metadata,
            Err(error) => {
                let error = format!("Could not update font embedding: {error}");
                self.message(&error);
                return Err(error);
            }
        };
        if !saved_entries.is_empty() {
            candidate.resources.push(metadata);
        }
        if let Some(target) = &draft.pending_text_target {
            let Some(symbol) = candidate.symbol_mut(&target.symbol_id) else {
                let error =
                    "Could not update font embedding: selected text symbol no longer exists."
                        .to_owned();
                self.message(&error);
                return Err(error);
            };
            let mut updated = false;
            for layer in &mut symbol.timeline.layers {
                let Some(key_index) = layer
                    .keyframes
                    .iter()
                    .rposition(|keyframe| keyframe.frame <= target.frame)
                else {
                    continue;
                };
                let Some(element) = layer.keyframes[key_index]
                    .elements
                    .iter_mut()
                    .find(|element| element.id == target.element_id)
                else {
                    continue;
                };
                if let neoflash_core::ElementKind::TextBox { format, .. } = &mut element.kind {
                    let mut options = format.field_options().unwrap_or_default();
                    options.embed_fonts = true;
                    format.set_field_options(Some(options));
                    updated = true;
                }
                break;
            }
            if !updated {
                let error =
                    "Could not update font embedding: selected text field no longer exists."
                        .to_owned();
                self.message(&error);
                return Err(error);
            }
        }
        candidate.format_version = candidate.format_version.max(3);
        candidate.upgrade_format_for_features();
        if let Err(error) = candidate.validate() {
            let error = format!("Could not update font embedding: {error}");
            self.message(&error);
            return Err(error);
        }

        self.finish_edits();
        self.transact_named("Font embedding", |editor| {
            editor.project = candidate;
        });

        ctx.data_mut(|data| {
            data.remove::<FontEmbeddingDraft>(draft_id());
            data.remove::<FontEmbeddingDraft>(inline_draft_id());
        });
        self.message(format!("Updated font embedding ({count} font(s))."));
        Ok(count)
    }

    /// `+`: a new face of the first available family with the four basic ranges.
    fn add_font_embedding_face(&self, draft: &mut FontEmbeddingDraft) {
        let idx = draft.entries.len() + 1;
        let default_family = self
            .fonts
            .families
            .first()
            .cloned()
            .unwrap_or_else(|| "Inter".to_owned());
        draft.entries.push(EmbeddedFontEntry {
            id: new_id("res"),
            name: format!("Embedded Font {idx}"),
            family: default_family,
            style: "Regular".to_owned(),
            ranges: [true, true, true, true, false, false, false, false],
            extra_chars: String::new(),
            export_for_actionscript: false,
            class_name: String::new(),
        });
        draft.selected = draft.entries.len() - 1;
    }

    /// Screenshot aid (`NEOFLASH_SCREENSHOT_DIALOG=font_embedding_face`): the dialog with one face.
    pub(crate) fn open_font_embedding_with_face(&mut self, ctx: &egui::Context) {
        self.open_font_embedding(ctx);
        if let Some(mut draft) = ctx.data(|d| d.get_temp::<FontEmbeddingDraft>(draft_id())) {
            self.add_font_embedding_face(&mut draft);
            ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
        }
    }

    /// Sidebar: one row per embedded font, then Add / Remove.
    fn font_embedding_sidebar(&mut self, ui: &mut Ui, draft: &mut FontEmbeddingDraft) {
        use crate::theme::ScrollTheme as _;
        let nf = neoflash_ui::palette(ui.ctx());
        let list_height = (ui.available_height() - metrics::control() - space::s2()).max(0.0);
        egui::ScrollArea::vertical()
            .id_salt("font-embedding-fonts")
            .max_height(list_height)
            .auto_shrink([false, false])
            .themed()
            .show(ui, |ui| {
                if draft.entries.is_empty() {
                    ui.add_space(space::s1());
                    ui.horizontal(|ui| {
                        ui.add_space(space::s3());
                        ui.label(
                            rich("No embedded fonts", TextStyle::Small).color(nf.text_secondary),
                        );
                    });
                }
                for i in 0..draft.entries.len() {
                    let font_name = draft.entries[i].name.clone();
                    if nav_row(ui, i16::FONT, &font_name, i == draft.selected, None).clicked() {
                        draft.selected = i;
                    }
                }
            });
        ui.add_space(space::s2());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::s1();
            if ui.add(c::IconButton::new(i16::PLUS, "Add font")).clicked() {
                self.add_font_embedding_face(draft);
            }
            if ui
                .add_enabled(
                    !draft.entries.is_empty(),
                    c::IconButton::new(i16::MINUS, "Remove font"),
                )
                .clicked()
                && !draft.entries.is_empty()
            {
                draft.entries.remove(draft.selected);
                draft.selected = draft.selected.min(draft.entries.len().saturating_sub(1));
            }
        });
    }

    /// Page of the selected font on the form grid; the character ranges are two columns.
    fn font_embedding_page(&mut self, ui: &mut Ui, draft: &mut FontEmbeddingDraft) {
        let nf = neoflash_ui::palette(ui.ctx());
        let Some(entry) = draft.entries.get_mut(draft.selected) else {
            ui.label(
                rich(
                    "No embedded fonts configured in this document.",
                    TextStyle::BodyStrong,
                )
                .color(nf.text_primary),
            );
            ui.label(
                rich(
                    "Click + to add a font face and choose character ranges to embed.",
                    TextStyle::Small,
                )
                .color(nf.text_secondary),
            );
            return;
        };
        form_row(ui, "Name", |ui| {
            let width = ui.available_width();
            ui.add(c::TextField::new(&mut entry.name, "Name").width(width));
        });
        form_row(ui, "Family", |ui| {
            let shown = entry.family.clone();
            c::Select::new(ui.id().with("font-embed-family"), &shown)
                .width(metrics::field_l())
                .show(ui, |ui| {
                    let mut families: Vec<&str> =
                        self.fonts.families.iter().map(String::as_str).collect();
                    families.extend(FONT_FAMILIES);
                    families.sort_unstable_by_key(|family| family.to_ascii_lowercase());
                    families.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
                    for fam in families {
                        if ui
                            .add(c::MenuItem::new(fam).checked(entry.family == fam))
                            .clicked()
                        {
                            entry.family = fam.to_owned();
                            ui.close();
                        }
                    }
                });
        });
        form_row(ui, "Style", |ui| {
            let shown = entry.style.clone();
            c::Select::new(ui.id().with("font-embed-style"), &shown)
                .width(metrics::field_l())
                .show(ui, |ui| {
                    for st in FONT_STYLES {
                        if ui
                            .add(c::MenuItem::new(st).checked(entry.style == st))
                            .clicked()
                        {
                            entry.style = st.to_owned();
                            ui.close();
                        }
                    }
                });
        });
        group_caption(ui, "Character ranges");
        let half = RANGE_SPECS.len().div_ceil(2);
        let column = ((ui.available_width() - space::s4()) / 2.0).floor();
        let row_gap = metrics::row_gap();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = space::s4();
            for range in [0..half, half..RANGE_SPECS.len()] {
                ui.vertical(|ui| {
                    ui.set_width(column);
                    ui.spacing_mut().item_spacing.y = row_gap;
                    for idx in range {
                        ui.add(c::Checkbox::new(&mut entry.ranges[idx], RANGE_SPECS[idx].0));
                    }
                });
            }
        });
        form_row(ui, "Also include", |ui| {
            let width = ui.available_width();
            ui.add(c::TextField::new(&mut entry.extra_chars, "Also include").width(width));
        });
        group_caption(ui, "ActionScript");
        form_row(ui, "", |ui| {
            let as_resp = ui.add_enabled(
                false,
                c::Checkbox::new(
                    &mut entry.export_for_actionscript,
                    "Export for ActionScript",
                ),
            );
            let _ = c::not_available(as_resp, "ActionScript font class export");
        });
        form_row(ui, "Class", |ui| {
            let cls_resp = ui.add_enabled(
                false,
                c::TextField::new(&mut entry.class_name, "Class").width(metrics::field_l()),
            );
            let _ = c::not_available(cls_resp, "ActionScript font class name");
        });
    }

    pub(crate) fn font_embedding_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<FontEmbeddingDraft>(draft_id())) else {
            return;
        };
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut do_apply = false;

        if !draft.entries.is_empty() {
            draft.selected = draft.selected.min(draft.entries.len() - 1);
        } else {
            draft.selected = 0;
        }
        // Same shell as Preferences: one size whatever the font list and the selected face.
        let size = font_embedding_size(ctx);
        let modal = egui::Modal::new(egui::Id::new("font-embedding"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                let shell = DialogShell::show(ui, "Font Embedding", size, metrics::sidebar_wide());
                close_cancel |= shell.close;
                shell.sidebar(ui, |ui| self.font_embedding_sidebar(ui, &mut draft));
                shell.page(ui, "font-embedding-page", |ui, _| {
                    self.font_embedding_page(ui, &mut draft);
                });
                let badge_text = if let Some(entry) = draft.entries.get(draft.selected) {
                    format!(
                        "{} glyphs · subset estimate ~ {} kB",
                        entry.glyph_count(),
                        entry.approx_kb()
                    )
                } else {
                    "0 embedded fonts".to_owned()
                };
                shell.footer(
                    ui,
                    |ui| {
                        ui.add(c::Badge::new(&badge_text, c::BadgeType::Accent));
                    },
                    |ui| {
                        do_apply |= ui.add(c::Button::primary("OK")).clicked();
                        close_cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
            });

        if !close_cancel && !do_apply {
            match kb.resolve(ctx, &modal, true) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close_cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => do_apply = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }

        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));

        if close_cancel {
            self.cancel_font_embedding(ctx);
            return;
        }
        if do_apply {
            let _ = self.confirm_font_embedding(ctx);
        }
    }
}
