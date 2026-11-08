//! Font embedding modal dialog and live floating body (NF2 `Dialog / Font embedding`, `DL-11`,
//! `W3.8`; reference `publish-misc-dialogs.png`), opened from `Library menu ▸ Font embedding…`
//! and `Window ▸ Panels ▸ Planned panels ▸ Font embedding`.

use super::planned_dialogs::group;
use crate::app::Editor;
use eframe::egui::{self, Ui};
use neoflash_core::{ResourceUnit, new_id};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;
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

const FONT_STYLES: [&str; 4] = ["Regular", "Semi Bold", "Bold", "Italic"];

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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FontEmbeddingDraft {
    pub(crate) entries: Vec<EmbeddedFontEntry>,
    pub(crate) selected: usize,
    original_entries: Vec<EmbeddedFontEntry>,
}

impl Default for FontEmbeddingDraft {
    fn default() -> Self {
        Self {
            entries: vec![
                EmbeddedFontEntry {
                    id: "font-inter-regular".to_owned(),
                    name: "Inter Regular".to_owned(),
                    family: "Inter".to_owned(),
                    style: "Regular".to_owned(),
                    ranges: [true, true, true, true, false, true, false, false],
                    extra_chars: "→ ← ★ © ®".to_owned(),
                    export_for_actionscript: true,
                    class_name: "InterRegular".to_owned(),
                },
                EmbeddedFontEntry {
                    id: "font-inter-semibold".to_owned(),
                    name: "Inter Semi Bold".to_owned(),
                    family: "Inter".to_owned(),
                    style: "Semi Bold".to_owned(),
                    ranges: [true, true, true, true, false, false, false, false],
                    extra_chars: String::new(),
                    export_for_actionscript: false,
                    class_name: "InterSemiBold".to_owned(),
                },
                EmbeddedFontEntry {
                    id: "font-roboto-mono".to_owned(),
                    name: "Roboto Mono".to_owned(),
                    family: "Roboto Mono".to_owned(),
                    style: "Regular".to_owned(),
                    ranges: [true, true, true, true, true, false, false, false],
                    extra_chars: String::new(),
                    export_for_actionscript: false,
                    class_name: "RobotoMono".to_owned(),
                },
            ],
            selected: 0,
            original_entries: Vec::new(),
        }
    }
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-font-embedding-draft")
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

impl Editor {
    /// Open the `Font embedding` modal dialog (`DL-11`).
    pub(crate) fn open_font_embedding(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        let draft = embedding_draft_for_project(&self.project);
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
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

    pub(crate) fn font_embedding_body(
        &mut self,
        ui: &mut Ui,
        draft: &mut FontEmbeddingDraft,
    ) -> (bool, bool) {
        let mut close_cancel = false;
        let mut do_apply = false;

        if draft.entries.is_empty() {
            *draft = FontEmbeddingDraft::default();
        }
        draft.selected = draft.selected.min(draft.entries.len().saturating_sub(1));

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = space::S5;
            ui.allocate_ui_with_layout(
                egui::vec2(220.0, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(220.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for i in 0..draft.entries.len() {
                        let font_name = draft.entries[i].name.clone();
                        let out = c::TreeItem::new(&font_name, c::TreeItemType::Symbol(i16::FONT))
                            .selected(i == draft.selected)
                            .show(ui);
                        if out.row.clicked() {
                            draft.selected = i;
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui.add(c::IconButton::new(i16::PLUS, "Add font")).clicked() {
                            let idx = draft.entries.len() + 1;
                            draft.entries.push(EmbeddedFontEntry {
                                id: new_id("res"),
                                name: format!("Embedded Font {idx}"),
                                family: "Inter".to_owned(),
                                style: "Regular".to_owned(),
                                ranges: [true, true, true, true, false, false, false, false],
                                extra_chars: String::new(),
                                export_for_actionscript: false,
                                class_name: format!("EmbeddedFont{idx}"),
                            });
                            draft.selected = draft.entries.len() - 1;
                        }
                        if ui
                            .add_enabled(
                                draft.entries.len() > 1,
                                c::IconButton::new(i16::MINUS, "Remove font"),
                            )
                            .clicked()
                            && draft.entries.len() > 1
                        {
                            draft.entries.remove(draft.selected);
                            draft.selected =
                                draft.selected.min(draft.entries.len().saturating_sub(1));
                        }
                    });
                },
            );

            let entry = &mut draft.entries[draft.selected];
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::S3;
                ui.horizontal(|ui| {
                    ui.add(
                        c::TextField::new(&mut entry.name, "Name")
                            .prefix("Name")
                            .width(220.0),
                    );
                    let fam_lbl = format!("Family: {}", entry.family);
                    c::Select::new(ui.id().with("font-embed-family"), &fam_lbl)
                        .width(200.0)
                        .show(ui, |ui| {
                            for fam in FONT_FAMILIES {
                                if ui
                                    .add(
                                        c::MenuItem::new(&format!("Family: {fam}"))
                                            .checked(entry.family == fam),
                                    )
                                    .clicked()
                                {
                                    entry.family = fam.to_owned();
                                    ui.close();
                                }
                            }
                        });
                });
                let style_lbl = format!("Style: {}", entry.style);
                c::Select::new(ui.id().with("font-embed-style"), &style_lbl)
                    .width(220.0)
                    .show(ui, |ui| {
                        for st in FONT_STYLES {
                            if ui
                                .add(
                                    c::MenuItem::new(&format!("Style: {st}"))
                                        .checked(entry.style == st),
                                )
                                .clicked()
                            {
                                entry.style = st.to_owned();
                                ui.close();
                            }
                        }
                    });
                group(ui, "Character ranges");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(space::S4, space::S2);
                    for (idx, (label, _)) in RANGE_SPECS.iter().enumerate() {
                        ui.add(c::Checkbox::new(&mut entry.ranges[idx], label));
                    }
                });
                ui.add(
                    c::TextField::new(&mut entry.extra_chars, "Also include")
                        .prefix("Also include")
                        .width(ui.available_width()),
                );
                ui.horizontal(|ui| {
                    ui.add(c::Checkbox::new(
                        &mut entry.export_for_actionscript,
                        "Export for ActionScript",
                    ));
                    ui.add(
                        c::TextField::new(&mut entry.class_name, "Class")
                            .prefix("Class")
                            .width(200.0),
                    );
                });
            });
        });

        let entry = &draft.entries[draft.selected];
        let badge_text = format!(
            "{} glyphs · ~ {} kB",
            entry.glyph_count(),
            entry.approx_kb()
        );
        ui.add_space(space::S2);
        ui.horizontal(|ui| {
            ui.add(c::Badge::new(&badge_text, c::BadgeType::Accent));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(c::Button::primary("OK")).clicked() {
                    do_apply = true;
                }
                if ui.add(c::Button::new("Cancel")).clicked() {
                    close_cancel = true;
                }
            });
        });
        (close_cancel, do_apply)
    }

    pub(crate) fn font_embedding_inline_panel(&mut self, ui: &mut Ui) -> bool {
        let ctx = ui.ctx().clone();
        let mut draft = ctx
            .data(|d| d.get_temp::<FontEmbeddingDraft>(inline_draft_id()))
            .unwrap_or_else(|| embedding_draft_for_project(&self.project));
        let (close_cancel, do_apply) = self.font_embedding_body(ui, &mut draft);
        ctx.data_mut(|d| d.insert_temp(inline_draft_id(), draft));
        if do_apply {
            return self.confirm_font_embedding(&ctx).is_ok();
        }
        close_cancel
    }

    pub(crate) fn font_embedding_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<FontEmbeddingDraft>(draft_id())) else {
            return;
        };
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut do_apply = false;

        let modal = egui::Modal::new(egui::Id::new("font-embedding"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(760.0);
                let header_top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Font embedding",
                }
                .show(ui);
                ui.painter().hline(
                    header_top.x..=header_top.x + 760.0,
                    header_top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(760.0 - 2.0 * space::S5);
                        let (c, ok) = self.font_embedding_body(ui, &mut draft);
                        close_cancel |= c;
                        do_apply |= ok;
                    });
            });

        if modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close_cancel = true;
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
